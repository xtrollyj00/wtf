//! Records and streams (spec §16): what decoders produce. A record has a start, an end
//! and fields; a stream has a name, a schema (its fields' names and widths) and its records in
//! time order, worked out once. The stateless sampler makes one record per clock cycle a predicate
//! holds, its fields sampled as the predicate is.

use crate::query::{self, Search};
use crate::search::{Probe, Query};
use crate::time::Tick;
use crate::trace::{Trace, Val};

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    /// Bit width; None for real and string values.
    pub width: Option<u32>,
    /// Its values are durations in ticks (a pair's `delay`), shown as times in the unit asked for.
    pub time: bool,
    /// Its values are two's complement (a computed signal that can be negative).
    pub signed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub start: Tick,
    pub end: Tick,
    /// One value per field of the schema, in its order; None where the signal had none.
    pub fields: Vec<Option<Val>>,
}

pub struct Stream {
    pub name: String,
    pub schema: Vec<Field>,
    /// In order of start.
    pub records: Vec<Record>,
    /// Its records are violations (`check`, a pair's `unmatched`), drawn red.
    pub violations: bool,
}

/// A signal made from a stream: path, width, values in time order.
pub type Signal = (String, Option<u32>, Vec<(Tick, Val)>);

/// A stateless protocol (`[[protocol]]`): a record per cycle of `clock` where `when` holds, with
/// `fields` (field, signal) sampled with it; signals named as in a search. Without roles it is a
/// stream of its own, on the signals it names; with roles (names its expressions use) it is a type,
/// put on signals by a `Decode`. Kept to sample again on `:e`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sampler {
    pub name: String,
    /// Empty: each instance gives it.
    pub clock: String,
    pub when: String,
    pub fields: Vec<(String, String)>,
    pub roles: Vec<String>,
}

/// An instance of a protocol (`[[decode]]`): the stream `name`, the protocol's roles played by
/// these signals (role, signal), at `clock` (empty: the protocol's). For `frame` and `pair` the
/// roles are streams (`input`; `request`, `response`).
#[derive(Clone, Debug, PartialEq)]
pub struct Decode {
    pub name: String,
    pub protocol: String,
    pub clock: String,
    pub signals: Vec<(String, String)>,
    /// The decoder's settings, for `frame` and `pair` (`last = "tlast"`).
    pub params: Vec<(String, String)>,
}

/// Records with the same values of some fields (`Stream::group_by`): the values, how many, and
/// the first and last start.
pub struct Group {
    pub values: Vec<Option<Val>>,
    pub count: usize,
    pub first: Tick,
    pub last: Tick,
}

/// Some of a stream's records, and how many there are in the range asked.
pub struct Page<'a> {
    pub records: &'a [Record],
    pub total: usize,
}

impl Stream {
    /// The stream as signals (path, width, values from `start`): itself, `#n` while record n
    /// lasts and empty between records, and a signal per field, `z` (or empty) between records.
    /// Where records overlap (pairs), the latest started of those still open is shown.
    pub fn signals(&self, start: Tick) -> Vec<Signal> {
        let gap = |w: Option<u32>| w.map_or(Val::Text(String::new()), |w| Val::Bits("z".repeat(w as usize)));
        // the records open and close in time order; at one time, closes first, so one record's end
        // can be the next one's start
        let mut events: Vec<(Tick, bool, usize)> =
            self.records.iter().enumerate().filter(|(_, r)| r.end > r.start).flat_map(|(i, r)| [(r.start, true, i), (r.end, false, i)]).collect();
        events.sort();
        let over = |value: &dyn Fn(usize) -> Val, w| {
            let mut open = std::collections::BTreeSet::new();
            let mut out = vec![(start, gap(w))];
            for &(t, opens, i) in &events {
                if opens {
                    open.insert(i);
                } else {
                    open.remove(&i);
                }
                out.push((t, open.last().map_or(gap(w), |&i| value(i))));
            }
            out
        };
        let mut out = vec![(self.name.clone(), None, over(&|i| Val::Text(format!("#{i}")), None))];
        for (k, f) in self.schema.iter().enumerate() {
            let value = |i: usize| self.records[i].fields[k].clone().unwrap_or(gap(f.width));
            out.push((format!("{}.{}", self.name, f.name), f.width, over(&value, f.width)));
        }
        out
    }

