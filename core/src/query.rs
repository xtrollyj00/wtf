//! Questions about a trace that answer with data, for the interfaces to show (spec §8, §15):
//! where a search expression matches, as runs of consecutive clock cycles, and how a signal
//! changes over a time range.

use crate::search::{self, Probe, Query, Target};
use crate::time::Tick;
use crate::trace::Trace;
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

#[derive(PartialEq, Debug)]
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
    let past = |x: Tick| s.to.is_some_and(|to| if s.forward { x > to } else { x < to });
    let mut found = Found { runs: Vec::new(), total: 0, more: false };
    let mut t = s.from;
    while let Some(x) = search::find_query(trace, q, t, s.forward).filter(|&x| !past(x)) {
        if found.runs.len() == s.runs {
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

/// A clocked search by stretches of cycles: cycle k (from edge k) holds if the expression does on
/// the values sampled just before its ending edge, row k + 1, and `past(x)` reads x n rows
/// earlier. Those values change only where a signal read changes (n cycles later for `past`), so
/// the expression is evaluated once per stretch between such changes, and the stretch matches or
/// not as a whole: the cost follows the signals' changes, not the trace's cycles.
// shortcut: works out every stretch of the range before taking any, so a search stopped at a limit
// still walks its signals to the end of the range; walk them lazily if that is ever too slow.
fn find_clocked(trace: &Trace, q: &Query, clock: Probe, s: &Search) -> Found {
    let mut found = Found { runs: Vec::new(), total: 0, more: false };
    let edges = edges(trace, clock);
    // the cycles to look at, lo..end in time order: each needs an ending edge
    let (lo, hi) = if s.forward {
        let a = edges.partition_point(|&e| e <= s.from);
        let b = s.to.map_or(edges.len(), |to| edges.partition_point(|&e| e <= to));
        (a, b)
    } else {
        let b = edges.partition_point(|&e| e < s.from);
        let a = s.to.map_or(0, |to| edges.partition_point(|&e| e < to));
        (a, b)
    };
    let end = hi.min(edges.len().saturating_sub(1));
    if lo >= end {
        return found;
    }
    // each signal read, with the most samples back it is read
    let mut probes: Vec<(Probe, u32)> = Vec::new();
    search::probes(&q.expr, &mut probes);
    probes.sort_unstable();
    probes.dedup_by(|b, a| a.0 == b.0 && {
        a.1 = a.1.max(b.1);
        true
    });
    // where a stretch starts: lo, and each cycle whose samples differ from the cycle before's
    let mut starts = vec![lo];
    let mut cut = |k: usize| {
        if (lo..end).contains(&k) {
            starts.push(k);
        }
    };
    for &(p, most) in &probes {
        for n in 0..=most as usize {
            // where `past` n first reads a sample: before, it reads none
            cut(n.wrapping_sub(1));
        }
        // a change at x is first sampled at row r, the first edge after x, so read n rows back
        // by cycle r - 1 + n; the samples that matter are rows lo + 1 - most ..= end
        let first_row = (lo + 1).saturating_sub(most as usize);
        let after = first_row.checked_sub(1).map_or(0, |r| edges[r] - 1);
        let until = edges[end];
        // a signal's first value is a change too: before it, it has none
        let changes = trace.first_time(p.0).filter(|&f| f > after).into_iter().chain(trace.walk(p.0, p.1, after, true).map(|c| c.0));
        let mut r = 0;
        for x in changes.take_while(|&x| x < until) {
            r = gallop(&edges, r, x);
            for n in 0..=most as usize {
                cut((r + n).wrapping_sub(1));
            }
        }
    }
    starts.sort_unstable();
    starts.dedup();
    let stretches = starts.iter().enumerate().map(|(i, &a)| (a, starts.get(i + 1).map_or(end, |&b| b) - 1));
    let holds = |k: usize| search::holds(&q.expr, &|p, n| (k + 1).checked_sub(n as usize).map(|r| trace.value_of(p.0, p.1, edges[r] - 1)));
    let mut prev = None;
    if s.forward {
        for (a, b) in stretches {
            if holds(a) && !take(&mut found, &mut prev, &edges, s, a, b) {
                break;
            }
        }
    } else {
        for (a, b) in stretches.rev() {
            if holds(a) && !take(&mut found, &mut prev, &edges, s, a, b) {
                break;
            }
        }
    }
    found
}

/// The first edge after `x`, at or after `from`: steps 1, 2, 4 ... ahead, then a binary search of
/// the last step, so near edges, as a signal's next change usually is, cost little.
fn gallop(edges: &[Tick], from: usize, x: Tick) -> usize {
    let mut step = 1;
    while from + step < edges.len() && edges[from + step - 1] <= x {
        step *= 2;
    }
    let lo = from + step / 2;
    lo + edges[lo..(from + step).min(edges.len())].partition_point(|&e| e <= x)
}

/// Count the matching cycles a ..= b into `found`, in search order, merging consecutive ones into
/// runs; `prev` is the last cycle taken. False once a limit stops the search.
fn take(found: &mut Found, prev: &mut Option<usize>, edges: &[Tick], s: &Search, a: usize, b: usize) -> bool {
    let len = b - a + 1;
    let at = |i: usize| if s.forward { a + i } else { b - i };
    let mut i = 0;
    while i < len {
        let k = at(i);
        if s.merge && prev.is_some_and(|j| if s.forward { k == j + 1 } else { k + 1 == j }) {
            // the rest of the stretch extends the run, as far as the match limit lets it
            let room = s.matches.checked_sub(found.total).filter(|&r| r > 0).unwrap_or(u64::MAX);
            let n = ((len - i) as u64).min(room) as usize;
            let last = at(i + n - 1);
            let run = found.runs.last_mut().unwrap();
            (run.last, run.cycles) = (edges[last], run.cycles + n as u64);
            found.total += n as u64;
            *prev = Some(last);
            i += n;
        } else if found.runs.len() == s.runs {
            found.more = true;
            return false;
        } else {
            found.runs.push(Run { first: edges[k], last: edges[k], cycles: 1 });
            found.total += 1;
            *prev = Some(k);
            i += 1;
        }
        if found.total == s.matches {
            found.more = true;
            return false;
        }
    }
    true
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

/// The time whose values a match at `t` stands for: with a clock, just before the cycle's ending
/// edge, as a flip-flop samples them (values registered at the starting edge change just after
/// it); without, `t` itself.
pub fn sampled(trace: &Trace, q: &Query, t: Tick) -> Tick {
    // from the clock's edges, kept: a record each, so no walk over the clock for each
    let next = |c| {
        let e = edges(trace, c);
        e.get(e.partition_point(|&x| x <= t)).copied()
    };
    q.clock.and_then(next).map_or(t, |e| e - 1)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    /// The clocked search as spec §15 defines it, cycle by cycle, reading each value where it is
    /// sampled: the reference the fast one must agree with.
    fn reference(trace: &Trace, q: &Query, s: &Search) -> Found {
        let edges = edges(trace, q.clock.unwrap());
        let mut probes = Vec::new();
        search::probes(&q.expr, &mut probes);
        let back = probes.iter().map(|p| p.1 as usize).max().unwrap_or(0);
        let (lo, hi) = if s.forward {
            (edges.partition_point(|&e| e <= s.from), s.to.map_or(edges.len(), |to| edges.partition_point(|&e| e <= to)))
        } else {
            (s.to.map_or(0, |to| edges.partition_point(|&e| e < to)), edges.partition_point(|&e| e < s.from))
        };
        let first_row = (lo + 1).saturating_sub(back);
        // cycle k is sampled just before its ending edge, k + 1; `past` n rows before that
        let holds = |k: usize| {
            search::holds(&q.expr, &|p, n| (k + 1).checked_sub(n as usize).filter(|&r| r >= first_row).map(|r| trace.value_of(p.0, p.1, edges[r] - 1)))
        };
        let mut found = Found { runs: Vec::new(), total: 0, more: false };
        let order: Vec<usize> = if s.forward { (lo..hi).collect() } else { (lo..hi).rev().collect() };
        let mut prev: Option<usize> = None;
        for k in order.into_iter().filter(|&k| k + 1 < edges.len() && holds(k)) {
            let x = edges[k];
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

    /// xorshift64: the same traces every run.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    /// A random trace: an irregular clock, 1-bit signals and a 4-bit bus changing anywhere (on an
    /// edge, just before one, twice at one time), with x and z, and one that gets a value late.
    fn random_vcd(rng: &mut Rng) -> (String, Vec<Tick>) {
        let end = 120 + rng.next(80);
        let mut ev: Vec<(Tick, char, String)> = Vec::new();
        let (mut t, mut level, mut rises) = (rng.next(4), rng.next(2), Vec::new());
        ev.push((0, '!', level.to_string()));
        while t < end {
            t += 1 + rng.next(5);
            level ^= 1;
            ev.push((t, '!', level.to_string()));
            if level == 1 {
                rises.push(t);
            }
        }
        let bit = |rng: &mut Rng| ["0", "1", "1", "0", "x", "z"][rng.next(6) as usize].to_string();
        for id in ['a', 'b', 'c'] {
            ev.push((0, id, bit(rng)));
            for _ in 0..rng.next(40) {
                // a third of the changes on an edge or just before one
                let at = match rng.next(3) {
                    0 if !rises.is_empty() => rises[rng.next(rises.len() as u64) as usize] - rng.next(2),
                    _ => 1 + rng.next(end),
                };
                ev.push((at, id, bit(rng)));
            }
        }
        for _ in 0..rng.next(30) {
            let v = match rng.next(8) {
                0 => "bx1x0".to_string(),
                1 => "bz".to_string(),
                _ => format!("b{:b}", rng.next(16)),
            };
            ev.push((1 + rng.next(end), 'd', v));
        }
        ev.push((0, 'd', "b0".into()));
        let late = 20 + rng.next(60);
        ev.push((late, 'l', "1".into()));
        ev.push((late + 1 + rng.next(30), 'l', "0".into()));
        // in time order, a time's changes in the order made (two at a time: the last counts)
        ev.sort_by_key(|e| e.0);
        let mut vcd = String::from("$timescale 1ns $end\n$scope module top $end\n");
        for (id, w, name) in [('!', 1, "clk"), ('a', 1, "a"), ('b', 1, "b"), ('c', 1, "c"), ('d', 4, "d"), ('l', 1, "late")] {
            vcd += &format!("$var wire {w} {id} {name} $end\n");
        }
        vcd += "$upscope $end\n$enddefinitions $end\n";
        let mut at = None;
        for (t, id, v) in ev {
            if at != Some(t) {
                vcd += &format!("#{t}\n");
                at = Some(t);
            }
            vcd += &if v.starts_with('b') { format!("{v} {id}\n") } else { format!("{v}{id}\n") };
        }
        vcd += &format!("#{}\n", end + 10);
        (vcd, rises)
    }

    const EXPRS: &[&str] = &[
        "@clk a",
        "@clk !a",
        "@clk a && b",
        "@clk a || !c",
        "@clk d == 0x5",
        "@clk d != 3",
        "@clk d == x",
        "@clk d == z",
        "@clk d[1] && !a",
        "@clk d[2:1] == 2",
        "@clk past(a)",
        "@clk past(past(d)) == 2 || b",
        "@clk a && past(b) && past(past(c))",
        "@clk rose(a)",
        "@clk fell(b) && !past(c)",
        "@clk stable(d)",
        "@clk !stable(d) || late",
        "@clk stable(past(a))",
        "@clk late",
        "@clk !late",
        "@clk past(late) && !late",
        "@clk rose(d[0])",
        "@d[0] a || b",
        "@a !b",
    ];

    #[test]
    fn gallops_to_the_next_edge() {
        let edges: Vec<Tick> = vec![2, 3, 5, 9, 10, 17, 30, 31, 40];
        for from in 0..=edges.len() {
            for x in 0..45 {
                // from any edge at or before x's place, as the search keeps it
                if edges[..from].iter().all(|&e| e <= x) {
                    assert_eq!(gallop(&edges, from, x), edges.partition_point(|&e| e <= x), "from {from} x {x}");
                }
            }
        }
        assert_eq!(gallop(&[], 0, 5), 0);
    }

    /// The clocked search agrees with the reference on random traces, for every expression, from
    /// any time (on an edge, around one, before and after the trace), either way, with and without
    /// merging, under every limit.
    #[test]
    fn agrees_with_the_reference() {
        let dir = std::env::temp_dir().join(format!("wtf-query-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        let mut checked = 0;
        for seed in 0..30 {
            let (vcd, rises) = random_vcd(&mut rng);
            let path = dir.join(format!("{seed}.vcd"));
            std::fs::write(&path, &vcd).unwrap();
            let mut ws = Workspace::open(&path).unwrap();
            let end = ws.trace.end();
            let edge = |rng: &mut Rng| rises.get(rng.next(rises.len().max(1) as u64) as usize).copied().unwrap_or(0);
            for e in EXPRS {
                let q = ws.query(e).unwrap_or_else(|err| panic!("{e}: {err}"));
                let e1 = edge(&mut rng);
                let froms = [0, 1, end / 3, e1, e1.saturating_sub(1), e1 + 1, end, end + 1];
                for from in froms {
                    for to in [None, Some(edge(&mut rng)), Some(rng.next(end + 2))] {
                        for forward in [true, false] {
                            for merge in [true, false] {
                                for (runs, matches) in [(usize::MAX, u64::MAX), (1, u64::MAX), (2, u64::MAX), (usize::MAX, 1), (usize::MAX, 5), (3, 4)] {
                                    let s = Search { from, to, forward, merge, runs, matches };
                                    let want = reference(&ws.trace, &q, &s);
                                    assert_eq!(find(&ws.trace, &q, &s), want, "seed {seed}: {e} from {from} to {to:?} forward {forward} merge {merge} runs {runs} matches {matches}\n{vcd}");
                                    checked += want.total;
                                }
                            }
                        }
                    }
                }
            }
        }
        std::fs::remove_dir_all(&dir).ok();
        // the traces are busy enough to match a lot
        assert!(checked > 100_000, "{checked}");
    }
}
