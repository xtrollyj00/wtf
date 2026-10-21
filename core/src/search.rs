//! Value search (spec §8): what a search text means, and where a signal changes to it.
//! Also expressions over several signals (`valid && ready && data == 0x40`), optionally sampled
//! at the rising edges of a clock (`@clk ...`), and over time: `past(x)` is x's value one sample
//! earlier (a clock cycle with `@clk`, else one tick), `rose(x)`, `fell(x)` and `stable(x)` compare
//! x with it; `rose` and `fell` only from a known value (not U -> 0).

use crate::radix::RADIXES;
use crate::time::Tick;
use crate::trace::{Names, Trace, Val};

#[derive(Debug, PartialEq)]
pub enum Target {
    /// Any unknown or uninitialized bit.
    Unknown,
    /// Any high-impedance bit.
    HighZ,
    /// Exactly these bits, MSB first, as wide as the signal.
    Bits(String),
    /// A string or real value, compared as text.
    Text(String),
}

/// Matching is by number in any radix (`/0x40`, `/64`, `/0b1000000`), or by a name the file gives
/// the value (`names`, any case); `width` None is a string or real signal, searched as text.
pub fn parse(text: &str, width: Option<u32>, names: Option<&Names>) -> Result<Target, String> {
    let t = text.trim().to_lowercase();
    match t.as_str() {
        "" => return Err("empty search".into()),
        "x" | "u" => return Ok(Target::Unknown),
        "z" => return Ok(Target::HighZ),
        _ => {}
    }
    let Some(w) = width else { return Ok(Target::Text(text.trim().into())) };
    if let Some((bits, _)) = names.and_then(|n| n.iter().find(|(_, name)| name.eq_ignore_ascii_case(text.trim()))) {
        return Ok(Target::Bits(format!("{bits:0>w$}", w = w as usize)));
    }
    let bits = RADIXES.iter().filter_map(|r| r.parse).find_map(|p| p(&t, w)).ok_or(format!("not a value: {text}"))?;
    let bits = bits.trim_start_matches('0');
    if bits.len() > w as usize {
        return Err(format!("{} doesn't fit in {w} bits", text.trim()));
    }
    Ok(Target::Bits(format!("{bits:0>w$}", w = w as usize)))
}

/// A signal, or one bit of a bus.
pub type Probe = (usize, Option<u32>);

#[derive(Debug)]
pub enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    /// `sig == value`, `sig != value`, or a bare `sig` (true when it is 1 / nonzero); its value
    /// `past` samples earlier (`past(sig)`).
    Cmp { sig: Probe, past: u32, eq: bool, target: Option<Target> },
    /// `stable(sig)`: the same value as one sample earlier.
    Stable { sig: Probe, past: u32 },
}

pub struct Query {
    /// `@clk`: matches are rising edges of this signal where the expression holds.
    pub clock: Option<Probe>,
    pub expr: Expr,
}

/// Looks like an expression rather than a bare value for the selected signal.
pub fn is_expr(text: &str) -> bool {
    text.contains(['=', '!', '&', '|', '(', '@'])
}

/// A parsed operand: its name, probe, width, value names and how many samples back.
type Operand = (String, Probe, Option<u32>, Option<Names>, u32);

/// Names a signal: its probe, its width (None: real or string) and the names the file gives its
/// values. It may make computed signals for names like `data[7:4]`, so it is `FnMut`.
pub type Resolve<'a> = dyn FnMut(&str) -> Result<(Probe, Option<u32>, Option<Names>), String> + 'a;

/// Parse `[@clk] expr`. A value with a time unit (`delay == 40ns`) is that many ticks, by the
/// trace's femtoseconds per tick `fpt`.
pub fn parse_query(text: &str, fpt: Option<u128>, resolve: &mut Resolve) -> Result<Query, String> {
    let mut p = Parser { s: text.trim(), fpt, resolve };
    let clock = if p.eat("@") {
        let w = p.word();
        Some((p.resolve)(&w)?.0)
    } else {
        None
    };
    let expr = p.or()?;
    if !p.s.trim().is_empty() {
        return Err(format!("unexpected: {}", p.s.trim()));
    }
    Ok(Query { clock, expr })
}

struct Parser<'a> {
    s: &'a str,
    fpt: Option<u128>,
    resolve: &'a mut Resolve<'a>,
}

