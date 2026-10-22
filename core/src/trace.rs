//! Thin wrapper over wellen: format detection, hierarchy paths, value and transition lookup.
//! Also computed signals (spec §16): vars after the file's, answered from a list of
//! changes worked out once, so everything that works on a var works on them.

use crate::time::Tick;
use std::collections::HashMap;
use std::sync::Arc;
use wellen::{SignalRef, SignalValueRef, simple};

pub struct Var {
    pub path: String,
    /// Bit width; None for real and string variables.
    pub width: Option<u32>,
    /// The names the file gives values (an FST enum table): bits without leading zeros, name.
    pub names: Option<Names>,
    source: Source,
}

impl Var {
    /// The name of a value, if the file gives it one.
    pub fn name_of(&self, bits: &str) -> Option<&str> {
        let key = names_key(bits);
        self.names.as_ref()?.iter().find(|(b, _)| b == key).map(|(_, n)| n.as_str())
    }
}

/// The names a file gives a signal's values: bits without leading zeros, name.
pub type Names = Arc<[(String, String)]>;

/// Bits as enum tables are kept: without leading zeros, `0` for zero.
pub fn names_key(bits: &str) -> &str {
    match bits.trim_start_matches('0') {
        "" => "0",
        b => b,
    }
}

enum Source {
    File(SignalRef),
    /// Values in time order, each different from the one before; the first is the initial value,
    /// not a change (as a file's values at its first time are).
    Computed(Vec<(Tick, Val)>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Val {
    Bits(String),
    Text(String),
}

/// How many distinct change times fall in a tick range.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Changes {
    None,
    One,
    Many,
}

/// A clock's rising edges, shared by the searches that walk them.
pub(crate) type Edges = std::sync::Arc<Vec<Tick>>;

pub struct Trace {
    wave: simple::Waveform,
    pub fs_per_tick: Option<u128>,
    pub vars: Vec<Var>,
    pub by_path: HashMap<String, usize>,
    /// Clocks' rising edges by (var, bit), worked out once (`query::edges`): every clocked search
    /// walks them.
    pub(crate) edges: std::cell::RefCell<HashMap<(usize, Option<u32>), Edges>>,
}

impl Trace {
    /// wellen detects VCD / FST from the content, not the extension (spec §13).
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        // wellen panics on a missing file, so check first
        std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let wave = simple::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let h = wave.hierarchy();
        let fs_per_tick = crate::time::fs_per_tick(h.timescale());
        let vars: Vec<Var> = h
            .all_vars()
            .map(|r| &h[r])
            .map(|v| Var {
                path: v.full_name(h),
                width: v.signal_encoding(h).length(),
                names: v.enum_type(h).map(|(_, m)| m.iter().map(|(b, n)| (names_key(b).to_string(), n.to_string())).collect()),
                source: Source::File(v.signal_ref()),
            })
            .collect();
        let by_path = vars.iter().enumerate().map(|(i, v)| (v.path.clone(), i)).collect();
        Ok(Trace { wave, fs_per_tick, vars, by_path, edges: Default::default() })
    }

    pub fn end(&self) -> Tick {
        self.wave.time_table().last().copied().unwrap_or(0)
    }

    pub fn start(&self) -> Tick {
        self.wave.time_table().first().copied().unwrap_or(0)
    }

    /// Read the values of these variables from the file (lazy loading, spec §18). Computed ones
    /// have theirs already.
    pub fn load(&mut self, vars: &[usize]) {
        let refs: Vec<_> = vars
            .iter()
            .filter_map(|&v| match self.vars[v].source {
                Source::File(s) => Some(s),
                Source::Computed(_) => None,
            })
            .filter(|s| self.wave.get_signal(*s).is_none())
            .collect();
        if !refs.is_empty() {
            self.wave.load_signals(&refs);
        }
    }

    /// Add a computed signal from its values in time order (`changes`, times may repeat: the last
    /// value at a time counts), or replace the one with that path. Returns its var.
    pub fn add_computed(&mut self, path: &str, width: Option<u32>, changes: Vec<(Tick, Val)>) -> Result<usize, String> {
        let mut list: Vec<(Tick, Val)> = Vec::with_capacity(changes.len());
        for (t, v) in changes {
            match list.last_mut() {
                Some(last) if last.0 == t => last.1 = v,
                Some(last) if last.0 > t => return Err(format!("{path}: values out of time order")),
                _ => list.push((t, v)),
            }
            // an equal value is no change; done after a same-time replace, which can make one
            if list.len() > 1 && list[list.len() - 1].1 == list[list.len() - 2].1 {
                list.pop();
            }
        }
        let var = Var { path: path.to_string(), width, names: None, source: Source::Computed(list) };
        match self.by_path.get(path) {
            Some(&i) if matches!(self.vars[i].source, Source::Computed(_)) => {
                self.vars[i] = var;
                // its edges, if it was a clock, are another signal's now
                self.edges.get_mut().retain(|k, _| k.0 != i);
                Ok(i)
            }
            Some(_) => Err(format!("{path} is a signal of the file")),
            None => {
                self.vars.push(var);
                self.by_path.insert(path.to_string(), self.vars.len() - 1);
                Ok(self.vars.len() - 1)
            }
        }
    }

