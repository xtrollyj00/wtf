//! Questions about a trace that answer with data, for the interfaces to show (spec §8, §15):
//! where a search expression matches, as runs of consecutive clock cycles, and how a signal
//! changes over a time range.

use crate::search::{self, Probe, Query, Target};
use crate::time::Tick;
use crate::trace::{Trace, Val};
use std::sync::Arc;

/// Consecutive matches: with a clock, cycles one after another; without, a single match.
/// `first` and `last` are in search order (`last` is earlier when searching backward).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Run {
    pub first: Tick,
    pub last: Tick,
    pub cycles: u64,
}

impl Run {
    /// Start and end in time order.
    pub fn span(&self) -> (Tick, Tick) {
        (self.first.min(self.last), self.first.max(self.last))
    }
}

pub struct Found {
    pub runs: Vec<Run>,
    /// Matches counted, in all the runs.
    pub total: u64,
    /// The search stopped at a limit with more matches to come.
    pub more: bool,
}

/// Where to look and how much to collect.
pub struct Search {
    /// Search after (backward: before) this time.
    pub from: Tick,
    /// Stop at this time.
    pub to: Option<Tick>,
    pub forward: bool,
    /// Merge consecutive clock cycles into runs.
    pub merge: bool,
    /// Most runs to collect.
    pub runs: usize,
    /// Most matches to count.
    pub matches: u64,
}

/// The matches of `q`, as runs.
pub fn find(trace: &Trace, q: &Query, s: &Search) -> Found {
    if let Some(clock) = q.clock {
        return find_clocked(trace, q, clock, s);
    }
    let merge = q.clock.filter(|_| s.merge);
    let past = |x: Tick| s.to.is_some_and(|to| if s.forward { x > to } else { x < to });
    let mut found = Found { runs: Vec::new(), total: 0, more: false };
    let mut t = s.from;
    while let Some(x) = search::find_query(trace, q, t, s.forward).filter(|&x| !past(x)) {
        let extends = merge.zip(found.runs.last()).is_some_and(|(c, run)| next_edge(trace, c, run.last, s.forward) == Some(x));
        if extends {
            let run = found.runs.last_mut().unwrap();
            (run.last, run.cycles) = (x, run.cycles + 1);
        } else if found.runs.len() == s.runs {
            found.more = true;
            break;
        } else {
            found.runs.push(Run { first: x, last: x, cycles: 1 });
        }
        found.total += 1;
        t = x;
        if found.total == s.matches {
            found.more = true;
            break;
        }
    }
    found
}

/// A clocked search over a table rather than edge by edge: the clock's rising edges (`edges`), and
/// for each signal the expression reads its value at each sample, just before each edge, found by
/// one walk over its changes. A match is a cycle start, edge k, whose ending edge's sample, row
/// k + 1, holds; `past(x)` is the row before. As `search::find_query` finds them, without searching
/// for each edge and value.
fn find_clocked(trace: &Trace, q: &Query, clock: Probe, s: &Search) -> Found {
    let edges = edges(trace, clock);
    let mut probes = Vec::new();
    search::probes(&q.expr, &mut probes);
    let back = probes.iter().map(|p| p.1 as usize).max().unwrap_or(0);
    // the cycle starts to look at, in search order, within `from` and `to`
    let (lo, hi) = if s.forward {
        let a = edges.partition_point(|&e| e <= s.from);
        let b = s.to.map_or(edges.len(), |to| edges.partition_point(|&e| e <= to));
        (a, b)
    } else {
        let b = edges.partition_point(|&e| e < s.from);
        let a = s.to.map_or(0, |to| edges.partition_point(|&e| e < to));
        (a, b)
    };
    // the rows they read: each one's ending sample, and `back` before it
    let rows = (lo + 1).saturating_sub(back)..(hi + 1).min(edges.len());
    if lo >= hi || rows.is_empty() {
        return Found { runs: Vec::new(), total: 0, more: false };
    }
    let samples: Vec<Tick> = edges[rows.clone()].iter().map(|&e| e - 1).collect();
    let columns: std::collections::HashMap<Probe, Column> = probes.iter().map(|&(p, _)| (p, Column::new(trace, p, &samples))).collect();
    let holds = |row: usize| {
        search::holds(&q.expr, &|p, n| {
            let r = row.checked_sub(n as usize).filter(|&r| r >= rows.start)?;
            Some(columns[&p].at(r - rows.start))
        })
    };
    let mut found = Found { runs: Vec::new(), total: 0, more: false };
    let order: Box<dyn Iterator<Item = usize>> = if s.forward { Box::new(lo..hi) } else { Box::new((lo..hi).rev()) };
    let mut prev: Option<usize> = None;
    for k in order.filter(|&k| k + 1 < edges.len() && holds(k + 1)) {
        let x = edges[k];
        // the cycle right after (backward: before) the last match's
        let extends = s.merge && prev.is_some_and(|j| if s.forward { k == j + 1 } else { k + 1 == j });
        prev = Some(k);
        if extends {
            let run = found.runs.last_mut().unwrap();
            (run.last, run.cycles) = (x, run.cycles + 1);
        } else if found.runs.len() == s.runs {
            found.more = true;
            break;
        } else {
            found.runs.push(Run { first: x, last: x, cycles: 1 });
        }
        found.total += 1;
        if found.total == s.matches {
            found.more = true;
            break;
        }
    }
    found
}

