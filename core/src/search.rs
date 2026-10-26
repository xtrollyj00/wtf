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
    /// `sig == value`, `sig < value` (the other operators too), or a bare `sig` (true when it is
    /// 1 / nonzero, `op` Eq); its value `past` samples earlier (`past(sig)`).
    Cmp { sig: Probe, past: u32, op: Op, target: Option<Target> },
    /// `stable(sig)`: the same value as one sample earlier.
    Stable { sig: Probe, past: u32 },
    /// `a != b`: two signals (a record's fields in `records` `where`), as unsigned numbers or text.
    Signals { a: Probe, a_past: u32, op: Op, b: Probe, b_past: u32 },
    /// `a + b == c`: numbers compared exactly; `b` None is `x`, an unknown result.
    Arith { a: Term, op: Op, b: Option<Term> },
    /// A bare `a & b`: true when it is known and nonzero.
    Truth(Term),
}

/// How `Expr::Cmp` compares: `<` and the like as unsigned numbers, an unknown bit never.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// The operators, longest first, as the parser tries them.
const OPS: [(&str, Op); 6] = [("==", Op::Eq), ("!=", Op::Ne), ("<=", Op::Le), (">=", Op::Ge), ("<", Op::Lt), (">", Op::Gt)];

/// An arithmetic operator (spec §8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

/// The arithmetic operators, longest first; `&&` and `||` are tried before them.
const BINS: [(&str, Bin); 8] = [("<<", Bin::Shl), (">>", Bin::Shr), ("+", Bin::Add), ("-", Bin::Sub), ("*", Bin::Mul), ("&", Bin::And), ("|", Bin::Or), ("^", Bin::Xor)];

impl Bin {
    fn sym(self) -> &'static str {
        BINS.iter().find(|b| b.1 == self).map_or("", |b| b.0)
    }

    /// Bitwise and shifts: they don't mix with another operator without parentheses.
    fn strict(self) -> bool {
        !matches!(self, Bin::Add | Bin::Sub | Bin::Mul)
    }
}

/// A value computed from signals (spec §8): an exact integer, None where unknown.
#[derive(Debug)]
pub enum Term {
    /// A signal, `past` samples back; `signed` reads its bits as two's complement.
    Sig { sig: Probe, past: u32, width: u32, signed: bool },
    Num(i128),
    Bin(Bin, Box<Term>, Box<Term>),
    /// `~x`: x's bits inverted at its width.
    Inv(Box<Term>, u32),
    /// `signed(x)`: x's bits at its width as two's complement.
    Signed(Box<Term>, u32),
    /// `(a + b)[31:0]`: bits hi down to lo, unsigned.
    Slice(Box<Term>, u32, u32),
    /// A comparison as a value: 1 or 0.
    Bool(Box<Expr>),
}

/// What a term's values fit in: `bits` bits, two's complement if `signed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub bits: u32,
    pub signed: bool,
}

impl Shape {
    /// Bits as a signed number.
    fn sbits(self) -> u32 {
        self.bits + u32::from(!self.signed)
    }
}

/// The widest operand and result arithmetic takes (spec §18).
const MAX_OPERAND: u32 = 64;
const MAX_RESULT: u32 = 127;

impl Term {
    /// The values it can have, worked out before searching.
    pub fn shape(&self) -> Shape {
        match self {
            Term::Sig { width, signed, .. } => Shape { bits: *width, signed: *signed },
            Term::Num(n) if *n < 0 => Shape { bits: 129 - (!n).leading_zeros(), signed: true },
            Term::Num(n) => Shape { bits: (128 - n.leading_zeros()).max(1), signed: false },
            Term::Bin(op, l, r) => {
                let (a, b) = (l.shape(), r.shape());
                let signed = a.signed || b.signed;
                let widest = if signed { a.sbits().max(b.sbits()) } else { a.bits.max(b.bits) };
                match op {
                    Bin::Add => Shape { bits: widest + 1, signed },
                    Bin::Sub => Shape { bits: a.sbits().max(b.sbits()) + 1, signed: true },
                    Bin::Mul => Shape { bits: a.bits + b.bits, signed },
                    Bin::And | Bin::Or | Bin::Xor => Shape { bits: widest, signed },
                    Bin::Shl => Shape { bits: a.bits.saturating_add(shift_max(r)), signed: a.signed },
                    Bin::Shr => a,
                }
            }
            Term::Inv(_, w) => Shape { bits: *w, signed: false },
            Term::Slice(_, hi, lo) => Shape { bits: hi - lo + 1, signed: false },
            Term::Signed(_, w) => Shape { bits: *w, signed: true },
            Term::Bool(_) => Shape { bits: 1, signed: false },
        }
    }