    /// When the var first has a value.
    pub fn first_time(&self, var: usize) -> Option<Tick> {
        match &self.vars[var].source {
            Source::Computed(list) => list.first().map(|x| x.0),
            Source::File(s) => self.wave.get_signal(*s)?.time_indices().first().map(|&i| self.wave.time_table()[i as usize]),
        }
    }

    pub fn is_computed(&self, var: usize) -> bool {
        self.computed(var).is_some()
    }

    /// The values of a computed var, if it is one.
    fn computed(&self, var: usize) -> Option<&[(Tick, Val)]> {
        match &self.vars[var].source {
            Source::Computed(list) => Some(list),
            Source::File(_) => None,
        }
    }

    /// Time-table indices of the var's entries, and the position of the first real change
    /// (entries at the first time are the initial value, not a transition).
    fn indices(&self, var: usize) -> (&[u32], usize) {
        let ti = match self.vars[var].source {
            Source::File(s) => self.wave.get_signal(s).map_or(&[][..], |s| s.time_indices()),
            Source::Computed(_) => &[],
        };
        let first = ti.first().map_or(0, |&f| ti.partition_point(|&x| x <= f));
        (ti, first)
    }

    fn tt_pos(&self, t: Tick, inclusive: bool) -> u32 {
        let tt = self.wave.time_table();
        (if inclusive { tt.partition_point(|&x| x <= t) } else { tt.partition_point(|&x| x < t) }) as u32
    }

    pub fn value_at(&self, var: usize, t: Tick) -> Option<Val> {
        if let Some(list) = self.computed(var) {
            let i = list.partition_point(|x| x.0 <= t);
            return i.checked_sub(1).map(|i| list[i].1.clone());
        }
        let Source::File(s) = self.vars[var].source else { return None };
        let sig = self.wave.get_signal(s)?;
        let idx = self.tt_pos(t, true).checked_sub(1)?;
        let off = sig.get_offset(idx)?;
        // the last element at a time is the settled value after delta cycles
        Some(val(sig.get_value_at(&off, off.elements - 1)))
    }

    /// Changes in [t0, t1). Two binary searches, independent of the number of transitions.
    pub fn changes_in(&self, var: usize, t0: Tick, t1: Tick) -> Changes {
        if let Some(list) = self.computed(var) {
            let changes = list.get(1..).unwrap_or(&[]);
            let (a, b) = (changes.partition_point(|x| x.0 < t0), changes.partition_point(|x| x.0 < t1));
            return match b.saturating_sub(a) {
                0 => Changes::None,
                1 => Changes::One,
                _ => Changes::Many,
            };
        }
        let (ti, first) = self.indices(var);
        let (i0, i1) = (self.tt_pos(t0, false), self.tt_pos(t1, false));
        let a = ti.partition_point(|&x| x < i0).max(first);
        let b = ti.partition_point(|&x| x < i1);
        if a >= b {
            Changes::None
        } else if ti[a] == ti[b - 1] {
            Changes::One
        } else {
            Changes::Many
        }
    }

    pub fn next_change(&self, var: usize, t: Tick) -> Option<Tick> {
        if let Some(list) = self.computed(var) {
            let changes = list.get(1..).unwrap_or(&[]);
            return changes.get(changes.partition_point(|x| x.0 <= t)).map(|x| x.0);
        }
        let (ti, first) = self.indices(var);
        let i = self.tt_pos(t, true);
        let pos = ti.partition_point(|&x| x < i).max(first);
        ti.get(pos).map(|&x| self.wave.time_table()[x as usize])
    }

    pub fn prev_change(&self, var: usize, t: Tick) -> Option<Tick> {
        if let Some(list) = self.computed(var) {
            let changes = list.get(1..).unwrap_or(&[]);
            return changes.partition_point(|x| x.0 < t).checked_sub(1).map(|i| changes[i].0);
        }
        let (ti, first) = self.indices(var);
        let i = self.tt_pos(t, false);
        let pos = ti.partition_point(|&x| x < i);
        (pos > first).then(|| self.wave.time_table()[ti[pos - 1] as usize])
    }