    /// Record `i`'s short label, as a stream row shows it: `#3`, with a `len` field `#3 64B`, and
    /// the text and time fields' values but a frame's `data` (`#3 40ns`, `#0 dropped`).
    pub fn label(&self, i: usize, fpt: Option<u128>) -> String {
        let mut out = format!("#{i}");
        for (f, v) in self.schema.iter().zip(&self.records[i].fields) {
            match v {
                Some(Val::Text(t)) if !t.is_empty() && f.name != "data" => out += &format!(" {t}"),
                Some(Val::Bits(_)) if f.time => out += &format!(" {}", value_text(f, v, fpt, None, Some(3))),
                Some(Val::Bits(b)) if f.name == "len" => out += &u64::from_str_radix(b, 2).map_or(String::new(), |n| format!(" {n}B")),
                _ => {}
            }
        }
        out
    }

    /// Record `i`'s fields as (name, value) text, as `value_text` writes them, times in the unit that
    /// fits each.
    pub fn fields_text(&self, i: usize, fpt: Option<u128>) -> Vec<(&str, String)> {
        self.schema.iter().zip(&self.records[i].fields).map(|(f, v)| (f.name.as_str(), value_text(f, v, fpt, None, Some(3)))).collect()
    }

    /// The records starting in [from, to], at most `limit` of them, the earliest first.
    pub fn records(&self, from: Tick, to: Tick, limit: usize) -> Page<'_> {
        let a = self.records.partition_point(|r| r.start < from);
        let b = self.records.partition_point(|r| r.start <= to).max(a);
        Page { records: &self.records[a..b.min(a.saturating_add(limit))], total: b - a }
    }

    /// The numbers of the records starting in [from, to] for which `filter` holds, and how many
    /// start there in all. `filter` is a search expression over the fields, named bare (`addr !=
    /// 0x1C && resp == 0`), evaluated on each record, so never on the gaps between them; `past(x)`
    /// is x of the record before, `n` the record's number unless a field has that name, and a
    /// field compares with another field too (`req_addr != resp_addr`).
    pub fn select(&self, from: Tick, to: Tick, filter: Option<&str>, fpt: Option<u128>) -> Result<(Vec<usize>, usize), String> {
        let all = self.records(from, to, usize::MAX);
        let first = self.records.partition_point(|r| r.start < from);
        let numbers = first..first + all.records.len();
        let Some(text) = filter else { return Ok((numbers.collect(), all.total)) };
        // `n`, the record's number, is the probe after the fields
        let n = self.schema.len();
        let q = crate::search::parse_query(text, fpt, &mut |name| match self.schema.iter().position(|f| f.name == name) {
            Some(k) => Ok(((k, None), self.schema[k].width, None, self.schema[k].signed)),
            None if name == "n" => Ok(((n, None), Some(64), None, false)),
            None => {
                let names: Vec<&str> = self.schema.iter().map(|f| f.name.as_str()).collect();
                Err(format!("{}: no field {name} (fields: {}, and n, the record's number)", self.name, names.join(", ")))
            }
        })?;
        if q.clock.is_some() {
            return Err("a record filter has no clock: it is evaluated on each record".into());
        }
        let picked = numbers
            .filter(|&i| {
                crate::search::holds(&q.expr, &|(k, _), back| {
                    let i = i.checked_sub(back as usize)?;
                    Some(match k == n {
                        true => Some(Val::Bits(format!("{i:064b}"))),
                        false => self.records[i].fields[k].clone(),
                    })
                })
            })
            .collect();
        Ok((picked, all.total))
    }

    /// The distinct values of the fields named among `records`, in order of first appearance, so
    /// 128 identical polls are one group.
    pub fn group_by(&self, records: &[&Record], names: &[String]) -> Result<Vec<Group>, String> {
        let ks = names
            .iter()
            .map(|n| self.schema.iter().position(|f| f.name == *n).ok_or_else(|| {
                let all: Vec<&str> = self.schema.iter().map(|f| f.name.as_str()).collect();
                format!("{}: no field {n} (fields: {})", self.name, all.join(", "))
            }))
            .collect::<Result<Vec<_>, _>>()?;
        let mut groups: Vec<Group> = Vec::new();
        let mut index = std::collections::HashMap::new();
        for r in records {
            let values: Vec<Option<Val>> = ks.iter().map(|&k| r.fields[k].clone()).collect();
            let i = *index.entry(values.clone()).or_insert_with(|| {
                groups.push(Group { values, count: 0, first: r.start, last: r.start });
                groups.len() - 1
            });
            groups[i].count += 1;
            groups[i].last = r.start;
        }
        Ok(groups)
    }
}