    /// Its value, reading signals with `value` as `holds` does; None where unknown.
    pub fn value<V: std::borrow::Borrow<Val>>(&self, value: &dyn Fn(Probe, u32) -> Option<Option<V>>) -> Option<i128> {
        Some(match self {
            Term::Sig { sig, past, width, signed } => {
                let v = value(*sig, *past)??;
                let Val::Bits(b) = v.borrow() else { return None };
                let n = number(b)?;
                if *signed { to_signed(n, *width) } else { n }
            }
            Term::Num(n) => *n,
            Term::Bin(op, a, b) => {
                let (x, y) = (a.value(value)?, b.value(value)?);
                match op {
                    Bin::Add => x.checked_add(y)?,
                    Bin::Sub => x.checked_sub(y)?,
                    Bin::Mul => x.checked_mul(y)?,
                    Bin::And => x & y,
                    Bin::Or => x | y,
                    Bin::Xor => x ^ y,
                    Bin::Shl => x.checked_shl(u32::try_from(y).ok()?)?,
                    Bin::Shr => x >> y.clamp(0, 127),
                }
            }
            Term::Inv(a, w) => !a.value(value)? & mask(*w),
            Term::Signed(a, w) => to_signed(a.value(value)? & mask(*w), *w),
            Term::Slice(a, hi, lo) => (a.value(value)? >> lo) & mask(hi - lo + 1),
            Term::Bool(e) => i128::from(holds(e, value)),
        })
    }

    /// The signals it reads, each with how many samples back.
    pub fn probes(&self, out: &mut Vec<(Probe, u32)>) {
        match self {
            Term::Sig { sig, past, .. } => out.push((*sig, *past)),
            Term::Num(_) => {}
            Term::Bin(_, a, b) => {
                a.probes(out);
                b.probes(out);
            }
            Term::Inv(a, _) | Term::Signed(a, _) | Term::Slice(a, ..) => a.probes(out),
            Term::Bool(e) => probes(e, out),
        }
    }

    /// The width it has exactly (a signal, a slice, `~`, `signed`), for `~` and `signed`.
    fn width(&self) -> Option<u32> {
        match self {
            Term::Sig { width, .. } | Term::Inv(_, width) | Term::Signed(_, width) => Some(*width),
            Term::Slice(_, hi, lo) => Some(hi - lo + 1),
            _ => None,
        }
    }

    /// The widest signal in it, to suggest a slice.
    fn widest_signal(&self) -> Option<u32> {
        match self {
            Term::Sig { width, .. } => Some(*width),
            Term::Bin(_, a, b) => a.widest_signal().max(b.widest_signal()),
            Term::Inv(a, _) | Term::Signed(a, _) => a.widest_signal(),
            _ => None,
        }
    }
}

/// How far a shift by `b` can move bits.
fn shift_max(b: &Term) -> u32 {
    match b {
        Term::Num(n) => u32::try_from(*n).unwrap_or(u32::MAX),
        b => 1u32.checked_shl(b.shape().bits).map_or(u32::MAX, |m| m - 1),
    }
}

/// The low `w` bits set (w up to 127).
fn mask(w: u32) -> i128 {
    1i128.wrapping_shl(w).wrapping_sub(1)
}

/// `n`, `w` bits wide, as two's complement.
fn to_signed(n: i128, w: u32) -> i128 {
    if w > 0 && (n >> (w - 1)) & 1 == 1 { n.wrapping_sub(1i128.wrapping_shl(w)) } else { n }
}

/// Bits as an unsigned number; None with an unknown bit or more than 127 bits.
fn number(bits: &str) -> Option<i128> {
    let (b, _) = unsigned(bits)?;
    if b > MAX_RESULT as usize {
        return None;
    }
    let b: String = bits.chars().map(|c| if c == 'h' { '1' } else if c == 'l' { '0' } else { c }).collect();
    i128::from_str_radix(&b, 2).ok()
}

pub struct Query {
    /// `@clk`: matches are rising edges of this signal where the expression holds.
    pub clock: Option<Probe>,
    pub expr: Expr,
}

/// Looks like an expression rather than a bare value for the selected signal.
pub fn is_expr(text: &str) -> bool {
    text.contains(['=', '!', '&', '|', '(', '@', '<', '>', '+', '*', '^', '~']) || text.get(1..).is_some_and(|t| t.contains('-'))
}

/// A parsed operand: its name, probe, width, value names, how many samples back, and whether it
/// reads signed.
type Operand = (String, Probe, Option<u32>, Option<Names>, u32, bool);

/// What a name is: its probe, its width (None: real or string), the names the file gives its
/// values, and whether its values are signed (a computed signal that can be negative). It may
/// make computed signals for names like `data[7:4]`, so it is `FnMut`.
pub type Resolve<'a> = dyn FnMut(&str) -> Result<(Probe, Option<u32>, Option<Names>, bool), String> + 'a;

/// The operators, for the error on text that isn't one.
const OPERATORS: &str = "== != < <= > >= && || ! + - * & | ^ ~ << >> and parentheses";