    /// The var's changes after `t` (backward: before it), nearest first, each with its value: one
    /// search to start, then a step per change, where `next_change` and `value_at` search each
    /// time. For whatever walks a signal (searches, clock edges, decoders).
    fn walk_var(&self, var: usize, t: Tick, forward: bool) -> Box<dyn Iterator<Item = (Tick, Val)> + '_> {
        if let Some(list) = self.computed(var) {
            let changes = list.get(1..).unwrap_or(&[]);
            return if forward {
                Box::new(changes[changes.partition_point(|x| x.0 <= t)..].iter().cloned())
            } else {
                Box::new(changes[..changes.partition_point(|x| x.0 < t)].iter().rev().cloned())
            };
        }
        let (ti, first) = self.indices(var);
        let Some(sig) = (match self.vars[var].source {
            Source::File(s) => self.wave.get_signal(s),
            Source::Computed(_) => None,
        }) else {
            return Box::new(std::iter::empty());
        };
        let (tt, data) = (self.wave.time_table(), sig.data());
        // entries at one time are delta cycles: the last is the settled value
        if forward {
            let mut pos = ti.partition_point(|&x| x < self.tt_pos(t, true)).max(first);
            Box::new(std::iter::from_fn(move || {
                let g = *ti.get(pos)?;
                pos += ti[pos..].iter().take_while(|&&x| x == g).count();
                Some((tt[g as usize], val(data.get_value_at(pos - 1))))
            }))
        } else {
            let mut pos = ti.partition_point(|&x| x < self.tt_pos(t, false));
            Box::new(std::iter::from_fn(move || {
                if pos <= first {
                    return None;
                }
                let (g, v) = (ti[pos - 1], val(data.get_value_at(pos - 1)));
                pos -= ti[first..pos].iter().rev().take_while(|&&x| x == g).count();
                Some((tt[g as usize], v))
            }))
        }
    }
}

fn val(v: SignalValueRef) -> Val {
    match v.to_bit_string() {
        Some(b) => Val::Bits(b),
        None => match v {
            SignalValueRef::String(s) => Val::Text(s.to_string()),
            SignalValueRef::Real(r) => Val::Text(r.to_string()),
            _ => Val::Text(String::new()),
        },
    }
}

/// Bit `bit` of a value (0 the LSB), or the value itself.
fn bit_of(v: Val, bit: Option<u32>) -> Option<Val> {
    match (bit, v) {
        (None, v) => Some(v),
        (Some(b), Val::Bits(s)) => s.chars().rev().nth(b as usize).map(|c| Val::Bits(c.to_string())),
        (Some(_), Val::Text(_)) => None,
    }
}

// One bit of a bus, for buses expanded into their bits; `bit` None is the whole variable.
// shortcut: a bit's changes are found by checking each change of its bus, so a bit of a busy bus
// costs time proportional to the bus's changes in view; index bits if that is ever too slow.
impl Trace {
    pub fn value_of(&self, var: usize, bit: Option<u32>, t: Tick) -> Option<Val> {
        bit_of(self.value_at(var, t)?, bit)
    }