impl Parser<'_> {
    fn eat(&mut self, tok: &str) -> bool {
        self.s = self.s.trim_start();
        let found = self.s.starts_with(tok);
        if found {
            self.s = &self.s[tok.len()..];
        }
        found
    }

    /// A name or a value: everything up to a space, a parenthesis or an operator, except inside
    /// braces (`{a, b}` is one name).
    fn word(&mut self) -> String {
        self.s = self.s.trim_start();
        let mut depth = 0;
        let end = self
            .s
            .find(|c: char| {
                depth += (c == '{') as i32 - (c == '}') as i32;
                depth == 0 && (c.is_whitespace() || "()&|!=".contains(c))
            })
            .unwrap_or(self.s.len());
        let (w, rest) = self.s.split_at(end);
        self.s = rest;
        w.to_string()
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut e = self.and()?;
        while self.eat("||") {
            e = Expr::Or(Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        while self.eat("&&") {
            e = Expr::And(Box::new(e), Box::new(self.unary()?));
        }
        Ok(e)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.s.trim_start().starts_with("!=") {
            return Err("missing signal before !=".into());
        }
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat("(") {
            let e = self.or()?;
            return if self.eat(")") { Ok(e) } else { Err("missing )".into()) };
        }
        // `rose(`, `fell(`, `stable(`; otherwise the word is read again as an operand
        let start = self.s;
        let f = self.word();
        if matches!(f.as_str(), "rose" | "fell" | "stable") && self.eat("(") {
            let (name, sig, width, _, past) = self.operand()?;
            if !self.eat(")") {
                return Err("missing )".into());
            }
            let cmp = |past, eq, target| Expr::Cmp { sig, past, eq, target };
            // from a known value only: a valid going from U to 0 in reset didn't fall
            let zero = || width.map(|w| Some(Target::Bits("0".repeat(w as usize)))).ok_or(format!("{f}({name}): not a bit signal"));
            return Ok(match f.as_str() {
                "rose" => Expr::And(Box::new(cmp(past, true, None)), Box::new(cmp(past + 1, true, zero()?))),
                "fell" => Expr::And(Box::new(cmp(past, true, zero()?)), Box::new(cmp(past + 1, true, None))),
                _ => Expr::Stable { sig, past },
            });
        }
        self.s = start;
        let (name, sig, width, names, past) = self.operand()?;
        let eq = if self.eat("==") {
            true
        } else if self.eat("!=") {
            false
        } else {
            return Ok(Expr::Cmp { sig, past, eq: true, target: None });
        };
        let mut value = self.word();
        if crate::time::has_unit(&value) {
            value = (crate::time::parse(&value, self.fpt).map_err(|e| format!("{name}: {e}"))?.round() as u64).to_string();
        }
        let target = parse(&value, width, names.as_ref()).map_err(|e| format!("{name}: {e}"))?;
        Ok(Expr::Cmp { sig, past, eq, target: Some(target) })
    }

    /// A signal, or `past(...)` of one: its name, probe, width, value names and how many samples
    /// back.
    fn operand(&mut self) -> Result<Operand, String> {
        let name = self.word();
        if name == "past" && self.eat("(") {
            let (name, sig, width, names, past) = self.operand()?;
            return if self.eat(")") { Ok((format!("past({name})"), sig, width, names, past + 1)) } else { Err("missing )".into()) };
        }
        if name.is_empty() {
            return Err(format!("expected a signal at: {}", self.s.trim()));
        }
        let (sig, width, names) = (self.resolve)(&name)?;
        Ok((name, sig, width, names, 0))
    }
}

/// The sample before the one at `t`, if any.
type Prev<'a> = &'a dyn Fn(Tick) -> Option<Tick>;

/// The sample `n` samples before `t`.
fn back(prev: Prev, t: Tick, n: u32) -> Option<Tick> {
    (0..n).try_fold(t, |t, _| prev(t))
}

fn eval(trace: &Trace, e: &Expr, t: Tick, prev: Prev) -> bool {
    holds(e, &|sig, n| back(prev, t, n).map(|t| trace.value_of(sig.0, sig.1, t)))
}

/// Whether `e` holds, given `value(sig, n)`: sig's value `n` samples back, None where there is no
/// such sample (and Some(None) where the signal has no value there). Searches sample a trace;
/// `records` `where` samples a stream's records, a field per probe.
pub fn holds<V: std::borrow::Borrow<Val>>(e: &Expr, value: &dyn Fn(Probe, u32) -> Option<Option<V>>) -> bool {
    match e {
        Expr::Or(a, b) => holds(a, value) || holds(b, value),
        Expr::And(a, b) => holds(a, value) && holds(b, value),
        Expr::Not(a) => !holds(a, value),
        Expr::Stable { sig, past } => match (value(*sig, *past), value(*sig, past + 1)) {
            (Some(a), Some(b)) => a.as_ref().map(|v| v.borrow()) == b.as_ref().map(|v| v.borrow()),
            _ => false,
        },
        Expr::Cmp { sig, past, eq, target } => {
            let v = value(*sig, *past).flatten();
            let v = v.as_ref().map(|v| v.borrow());
            let hit = match target {
                Some(target) => matches(target, v),
                // as in an `if`: some bit is 1 and none is unknown
                None => matches!(&v, Some(Val::Bits(b)) if b.contains(['1', 'h']) && !b.contains(['x', 'u', 'w', '-', 'z'])),
            };
            hit == *eq
        }
    }
}

/// The signals an expression reads, each with how many samples back (the most of `stable`'s two).
pub fn probes(e: &Expr, out: &mut Vec<(Probe, u32)>) {
    match e {
        Expr::Or(a, b) | Expr::And(a, b) => {
            probes(a, out);
            probes(b, out);
        }
        Expr::Not(a) => probes(a, out),
        Expr::Cmp { sig, past, .. } => out.push((*sig, *past)),
        Expr::Stable { sig, past } => out.extend([(*sig, *past), (*sig, past + 1)]),
    }
}

/// The next (previous) match after (before) `from`: where the expression becomes true. With a
/// clock, the start of each clock cycle (a rising edge) whose ending edge samples it true, as a
/// flip-flop does just before the edge: the cursor lands where the transfer's cycle begins.
pub fn find_query(trace: &Trace, q: &Query, from: Tick, forward: bool) -> Option<Tick> {
    let rise = Target::Bits("1".into());
    let edge = |t, forward| q.clock.and_then(|(cv, cb)| find(trace, cv, cb, &rise, t, forward));
    // with a clock, a sample is just before an edge, and the one before it just before the edge
    // before; without, the tick before
    let prev = |t: Tick| if q.clock.is_some() { edge(t + 1, false).and_then(|e| e.checked_sub(1)) } else { t.checked_sub(1) };
    let held = |t: Tick| t.checked_sub(1).is_some_and(|p| eval(trace, &q.expr, p, &prev));
    let mut t = from;
    if q.clock.is_some() {
        let mut start = edge(from, forward)?;
        loop {
            if edge(start, true).is_some_and(held) {
                return Some(start);
            }
            start = edge(start, forward)?;
        }
    }
    let mut sigs = Vec::new();
    probes(&q.expr, &mut sigs);
    loop {
        // `past(x)` changes `n` ticks after x does
        let next = sigs.iter().filter_map(|&((v, b), n)| {
            let n = n as Tick;
            if forward { trace.next_change_of(v, b, t.saturating_sub(n)).map(|c| c + n) } else { trace.prev_change_of(v, b, t.checked_sub(n)?).map(|c| c + n) }
        });
        t = if forward { next.min()? } else { next.max()? };
        if eval(trace, &q.expr, t, &prev) && !held(t) {
            return Some(t);
        }
    }
}

fn matches(target: &Target, v: Option<&Val>) -> bool {
    match (target, v) {
        (Target::Unknown, Some(Val::Bits(b))) => b.contains(['x', 'u', 'w', '-']),
        (Target::HighZ, Some(Val::Bits(b))) => b.contains('z'),
        (Target::Bits(p), Some(Val::Bits(b))) => {
            b.chars().map(|c| match c {
                'h' => '1',
                'l' => '0',
                c => c,
            })
            .eq(p.chars())
        }
        (Target::Text(p), Some(Val::Text(t))) => p == t,
        _ => false,
    }
}

/// The next (or previous) time after (before) `from` where the signal changes to the target, so
/// a match is the start of a matching segment.
// shortcut: walks the changes one by one; index values if searching huge traces is too slow (milestone 6).
pub fn find(trace: &Trace, var: usize, bit: Option<u32>, target: &Target, from: Tick, forward: bool) -> Option<Tick> {
    let mut t = from;
    loop {
        t = if forward { trace.next_change_of(var, bit, t)? } else { trace.prev_change_of(var, bit, t)? };
        let before = t.checked_sub(1).is_some_and(|p| matches(target, trace.value_of(var, bit, p).as_ref()));
        if !before && matches(target, trace.value_of(var, bit, t).as_ref()) {
            return Some(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        let bits = |s: &str| Ok(Target::Bits(s.into()));
        assert_eq!(parse("0x40", Some(8), None), bits("01000000"));
        assert_eq!(parse("64", Some(8), None), bits("01000000"));
        assert_eq!(parse(" 0B1000000 ", Some(8), None), bits("01000000"));
        assert_eq!(parse("-12", Some(8), None), bits("11110100"));
        assert_eq!(parse("1", Some(1), None), bits("1"));
        assert_eq!(parse("X", Some(8), None), Ok(Target::Unknown));
        assert_eq!(parse("0x100", Some(8), None), Err("0x100 doesn't fit in 8 bits".into()));
        assert_eq!(parse("-129", Some(8), None), Err("-129 doesn't fit in 8 bits".into()));
        assert_eq!(parse("abc", Some(8), None), Err("not a value: abc".into()));
    }

    #[test]
    fn finds() {
        let mut t = Trace::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/data/simple.vcd").as_ref()).unwrap();
        let (d, clk) = (t.by_path["top.cpu.data"], t.by_path["top.clk"]);
        t.load(&[d, clk]);
        let find = |var, target: &str, from, fwd| find(&t, var, None, &parse(target, t.vars[var].width, None).unwrap(), from, fwd);
        // data: x, 0x43 @10, 0x96 @20, 0xE1 @30, 0x3A @40 ..., 0x7F @44, z @50, 0x40 @60
        assert_eq!(find(d, "0x96", 0, true), Some(20));
        assert_eq!(find(d, "150", 25, true), None); // no wrap-around
        assert_eq!(find(d, "z", 0, true), Some(50));
        assert_eq!(find(d, "x", 5, true), None); // x only at time 0, which is no change
        assert_eq!(find(d, "0x43", 70, false), Some(10));
        // 1-bit: /1 is a rising edge
        assert_eq!(find(clk, "1", 5, true), Some(15)); // clk rises at 5, 15, 25 ...
        assert_eq!(find(clk, "0", 10, true), Some(20));
    }

    #[test]
    fn expressions() {
        let mut t = Trace::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/data/simple.vcd").as_ref()).unwrap();
        t.load(&(0..t.vars.len()).collect::<Vec<_>>());
        let resolve = |n: &str| t.by_path.get(&format!("top.{n}")).map(|&v| ((v, None), t.vars[v].width, None)).ok_or(format!("unknown signal: {n}"));
        let q = |s: &str| parse_query(s, None, &mut resolve.clone()).unwrap();
        let find = |s: &str, from, fwd| find_query(&t, &q(s), from, fwd);
        // busy is 1 from 20 to 40; data 0x96 @20, 0xE1 @30, z @50; clk rises at 5, 15, 25, 35, 45, ...
        assert_eq!(find("cpu.busy && cpu.data == 0x96", 0, true), Some(20));
        assert_eq!(find("cpu.busy && !(cpu.data == z)", 0, true), Some(20));
        assert_eq!(find("cpu.busy && cpu.data != 0x96", 0, true), Some(30));
        // a match is where it becomes true: still true when 0x96 turns into 0xE1 at 30
        assert_eq!(find("cpu.data == 0x96 || cpu.data == 0xe1", 0, true), Some(20));
        assert_eq!(find("cpu.data == 0x96 || cpu.data == 0xe1", 20, true), None);
        assert_eq!(find("cpu.busy", 70, false), Some(20));
        // with a clock: the cycles whose ending edge samples busy (edges 25 and 35), by their start
        assert_eq!(find("@clk cpu.busy", 0, true), Some(15));
        assert_eq!(find("@clk cpu.busy", 15, true), Some(25));
        assert_eq!(find("@clk cpu.busy", 25, true), None);
        assert_eq!(find("@clk cpu.busy", 70, false), Some(25));
        assert_eq!(find("@clk cpu.busy", 25, false), Some(15));
        let err = |s: &str| parse_query(s, None, &mut resolve.clone()).err().unwrap();
        assert_eq!(err("nope && cpu.busy"), "unknown signal: nope");
        assert_eq!(err("cpu.busy && (cpu.data == 1"), "missing )");
        assert_eq!(err("!= 3"), "missing signal before !=");
        assert_eq!(err("cpu.busy )"), "unexpected: )");
        assert_eq!(err("cpu.state == 0x10"), "cpu.state: 0x10 doesn't fit in 3 bits");
    }
}