/// A field's value as text: numbers in hex (signed ones in decimal), 1-bit ones as the bit, `-` where there is none, and a
/// time field's ticks as a time in `unit` to `decimals`, as `time::format_as` takes them.
pub fn value_text(f: &Field, v: &Option<Val>, fpt: Option<u128>, unit: Option<usize>, decimals: Option<u32>) -> String {
    match v {
        None => "-".into(),
        Some(Val::Bits(b)) if f.time => u64::from_str_radix(b, 2).map_or("x".into(), |t| crate::time::format_as(t, fpt, unit, decimals)),
        Some(Val::Bits(b)) if b.len() == 1 => b.to_uppercase(),
        Some(Val::Bits(b)) if f.signed => (crate::radix::RADIXES[crate::radix::SDEC].format)(b, 0),
        Some(Val::Bits(b)) => (crate::radix::RADIXES[crate::radix::DEFAULT].format)(b, 0),
        Some(Val::Text(t)) => t.clone(),
    }
}

/// A record per value of a signal (or bit), from where it takes the value to its next change, in
/// the field `value`: what SPI and UART decode, as they sample by time. Its values must be loaded.
pub fn changes(trace: &Trace, name: &str, p: Probe) -> Stream {
    let start = trace.start();
    let mut changes = vec![(start, trace.value_of(p.0, p.1, start))];
    changes.extend(trace.walk(p.0, p.1, start, true));
    let end = trace.end();
    let ends: Vec<Tick> = changes.iter().skip(1).map(|c| c.0).collect();
    let records = changes
        .into_iter()
        .enumerate()
        .map(|(i, (t, v))| Record { start: t, end: ends.get(i).copied().unwrap_or(end.max(t + 1)), fields: vec![v] })
        .collect();
    let schema = vec![Field { name: "value".into(), width: trace.probe_width(p), time: false, signed: false }];
    Stream { name: name.into(), schema, records, violations: false }
}

/// The stateless sampler: a record for each clock cycle whose ending edge samples `q` true, from
/// the cycle's starting edge to its ending one, with `fields` (name, signal, width) sampled where
/// the predicate is (just before the ending edge).
pub fn sample(trace: &Trace, name: &str, q: &Query, fields: &[(String, Probe, Option<u32>)]) -> Result<Stream, String> {
    if q.clock.is_none() {
        return Err(format!("{name}: a stream samples at a clock (@clk ...)"));
    }
    let found = query::find(trace, q, &Search { from: 0, to: None, forward: true, merge: false, runs: usize::MAX, matches: u64::MAX });
    let records = found
        .runs
        .iter()
        .map(|r| {
            let at = query::sampled(trace, q, r.first);
            Record { start: r.first, end: at + 1, fields: fields.iter().map(|(_, p, _)| trace.value_of(p.0, p.1, at)).collect() }
        })
        .collect();
    let schema = fields.iter().map(|(n, p, w)| Field { name: n.clone(), width: *w, time: false, signed: p.1.is_none() && trace.vars[p.0].signed }).collect();
    Ok(Stream { name: name.to_string(), schema, records, violations: false })
}