    /// The changes of a var or one of its bits after `t` (backward: before it), nearest first,
    /// each with the value it changes to; a bit's are the bus changes that change it.
    pub fn walk(&self, var: usize, bit: Option<u32>, t: Tick, forward: bool) -> Box<dyn Iterator<Item = (Tick, Option<Val>)> + '_> {
        let changes = self.walk_var(var, t, forward).map(move |(x, v)| (x, bit_of(v, bit)));
        if bit.is_none() {
            return Box::new(changes);
        }
        // a bus change changes the bit if the bit differs from just before: forward, the last
        // change's; backward, the next one's; at the end of the walk, looked up
        let before = move |x: Tick| x.checked_sub(1).and_then(|p| self.value_of(var, bit, p));
        if forward {
            let mut last = None;
            Box::new(changes.filter(move |(x, v)| {
                let changed = *v != last.take().unwrap_or_else(|| before(*x));
                last = Some(v.clone());
                changed
            }))
        } else {
            let mut changes = changes.peekable();
            Box::new(std::iter::from_fn(move || {
                loop {
                    let (x, v) = changes.next()?;
                    let prev = match changes.peek() {
                        Some(c) => c.1 != v,
                        None => before(x) != v,
                    };
                    if prev {
                        return Some((x, v));
                    }
                }
            }))
        }
    }

    /// Changes of every bit of a bus in each tick range, as `[bit][range]` with bit 0 the LSB.
    /// One pass over the bus's changes for all bits, stopping in a range once every bit is busy.
    pub fn bit_changes(&self, var: usize, ranges: &[(Tick, Tick)]) -> Vec<Vec<Changes>> {
        let w = self.vars[var].width.unwrap_or(0) as usize;
        let mut out = vec![vec![Changes::None; ranges.len()]; w];
        let bits = |t| match self.value_at(var, t) {
            Some(Val::Bits(b)) if b.len() == w => b.into_bytes(),
            _ => vec![0; w],
        };
        for (c, &(a, b)) in ranges.iter().enumerate() {
            if self.changes_in(var, a, b) == Changes::None {
                continue;
            }
            let mut prev = None;
            let mut busy = 0;
            let mut t = a.saturating_sub(1);
            while busy < w
                && let Some(x) = self.next_change(var, t).filter(|&x| x < b)
            {
                let old = prev.get_or_insert_with(|| bits(x - 1));
                let new = bits(x);
                for k in (0..w).filter(|&k| old[k] != new[k]) {
                    let ch = &mut out[w - 1 - k][c];
                    *ch = match ch {
                        Changes::None => Changes::One,
                        _ => {
                            busy += (*ch == Changes::One) as usize;
                            Changes::Many
                        }
                    };
                }
                *old = new;
                t = x;
            }
        }
        out
    }

    // a whole var's change needs no value, which a walk makes
    pub fn next_change_of(&self, var: usize, bit: Option<u32>, t: Tick) -> Option<Tick> {
        if bit.is_none() { self.next_change(var, t) } else { self.walk(var, bit, t, true).next().map(|c| c.0) }
    }

    pub fn prev_change_of(&self, var: usize, bit: Option<u32>, t: Tick) -> Option<Tick> {
        if bit.is_none() { self.prev_change(var, t) } else { self.walk(var, bit, t, false).next().map(|c| c.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(ext: &str) -> Trace {
        let mut t = Trace::open(format!("{}/../tests/data/simple.{ext}", env!("CARGO_MANIFEST_DIR")).as_ref()).unwrap();
        let all: Vec<usize> = (0..t.vars.len()).collect();
        t.load(&all);
        t
    }

    fn bits(t: &Trace, path: &str, at: Tick) -> String {
        match t.value_at(t.by_path[path], at) {
            Some(Val::Bits(b)) => b,
            _ => "none".into(),
        }
    }

    #[test]
    fn computed_vars() {
        let mut t = open("vcd");
        let b = |s: &str| Val::Bits(s.into());
        // repeats and same-time values fold away; the first value is not a change
        let v = t.add_computed("calc.x", Some(2), vec![(0, b("00")), (10, b("01")), (10, b("10")), (20, b("10")), (30, b("11"))]).unwrap();
        assert_eq!(v, t.vars.len() - 1);
        assert_eq!(t.by_path["calc.x"], v);
        assert_eq!((t.value_at(v, 0), t.value_at(v, 9), t.value_at(v, 10), t.value_at(v, 99)), (Some(b("00")), Some(b("00")), Some(b("10")), Some(b("11"))));
        assert_eq!((t.next_change(v, 0), t.next_change(v, 10), t.next_change(v, 30)), (Some(10), Some(30), None));
        assert_eq!((t.prev_change(v, 30), t.prev_change(v, 11), t.prev_change(v, 10)), (Some(10), Some(10), None));
        assert_eq!((t.changes_in(v, 0, 10), t.changes_in(v, 0, 11), t.changes_in(v, 5, 31)), (Changes::None, Changes::One, Changes::Many));
        // bits of it, as of any bus
        assert_eq!(t.next_change_of(v, Some(0), 0), Some(30));
        assert_eq!(t.bit_changes(v, &[(0, 15), (15, 40)]), [[Changes::None, Changes::One], [Changes::One, Changes::None]]);
        // values before the first: none
        let late = t.add_computed("calc.late", Some(1), vec![(40, b("1"))]).unwrap();
        assert_eq!((t.value_at(late, 39), t.next_change(late, 0)), (None, None));
        // the same path replaces it in place; a file signal's path can't be taken
        assert_eq!(t.add_computed("calc.x", Some(1), vec![(0, b("1"))]), Ok(v));
        assert_eq!(t.value_at(v, 50), Some(b("1")));
        assert_eq!(t.add_computed("top.clk", Some(1), vec![]), Err("top.clk is a signal of the file".into()));
        assert_eq!(t.add_computed("calc.y", Some(1), vec![(5, b("1")), (2, b("0"))]), Err("calc.y: values out of time order".into()));
        t.load(&[v, late]);
    }

    #[test]
    fn vcd_and_fst_agree() {
        let (v, f) = (open("vcd"), open("fst"));
        assert_eq!(v.fs_per_tick, Some(1_000_000));
        assert_eq!(v.fs_per_tick, f.fs_per_tick);
        assert_eq!((v.start(), v.end()), (f.start(), f.end()));
        let paths: Vec<_> = v.vars.iter().map(|x| &x.path).collect();
        assert_eq!(paths, f.vars.iter().map(|x| &x.path).collect::<Vec<_>>());
        for var in &v.vars {
            let p = &var.path;
            for t in 0..=v.end() + 1 {
                assert_eq!(bits(&v, p, t), bits(&f, p, t), "{p} @ {t}");
                assert_eq!(v.next_change(v.by_path[p], t), f.next_change(f.by_path[p], t), "{p} @ {t}");
            }
        }
    }

    /// `walk` against the changes read tick by tick: every var and bit, both ways, from every tick.
    #[test]
    fn walks() {
        for ext in ["vcd", "fst"] {
            let mut t = open(ext);
            let b = |s: &str| Val::Bits(s.into());
            t.add_computed("calc.x", Some(2), vec![(0, b("00")), (10, b("01")), (30, b("11"))]).unwrap();
            let end = t.end() + 2;
            for var in 0..t.vars.len() {
                let bits = (0..t.vars[var].width.filter(|&w| w > 1).unwrap_or(0)).map(Some);
                for bit in std::iter::once(None).chain(bits) {
                    let v = |x: Tick| t.value_of(var, bit, x);
                    let changes: Vec<(Tick, Option<Val>)> = (1..end).filter(|&x| v(x) != v(x - 1)).map(|x| (x, v(x))).collect();
                    for from in 0..end {
                        let fwd: Vec<_> = changes.iter().filter(|c| c.0 > from).cloned().collect();
                        let back: Vec<_> = changes.iter().rev().filter(|c| c.0 < from).cloned().collect();
                        let at = format!("{ext} {} {bit:?} from {from}", t.vars[var].path);
                        assert_eq!(t.walk(var, bit, from, true).collect::<Vec<_>>(), fwd, "{at}");
                        assert_eq!(t.walk(var, bit, from, false).collect::<Vec<_>>(), back, "{at}");
                    }
                }
            }
        }
    }

    #[test]
    fn lookups() {
        let t = open("vcd");
        let d = t.by_path["top.cpu.data"];
        assert_eq!(bits(&t, "top.cpu.data", 0), "xxxxxxxx");
        assert_eq!(bits(&t, "top.cpu.data", 12), "01000011");
        assert_eq!(bits(&t, "top.cpu.state", 39), "010");
        assert_eq!(t.next_change(d, 0), Some(10));
        assert_eq!(t.next_change(d, 10), Some(20));
        assert_eq!(t.prev_change(d, 20), Some(10));
        assert_eq!(t.prev_change(d, 10), None); // time 0 is the initial value
        assert_eq!(t.changes_in(d, 0, 10), Changes::None);
        assert_eq!(t.changes_in(d, 10, 11), Changes::One);
        assert_eq!(t.changes_in(d, 40, 45), Changes::Many);
        assert_eq!(t.end(), 80);
    }

    #[test]
    fn single_bits() {
        let t = open("vcd");
        let d = t.by_path["top.cpu.data"];
        // data: 0x43 (01000011) at 10, 0x96 (10010110) at 20
        assert_eq!(t.value_of(d, Some(0), 12), Some(Val::Bits("1".into())));
        assert_eq!(t.value_of(d, Some(7), 12), Some(Val::Bits("0".into())));
        assert_eq!(t.value_of(d, Some(1), 20), Some(Val::Bits("1".into())));
        // bit 1 stays 1 from 0x43 to 0x96: no change at 20; 40..45 holds several values
        let c = t.bit_changes(d, &[(20, 21), (40, 45)]);
        assert_eq!((c[1][0], c[0][0]), (Changes::None, Changes::One));
        // 0xE1 -> 0x3A 0x3B 0x3C 0x3D 0x7F: bit 6 falls and rises, bit 7 only falls, bit 5 stays 1
        assert_eq!((c[0][1], c[6][1], c[7][1], c[5][1]), (Changes::Many, Changes::Many, Changes::One, Changes::None));
        assert!(t.next_change_of(d, Some(1), 10).is_none_or(|x| x > 20));
        assert_eq!(t.next_change_of(d, Some(0), 10), Some(20));
        assert_eq!(t.prev_change_of(d, Some(0), 20), Some(10));
    }
}