/// Parse `[@clk] expr`. A value with a time unit (`delay == 40ns`) is that many ticks, by the
/// trace's femtoseconds per tick `fpt`.
pub fn parse_query(text: &str, fpt: Option<u128>, resolve: &mut Resolve) -> Result<Query, String> {
    let mut p = Parser { s: text.trim(), len: text.trim().len(), fpt, resolve, names: Vec::new() };
    let clock = if p.eat("@") {
        let w = p.word();
        match (p.resolve)(&w) {
            Ok(r) => Some(r.0),
            // `@rising(clk)`, `@rising_edge(clk)`: the clock inside, if it is one
            Err(e) => match w.split_once('(').and_then(|(_, rest)| rest.strip_suffix(')')) {
                Some(inner) if (p.resolve)(inner).is_ok() => return Err(format!("@ takes the clock signal itself, as its rising edges: @{inner}")),
                _ => return Err(e),
            },
        }
    } else {
        None
    };
    let expr = p.or()?;
    if !p.s.trim().is_empty() {
        return Err(format!("unexpected: {} (operators: {OPERATORS})", p.s.trim()));
    }
    Ok(Query { clock, expr })
}

/// A computed signal's expression (spec §16): a value at any time, so no `past`, `rose`, `fell`
/// or `stable`; a comparison is a 1-bit value. Also returns where each name is in `text` (a byte
/// range) and what it names, for the signal's path.
pub fn parse_term(text: &str, fpt: Option<u128>, resolve: &mut Resolve) -> Result<(Term, Vec<Named>), String> {
    let mut p = Parser { s: text, len: text.len(), fpt, resolve, names: Vec::new() };
    let e = p.or()?;
    if !p.s.trim().is_empty() {
        return Err(format!("unexpected: {} (operators: {OPERATORS})", p.s.trim()));
    }
    let mut sigs = Vec::new();
    probes(&e, &mut sigs);
    if sigs.iter().any(|s| s.1 > 0) {
        return Err(format!("{}: past, rose, fell and stable need a clock; use them in find with @clk", text.trim()));
    }
    let term = match e {
        Expr::Truth(t) => t,
        e => Term::Bool(Box::new(e)),
    };
    let mut names = p.names;
    names.sort_by_key(|n| n.0.start);
    names.dedup_by_key(|n| n.0.start);
    Ok((term, names))
}

/// A name read: where it is in the text (a byte range), and its probe.
pub type Named = (std::ops::Range<usize>, Probe);

struct Parser<'a> {
    s: &'a str,
    /// The text's length, so `len - s.len()` is where `s` is in it.
    len: usize,
    fpt: Option<u128>,
    resolve: &'a mut Resolve<'a>,
    /// Each name read.
    names: Vec<Named>,
}

/// A chain's operand: a signal as named, or a computed value.
enum Atom {
    Signal(Operand),
    Term(Term),
}

/// Operands joined by arithmetic operators, with their text: the whole, the first operand's and
/// the last's, and the bitwise operator (`&`, `|`, `^`) among them if any, for the errors on
/// mixing it with a comparison.
struct Chain {
    atom: Atom,
    src: String,
    strict: Option<Bin>,
    first: String,
    last: String,
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

    /// The text read since `at` (a former `s`).
    fn since(&self, at: &str) -> String {
        at[..at.len() - self.s.len()].trim().to_string()
    }