/// A signal's value at each sample of a table, as an index into its values (one per change).
struct Column {
    rows: Vec<u32>,
    vals: Vec<Option<Val>>,
}

impl Column {
    /// One walk over the signal's changes alongside the samples, in time order.
    fn new(trace: &Trace, p: Probe, samples: &[Tick]) -> Self {
        let (mut rows, mut vals) = (Vec::with_capacity(samples.len()), Vec::new());
        let Some(&t0) = samples.first() else { return Column { rows, vals } };
        vals.push(trace.value_of(p.0, p.1, t0));
        let mut changes = trace.walk(p.0, p.1, t0, true).peekable();
        for &t in samples {
            // the value at t: the last change up to it, if there are new ones
            if let Some(v) = std::iter::from_fn(|| changes.next_if(|c| c.0 <= t)).last() {
                vals.push(v.1);
            }
            rows.push(vals.len() as u32 - 1);
        }
        Column { rows, vals }
    }

    fn at(&self, row: usize) -> Option<&Val> {
        self.vals[self.rows[row] as usize].as_ref()
    }
}

/// A clock's rising edges: where it becomes 1. Worked out once per clock, and kept in the trace.
pub fn edges(trace: &Trace, clock: Probe) -> Arc<Vec<Tick>> {
    if let Some(e) = trace.edges.borrow().get(&clock) {
        return e.clone();
    }
    let rise = Target::Bits("1".into());
    let out: crate::trace::Edges = Arc::new(search::find_all(trace, clock.0, clock.1, &rise, 0, true).collect());
    trace.edges.borrow_mut().insert(clock, out.clone());
    out
}

/// The rising edge of `clock` after (backward: before) `t`.
fn next_edge(trace: &Trace, clock: Probe, t: Tick, forward: bool) -> Option<Tick> {
    search::find(trace, clock.0, clock.1, &Target::Bits("1".into()), t, forward)
}

/// The time whose values a match at `t` stands for: with a clock, just before the cycle's ending
/// edge, as a flip-flop samples them (values registered at the starting edge change just after
/// it); without, `t` itself.
pub fn sampled(trace: &Trace, q: &Query, t: Tick) -> Tick {
    q.clock.and_then(|c| next_edge(trace, c, t, true)).map_or(t, |e| e - 1)
}

/// The times a signal (or bit) changes after `from`, up to `to`, at most `budget` of them, which
/// is spent. Returns the times and whether it stopped at the budget with more to come. Loads the
/// signal's values if they aren't yet.
pub fn changes(trace: &mut Trace, p: Probe, from: Tick, to: Tick, budget: &mut usize) -> (Vec<Tick>, bool) {
    trace.load(&[p.0]);
    let mut out = Vec::new();
    for (x, _) in trace.walk(p.0, p.1, from, true).take_while(|c| c.0 <= to) {
        if *budget == 0 {
            return (out, true);
        }
        out.push(x);
        *budget -= 1;
    }
    (out, false)
}
