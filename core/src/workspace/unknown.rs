//! Why a name is no signal, worked out (`Workspace::unknown`) apart from how it is said
//! (`Display`): the error lists what is there instead, so the next try is a pick (spec §15).

use super::Workspace;
use crate::trace::{canon, join, parts};
use std::fmt;

/// A name that resolves to no signal, and what the trace has instead.
#[derive(Debug, PartialEq)]
pub struct Unknown {
    /// As it was written.
    pub name: String,
    pub why: Why,
}

#[derive(Debug, PartialEq)]
pub enum Why {
    /// A scope: a module has no value of its own, its signals do. How many, and the first ones.
    Scope { signals: usize, first: Vec<String> },
    /// The start of the name is a signal without fields or elements (a packed struct dumped as one
    /// vector, as Verilator's VCD does): its path, its width, and the start as it was written.
    NoFields { signal: String, width: Option<u32>, real: bool, start: String },
    /// The start of the name is a struct, array or stream that has other fields: its path, the
    /// part it has not, and its fields.
    NoField { whole: String, missing: String, fields: Vec<String> },
    /// None of those: the nearest scopes and signals, maybe none.
    Nearest(Vec<String>),
}

/// How many signals or fields an error lists.
const LISTED: usize = 4;
const FIELDS: usize = 8;

impl Workspace {
    /// Why `name` names no signal: a scope, a signal or whole without that field, or else the
    /// nearest names.
    pub fn unknown(&self, name: &str) -> Unknown {
        let why = self.scope_of(name).or_else(|| self.no_field(name)).unwrap_or_else(|| Why::Nearest(self.trace.nearest(name, 3)));
        Unknown { name: name.to_string(), why }
    }

    /// The signals in the scope `name` names, if it names one: a part of the paths, at the end of
    /// none.
    fn scope_of(&self, name: &str) -> Option<Why> {
        let key = parts(&canon(name)).into_iter().map(str::to_string).collect::<Vec<_>>();
        let canon = self.trace.canon_paths();
        let last = key.last().map_or("", String::as_str);
        let inside: Vec<usize> = (0..self.trace.vars.len())
            .filter(|&v| {
                // split only the paths the name's last part is in
                let c = &canon[v];
                if !c.contains(last) {
                    return false;
                }
                let p = parts(c);
                p.len() > key.len() && p.windows(key.len()).take(p.len() - key.len()).any(|w| w == key)
            })
            .collect();
        (!inside.is_empty()).then(|| Why::Scope { signals: inside.len(), first: inside.iter().take(LISTED).map(|&v| self.trace.vars[v].path.clone()).collect() })
    }

    /// The longest start of `name` that is one signal, when the rest is a field or element of it
    /// the trace doesn't have.
    fn no_field(&self, name: &str) -> Option<Why> {
        let written = parts(name);
        let bracketed = crate::trace::brackets(name);
        let p = parts(&bracketed);
        (1..p.len()).rev().find_map(|k| {
            let start = join(p[..k].iter().copied());
            let v = self.by_end(&start).ok()?;
            let var = &self.trace.vars[v];
            // what is a part deeper: a struct's or array's fields, a stream's
            let deeper: Vec<String> = (self.trace.vars.iter())
                .filter_map(|x| x.path.strip_prefix(&var.path).filter(|r| r.starts_with(['.', '['])))
                .map(|r| r.trim_start_matches('.'))
                .filter(|r| parts(r).len() == 1)
                .map(str::to_string)
                .collect();
            let as_written = if written.len() == p.len() { join(written[..k].iter().copied()) } else { start.clone() };
            Some(match deeper.is_empty() {
                true => Why::NoFields { signal: var.path.clone(), width: var.width, real: var.real, start: as_written },
                false => Why::NoField { whole: var.path.clone(), missing: p[k].trim_start_matches('.').to_string(), fields: deeper },
            })
        })
    }
}

impl fmt::Display for Unknown {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let name = &self.name;
        match &self.why {
            Why::Scope { signals, first } => {
                let more = if *signals > first.len() { ", ..." } else { "" };
                write!(f, "unknown signal: {name} is a scope of {signals} signals ({}{more}); name one, or `add` the scope to show them all", first.join(", "))
            }
            Why::NoFields { signal, width, real, start } => {
                let kind = match (width, real) {
                    (Some(w), _) => format!("{w}-bit"),
                    (None, true) => "real".into(),
                    (None, false) => "text".into(),
                };
                write!(f, "unknown signal: {name}: {signal} is one {kind} signal in this trace, without fields or elements")?;
                match width {
                    // a packed struct's fields are bits of it
                    Some(w) if *w > 1 => write!(f, "; slice it, `{start}[{}:0]`", w - 1),
                    _ => Ok(()),
                }
            }
            Why::NoField { whole, missing, fields } => {
                let more = if fields.len() > FIELDS { ", ..." } else { "" };
                let listed: Vec<&str> = fields.iter().take(FIELDS).map(String::as_str).collect();
                write!(f, "unknown signal: {name}: {whole} has no {missing}; it has {}{more}", listed.join(", "))
            }
            Why::Nearest(near) if near.is_empty() => write!(f, "unknown signal: {name}"),
            Why::Nearest(near) => write!(f, "unknown signal: {name} (nearest: {})", near.join(", ")),
        }
    }
}
