//! Playing a protocol written on the trait: the cycles it is fed, the roles' values at each, and
//! the streams it makes of them.

use std::sync::Arc;

use super::Workspace;
use super::reason::Reasons;
use crate::protocol::{self, Default, Out, Roles};
use crate::query;
use crate::stream::{self, Decode, Stream};
use crate::time::Tick;
use crate::trace::Val;

/// The value of a role its instance has no signal for.
fn default_value(role: &protocol::RoleDef, given: &protocol::Given) -> Option<Val> {
    let width = |name: &str| given.iter().find(|g| g.0 == name).and_then(|g| g.1);
    let w = width(role.name).unwrap_or(1) as usize;
    match role.default? {
        Default::Zero => Some(Val::Bits("0".repeat(w))),
        Default::Ones => Some(Val::Bits("1".repeat(w))),
        Default::Bits(b) => Some(Val::Bits(b.to_string())),
        Default::Bytes(src) => Some(Val::Bits("1".repeat(width(src)?.div_ceil(8) as usize))),
        Default::Same(_) => None,
    }
}

impl Workspace {
    /// The times a protocol without a clock is fed at: the trace's start, each change of its roles'
    /// signals, and its end (one past a last change, so that it is a cycle of its own).
    fn change_cycles(&self, signals: &[(usize, usize, Option<u32>)]) -> Vec<Tick> {
        let start = self.trace.start();
        let mut times = vec![start];
        for &(_, var, bit) in signals {
            times.extend(self.trace.walk(var, bit, start, true).map(|(t, _)| t));
        }
        times.sort_unstable();
        times.dedup();
        let end = self.trace.end();
        let last = times.last().copied().unwrap_or(end);
        times.push(if last >= end { last + 1 } else { end });
        times
    }

    /// Adds `d` as an instance of the protocol `any`: its clock's cycles, the roles' values at
    /// each, and the streams the protocol makes. Returns the first stream's var.
    pub(super) fn decode_protocol(
        &mut self,
        d: &Decode,
        any: protocol::Any,
    ) -> Result<usize, String> {
        let def = any.def();
        let required = self.required(d, def);
        let (_, optional) = def.roles();
        let mut errors = Vec::new();
        let given = self.bind(d, &required, &optional, def.params(), &mut errors);
        if d.clock.is_empty() && def.clocked() {
            errors.push(super::reason::Reason {
                on: vec!["clock".into()],
                why: super::reason::Why::NoClock,
            });
        }
        if !errors.is_empty() {
            self.roles.clear();
            return Err(Reasons {
                name: None,
                list: errors,
            }
            .to_string());
        }
        // the roles with a signal, and where it is: as `bind` found it, whether given to the
        // instance or by the prefix
        let given: Vec<(&str, Option<u32>)> = given.iter().map(|(r, w)| (r.as_str(), *w)).collect();
        let prefix = d
            .params
            .iter()
            .find(|p| p.0 == "prefix")
            .map(|p| p.1.clone());
        let mut signals: Vec<(usize, usize, Option<u32>)> = Vec::new();
        for (role, _) in &given {
            let i = def
                .roles
                .iter()
                .position(|r| r.name == *role)
                .ok_or_else(|| format!("{role}: not a role of {}", d.protocol))?;
            let path = match d.signals.iter().find(|s| s.0 == *role) {
                Some((_, path)) => path.clone(),
                None => format!(
                    "{}{role}",
                    prefix
                        .clone()
                        .ok_or_else(|| format!("no signal for {role}"))?
                ),
            };
            let (var, bit) = self.resolve(&path)?;
            signals.push((i, var, bit));
        }
        let clock = match def.clocked {
            true => Some(
                self.resolve(&d.clock)
                    .map_err(|e| format!("{}: {e}", d.clock))?,
            ),
            false => None,
        };
        let mut load: Vec<usize> = signals.iter().map(|s| s.1).collect();
        load.extend(clock.map(|c| c.0));
        self.trace.load(&load);
        let params: Vec<(String, String)> = d
            .params
            .iter()
            .filter(|p| p.0 != "prefix")
            .cloned()
            .collect();
        let mut instance = any.make(&given, &params, self.trace.fs_per_tick)?;
        let mut out: Vec<Out> = Vec::new();
        let mut values: Vec<Option<Val>> = vec![None; def.roles.len()];
        // a clock among the roles is not one: every change of them is a cycle
        let cycles = match clock {
            Some(clock) => query::edges(&self.trace, clock),
            None => Arc::new(self.change_cycles(&signals)),
        };
        for cycle in cycles.windows(2) {
            let (start, end) = (cycle[0], cycle[1]);
            for (i, role) in def.roles.iter().enumerate() {
                values[i] = match signals.iter().find(|s| s.0 == i) {
                    Some(&(_, var, bit)) => self.trace.value_of(var, bit, end - 1),
                    None => default_value(role, &given),
                };
            }
            // a default that reads another role takes what that one has this cycle
            for (i, role) in def.roles.iter().enumerate() {
                if signals.iter().any(|s| s.0 == i) {
                    continue;
                }
                if let Some(Default::Same(src)) = role.default {
                    values[i] = def
                        .roles
                        .iter()
                        .position(|r| r.name == src)
                        .and_then(|j| values[j].clone());
                }
            }
            out.extend(instance.feed(start, end, &Roles::new(&values, def.roles)));
        }
        out.extend(instance.finish());
        out.sort_by_key(|o| match o {
            Out::Record { start, .. } | Out::Violation { start, .. } => *start,
        });
        let streams = &instance.desc().streams;
        let mut first = None;
        for sd in streams {
            let records = out
                .iter()
                .filter_map(|o| match o {
                    Out::Record {
                        stream: name,
                        start,
                        end,
                        fields,
                    } if *name == sd.name => Some(stream::Record {
                        start: *start,
                        end: *end,
                        fields: fields.clone(),
                    }),
                    // a violation is the record of the stream that holds them, its fields by name:
                    // a report is the one with them all (`errors`), not a side's unmatched stream
                    Out::Violation {
                        rule,
                        start,
                        end,
                        detail,
                        severity,
                    } if sd.violations && sd.fields.iter().any(|f| f.name == "severity") => {
                        Some(stream::Record {
                            start: *start,
                            end: *end,
                            fields: sd
                                .fields
                                .iter()
                                .map(|f| match f.name.as_str() {
                                    "rule" => Some(Val::Text(rule.to_string())),
                                    "detail" => Some(Val::Text(detail.clone())),
                                    "severity" => Some(Val::Text(severity.text().to_string())),
                                    _ => None,
                                })
                                .collect(),
                        })
                    }
                    _ => None,
                })
                .collect();
            // made from what it says, by the instance's names: a frame's beats (spec §16 "Results")
            let inputs = sd
                .from
                .iter()
                .map(|s| format!("{}.{}", d.name, s))
                .collect();
            let s = Stream {
                name: format!("{}.{}", d.name, sd.name),
                schema: sd.fields.clone(),
                records,
                violations: sd.violations,
                inputs,
            };
            let var = self.add_signals(s)?;
            first.get_or_insert(var);
        }
        first.ok_or_else(|| format!("{}: no streams", d.protocol))
    }
}