    /// A name or a value: everything up to a space, a parenthesis or an operator, except inside
    /// braces (`{a, b}` is one name) and VHDL's parentheses: one right after a name is part of it
    /// (`gen(4).x`, `data(7 downto 4)`), unless the name is a function's (`rose(x)`).
    fn word(&mut self) -> String {
        self.s = self.s.trim_start();
        let (mut braces, mut parens) = (0, 0);
        let mut end = self.s.len();
        for (i, c) in self.s.char_indices() {
            let name_before = i > 0 && !matches!(&self.s[..i], "rose" | "fell" | "stable" | "past" | "signed");
            match c {
                '{' => braces += 1,
                '}' => braces -= 1,
                '(' if parens > 0 || name_before => parens += 1,
                ')' if parens > 0 => parens -= 1,
                c if braces == 0 && parens == 0 && (c.is_whitespace() || "()&|!=<>+-*^~".contains(c)) => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        let (w, rest) = self.s.split_at(end);
        self.s = rest;
        w.to_string()
    }

    /// A value: a word, with a leading minus (`-12`).
    fn value_word(&mut self) -> String {
        let minus = self.eat("-");
        let w = self.word();
        if minus { format!("-{w}") } else { w }
    }

    /// The arithmetic operator next, without reading it.
    fn peek_bin(&self) -> Option<(&'static str, Bin)> {
        let s = self.s.trim_start();
        if s.starts_with("&&") || s.starts_with("||") {
            return None;
        }
        BINS.iter().copied().find(|(sym, _)| s.starts_with(sym))
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
        // `rose(`, `fell(`, `stable(`; otherwise the word is read again as an operand
        let start = self.s;
        let f = self.word();
        if matches!(f.as_str(), "rose" | "fell" | "stable") && self.eat("(") {
            let (name, sig, width, _, past, _) = self.operand()?;
            if !self.eat(")") {
                return Err("missing )".into());
            }
            let cmp = |past, target| Expr::Cmp { sig, past, op: Op::Eq, target };
            // from a known value only: a valid going from U to 0 in reset didn't fall
            let zero = || width.map(|w| Some(Target::Bits("0".repeat(w as usize)))).ok_or(format!("{f}({name}): not a bit signal"));
            return Ok(match f.as_str() {
                "rose" => Expr::And(Box::new(cmp(past, None)), Box::new(cmp(past + 1, zero()?))),
                "fell" => Expr::And(Box::new(cmp(past, zero()?)), Box::new(cmp(past + 1, None))),
                _ => Expr::Stable { sig, past },
            });
        }
        self.s = start;
        let left = self.chain()?;
        let Some(&(sym, op)) = OPS.iter().find(|(sym, _)| self.eat(sym)) else {
            return Ok(match left.atom {
                Atom::Signal((_, sig, _, _, past, _)) => Expr::Cmp { sig, past, op: Op::Eq, target: None },
                Atom::Term(Term::Bool(e)) => *e,
                Atom::Term(t) => Expr::Truth(t),
            });
        };
        let right = self.s;
        let value = self.value_word();
        let more = self.peek_bin().is_some() || self.s.trim_start().starts_with('(');
        // a signed signal compares as a number, below
        if let Atom::Signal((name, sig, width, names, past, false)) = &left.atom
            && !value.is_empty()
            && !more
        {
            let mut value = value.clone();
            if crate::time::has_unit(&value) {
                value = (crate::time::parse(&value, self.fpt).map_err(|e| format!("{name}: {e}"))?.round() as u64).to_string();
            }
            match parse(&value, *width, names.as_ref()) {
                Ok(target) => {
                    if !matches!(op, Op::Eq | Op::Ne) && !matches!(target, Target::Bits(_)) {
                        return Err(format!("{name} {sym} {value}: {sym} compares numbers; x, z and text only with == or !="));
                    }
                    return Ok(Expr::Cmp { sig: *sig, past: *past, op, target: Some(target) });
                }
                // not a value: a signal (`mi_addr != reg_waddr`, `data != past(data)`)
                Err(e) if (self.resolve)(&value).is_err() => return Err(format!("{name}: {e}")),
                Err(_) => {}
            }
        }
        if !more && matches!(value.to_lowercase().as_str(), "x" | "u" | "z") {
            let whole = format!("{} {sym} {value}", left.src);
            return match (value.to_lowercase().as_str(), op) {
                ("z", _) => Err(format!("{whole}: an arithmetic result is a number or x")),
                (_, Op::Eq | Op::Ne) => Ok(Expr::Arith { a: self.term(left.atom, &left.src)?, op, b: None }),
                _ => Err(format!("{whole}: {sym} compares numbers; x only with == or !=")),
            };
        }
        self.s = right;
        let r = self.chain()?;
        let whole = format!("{} {sym} {}", left.src, r.src);
        if let Some(b) = left.strict {
            let before = left.src[..left.src.len() - left.last.len()].trim_end();
            return Err(format!("{whole}: {} and {sym} need parentheses: ({}) {sym} {} or {before} ({} {sym} {})", b.sym(), left.src, r.src, left.last, r.src));
        }
        if let Some(b) = r.strict {
            let after = r.src[r.first.len()..].trim_start();
            return Err(format!("{whole}: {sym} and {} need parentheses: {} {sym} ({}) or ({} {sym} {}) {after}", b.sym(), left.src, r.src, left.src, r.first));
        }
        Ok(match (left.atom, r.atom) {
            (Atom::Signal(a), Atom::Signal(b)) if !a.5 && !b.5 => Expr::Signals { a: a.1, a_past: a.4, op, b: b.1, b_past: b.4 },
            (a, b) => Expr::Arith { a: self.term(a, &left.src)?, op, b: Some(self.term(b, &r.src)?) },
        })
    }

    /// Operands joined by arithmetic operators: `*` before `+` and `-`; a strict operator (`&`,
    /// `<<`, ...) only with more of itself unless in parentheses (spec §8).
    fn chain(&mut self) -> Result<Chain, String> {
        self.s = self.s.trim_start();
        let at = self.s;
        let mut atoms = vec![(self.atom()?, self.since(at))];
        let mut ops = Vec::new();
        while let Some((sym, op)) = self.peek_bin() {
            self.eat(sym);
            self.s = self.s.trim_start();
            let at = self.s;
            atoms.push((self.atom()?, self.since(at)));
            ops.push(op);
        }
        let src = self.since(at);
        let (first, last) = (atoms[0].1.clone(), atoms[atoms.len() - 1].1.clone());
        if ops.is_empty() {
            let (atom, _) = atoms.pop().unwrap();
            return Ok(Chain { atom, src, strict: None, first, last });
        }
        if let Some(i) = (0..ops.len() - 1).find(|&i| ops[i] != ops[i + 1] && (ops[i].strict() || ops[i + 1].strict())) {
            let (a, b, c, x, y) = (&atoms[i].1, &atoms[i + 1].1, &atoms[i + 2].1, ops[i].sym(), ops[i + 1].sym());
            return Err(format!("{src}: {x} and {y} need parentheses: ({a} {x} {b}) {y} {c} or {a} {x} ({b} {y} {c})"));
        }
        let mut terms = Vec::new();
        let mut joins = Vec::new();
        for (k, (atom, text)) in atoms.into_iter().enumerate() {
            let t = (self.term(atom, &text)?, text);
            match k.checked_sub(1).map(|j| ops[j]) {
                Some(Bin::Mul) => {
                    let l = terms.pop().unwrap();
                    terms.push(self.bin(Bin::Mul, l, t)?);
                }
                Some(op) => {
                    joins.push(op);
                    terms.push(t);
                }
                None => terms.push(t),
            }
        }
        let mut terms = terms.into_iter();
        let mut t = terms.next().unwrap();
        for (op, r) in joins.into_iter().zip(terms) {
            t = self.bin(op, t, r)?;
        }
        // shifts bind tighter than comparisons, as everyone reads them; `&`, `|` and `^` don't
        let bitwise = ops.iter().copied().find(|o| matches!(o, Bin::And | Bin::Or | Bin::Xor));
        Ok(Chain { atom: Atom::Term(t.0), src, strict: bitwise, first, last })
    }

    /// `a op b`, checked for size before searching.
    fn bin(&mut self, op: Bin, (a, at): (Term, String), (b, bt): (Term, String)) -> Result<(Term, String), String> {
        let src = format!("{at} {} {bt}", op.sym());
        if matches!(op, Bin::Shl | Bin::Shr) && b.shape().signed {
            return Err(format!("{src}: {bt} can be negative; shift by an unsigned amount"));
        }
        let t = Term::Bin(op, Box::new(a), Box::new(b));
        let bits = t.shape().bits;
        if bits > MAX_RESULT {
            let Term::Bin(_, a, b) = &t else { unreachable!() };
            if op == Bin::Shl && !matches!(**b, Term::Num(_)) && a.shape().bits < MAX_RESULT {
                // the widest amount that fits: a.bits + 2^k - 1 <= 127
                let k = (MAX_RESULT - a.shape().bits + 1).ilog2();
                let w = b.shape().bits;
                return Err(format!("{src}: {bt} is {w} bits, so the result could be 2^{w} bits; shift by a slice, {at} << {bt}[{}:0]", k - 1));
            }
            return Err(format!("{src}: the result could be {bits} bits, more than {MAX_RESULT}; slice an operand to fewer bits"));
        }
        Ok((t, src))
    }

    /// An operand as a number: a signal of up to 64 bits.
    fn term(&mut self, atom: Atom, src: &str) -> Result<Term, String> {
        match atom {
            Atom::Term(t) => Ok(t),
            Atom::Signal((name, sig, width, _, past, signed)) => match width {
                None => Err(format!("{name} is a real or string signal; arithmetic takes numbers")),
                Some(w) if w > MAX_OPERAND => Err(format!("{src}: {name} is {w} bits; arithmetic takes up to {MAX_OPERAND}: slice it, as {name}[63:0]")),
                Some(width) => Ok(Term::Sig { sig, past, width, signed }),
            },
        }
    }

    /// An operand: `(...)` and its slice, `~x`, `signed(x)`, a number or a signal.
    fn atom(&mut self) -> Result<Atom, String> {
        self.s = self.s.trim_start();
        let at = self.s;
        if self.eat("~") {
            let a = self.atom()?;
            let src = self.since(at);
            let t = self.term(a, &src)?;
            let Some(w) = t.width() else {
                let w = t.widest_signal().unwrap_or(32);
                return Err(format!("{src}: ~ inverts bits at a width, so give one with a slice: {src}[{}:0]", w - 1));
            };
            return Ok(Atom::Term(Term::Inv(Box::new(t), w)));
        }
        if self.eat("(") {
            // a value in parentheses; else a condition (`(a || b)`), a 1-bit value
            let inner = self.s;
            let atom = match self.chain() {
                Ok(c) if self.eat(")") => c.atom,
                _ => {
                    self.s = inner;
                    let e = self.or()?;
                    if !self.eat(")") {
                        return Err("missing )".into());
                    }
                    Atom::Term(Term::Bool(Box::new(e)))
                }
            };
            if !self.s.starts_with('[') {
                return Ok(atom);
            }
            let src = self.since(at);
            let t = self.term(atom, &src)?;
            self.eat("[");
            let range = self.s.split_once(']').map(|(r, _)| r).unwrap_or("");
            let (hi, lo) = range.split_once(':').and_then(|(h, l)| Some((h.trim().parse::<u32>().ok()?, l.trim().parse::<u32>().ok()?))).ok_or(format!("{src}[{range}]: a slice is [hi:lo]"))?;
            self.s = &self.s[range.len() + 1..];
            if hi < lo || hi >= MAX_RESULT {
                return Err(format!("{src}[{hi}:{lo}]: a slice is [hi:lo], hi from lo to {}", MAX_RESULT - 1));
            }
            return Ok(Atom::Term(Term::Slice(Box::new(t), hi, lo)));
        }
        if self.s.starts_with(|c: char| c.is_ascii_digit()) || (self.s.starts_with('-') && self.s[1..].starts_with(|c: char| c.is_ascii_digit())) {
            let w = self.value_word();
            return number_text(&w, self.fpt).map(|n| Atom::Term(Term::Num(n)));
        }
        let start = self.s;
        if self.word() == "signed" && self.eat("(") {
            let c = self.chain()?;
            if !self.eat(")") {
                return Err("missing )".into());
            }
            let t = self.term(c.atom, &c.src)?;
            let Some(w) = t.width() else {
                let w = t.widest_signal().unwrap_or(32);
                return Err(format!("signed({}): signed reads bits at a width, so give one with a slice: signed(({})[{}:0])", c.src, c.src, w - 1));
            };
            return Ok(Atom::Term(Term::Signed(Box::new(t), w)));
        }
        self.s = start;
        self.operand().map(Atom::Signal)
    }

    /// A signal, or `past(...)` of one: its name, probe, width, value names, how many samples
    /// back and whether it reads signed.
    fn operand(&mut self) -> Result<Operand, String> {
        self.s = self.s.trim_start();
        let at = self.len - self.s.len();
        let name = self.word();
        if name == "past" && self.eat("(") {
            let (name, sig, width, names, past, signed) = self.operand()?;
            return if self.eat(")") { Ok((format!("past({name})"), sig, width, names, past + 1, signed)) } else { Err("missing )".into()) };
        }
        if name.is_empty() {
            return Err(format!("expected a signal at: {}", self.s.trim()));
        }
        let (sig, width, names, signed) = (self.resolve)(&name)?;
        self.names.push((at..at + name.len(), sig));
        Ok((name, sig, width, names, 0, signed))
    }
}

/// A number in an expression: decimal, `0x`, `0b` or `0o`, maybe negative, or a time (ticks).
fn number_text(text: &str, fpt: Option<u128>) -> Result<i128, String> {
    if crate::time::has_unit(text) {
        return crate::time::parse(text, fpt).map(|t| t.round() as i128);
    }
    let (neg, t) = text.strip_prefix('-').map_or((false, text), |t| (true, t));
    let t = t.replace('_', "").to_lowercase();
    let n = match t.get(..2) {
        Some("0x") => i128::from_str_radix(&t[2..], 16),
        Some("0b") => i128::from_str_radix(&t[2..], 2),
        Some("0o") => i128::from_str_radix(&t[2..], 8),
        _ => t.parse(),
    }
    .map_err(|_| format!("not a number: {text}"))?;
    Ok(if neg { -n } else { n })
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
        Expr::Signals { a, a_past, op, b, b_past } => {
            let (x, y) = (value(*a, *a_past).flatten(), value(*b, *b_past).flatten());
            let order = match (x.as_ref().map(|v| v.borrow()), y.as_ref().map(|v| v.borrow())) {
                (Some(Val::Bits(x)), Some(Val::Bits(y))) => unsigned(x).zip(unsigned(y)).map(|(x, y)| x.cmp(&y)),
                (Some(Val::Text(x)), Some(Val::Text(y))) => Some(x.cmp(y)),
                _ => None,
            };
            use std::cmp::Ordering::*;
            match op {
                Op::Eq => order == Some(Equal),
                Op::Ne => order != Some(Equal),
                Op::Lt => order == Some(Less),
                Op::Le => matches!(order, Some(Less | Equal)),
                Op::Gt => order == Some(Greater),
                Op::Ge => matches!(order, Some(Greater | Equal)),
            }
        }
        Expr::Truth(t) => t.value(value).is_some_and(|v| v != 0),
        Expr::Arith { a, op, b } => {
            let x = a.value(value);
            let Some(b) = b else { return (x.is_none()) == (*op == Op::Eq) };
            use std::cmp::Ordering::*;
            let order = x.zip(b.value(value)).map(|(x, y)| x.cmp(&y));
            match op {
                Op::Eq => order == Some(Equal),
                Op::Ne => order != Some(Equal),
                Op::Lt => order == Some(Less),
                Op::Le => matches!(order, Some(Less | Equal)),
                Op::Gt => order == Some(Greater),
                Op::Ge => matches!(order, Some(Greater | Equal)),
            }
        }
        Expr::Cmp { sig, past, op, target } => {
            let v = value(*sig, *past).flatten();
            let v = v.as_ref().map(|v| v.borrow());
            let hit = || match target {
                Some(target) => matches(target, v),
                // as in an `if`: some bit is 1 and none is unknown
                None => matches!(&v, Some(Val::Bits(b)) if b.contains(['1', 'h']) && !b.contains(['x', 'u', 'w', '-', 'z'])),
            };
            let order = || match (target, v) {
                (Some(Target::Bits(p)), Some(Val::Bits(b))) => unsigned(b).map(|b| b.cmp(&unsigned(p).unwrap_or_default())),
                _ => None,
            };
            use std::cmp::Ordering::*;
            match op {
                Op::Eq => hit(),
                Op::Ne => !hit(),
                Op::Lt => order() == Some(Less),
                Op::Le => matches!(order(), Some(Less | Equal)),
                Op::Gt => order() == Some(Greater),
                Op::Ge => matches!(order(), Some(Greater | Equal)),
            }
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
        Expr::Signals { a, a_past, b, b_past, .. } => out.extend([(*a, *a_past), (*b, *b_past)]),
        Expr::Truth(t) => t.probes(out),
        Expr::Arith { a, b, .. } => {
            a.probes(out);
            b.iter().for_each(|b| b.probes(out));
        }
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
    // a value of one signal (`/0x40`): one walk over its changes
    if let Expr::Cmp { sig, past: 0, op: Op::Eq, target: Some(target) } = &q.expr {
        return find(trace, sig.0, sig.1, target, from, forward);
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

/// Bits as an unsigned number that orders as one: no leading zeros, compared by length, then
/// digits; None with an unknown or high-impedance bit.
fn unsigned(bits: &str) -> Option<(usize, String)> {
    let b: String = bits.chars().map(|c| match c {
        'h' => Some('1'),
        'l' => Some('0'),
        '0' | '1' => Some(c),
        _ => None,
    }).collect::<Option<_>>()?;
    let b = b.trim_start_matches('0').to_string();
    Some((b.len(), b))
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
pub fn find(trace: &Trace, var: usize, bit: Option<u32>, target: &Target, from: Tick, forward: bool) -> Option<Tick> {
    find_all(trace, var, bit, target, from, forward).next()
}

/// Every such time, nearest first, from one walk over the signal's changes.
// shortcut: looks at every change; index values if searching huge traces is too slow (spec §18).
pub fn find_all<'a>(trace: &'a Trace, var: usize, bit: Option<u32>, target: &'a Target, from: Tick, forward: bool) -> impl Iterator<Item = Tick> + 'a {
    let mut changes = trace.walk(var, bit, from, forward).peekable();
    // forward, whether the change before matched
    let mut last = None;
    std::iter::from_fn(move || {
        while let Some((x, v)) = changes.next() {
            // the value just before x: forward the last change's, backward the next one's
            let before = match (forward, last) {
                (true, Some(m)) => m,
                (false, _) if let Some(c) = changes.peek() => matches(target, c.1.as_ref()),
                _ => x.checked_sub(1).is_some_and(|p| matches(target, trace.value_of(var, bit, p).as_ref())),
            };
            let now = matches(target, v.as_ref());
            last = Some(now);
            if now && !before {
                return Some(x);
            }
        }
        None
    })
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
        let resolve = |n: &str| t.by_path.get(&format!("top.{n}")).map(|&v| ((v, None), t.vars[v].width, None, false)).ok_or(format!("unknown signal: {n}"));
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
        // unsigned order: 0x43 @10 is the first above 0x40, 0x3A @40 the first below; z never compares
        assert_eq!(find("cpu.data > 0x40", 0, true), Some(10));
        assert_eq!(find("cpu.data>=0x96&&cpu.data<=0x96", 0, true), Some(20));
        assert_eq!(find("cpu.data < 0x40", 0, true), Some(40));
        assert_eq!(find("cpu.data < 0x40", 41, true), None);
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
        assert_eq!(err("cpu.busy )"), format!("unexpected: ) (operators: {OPERATORS})"));
        assert_eq!(err("cpu.data < z"), "cpu.data < z: < compares numbers; x, z and text only with == or !=");
        assert_eq!(err("cpu.state == 0x10"), "cpu.state: 0x10 doesn't fit in 3 bits");
    }
    /// Signals for arithmetic: a, b 32 bits, m, n 4, s 8 (signed, as `x - y` makes), t text, w
    /// 128 bits, u 8 bits unknown; each with one value.
    fn arith(text: &str) -> Result<bool, String> {
        let sigs: [(&str, Option<u32>, bool, &str); 8] = [
            ("a", Some(32), false, "11111111111111111111111111111111"),
            ("b", Some(32), false, "00000000000000000000000000000001"),
            ("m", Some(4), false, "0100"),
            ("s", Some(8), true, "11111100"),
            ("t", None, false, "idle"),
            ("w", Some(128), false, ""),
            ("n", Some(4), false, "1111"),
            ("u", Some(8), false, "x"),
        ];
        let q = parse_query(text, None, &mut |n| {
            let i = sigs.iter().position(|s| s.0 == n).ok_or(format!("unknown signal: {n}"))?;
            Ok(((i, None), sigs[i].1, None, sigs[i].2))
        })?;
        Ok(holds(&q.expr, &|(i, _), _| {
            Some(Some(match sigs[i].1 {
                Some(_) => Val::Bits(sigs[i].3.into()),
                None => Val::Text(sigs[i].3.into()),
            }))
        }))
    }

    #[test]
    fn arithmetic() {
        let yes = |s: &str| assert_eq!(arith(s), Ok(true), "{s}");
        let no = |s: &str| assert_eq!(arith(s), Ok(false), "{s}");
        // exact: nothing wraps unless sliced
        yes("a + b == 0x100000000");
        yes("(a + b)[31:0] == 0");
        yes("b - a == -0xFFFFFFFE");
        yes("b - a < 0");
        yes("a * a == 0xFFFFFFFE00000001");
        // * before +, a chain of one strict operator, shifts
        yes("b + b * 3 == 4");
        yes("1 << m == 16");
        yes("a >> 28 == 15");
        yes("(a & 0xF0 & 0x30) == 0x30");
        yes("(a & 0xF) == 15");
        yes("~m == 0b1011");
        // signed: two's complement at the width; a signed signal; exact comparisons
        yes("signed(m) == 4");
        yes("signed(n) == -1");
        yes("s == -4");
        yes("s < 0");
        yes("s != m");
        yes("s < b");
        yes("s >> 1 == -2");
        yes("signed(a) + 1 == 0");
        no("a < s");
        // comparisons as values, and bare values as conditions
        yes("(a == 0) + 1 == 1");
        yes("m & 4");
        no("m & 3");
        // as before: plain signals and values
        yes("m == 4");
        yes("m != b");
        yes("t == idle");
    }

    #[test]
    fn arithmetic_unknowns() {
        // u is unknown: its result too, which equals x and no number
        assert_eq!(arith("u + 1 == x"), Ok(true));
        assert_eq!(arith("u + 1 != x"), Ok(false));
        assert_eq!(arith("u + 1 != 5"), Ok(true));
        assert_eq!(arith("u + 1 == 5"), Ok(false));
        assert_eq!(arith("u + 1 < 5"), Ok(false));
        assert_eq!(arith("u & 1"), Ok(false));
    }

    #[test]
    fn arithmetic_errors() {
        let err = |s: &str| arith(s).expect_err(s);
        assert_eq!(err("m & 3 == 0"), "m & 3 == 0: & and == need parentheses: (m & 3) == 0 or m & (3 == 0)");
        assert_eq!(err("0 == m & 3"), "0 == m & 3: == and & need parentheses: 0 == (m & 3) or (0 == m) & 3");
        assert_eq!(err("a + m & 3 == 0"), "a + m & 3: + and & need parentheses: (a + m) & 3 or a + (m & 3)");
        assert_eq!(err("a << b == 0"), "a << b: b is 32 bits, so the result could be 2^32 bits; shift by a slice, a << b[5:0]");
        assert_eq!(err("a << s == 0"), "a << s: s can be negative; shift by an unsigned amount");
        assert_eq!(err("~(a + b) == 0"), "~(a + b): ~ inverts bits at a width, so give one with a slice: ~(a + b)[31:0]");
        assert_eq!(err("signed(a + b) == 0"), "signed(a + b): signed reads bits at a width, so give one with a slice: signed((a + b)[31:0])");
        assert_eq!(err("w + 1 == 0"), "w: w is 128 bits; arithmetic takes up to 64: slice it, as w[63:0]");
        assert_eq!(err("t + 1 == 0"), "t is a real or string signal; arithmetic takes numbers");
        assert_eq!(err("a * a * a * a == 0"), "a * a * a * a: the result could be 128 bits, more than 127; slice an operand to fewer bits");
        assert_eq!(err("a + 1 < x"), "a + 1 < x: < compares numbers; x only with == or !=");
        assert_eq!(err("a + 1 == z"), "a + 1 == z: an arithmetic result is a number or x");
        assert_eq!(err("(a + b)[200:0] == 0"), "(a + b)[200:0]: a slice is [hi:lo], hi from lo to 126");
    }
}
