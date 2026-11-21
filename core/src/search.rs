//! Value search (spec §8): what a search text means, and where a signal changes to it.
//! Also expressions over several signals (`valid && ready && data == 0x40`), optionally sampled
//! at the rising edges of a clock (`@clk ...`), and over time at that clock, as SVA's sampled
//! value functions (IEEE 1800 §16.9.3): `past(e, n)` is any expression e n samples earlier
//! (`past(e)` one), `rose(e)` and `fell(e)` are e's bit 0 becoming 1 / 0 from anything else, and
//! `stable(e)` is e's whole value the same as a sample earlier, x included; the three are always
//! 1 or 0, and before the first sample the earlier value is x.

use dashu_int::{IBig, UBig, ops::BitTest};

use crate::radix::RADIXES;
use crate::time::Tick;
use crate::trace::{Names, Trace, Val};

#[derive(Debug, Clone, PartialEq)]
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
    let Some(w) = width else {
        return Ok(Target::Text(text.trim().into()));
    };
    if let Some(sv) = sv_literal(text.trim()) {
        let bits = match sv? {
            Sv::Fill(c) => c.to_string().repeat(w as usize),
            Sv::Bits { bits, .. } => bits,
        };
        let fill = if bits.starts_with(['x', 'z']) {
            &bits[..1]
        } else {
            "0"
        };
        let bits = bits.trim_start_matches('0');
        if bits.len() > w as usize {
            return Err(format!("{} doesn't fit in {w} bits", text.trim()));
        }
        return Ok(Target::Bits(format!(
            "{}{bits}",
            fill.repeat(w as usize - bits.len())
        )));
    }
    if let Some((bits, _)) = names.and_then(|n| {
        n.iter()
            .find(|(_, name)| name.eq_ignore_ascii_case(text.trim()))
    }) {
        return Ok(Target::Bits(format!("{bits:0>w$}", w = w as usize)));
    }
    let bits = RADIXES
        .iter()
        .filter_map(|r| r.parse)
        .find_map(|p| p(&t, w))
        .ok_or(format!("not a value: {text}"))?;
    let bits = bits.trim_start_matches('0');
    if bits.len() > w as usize {
        return Err(format!("{} doesn't fit in {w} bits", text.trim()));
    }
    Ok(Target::Bits(format!("{bits:0>w$}", w = w as usize)))
}

/// A signal, or one bit of a bus.
pub type Probe = (usize, Option<u32>);

#[derive(Debug, Clone)]
pub enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    /// `sig == value`, `sig < value` (the other operators too), or a bare `sig` (true when it is
    /// 1 / nonzero, `op` Eq); its value `past` samples earlier (`past(sig)`).
    Cmp {
        sig: Probe,
        past: u32,
        op: Op,
        target: Option<Target>,
    },
    /// `a != b`: two signals (a record's fields in `records` `where`), as unsigned numbers or text;
    /// `data != past(data)`.
    Signals {
        a: Probe,
        a_past: u32,
        op: Op,
        b: Probe,
        b_past: u32,
    },
    /// `a + b == c`: numbers compared exactly; `b` None is `x`, an unknown result.
    Arith {
        a: Term,
        op: Op,
        b: Option<Term>,
    },
    /// A bare `a & b`: true when it is known and nonzero.
    Truth(Term),
    /// `contains(sig, "text")`: sig's text holds `text`, case and spaces as written.
    Contains {
        sig: Probe,
        past: u32,
        text: String,
    },
    /// `rose(e)`, `fell(e)`, `stable(e)` (spec §8), as SVA's (IEEE 1800 §16.9.3): e read as `now`
    /// against `was`, the same read a sample earlier; always known.
    Func {
        f: Func,
        now: Arg,
        was: Arg,
    },
}

/// The functions comparing an expression with itself a sample earlier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Func {
    /// Bit 0 becomes 1 from anything else: 0, x or z.
    Rose,
    /// Bit 0 becomes 0 from anything else.
    Fell,
    /// The whole value is the same, as SV's `===` compares: x staying x is stable.
    Stable,
}

/// What a `Func` reads: a signal as it is (its bits, x and z kept, or its text), `past` samples
/// back, or a computed value, which is x as a whole with an unknown bit (spec §8).
#[derive(Debug, Clone)]
pub enum Arg {
    Sig(Probe, u32),
    Term(Term),
}

impl Arg {
    /// Its value as `===` compares it; None where it is x: no sample (before the first, a 4-state
    /// signal's default), no value, every bit unknown, or a computed value with an unknown bit.
    fn read<V: std::borrow::Borrow<Val>>(
        &self,
        value: &dyn Fn(Probe, u32) -> Option<Option<V>>,
    ) -> Option<Four<V>> {
        match self {
            Arg::Sig(sig, past) => {
                let v = value(*sig, *past)??;
                match v.borrow() {
                    Val::Bits(b) if b.chars().all(|c| four(c) == 'x') => None,
                    _ => Some(Four::Sig(v)),
                }
            }
            Arg::Term(t) => t.value(value).map(Four::Num),
        }
    }

    fn shift(&mut self, n: u32) {
        match self {
            Arg::Sig(_, past) => *past = past.saturating_add(n),
            Arg::Term(t) => t.shift(n),
        }
    }

    fn probes(&self, out: &mut Vec<(Probe, u32)>) {
        match self {
            Arg::Sig(sig, past) => out.push((*sig, *past)),
            Arg::Term(t) => t.probes(out),
        }
    }
}

/// A bit as `===` compares it: h is 1, l is 0, and any other unknown (u, w, -) is x.
fn four(c: char) -> char {
    match c {
        '1' | 'h' => '1',
        '0' | 'l' => '0',
        'z' => 'z',
        _ => 'x',
    }
}

/// A value as `Arg::read` reads it: a signal's, read in place, or a computed one.
enum Four<V> {
    Sig(V),
    Num(IBig),
}

impl<V: std::borrow::Borrow<Val>> Four<V> {
    /// Bit 0; None where it is x or z.
    fn lsb(&self) -> Option<bool> {
        match self {
            Four::Sig(v) => match v.borrow() {
                Val::Bits(b) => match b.chars().last().map(four) {
                    Some('1') => Some(true),
                    Some('0') => Some(false),
                    _ => None,
                },
                Val::Text(_) => None,
            },
            // two's complement: -1's bit 0 is 1
            Four::Num(n) => Some((n.clone() & IBig::ONE) != IBig::ZERO),
        }
    }

    /// Whether the two are the same, as SV's `===`.
    fn same(&self, o: &Self) -> bool {
        match (self, o) {
            (Four::Sig(a), Four::Sig(b)) => match (a.borrow(), b.borrow()) {
                (Val::Bits(a), Val::Bits(b)) => {
                    a.len() == b.len() && a.chars().map(four).eq(b.chars().map(four))
                }
                (Val::Text(a), Val::Text(b)) => a == b,
                _ => false,
            },
            (Four::Num(a), Four::Num(b)) => a == b,
            _ => false,
        }
    }
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
const OPS: [(&str, Op); 6] = [
    ("==", Op::Eq),
    ("!=", Op::Ne),
    ("<=", Op::Le),
    (">=", Op::Ge),
    ("<", Op::Lt),
    (">", Op::Gt),
];

/// An arithmetic operator (spec §8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

/// The arithmetic operators, longest first; `&&` and `||` are tried before them.
const BINS: [(&str, Bin); 10] = [
    ("<<", Bin::Shl),
    (">>", Bin::Shr),
    ("+", Bin::Add),
    ("-", Bin::Sub),
    ("*", Bin::Mul),
    ("/", Bin::Div),
    ("%", Bin::Rem),
    ("&", Bin::And),
    ("|", Bin::Or),
    ("^", Bin::Xor),
];

impl Bin {
    fn sym(self) -> &'static str {
        BINS.iter().find(|b| b.1 == self).map_or("", |b| b.0)
    }

    /// Bitwise and shifts: they don't mix with another operator without parentheses.
    fn strict(self) -> bool {
        !matches!(self, Bin::Add | Bin::Sub | Bin::Mul | Bin::Div | Bin::Rem)
    }
}

/// A value computed from signals (spec §8): an exact integer, None where unknown.
#[derive(Debug, Clone)]
pub enum Term {
    /// A signal, `past` samples back; `signed` reads its bits as two's complement.
    Sig {
        sig: Probe,
        past: u32,
        width: u32,
        signed: bool,
    },
    Num(IBig),
    Bin(Bin, Box<Term>, Box<Term>),
    /// `~x`: x's bits inverted at its width.
    Inv(Box<Term>, u32),
    /// `signed(x)`: x's bits at its width as two's complement.
    Signed(Box<Term>, u32),
    /// `(a + b)[31:0]`: bits hi down to lo, unsigned.
    Slice(Box<Term>, u32, u32),
    /// A comparison as a value: 1 or 0.
    Bool(Box<Expr>),
    /// `c ? a : b`: x when c reads an unknown bit.
    Cond(Box<Expr>, Box<Term>, Box<Term>),
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

/// The widest result arithmetic takes (spec §8): it bounds a shift by a signal, which could
/// otherwise be 2^32 bits wide.
const MAX_RESULT: u32 = 1 << 16;

impl Term {
    /// The values it can have, worked out before searching.
    pub fn shape(&self) -> Shape {
        match self {
            Term::Sig { width, signed, .. } => Shape {
                bits: *width,
                signed: *signed,
            },
            Term::Num(n) if *n < IBig::ZERO => Shape {
                bits: (!n.clone()).bit_len() as u32 + 1,
                signed: true,
            },
            Term::Num(n) => Shape {
                bits: (n.bit_len() as u32).max(1),
                signed: false,
            },
            Term::Bin(op, l, r) => {
                let (a, b) = (l.shape(), r.shape());
                let signed = a.signed || b.signed;
                let widest = if signed {
                    a.sbits().max(b.sbits())
                } else {
                    a.bits.max(b.bits)
                };
                match op {
                    Bin::Add => Shape {
                        bits: widest + 1,
                        signed,
                    },
                    Bin::Sub => Shape {
                        bits: a.sbits().max(b.sbits()) + 1,
                        signed: true,
                    },
                    Bin::Mul => Shape {
                        bits: a.bits + b.bits,
                        signed,
                    },
                    // a signed quotient can be one bit wider: -8 / -1
                    Bin::Div => Shape {
                        bits: a.bits + u32::from(signed),
                        signed,
                    },
                    // smaller than the divisor, and no larger than the dividend
                    Bin::Rem if signed => Shape {
                        bits: a.sbits().min(b.sbits()),
                        signed,
                    },
                    Bin::Rem => Shape {
                        bits: a.bits.min(b.bits),
                        signed,
                    },
                    // `a & 31` is never wider than 31, and not negative if either side isn't
                    Bin::And => match (a.signed, b.signed) {
                        (false, false) => Shape {
                            bits: a.bits.min(b.bits),
                            signed: false,
                        },
                        (true, false) => b,
                        (false, true) => a,
                        (true, true) => Shape {
                            bits: widest,
                            signed: true,
                        },
                    },
                    Bin::Or | Bin::Xor => Shape {
                        bits: widest,
                        signed,
                    },
                    Bin::Shl => Shape {
                        bits: a.bits.saturating_add(shift_max(r)),
                        signed: a.signed,
                    },
                    Bin::Shr => a,
                }
            }
            Term::Inv(_, w) => Shape {
                bits: *w,
                signed: false,
            },
            Term::Slice(_, hi, lo) => Shape {
                bits: hi - lo + 1,
                signed: false,
            },
            Term::Signed(_, w) => Shape {
                bits: *w,
                signed: true,
            },
            Term::Bool(_) => Shape {
                bits: 1,
                signed: false,
            },
            Term::Cond(_, a, b) => {
                let (a, b) = (a.shape(), b.shape());
                let signed = a.signed || b.signed;
                Shape {
                    bits: if signed {
                        a.sbits().max(b.sbits())
                    } else {
                        a.bits.max(b.bits)
                    },
                    signed,
                }
            }
        }
    }

    /// Its value, reading signals with `value` as `holds` does; None where unknown.
    pub fn value<V: std::borrow::Borrow<Val>>(
        &self,
        value: &dyn Fn(Probe, u32) -> Option<Option<V>>,
    ) -> Option<IBig> {
        Some(match self {
            Term::Sig {
                sig,
                past,
                width,
                signed,
            } => {
                let v = value(*sig, *past)??;
                let Val::Bits(b) = v.borrow() else {
                    return None;
                };
                let n = number(b)?;
                if *signed { to_signed(n, *width) } else { n }
            }
            Term::Num(n) => n.clone(),
            Term::Bin(op, a, b) => {
                let (x, y) = (a.value(value)?, b.value(value)?);
                match op {
                    Bin::Add => x + y,
                    Bin::Sub => x - y,
                    Bin::Mul => x * y,
                    // by zero: x, as in SV; both truncate toward zero
                    Bin::Div | Bin::Rem if y == IBig::ZERO => return None,
                    Bin::Div => x / y,
                    Bin::Rem => x % y,
                    Bin::And => x & y,
                    Bin::Or => x | y,
                    Bin::Xor => x ^ y,
                    // the amount fits: `bin` bounds the result to MAX_RESULT bits
                    Bin::Shl => x << usize::try_from(&y).ok()?,
                    // past x's bits only its sign is left
                    Bin::Shr => {
                        let s = usize::try_from(&y).unwrap_or(usize::MAX).min(x.bit_len());
                        x >> s
                    }
                }
            }
            Term::Inv(a, w) => !a.value(value)? & mask(*w),
            Term::Signed(a, w) => to_signed(a.value(value)? & mask(*w), *w),
            Term::Slice(a, hi, lo) => (a.value(value)? >> *lo as usize) & mask(hi - lo + 1),
            Term::Bool(e) => IBig::from(u8::from(holds3(e, value)?)),
            // shortcut (#12): whole values: an unknown condition gives a where a == b, else all x, not SV's per-bit merge; upgrade if a partly known mux result matters
            Term::Cond(c, a, b) => match holds3(c, value) {
                Some(true) => a.value(value)?,
                Some(false) => b.value(value)?,
                None => a
                    .value(value)
                    .filter(|x| b.value(value).as_ref() == Some(x))?,
            },
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
            Term::Cond(c, a, b) => {
                probes(c, out);
                a.probes(out);
                b.probes(out);
            }
        }
    }

    /// The same value `n` samples earlier: `past(t, n)`.
    fn shift(&mut self, n: u32) {
        match self {
            Term::Sig { past, .. } => *past = past.saturating_add(n),
            Term::Num(_) => {}
            Term::Bin(_, a, b) => {
                a.shift(n);
                b.shift(n);
            }
            Term::Inv(a, _) | Term::Signed(a, _) | Term::Slice(a, ..) => a.shift(n),
            Term::Bool(e) => shift(e, n),
            Term::Cond(c, a, b) => {
                shift(c, n);
                a.shift(n);
                b.shift(n);
            }
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
            Term::Bin(_, a, b) | Term::Cond(_, a, b) => a.widest_signal().max(b.widest_signal()),
            Term::Inv(a, _) | Term::Signed(a, _) => a.widest_signal(),
            _ => None,
        }
    }
}

/// How far a shift by `b` can move bits.
fn shift_max(b: &Term) -> u32 {
    match b {
        Term::Num(n) => u32::try_from(n).unwrap_or(u32::MAX),
        b => 1u32.checked_shl(b.shape().bits).map_or(u32::MAX, |m| m - 1),
    }
}

/// The low `w` bits set.
fn mask(w: u32) -> IBig {
    (IBig::ONE << w as usize) - IBig::ONE
}

/// `n`, `w` bits wide, as two's complement.
fn to_signed(n: IBig, w: u32) -> IBig {
    if w > 0 && n.bit(w as usize - 1) {
        n - (IBig::ONE << w as usize)
    } else {
        n
    }
}

/// Bits as an unsigned number; None with an unknown bit.
fn number(bits: &str) -> Option<IBig> {
    let (_, b) = unsigned(bits)?;
    if b.is_empty() {
        return Some(IBig::ZERO);
    }
    UBig::from_str_radix(&b, 2).ok().map(IBig::from)
}

#[derive(Debug)]
pub struct Query {
    /// `@clk`: matches are rising edges of this signal where the expression holds.
    pub clock: Option<Probe>,
    pub expr: Expr,
    /// The first `rose`, `fell`, `stable` or `past` in the text, as written: they need a clock.
    pub call: Option<String>,
}

impl Query {
    /// rose, fell, stable and past compare a sample with the one before, so a search with one
    /// needs a clock, as in SVA (spec §8): without one, an error that writes it, naming `clock()`.
    /// A `records` `where` reads the record before instead and doesn't ask.
    pub fn needs_clock(&self, text: &str, clock: impl FnOnce() -> String) -> Result<(), String> {
        match (&self.clock, &self.call) {
            (None, Some(call)) => Err(format!(
                "{call} needs a clock, as in SVA: @{} {}",
                clock(),
                text.trim()
            )),
            _ => Ok(()),
        }
    }
}

/// Looks like an expression rather than a bare value for the selected signal.
pub fn is_expr(text: &str) -> bool {
    text.contains([
        '=', '!', '&', '|', '(', '@', '<', '>', '+', '*', '/', '%', '^', '~', '?',
    ]) || text.get(1..).is_some_and(|t| t.contains('-'))
}

/// A parsed operand: its name, probe, width, value names, how many samples back, and whether it
/// reads signed.
type Operand = (String, Probe, Option<u32>, Option<Names>, u32, bool);

/// What a name is: its probe, its width (None: real or string), the names the file gives its
/// values, and whether its values are signed (a computed signal that can be negative). It may
/// make computed signals for names like `data[7:4]`, so it is `FnMut`.
pub type Resolve<'a> =
    dyn FnMut(&str) -> Result<(Probe, Option<u32>, Option<Names>, bool), String> + 'a;

/// The operators, for the error on text that isn't one.
const OPERATORS: &str = "== != < <= > >= && || ! + - * / % & | ^ ~ << >> ?: and parentheses";

/// A word from another tool's language that wtf writes another way, worked out from a name that
/// is no signal (`prev(x)`, `rising`) or text after an expression (`=1`); its `Display` shows the
/// form meant (spec §15 "The agent's guide").
#[derive(Debug, PartialEq)]
pub enum Borrowed {
    /// `rising`, `posedge`, `$rose(x)`: an edge, `rose(x)` or `fell(x)`; `arg` the signal if given.
    Edge {
        word: String,
        func: &'static str,
        arg: Option<String>,
    },
    /// `prev(x)`, `previous(x)`, `$past(x)`: `past(x)`.
    Past { word: String, arg: Option<String> },
    /// `$stable(x)`: `stable(x)`.
    Stable { word: String, arg: Option<String> },
    /// `changed(x)`, `$changed(x)`: a change is `!stable(x)`, no function of its own.
    Changed { word: String, arg: Option<String> },
    /// `next(x)`: an expression reads no later value.
    Ahead { word: String },
    /// `a = 1`: a comparison is `==`.
    Assign { rest: String },
}

impl Borrowed {
    /// What `name`, which resolved to no signal, meant, if it is a word of another language.
    pub fn of(name: &str) -> Option<Borrowed> {
        let name = name.trim();
        let (head, arg) = match name.find(['(', '[']) {
            Some(i) => (
                &name[..i],
                // one closing bracket: `$rose(a && (b || c))` keeps its own
                Some({
                    let arg = &name[i + 1..];
                    arg.strip_suffix([')', ']'])
                        .unwrap_or(arg)
                        .trim()
                        .to_string()
                }),
            ),
            None => (name, None),
        };
        let word = name.to_string();
        Some(
            match head.trim_start_matches('$').to_ascii_lowercase().as_str() {
                "rising" | "rise" | "posedge" | "rising_edge" | "rose" => Borrowed::Edge {
                    word,
                    func: "rose",
                    arg,
                },
                "falling" | "fall" | "negedge" | "falling_edge" | "fell" => Borrowed::Edge {
                    word,
                    func: "fell",
                    arg,
                },
                "prev" | "previous" | "past" => Borrowed::Past { word, arg },
                "stable" => Borrowed::Stable { word, arg },
                "changed" | "change" => Borrowed::Changed { word, arg },
                "next" | "future" | "nexttime" => Borrowed::Ahead { word },
                _ => return None,
            },
        )
    }

    /// What text left after a whole expression (`=1 && b`, `falling`) meant.
    fn after(rest: &str) -> Option<Borrowed> {
        let rest = rest.trim();
        if rest.starts_with('=') && !rest.starts_with("==") {
            return Some(Borrowed::Assign {
                rest: rest.to_string(),
            });
        }
        Borrowed::of(rest.split_whitespace().next().unwrap_or(""))
    }
}

impl std::fmt::Display for Borrowed {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let x = |arg: &Option<String>| arg.clone().filter(|a| !a.is_empty()).unwrap_or("x".into());
        match self {
            Borrowed::Edge {
                word,
                func,
                arg: None,
            } => write!(
                f,
                "{word}: `@clk` already samples at the clock's rising edges; an edge of a signal is {func}(x), SVA's ${func} without the $"
            ),
            Borrowed::Edge { word, func, arg } => write!(
                f,
                "{word}: an edge is {func}({}), SVA's ${func} without the $",
                x(arg)
            ),
            Borrowed::Past { word, arg } => write!(
                f,
                "{word}: the value a cycle earlier is past({}), SVA's $past without the $",
                x(arg)
            ),
            Borrowed::Stable { word, arg } => {
                write!(f, "{word}: stable({}), SVA's $stable without the $", x(arg))
            }
            Borrowed::Changed { word, arg } => {
                write!(f, "{word}: a change is !stable({}), SVA's $changed", x(arg))
            }
            Borrowed::Ahead { word } => write!(
                f,
                "{word}: an expression reads no later value; search on from a match instead (`find` `from`)"
            ),
            Borrowed::Assign { rest } => write!(f, "{rest}: compare with ==, not ="),
        }
    }
}

/// query = [ "@" signal ] expr. Parse `[@clk] expr`. A value with a time unit (`delay == 40ns`) is
/// that many ticks, by the trace's femtoseconds per tick `fpt`.
pub fn parse_query(text: &str, fpt: Option<u128>, resolve: &mut Resolve) -> Result<Query, String> {
    let mut p = Parser {
        s: text.trim(),
        len: text.trim().len(),
        fpt,
        resolve,
        names: Vec::new(),
        calls: Default::default(),
        first_call: None,
    };
    let clock = if p.eat("@") {
        let w = p.word();
        match (p.resolve)(&w) {
            Ok(r) => Some(r.0),
            // `@rising(clk)`, `@rising_edge(clk)`: the clock inside, if it is one
            Err(e) => match w
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
            {
                Some(inner) if (p.resolve)(inner).is_ok() => {
                    return Err(format!(
                        "@ takes the clock signal itself, as its rising edges: @{inner}"
                    ));
                }
                _ => return Err(e),
            },
        }
    } else {
        None
    };
    let expr = p.cond()?;
    if !p.s.trim().is_empty() {
        return Err(unexpected(p.s));
    }
    Ok(Query {
        clock,
        expr,
        call: p.first_call.map(|c| c.1),
    })
}

/// The error on text after a whole expression: the form meant, or the operators.
fn unexpected(rest: &str) -> String {
    Borrowed::after(rest).map_or_else(
        || format!("unexpected: {} (operators: {OPERATORS})", rest.trim()),
        |b| b.to_string(),
    )
}

/// A computed signal's expression (spec §16): a value at any time, so no `past`, `rose`, `fell`
/// or `stable`; a comparison is a 1-bit value. Also returns where each name is in `text` (a byte
/// range) and what it names, for the signal's path.
pub fn parse_term(
    text: &str,
    fpt: Option<u128>,
    resolve: &mut Resolve,
) -> Result<(Term, Vec<Named>), String> {
    let mut p = Parser {
        s: text,
        len: text.len(),
        fpt,
        resolve,
        names: Vec::new(),
        calls: Default::default(),
        first_call: None,
    };
    let e = p.cond()?;
    if !p.s.trim().is_empty() {
        return Err(unexpected(p.s));
    }
    let mut sigs = Vec::new();
    probes(&e, &mut sigs);
    if sigs.iter().any(|s| s.1 > 0) {
        return Err(format!(
            "{}: past, rose, fell and stable need a clock; use them in find with @clk",
            text.trim()
        ));
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
    /// Each call read, by where its argument starts, with the text after it: a call's argument is
    /// read again as an expression when it is no operand, so nested calls are read once each.
    calls: std::collections::HashMap<usize, (Atom, &'a str)>,
    /// The first call in the text: where its argument starts, and the call as written.
    first_call: Option<(usize, String)>,
}

/// A chain's operand: a signal as named, or a computed value.
#[derive(Clone)]
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
        let (mut braces, mut parens, mut brackets) = (0, 0, 0);
        let mut end = self.s.len();
        for (i, c) in self.s.char_indices() {
            let name_before =
                i > 0 && !matches!(&self.s[..i], "rose" | "fell" | "stable" | "past" | "signed");
            match c {
                '{' => braces += 1,
                '}' => braces -= 1,
                '(' if parens > 0 || name_before => parens += 1,
                ')' if parens > 0 => parens -= 1,
                '[' => brackets += 1,
                ']' => brackets -= 1,
                c if braces == 0
                    && parens == 0
                    && (c.is_whitespace()
                        || "()&|!=<>+-*/%^~?".contains(c)
                        || (matches!(c, ':' | ',') && brackets == 0)) =>
                {
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

    /// expr = or [ "?" value ":" value ]: a condition, or `c ? a : b`, a value: below `||`, as in SV; a nested `?` in parentheses.
    fn cond(&mut self) -> Result<Expr, String> {
        let c = self.or()?;
        if !self.eat("?") {
            return Ok(c);
        }
        let a = self.value()?;
        let b = if self.eat(":") { self.value()? } else { None };
        match (a, b) {
            (Some(a), Some(b)) if !self.s.trim_start().starts_with('?') => Ok(Expr::Truth(
                Term::Cond(Box::new(c), Box::new(a), Box::new(b)),
            )),
            _ => Err(format!(
                "? is c ? a : b, with a nested ? in parentheses: c ? a : (d ? e : f); at: {}",
                self.s.trim()
            )),
        }
    }

    /// value = chain: a branch of `?:`, operands joined by arithmetic operators; None if it ends
    /// before `:`.
    fn value(&mut self) -> Result<Option<Term>, String> {
        self.s = self.s.trim_start();
        if self.s.starts_with([':', ')']) || self.s.is_empty() {
            return Ok(None);
        }
        let c = self.chain()?;
        self.term(c.atom, &c.src).map(Some)
    }

    /// or = and { "||" and }
    fn or(&mut self) -> Result<Expr, String> {
        let mut e = self.and()?;
        while self.eat("||") {
            e = Expr::Or(Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }

    /// and = unary { "&&" unary }
    fn and(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        while self.eat("&&") {
            e = Expr::And(Box::new(e), Box::new(self.unary()?));
        }
        Ok(e)
    }

    /// unary = "!" unary | contains | chain [ cmp ( chain | literal ) ]
    fn unary(&mut self) -> Result<Expr, String> {
        if self.s.trim_start().starts_with("!=") {
            return Err("missing signal before !=".into());
        }
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        // contains = "contains" "(" atom "," '"' text '"' ")": a quote before the `)` closing
        // `contains(`; else a VHDL name (`contains(3)`), read below
        let quoted = |r: &str| {
            let mut depth = 0;
            for c in r.chars() {
                match c {
                    '"' => return true,
                    '(' => depth += 1,
                    ')' if depth == 0 => return false,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            false
        };
        if self
            .s
            .trim_start()
            .strip_prefix("contains(")
            .is_some_and(quoted)
        {
            self.eat("contains(");
            self.s = self.s.trim_start();
            let at = self.s;
            let Atom::Signal((name, sig, width, _, past, _)) = self.atom()? else {
                let src = self.since(at);
                return Err(format!(
                    "contains({src}, ...): {src} is a value; contains reads a string signal's text"
                ));
            };
            if width.is_some() {
                return Err(format!(
                    "contains({name}, ...): {name} is a bit signal; contains reads a string signal's text"
                ));
            }
            if !self.eat(",") || !self.eat("\"") {
                return Err(format!(
                    "contains({name}, ...): write the text in quotes, contains({name}, \"text\")"
                ));
            }
            let Some((text, rest)) = self.s.split_once('"') else {
                return Err(format!("contains({name}, ...): missing closing \""));
            };
            self.s = rest;
            if !self.eat(")") {
                return Err("missing )".into());
            }
            return Ok(Expr::Contains {
                sig,
                past,
                text: text.into(),
            });
        }
        let left = self.chain()?;
        let Some(&(sym, op)) = OPS.iter().find(|(sym, _)| self.eat(sym)) else {
            return Ok(truth(left.atom));
        };
        let right = self.s;
        let value = self.value_word();
        let mut not_value = None;
        let more = self.peek_bin().is_some() || self.s.trim_start().starts_with('(');
        // a signed signal compares as a number, below
        if let Atom::Signal((name, sig, width, names, past, false)) = &left.atom
            && !value.is_empty()
            && !more
        {
            let mut value = value.clone();
            if crate::time::has_unit(&value) {
                value = (crate::time::parse(&value, self.fpt)
                    .map_err(|e| format!("{name}: {e}"))?
                    .round() as u64)
                    .to_string();
            }
            match parse(&value, *width, names.as_ref()) {
                Ok(target) => {
                    if !matches!(op, Op::Eq | Op::Ne) && !matches!(target, Target::Bits(_)) {
                        return Err(format!(
                            "{name} {sym} {value}: {sym} compares numbers; x, z and text only with == or !="
                        ));
                    }
                    return Ok(Expr::Cmp {
                        sig: *sig,
                        past: *past,
                        op,
                        target: Some(target),
                    });
                }
                // not a value: a signal (`mi_addr != reg_waddr`, `data != past(data)`), or a field's
                // slice (`rd == insn[11:7]`), which the chain reads
                Err(e) if value.starts_with(|c: char| c.is_ascii_digit() || "'-".contains(c)) => {
                    return Err(format!("{name}: {e}"));
                }
                Err(e) => not_value = Some(format!("{name}: {e}")),
            }
        }
        if !more && matches!(value.to_lowercase().as_str(), "x" | "u" | "z") {
            let whole = format!("{} {sym} {value}", left.src);
            return match (value.to_lowercase().as_str(), op) {
                ("z", _) => Err(format!("{whole}: an arithmetic result is a number or x")),
                (_, Op::Eq | Op::Ne) => Ok(Expr::Arith {
                    a: self.term(left.atom, &left.src)?,
                    op,
                    b: None,
                }),
                _ => Err(format!(
                    "{whole}: {sym} compares numbers; x only with == or !="
                )),
            };
        }
        self.s = right;
        let r = self.chain().map_err(|e| not_value.unwrap_or(e))?;
        let whole = format!("{} {sym} {}", left.src, r.src);
        if let Some(b) = left.strict {
            let before = left.src[..left.src.len() - left.last.len()].trim_end();
            return Err(format!(
                "{whole}: {} and {sym} need parentheses: ({}) {sym} {} or {before} ({} {sym} {})",
                b.sym(),
                left.src,
                r.src,
                left.last,
                r.src
            ));
        }
        if let Some(b) = r.strict {
            let after = r.src[r.first.len()..].trim_start();
            return Err(format!(
                "{whole}: {sym} and {} need parentheses: {} {sym} ({}) or ({} {sym} {}) {after}",
                b.sym(),
                left.src,
                r.src,
                left.src,
                r.first
            ));
        }
        Ok(match (left.atom, r.atom) {
            (Atom::Signal(a), Atom::Signal(b)) if !a.5 && !b.5 => Expr::Signals {
                a: a.1,
                a_past: a.4,
                op,
                b: b.1,
                b_past: b.4,
            },
            (a, b) => Expr::Arith {
                a: self.term(a, &left.src)?,
                op,
                b: Some(self.term(b, &r.src)?),
            },
        })
    }

    /// chain = atom { arith atom }: operands joined by arithmetic operators, `*`, `/` and `%`
    /// before `+` and `-`; a strict operator (`&`, `<<`, ...) only with more of itself unless in
    /// parentheses (spec §8).
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
            return Ok(Chain {
                atom,
                src,
                strict: None,
                first,
                last,
            });
        }
        if let Some(i) = (0..ops.len() - 1)
            .find(|&i| ops[i] != ops[i + 1] && (ops[i].strict() || ops[i + 1].strict()))
        {
            let (a, b, c, x, y) = (
                &atoms[i].1,
                &atoms[i + 1].1,
                &atoms[i + 2].1,
                ops[i].sym(),
                ops[i + 1].sym(),
            );
            return Err(format!(
                "{src}: {x} and {y} need parentheses: ({a} {x} {b}) {y} {c} or {a} {x} ({b} {y} {c})"
            ));
        }
        let mut terms = Vec::new();
        let mut joins = Vec::new();
        for (k, (atom, text)) in atoms.into_iter().enumerate() {
            let t = (self.term(atom, &text)?, text);
            match k.checked_sub(1).map(|j| ops[j]) {
                Some(op @ (Bin::Mul | Bin::Div | Bin::Rem)) => {
                    let l = terms.pop().unwrap();
                    terms.push(self.bin(op, l, t)?);
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
        let bitwise = ops
            .iter()
            .copied()
            .find(|o| matches!(o, Bin::And | Bin::Or | Bin::Xor));
        Ok(Chain {
            atom: Atom::Term(t.0),
            src,
            strict: bitwise,
            first,
            last,
        })
    }

    /// `a op b`, checked for size before searching.
    fn bin(
        &mut self,
        op: Bin,
        (a, at): (Term, String),
        (b, bt): (Term, String),
    ) -> Result<(Term, String), String> {
        let src = format!("{at} {} {bt}", op.sym());
        if matches!(op, Bin::Shl | Bin::Shr) && b.shape().signed {
            return Err(format!(
                "{src}: {bt} can be negative; shift by an unsigned amount"
            ));
        }
        let t = Term::Bin(op, Box::new(a), Box::new(b));
        let bits = t.shape().bits;
        if bits > MAX_RESULT {
            let Term::Bin(_, a, b) = &t else {
                unreachable!()
            };
            if op == Bin::Shl && !matches!(**b, Term::Num(_)) && a.shape().bits < MAX_RESULT {
                // the widest amount that fits: a.bits + 2^k - 1 <= MAX_RESULT
                let k = (MAX_RESULT - a.shape().bits + 1).ilog2();
                let w = b.shape().bits;
                return Err(format!(
                    "{src}: {bt} is {w} bits, so the result could be 2^{w} bits; shift by a slice, {at} << {bt}[{}:0]",
                    k - 1
                ));
            }
            return Err(format!(
                "{src}: the result could be {bits} bits, more than {MAX_RESULT}; slice an operand to fewer bits"
            ));
        }
        Ok((t, src))
    }

    /// An operand as a number: a signal of any width.
    fn term(&mut self, atom: Atom, _src: &str) -> Result<Term, String> {
        match atom {
            Atom::Term(t) => Ok(t),
            Atom::Signal((name, sig, width, _, past, signed)) => match width {
                None => Err(format!(
                    "{name} is a real or string signal; arithmetic takes numbers"
                )),
                Some(width) => Ok(Term::Sig {
                    sig,
                    past,
                    width,
                    signed,
                }),
            },
        }
    }

    /// atom = "!" atom | "~" atom | "(" expr ")" [slice] | "signed" "(" chain ")" [slice] | call
    /// | number | signal: `(...)` and its slice, `~x`, `!x` (1 or 0), `signed(x)` and its slice, a
    /// number or a signal, which may be a field's slice or bit (`b[4:0]`, `b[3]`).
    fn atom(&mut self) -> Result<Atom, String> {
        self.s = self.s.trim_start();
        let at = self.s;
        if !self.s.starts_with("!=") && self.eat("!") {
            let a = self.atom()?;
            return Ok(Atom::Term(Term::Bool(Box::new(Expr::Not(Box::new(
                truth(a),
            ))))));
        }
        if self.eat("~") {
            let a = self.atom()?;
            let src = self.since(at);
            let t = self.term(a, &src)?;
            let Some(w) = t.width() else {
                let w = t.widest_signal().unwrap_or(32);
                return Err(format!(
                    "{src}: ~ inverts bits at a width, so give one with a slice: {src}[{}:0]",
                    w - 1
                ));
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
                    let e = self.cond()?;
                    if !self.eat(")") {
                        return Err("missing )".into());
                    }
                    match e {
                        Expr::Truth(t) => Atom::Term(t),
                        e => Atom::Term(Term::Bool(Box::new(e))),
                    }
                }
            };
            return self.slice(atom, at);
        }
        if self
            .s
            .starts_with(|c: char| c.is_ascii_digit() || c == '\'')
            || (self.s.starts_with('-')
                && self.s[1..].starts_with(|c: char| c.is_ascii_digit() || c == '\''))
        {
            let w = self.value_word();
            return number_text(&w, self.fpt).map(|n| Atom::Term(Term::Num(n)));
        }
        let start = self.s;
        let w = self.word();
        if matches!(w.as_str(), "rose" | "fell" | "stable" | "past") && self.eat("(") {
            return self.call(&w);
        }
        if w == "signed" && self.eat("(") {
            let c = self.chain()?;
            if !self.eat(")") {
                return Err("missing )".into());
            }
            let t = self.term(c.atom, &c.src)?;
            let Some(w) = t.width() else {
                let w = t.widest_signal().unwrap_or(32);
                return Err(format!(
                    "signed({}): signed reads bits at a width, so give one with a slice: signed(({})[{}:0])",
                    c.src,
                    c.src,
                    w - 1
                ));
            };
            return self.slice(Atom::Term(Term::Signed(Box::new(t), w)), at);
        }
        self.s = start;
        let err = match self.operand() {
            Ok(op) => return Ok(Atom::Signal(op)),
            Err(e) => e,
        };
        // a slice or bit of a name that has none of its own: a record's field (`b[4:0]`)
        self.s = start;
        let name = self.word();
        let Some((base, range)) = name.strip_suffix(']').and_then(|n| n.rsplit_once('[')) else {
            return Err(err);
        };
        let (hi, lo) = match range.split_once(':') {
            Some((h, l)) => (h.trim().parse::<u32>(), l.trim().parse::<u32>()),
            None => (range.trim().parse(), range.trim().parse()),
        };
        let (Ok(hi), Ok(lo), Ok((sig, width, names, signed))) = (hi, lo, (self.resolve)(base))
        else {
            return Err(err);
        };
        let t = self.term(
            Atom::Signal((base.to_string(), sig, width, names, 0, signed)),
            base,
        )?;
        if hi < lo || hi >= t.width().unwrap_or(0) {
            return Err(format!(
                "{name}: {base} has no bits {hi}:{lo} ({} bits)",
                t.width().unwrap_or(0)
            ));
        }
        Ok(Atom::Term(Term::Slice(Box::new(t), hi, lo)))
    }

    /// slice = "[" bit [ ":" bit ] "]": the value read since `at` sliced, if a `[hi:lo]` or a bit
    /// `[i]` follows (`(a + b)[7:0]`, `signed(x)[31:0]`).
    fn slice(&mut self, atom: Atom, at: &str) -> Result<Atom, String> {
        if !self.s.starts_with('[') {
            return Ok(atom);
        }
        let src = self.since(at);
        let t = self.term(atom, &src)?;
        self.eat("[");
        let range = self.s.split_once(']').map(|(r, _)| r).unwrap_or("");
        let bound = |b: &str| b.trim().parse::<u32>().ok();
        let (hi, lo) = match range.split_once(':') {
            Some((h, l)) => bound(h).zip(bound(l)),
            None => bound(range).map(|b| (b, b)),
        }
        .ok_or(format!("{src}[{range}]: a slice is [hi:lo], a bit [i]"))?;
        self.s = &self.s[range.len() + 1..];
        if hi < lo || hi >= MAX_RESULT {
            return Err(format!(
                "{src}[{hi}:{lo}]: a slice is [hi:lo], hi from lo to {}",
                MAX_RESULT - 1
            ));
        }
        Ok(Atom::Term(Term::Slice(Box::new(t), hi, lo)))
    }

    /// call = ("rose" | "fell" | "stable") "(" expr ")" | "past" "(" expr ["," count] ")", read
    /// after its `(`: a function of any expression over time (spec §8). `past` of a signal stays
    /// a signal, so it compares as one (`past(state) == READ`, `data != past(data)`); the others
    /// are `Expr::Func`, as SVA's.
    fn call(&mut self, f: &str) -> Result<Atom, String> {
        let key = self.len - self.s.len();
        if let Some((a, rest)) = self.calls.get(&key) {
            self.s = rest;
            return Ok(a.clone());
        }
        let a = self.read_call(f)?;
        self.calls.insert(key, (a.clone(), self.s));
        Ok(a)
    }

    fn read_call(&mut self, f: &str) -> Result<Atom, String> {
        self.s = self.s.trim_start();
        let at = self.s;
        // an operand (`valid`, `past(x)`), else a whole expression (`a && b`, `a + b`)
        let arg = match self.atom() {
            Ok(a) if self.s.trim_start().starts_with([',', ')']) => a,
            _ => {
                self.s = at;
                match self.cond()? {
                    Expr::Truth(t) => Atom::Term(t),
                    e => Atom::Term(Term::Bool(Box::new(e))),
                }
            }
        };
        let src = self.since(at);
        let count = if self.eat(",") {
            let n = self.value_word();
            if f != "past" {
                return Err(format!(
                    "{f}({src}, {n}): only past takes a count; {f}({src})"
                ));
            }
            match n.parse::<u32>() {
                Ok(c) if c > 0 => Some(c),
                _ => {
                    return Err(format!(
                        "past({src}, {n}): the count is a number of samples, past({src}, 2)"
                    ));
                }
            }
        } else {
            None
        };
        if !self.eat(")") {
            return Err(match count {
                Some(n) => format!("past({src}, {n}: missing ) after past's count"),
                None => format!("{f}({src}: missing ) after {f}'s argument"),
            });
        }
        let n = count.unwrap_or(1);
        // the first call in the text, outermost first, for `Query::needs_clock`
        let start = self.len - at.len();
        if self.first_call.as_ref().is_none_or(|c| start < c.0) {
            let call = match count {
                Some(n) => format!("{f}({src}, {n})"),
                None => format!("{f}({src})"),
            };
            self.first_call = Some((start, call));
        }
        Ok(match (f, arg) {
            ("past", Atom::Signal((name, sig, width, names, past, signed))) => {
                let name = match count {
                    Some(n) => format!("past({name}, {n})"),
                    None => format!("past({name})"),
                };
                Atom::Signal((name, sig, width, names, past.saturating_add(n), signed))
            }
            ("past", Atom::Term(mut t)) => {
                t.shift(n);
                Atom::Term(t)
            }
            (_, Atom::Signal((name, _, None, ..))) if f != "stable" => {
                return Err(format!("{f}({name}): not a bit signal"));
            }
            // as SVA's (spec §8): e now against e a sample earlier, a signal read as it is
            (_, a) => {
                let now = match a {
                    Atom::Signal((_, sig, _, _, past, _)) => Arg::Sig(sig, past),
                    Atom::Term(t) => Arg::Term(t),
                };
                let mut was = now.clone();
                was.shift(1);
                let f = match f {
                    "rose" => Func::Rose,
                    "fell" => Func::Fell,
                    _ => Func::Stable,
                };
                Atom::Term(Term::Bool(Box::new(Expr::Func { f, now, was })))
            }
        })
    }

    /// signal = a name, its bit or slice (`data[3]`, `data[7:4]`): its name, probe, width, value
    /// names, samples back (0; `past` adds them) and whether it reads signed.
    fn operand(&mut self) -> Result<Operand, String> {
        self.s = self.s.trim_start();
        let at = self.len - self.s.len();
        let name = self.word();
        if name.is_empty() {
            return Err(format!("expected a signal at: {}", self.s.trim()));
        }
        let (sig, width, names, signed) =
            (self.resolve)(&name).map_err(|e| Borrowed::of(&name).map_or(e, |b| b.to_string()))?;
        self.names.push((at..at + name.len(), sig));
        Ok((name, sig, width, names, 0, signed))
    }
}

/// An operand as a condition: a signal true when 1 / nonzero, a value when known and nonzero.
fn truth(a: Atom) -> Expr {
    match a {
        Atom::Signal((_, sig, _, _, past, _)) => Expr::Cmp {
            sig,
            past,
            op: Op::Eq,
            target: None,
        },
        Atom::Term(Term::Bool(e)) => *e,
        Atom::Term(t) => Expr::Truth(t),
    }
}

/// An SV number (`8'hFF`, `'b1010`, `4'sb1100`), its bits MSB first as wide as its size, or
/// without leading zeros if unsized; or `'0`, `'1`, `'x`, `'z`, as wide as what it meets.
#[derive(Debug, PartialEq)]
pub enum Sv {
    Bits {
        bits: String,
        signed: bool,
        sized: bool,
    },
    Fill(char),
}

/// Reads `text` as an SV number, if it is one (it has a `'`). Stricter than SV: a value that
/// doesn't fit its size is an error, not cut.
pub fn sv_literal(text: &str) -> Option<Result<Sv, String>> {
    let (size, rest) = text.split_once('\'')?;
    let form =
        || format!("{text}: an SV number is <size>'<b|o|d|h><digits>, e.g. 8'hFF, 'd12, 4'b10x0");
    let size = match size.replace('_', "") {
        s if s.is_empty() => None,
        s => match s.parse::<u32>() {
            Ok(n) if n > 0 => Some(n),
            _ => return Some(Err(form())),
        },
    };
    let rest = rest.to_lowercase();
    if size.is_none() && matches!(rest.as_str(), "0" | "1" | "x" | "z") {
        return Some(Ok(Sv::Fill(rest.chars().next().unwrap())));
    }
    let (signed, rest) = rest
        .strip_prefix('s')
        .map_or((false, rest.as_str()), |r| (true, r));
    let Some(base) = rest.chars().next() else {
        return Some(Err(form()));
    };
    let digits = rest[1..].replace('_', "").replace('?', "z");
    if digits.is_empty() {
        return Some(Err(form()));
    }
    let per = match base {
        'b' => 1,
        'o' => 3,
        'h' => 4,
        'd' if matches!(digits.as_str(), "x" | "z") => 0,
        'd' => match digits.parse::<UBig>() {
            Ok(n) => return Some(sized(text, format!("{n:b}"), size, signed)),
            Err(_) => return Some(Err(form())),
        },
        _ => return Some(Err(form())),
    };
    let mut bits = String::new();
    for c in digits.chars() {
        match (c, c.to_digit(1 << per.max(1))) {
            ('x' | 'z', _) => bits.push_str(&c.to_string().repeat(per.max(1))),
            (_, Some(d)) => bits.push_str(&format!("{d:0per$b}")),
            _ => return Some(Err(form())),
        }
    }
    Some(sized(text, bits, size, signed))
}

/// SV bits brought to their size: leading zeros dropped, then padded (with an x or z if that
/// leads, as SV does), or an error if the value is wider.
fn sized(text: &str, bits: String, size: Option<u32>, signed: bool) -> Result<Sv, String> {
    let fill = if bits.starts_with(['x', 'z']) {
        &bits[..1]
    } else {
        "0"
    };
    let bits = match bits.trim_start_matches('0') {
        "" => "0",
        b => b,
    };
    let Some(n) = size.map(|n| n as usize) else {
        return Ok(Sv::Bits {
            bits: bits.into(),
            signed,
            sized: false,
        });
    };
    if bits.len() > n {
        return Err(format!("{text}: the value doesn't fit in {n} bits"));
    }
    Ok(Sv::Bits {
        bits: format!("{}{bits}", fill.repeat(n - bits.len())),
        signed,
        sized: true,
    })
}

/// A number in an expression: decimal, `0x`, `0b` or `0o`, an SV number (`8'hFF`), maybe
/// negative, or a time (ticks).
fn number_text(text: &str, fpt: Option<u128>) -> Result<IBig, String> {
    if crate::time::has_unit(text) {
        return crate::time::parse(text, fpt)
            .map(|t| IBig::from(t.round() as i128))
            .map_err(String::from);
    }
    if let Some(sv) = sv_literal(text.trim_start_matches('-')) {
        let n = match sv? {
            Sv::Fill(c) => {
                return Err(format!(
                    "{text}: '{c} has no width in arithmetic; write one, as 32'h{}",
                    if c == '1' { "FFFFFFFF" } else { "0" }
                ));
            }
            Sv::Bits { bits, .. } if bits.contains(['x', 'z']) => {
                return Err(format!(
                    "{text}: arithmetic takes known numbers; compare with x instead (== x)"
                ));
            }
            Sv::Bits {
                bits,
                signed,
                sized,
            } => {
                let n = number(&bits).ok_or(format!("not a number: {text}"))?;
                if signed && sized {
                    to_signed(n, bits.len() as u32)
                } else {
                    n
                }
            }
        };
        return Ok(if text.starts_with('-') { -n } else { n });
    }
    let (neg, t) = text.strip_prefix('-').map_or((false, text), |t| (true, t));
    let t = t.replace('_', "").to_lowercase();
    let n = match t.get(..2) {
        Some("0x") => IBig::from_str_radix(&t[2..], 16),
        Some("0b") => IBig::from_str_radix(&t[2..], 2),
        Some("0o") => IBig::from_str_radix(&t[2..], 8),
        _ => t.parse(),
    }
    .map_err(|_| format!("not a number: {text}"))?;
    Ok(if neg { -n } else { n })
}

/// Whether `e` holds at `t` without a clock, which has no `past` (`Query::needs_clock`).
fn eval(trace: &Trace, e: &Expr, t: Tick) -> bool {
    holds(e, &|sig, _| Some(trace.value_of(sig.0, sig.1, t)))
}

/// Whether `e` holds, given `value(sig, n)`: sig's value `n` samples back, None where there is no
/// such sample (and Some(None) where the signal has no value there). Searches sample a trace;
/// `records` `where` samples a stream's records, a field per probe. Unknown is not a match.
pub fn holds<V: std::borrow::Borrow<Val>>(
    e: &Expr,
    value: &dyn Fn(Probe, u32) -> Option<Option<V>>,
) -> bool {
    holds3(e, value) == Some(true)
}

/// `holds`, three-valued as SV's conditions are (spec §8): None where an unknown decides it.
/// `&&` and `||` know their answer when one known side decides it (`0 && x` is 0); a comparison
/// with an unknown operand is unknown, except with `x` or `z`, which ask about unknown bits.
fn holds3<V: std::borrow::Borrow<Val>>(
    e: &Expr,
    value: &dyn Fn(Probe, u32) -> Option<Option<V>>,
) -> Option<bool> {
    use std::cmp::Ordering::{self, *};
    let compare = |op: Op, order: Option<Ordering>| {
        order.map(|o| match op {
            Op::Eq => o == Equal,
            Op::Ne => o != Equal,
            Op::Lt => o == Less,
            Op::Le => o != Greater,
            Op::Gt => o == Greater,
            Op::Ge => o != Less,
        })
    };
    match e {
        Expr::Or(a, b) => match (holds3(a, value), holds3(b, value)) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        },
        Expr::And(a, b) => match (holds3(a, value), holds3(b, value)) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        },
        Expr::Not(a) => holds3(a, value).map(|h| !h),
        Expr::Func { f, now, was } => {
            let (now, was) = (now.read(value), was.read(value));
            let bit = |v: &Option<Four<V>>| v.as_ref().and_then(Four::lsb);
            Some(match f {
                Func::Rose => bit(&now) == Some(true) && bit(&was) != Some(true),
                Func::Fell => bit(&now) == Some(false) && bit(&was) != Some(false),
                Func::Stable => match (&now, &was) {
                    (Some(a), Some(b)) => a.same(b),
                    (a, b) => a.is_none() && b.is_none(),
                },
            })
        }
        Expr::Signals {
            a,
            a_past,
            op,
            b,
            b_past,
        } => {
            let (x, y) = (value(*a, *a_past).flatten(), value(*b, *b_past).flatten());
            let order = match (
                x.as_ref().map(|v| v.borrow()),
                y.as_ref().map(|v| v.borrow()),
            ) {
                (Some(Val::Bits(x)), Some(Val::Bits(y))) => {
                    unsigned(x).zip(unsigned(y)).map(|(x, y)| x.cmp(&y))
                }
                (Some(Val::Text(x)), Some(Val::Text(y))) => Some(x.cmp(y)),
                _ => None,
            };
            compare(*op, order)
        }
        Expr::Truth(t) => t.value(value).map(|v| v != IBig::ZERO),
        Expr::Contains { sig, past, text } => value(*sig, *past)
            .flatten()
            .map(|v| matches!(v.borrow(), Val::Text(t) if t.contains(text.as_str()))),
        Expr::Arith { a, op, b } => {
            let x = a.value(value);
            // `== x`: whether the result is unknown, always known
            let Some(b) = b else {
                return Some(x.is_none() == (*op == Op::Eq));
            };
            compare(*op, x.zip(b.value(value)).map(|(x, y)| x.cmp(&y)))
        }
        Expr::Cmp {
            sig,
            past,
            op,
            target,
        } => {
            let sample = value(*sig, *past);
            // before the first sample the value is x, a 4-state signal's default (spec §8)
            let before = sample.is_none();
            let v = sample.flatten();
            let v = v.as_ref().map(|v| v.borrow());
            let unknown = |b: &str| b.contains(['x', 'u', 'w', '-', 'z']);
            match (target, v) {
                // `== x`, `== z`: whether it has such a bit, always known
                (Some(t @ (Target::Unknown | Target::HighZ)), _) => {
                    let has = matches(t, v) || (before && *t == Target::Unknown);
                    Some(has == (*op == Op::Eq))
                }
                (Some(t @ Target::Bits(p)), Some(Val::Bits(b))) => match (unsigned(b), unsigned(p))
                {
                    (Some(b), Some(p)) => compare(*op, Some(b.cmp(&p))),
                    // a value with x or z bits (`4'b1x01`) matches them exactly
                    (_, None) if matches!(op, Op::Eq | Op::Ne) => {
                        Some(matches(t, v) == (*op == Op::Eq))
                    }
                    _ => None,
                },
                (Some(Target::Text(p)), Some(Val::Text(t))) => {
                    compare(*op, Some(t.as_str().cmp(p.as_str())))
                }
                // as in an `if`: some bit is 1; unknown with an unknown bit
                (None, Some(Val::Bits(b))) if unknown(b) => None,
                (None, Some(Val::Bits(b))) => Some(b.contains(['1', 'h'])),
                (None, Some(_)) => Some(false),
                // no value there
                _ => None,
            }
        }
    }
}

/// The signals an expression reads, each with how many samples back.
pub fn probes(e: &Expr, out: &mut Vec<(Probe, u32)>) {
    match e {
        Expr::Or(a, b) | Expr::And(a, b) => {
            probes(a, out);
            probes(b, out);
        }
        Expr::Not(a) => probes(a, out),
        Expr::Cmp { sig, past, .. } => out.push((*sig, *past)),
        Expr::Signals {
            a,
            a_past,
            b,
            b_past,
            ..
        } => out.extend([(*a, *a_past), (*b, *b_past)]),
        Expr::Contains { sig, past, .. } => out.push((*sig, *past)),
        Expr::Truth(t) => t.probes(out),
        Expr::Arith { a, b, .. } => {
            a.probes(out);
            b.iter().for_each(|b| b.probes(out));
        }
        Expr::Func { now, was, .. } => {
            now.probes(out);
            was.probes(out);
        }
    }
}

/// `e` read `n` samples earlier, `past(e, n)`: every read in it shifted, so it costs nothing to
/// search.
fn shift(e: &mut Expr, n: u32) {
    match e {
        Expr::Or(a, b) | Expr::And(a, b) => {
            shift(a, n);
            shift(b, n);
        }
        Expr::Not(a) => shift(a, n),
        Expr::Cmp { past, .. } | Expr::Contains { past, .. } => *past = past.saturating_add(n),
        Expr::Signals { a_past, b_past, .. } => {
            *a_past = a_past.saturating_add(n);
            *b_past = b_past.saturating_add(n);
        }
        Expr::Truth(t) => t.shift(n),
        Expr::Arith { a, b, .. } => {
            a.shift(n);
            if let Some(b) = b {
                b.shift(n);
            }
        }
        Expr::Func { now, was, .. } => {
            now.shift(n);
            was.shift(n);
        }
    }
}

/// The next (previous) match after (before) `from`: where the expression becomes true. With a
/// clock, the start of each clock cycle (a rising edge) whose ending edge samples it true, as a
/// flip-flop does just before the edge: the cursor lands where the transfer's cycle begins.
pub fn find_query(trace: &Trace, q: &Query, from: Tick, forward: bool) -> Option<Tick> {
    if q.clock.is_some() {
        let s = crate::query::Search {
            from,
            to: None,
            forward,
            merge: false,
            runs: 1,
            matches: 1,
        };
        return crate::query::find(trace, q, &s)
            .runs
            .first()
            .map(|r| r.first);
    }
    let held = |t: Tick| t.checked_sub(1).is_some_and(|p| eval(trace, &q.expr, p));
    let mut t = from;
    // a value of one signal (`/0x40`): one walk over its changes
    if let Expr::Cmp {
        sig,
        past: 0,
        op: Op::Eq,
        target: Some(target),
    } = &q.expr
    {
        return find(trace, sig.0, sig.1, target, from, forward);
    }
    let mut sigs = Vec::new();
    probes(&q.expr, &mut sigs);
    loop {
        let next = sigs.iter().filter_map(|&((v, b), _)| {
            if forward {
                trace.next_change_of(v, b, t)
            } else {
                trace.prev_change_of(v, b, t)
            }
        });
        t = if forward { next.min()? } else { next.max()? };
        if eval(trace, &q.expr, t) && !held(t) {
            return Some(t);
        }
    }
}

/// Bits as an unsigned number that orders as one: no leading zeros, compared by length, then
/// digits; None with an unknown or high-impedance bit.
fn unsigned(bits: &str) -> Option<(usize, String)> {
    let b: String = bits
        .chars()
        .map(|c| match c {
            'h' => Some('1'),
            'l' => Some('0'),
            '0' | '1' => Some(c),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let b = b.trim_start_matches('0').to_string();
    Some((b.len(), b))
}

fn matches(target: &Target, v: Option<&Val>) -> bool {
    match (target, v) {
        (Target::Unknown, Some(Val::Bits(b))) => b.contains(['x', 'u', 'w', '-']),
        (Target::HighZ, Some(Val::Bits(b))) => b.contains('z'),
        (Target::Bits(p), Some(Val::Bits(b))) => b
            .chars()
            .map(|c| match c {
                'h' => '1',
                'l' => '0',
                c => c,
            })
            .eq(p.chars()),
        (Target::Text(p), Some(Val::Text(t))) => p == t,
        _ => false,
    }
}

/// The next (or previous) time after (before) `from` where the signal changes to the target, so
/// a match is the start of a matching segment.
pub fn find(
    trace: &Trace,
    var: usize,
    bit: Option<u32>,
    target: &Target,
    from: Tick,
    forward: bool,
) -> Option<Tick> {
    find_all(trace, var, bit, target, from, forward).next()
}

/// Every such time, nearest first, from one walk over the signal's changes.
// shortcut (#15): looks at every change; index values if searching huge traces is too slow.
pub fn find_all<'a>(
    trace: &'a Trace,
    var: usize,
    bit: Option<u32>,
    target: &'a Target,
    from: Tick,
    forward: bool,
) -> impl Iterator<Item = Tick> + 'a {
    let mut changes = trace.walk(var, bit, from, forward).peekable();
    // forward, whether the change before matched
    let mut last = None;
    std::iter::from_fn(move || {
        while let Some((x, v)) = changes.next() {
            // the value just before x: forward the last change's, backward the next one's
            let before = match (forward, last) {
                (true, Some(m)) => m,
                (false, _) if let Some(c) = changes.peek() => matches(target, c.1.as_ref()),
                _ => x
                    .checked_sub(1)
                    .is_some_and(|p| matches(target, trace.value_of(var, bit, p).as_ref())),
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
        assert_eq!(
            parse("0x100", Some(8), None),
            Err("0x100 doesn't fit in 8 bits".into())
        );
        assert_eq!(
            parse("-129", Some(8), None),
            Err("-129 doesn't fit in 8 bits".into())
        );
        assert_eq!(parse("abc", Some(8), None), Err("not a value: abc".into()));
    }

    #[test]
    fn finds() {
        let mut t =
            Trace::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/data/simple.vcd").as_ref())
                .unwrap();
        let (d, clk) = (t.by_path["top.cpu.data"], t.by_path["top.clk"]);
        t.load(&[d, clk]);
        let find = |var, target: &str, from, fwd| {
            find(
                &t,
                var,
                None,
                &parse(target, t.vars[var].width, None).unwrap(),
                from,
                fwd,
            )
        };
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
        let mut t =
            Trace::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/data/simple.vcd").as_ref())
                .unwrap();
        t.load(&(0..t.vars.len()).collect::<Vec<_>>());
        let resolve = |n: &str| {
            t.by_path
                .get(&format!("top.{n}"))
                .map(|&v| ((v, None), t.vars[v].width, None, false))
                .ok_or(format!("unknown signal: {n}"))
        };
        let q = |s: &str| parse_query(s, None, &mut resolve.clone()).unwrap();
        let find = |s: &str, from, fwd| find_query(&t, &q(s), from, fwd);
        // busy is 1 from 20 to 40; data 0x96 @20, 0xE1 @30, z @50; clk rises at 5, 15, 25, 35, 45, ...
        assert_eq!(find("cpu.busy && cpu.data == 0x96", 0, true), Some(20));
        assert_eq!(find("cpu.busy && !(cpu.data == z)", 0, true), Some(20));
        assert_eq!(find("cpu.busy && cpu.data != 0x96", 0, true), Some(30));
        // a match is where it becomes true: still true when 0x96 turns into 0xE1 at 30
        assert_eq!(
            find("cpu.data == 0x96 || cpu.data == 0xe1", 0, true),
            Some(20)
        );
        assert_eq!(find("cpu.data == 0x96 || cpu.data == 0xe1", 20, true), None);
        assert_eq!(find("cpu.busy", 70, false), Some(20));
        // unsigned order: 0x43 @10 is the first above 0x40, 0x3A @40 the first below; z never compares
        assert_eq!(find("cpu.data > 0x40", 0, true), Some(10));
        assert_eq!(find("cpu.data>=0x96&&cpu.data<=0x96", 0, true), Some(20));
        assert_eq!(find("cpu.data < 0x40", 0, true), Some(40));
        assert_eq!(find("cpu.data < 0x40", 41, true), None);
        // !(data < 0x30) is 1, then x where data is z, then 1 again: a second match at 60
        assert_eq!(find("!(cpu.data < 0x30)", 10, true), Some(60));
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
        assert_eq!(
            err("cpu.busy )"),
            format!("unexpected: ) (operators: {OPERATORS})")
        );
        assert_eq!(
            err("cpu.data < z"),
            "cpu.data < z: < compares numbers; x, z and text only with == or !="
        );
        assert_eq!(
            err("cpu.state == 0x10"),
            "cpu.state: 0x10 doesn't fit in 3 bits"
        );
        assert_eq!(
            err("cpu.data == 40ns"),
            "cpu.data: file has no timescale; use ticks"
        );
        assert_eq!(
            err("cpu.data + 40ns > 0"),
            "file has no timescale; use ticks"
        );
        // words of other languages: the form meant
        assert_eq!(
            err("@clk rising"),
            "rising: `@clk` already samples at the clock's rising edges; an edge of a signal is rose(x), SVA's $rose without the $"
        );
        assert_eq!(
            err("prev(cpu.busy) && cpu.busy"),
            "prev(cpu.busy): the value a cycle earlier is past(cpu.busy), SVA's $past without the $"
        );
        assert_eq!(
            err("$rose(cpu.busy)"),
            "$rose(cpu.busy): an edge is rose(cpu.busy), SVA's $rose without the $"
        );
        assert_eq!(
            err("@clk cpu.busy falling"),
            "falling: `@clk` already samples at the clock's rising edges; an edge of a signal is fell(x), SVA's $fell without the $"
        );
        assert_eq!(
            err("cpu.busy=1 && cpu.data == 0"),
            "=1 && cpu.data == 0: compare with ==, not ="
        );
        assert_eq!(
            err("@clk changed(cpu.data)"),
            "changed(cpu.data): a change is !stable(cpu.data), SVA's $changed"
        );
        assert_eq!(
            err("next(cpu.data) == 0"),
            "next(cpu.data): an expression reads no later value; search on from a match instead (`find` `from`)"
        );
        // a function of an expression: errors name the function and what it expected
        assert_eq!(
            err("rose(cpu.busy && cpu.data == 0"),
            "rose(cpu.busy && cpu.data == 0: missing ) after rose's argument"
        );
        assert_eq!(
            err("!past(cpu.busy, 2"),
            "past(cpu.busy, 2: missing ) after past's count"
        );
        for count in ["cpu.data", "0", "-1"] {
            assert_eq!(
                err(&format!("past(cpu.busy, {count})")),
                format!(
                    "past(cpu.busy, {count}): the count is a number of samples, past(cpu.busy, 2)"
                )
            );
        }
        assert_eq!(
            err("rose(cpu.busy, 2)"),
            "rose(cpu.busy, 2): only past takes a count; rose(cpu.busy)"
        );
        assert_eq!(err("fell(cpu.busy && nope)"), "unknown signal: nope");
        // borrowed words quote an expression argument back as written
        assert_eq!(
            err("$rose(cpu.busy && (cpu.data == 1 || cpu.data == 2))"),
            "$rose(cpu.busy && (cpu.data == 1 || cpu.data == 2)): an edge is rose(cpu.busy && (cpu.data == 1 || cpu.data == 2)), SVA's $rose without the $"
        );
        assert_eq!(
            err("!prev(cpu.busy && cpu.data == 0)"),
            "prev(cpu.busy && cpu.data == 0): the value a cycle earlier is past(cpu.busy && cpu.data == 0), SVA's $past without the $"
        );
        assert_eq!(
            err("rose(cpu.busy) && $stable(cpu.data + 1)"),
            "$stable(cpu.data + 1): stable(cpu.data + 1), SVA's $stable without the $"
        );
    }
    /// `text` over one bits signal `v`, as wide as `now`, read now and a sample earlier (`was`
    /// None: no sample before).
    fn sampled(text: &str, was: Option<&str>, now: &str) -> bool {
        let q = parse_query(text, None, &mut |n| match n {
            "v" => Ok(((0, None), Some(now.len() as u32), None, false)),
            _ => Err(format!("unknown signal: {n}")),
        })
        .unwrap();
        holds(&q.expr, &|_, back| match back {
            0 => Some(Some(Val::Bits(now.into()))),
            1 => was.map(|w| Some(Val::Bits(w.into()))),
            _ => None,
        })
    }

    #[test]
    fn sampled_value_functions() {
        // IEEE 1800 §16.9.3: rose and fell on bit 0, from anything else, x and z too
        assert!(sampled("rose(v)", Some("0000"), "0001"));
        assert!(sampled("rose(v)", Some("0010"), "0011"));
        assert!(!sampled("rose(v)", Some("0001"), "0011"));
        assert!(sampled("rose(v)", Some("xxxx"), "0001"));
        assert!(sampled("rose(v)", Some("000z"), "0001"));
        assert!(sampled("rose(v)", None, "0001"));
        assert!(!sampled("rose(v)", Some("0000"), "000x"));
        assert!(sampled("!rose(v)", Some("0000"), "000x"));
        assert!(sampled("fell(v)", Some("0001"), "0000"));
        assert!(sampled("fell(v)", Some("xxxx"), "1110"));
        assert!(sampled("fell(v)", None, "0000"));
        assert!(!sampled("fell(v)", Some("0000"), "0000"));
        // stable: the whole value, as ===
        assert!(sampled("stable(v)", Some("0101"), "0101"));
        assert!(sampled("stable(v)", Some("xxxx"), "xxxx"));
        assert!(sampled("stable(v)", Some("1x0z"), "1x0z"));
        assert!(!sampled("stable(v)", Some("1x00"), "0x00"));
        assert!(!sampled("stable(v)", Some("zzzz"), "xxxx"));
        assert!(sampled("stable(v)", None, "xxxx"));
        assert!(!sampled("stable(v)", None, "0000"));
        assert!(sampled("!stable(v)", Some("0x00"), "1x00"));
        // of an expression: its value, x as a whole with an unknown bit
        assert!(sampled("rose(v == 3)", Some("0010"), "0011"));
        assert!(sampled("rose(v == 3)", Some("xxxx"), "0011"));
        assert!(sampled("rose(v + 1)", Some("0001"), "0010"));
        assert!(sampled("stable(v + 1)", Some("000x"), "x000"));
        assert!(!sampled("stable(v + 1)", Some("0001"), "0010"));
        // before the first sample, x
        assert!(sampled("past(v) == x", None, "0000"));
        assert!(!sampled("past(v) == x", Some("0000"), "0000"));
        // a search with one needs a clock: the first call, outermost, and the text with one
        let q = |s: &str| {
            parse_query(s, None, &mut |n| match n {
                "v" | "clk" => Ok(((0, None), Some(1), None, false)),
                _ => Err(format!("unknown signal: {n}")),
            })
            .unwrap()
        };
        let clock = || "top.clk".to_string();
        assert_eq!(
            q("v && rose(past(v, 2))").needs_clock("v && rose(past(v, 2))", clock),
            Err("rose(past(v, 2)) needs a clock, as in SVA: @top.clk v && rose(past(v, 2))".into())
        );
        assert_eq!(q("@clk rose(v)").needs_clock("@clk rose(v)", clock), Ok(()));
        assert_eq!(q("v == 1").needs_clock("v == 1", clock), Ok(()));
    }

    /// Signals for arithmetic: a, b 32 bits, m, n 4, s 8 (signed, as `x - y` makes), t text, w
    /// 128 bits all ones, u 8 bits unknown; each with one value.
    fn arith(text: &str) -> Result<bool, String> {
        let ones = "1".repeat(128);
        let sigs: [(&str, Option<u32>, bool, &str); 8] = [
            ("a", Some(32), false, "11111111111111111111111111111111"),
            ("b", Some(32), false, "00000000000000000000000000000001"),
            ("m", Some(4), false, "0100"),
            ("s", Some(8), true, "11111100"),
            ("t", None, false, "idle"),
            ("w", Some(128), false, &ones),
            ("n", Some(4), false, "1111"),
            ("u", Some(8), false, "x"),
        ];
        let q = parse_query(text, None, &mut |n| {
            let i = sigs
                .iter()
                .position(|s| s.0 == n)
                .ok_or(format!("unknown signal: {n}"))?;
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
        // & bounds a shift amount; ! is an operand too; slices and bits of a name without its own
        yes("1 << (a & 3) == 8");
        yes("(m == 4) != !b");
        yes("!(m == 3) == 1");
        yes("m[2] == 1 && m[3:2] == 1 && m[1:0] == 0");
        // any width: wide operands, results past 128 bits, exact
        yes("w + 1 == 0x100000000000000000000000000000000");
        yes("(w + 1)[128:128] == 1");
        yes("signed(w) == -1");
        yes("a * a * a * a == 0xFFFFFFFC00000005FFFFFFFC00000001");
        yes("(w << 100) >> 100 == w");
        yes("w[127:64] == 0xFFFFFFFFFFFFFFFF");
    }

    #[test]
    fn arithmetic_unknowns() {
        // u is unknown: its result too, which equals x; a comparison with a number is unknown,
        // so neither it nor its negation matches
        assert_eq!(arith("u + 1 == x"), Ok(true));
        assert_eq!(arith("u + 1 != x"), Ok(false));
        assert_eq!(arith("u + 1 != 5"), Ok(false));
        assert_eq!(arith("!(u + 1 == 5)"), Ok(false));
        assert_eq!(arith("u + 1 == 5"), Ok(false));
        assert_eq!(arith("u + 1 < 5"), Ok(false));
        assert_eq!(arith("!(u + 1 < 5)"), Ok(false));
        assert_eq!(arith("u & 1"), Ok(false));
        assert_eq!(arith("!(u & 1)"), Ok(false));
        assert_eq!(arith("u != 1"), Ok(false));
        assert_eq!(arith("u != m"), Ok(false));
        assert_eq!(arith("!u"), Ok(false));
        assert_eq!(arith("u == x && !(u != x)"), Ok(true));
        // && and || know what one known side decides, as SV
        assert_eq!(arith("!(m == 5 && u == 1)"), Ok(true));
        assert_eq!(arith("!(m == 4 && u == 1)"), Ok(false));
        assert_eq!(arith("m == 4 || u == 1"), Ok(true));
        assert_eq!(arith("!(m == 5 || u == 1)"), Ok(false));
        // as a value: unknown
        assert_eq!(arith("(u == 1) + 1 == x"), Ok(true));
    }

    #[test]
    fn contains_text() {
        let yes = |s: &str| assert_eq!(arith(s), Ok(true), "{s}");
        let no = |s: &str| assert_eq!(arith(s), Ok(false), "{s}");
        yes(r#"contains(t, "dl")"#);
        yes(r#"contains(t, "idle")"#);
        no(r#"contains(t, "DL")"#);
        no(r#"contains(t, "id le")"#);
        no(r#"contains(t, "e)")"#);
        yes(r#"!contains(t, "x") && m == 4"#);
        yes(r#"(contains( t , "id") ? 1 : 0) == 1"#);
        yes(r#"contains(past(t), "dl") || m == 4"#);
        assert_eq!(
            arith(r#"contains(m, "a")"#),
            Err(
                r#"contains(m, ...): m is a bit signal; contains reads a string signal's text"#
                    .into()
            )
        );
        assert_eq!(
            arith(r#"contains(~m, "a")"#),
            Err(
                r#"contains(~m, ...): ~m is a value; contains reads a string signal's text"#.into()
            )
        );
        assert_eq!(
            arith(r#"contains(t, dl")"#),
            Err(r#"contains(t, ...): write the text in quotes, contains(t, "text")"#.into())
        );
        assert_eq!(
            arith(r#"contains(t, "dl)"#),
            Err(r#"contains(t, ...): missing closing ""#.into())
        );
        assert_eq!(arith(r#"contains(t, "dl""#), Err("missing )".into()));
        // without a quote, a VHDL name as before
        assert_eq!(
            arith("contains(3) == 1"),
            Err("unknown signal: contains(3)".into())
        );
        assert_eq!(
            arith(r#"contains(3) == 1 && t == "idle""#),
            Err("unknown signal: contains(3)".into())
        );
    }

    #[test]
    fn sv_literals() {
        let bits = |b: &str, signed, sized| {
            Some(Ok(Sv::Bits {
                bits: b.into(),
                signed,
                sized,
            }))
        };
        assert_eq!(sv_literal("8'hF"), bits("00001111", false, true));
        assert_eq!(sv_literal("'hF_F"), bits("11111111", false, false));
        assert_eq!(sv_literal("4'b10x0"), bits("10x0", false, true));
        assert_eq!(sv_literal("8'hx"), bits("xxxxxxxx", false, true));
        assert_eq!(sv_literal("12'd100"), bits("000001100100", false, true));
        assert_eq!(sv_literal("4'sb1100"), bits("1100", true, true));
        assert_eq!(sv_literal("3'o7"), bits("111", false, true));
        assert_eq!(sv_literal("'1"), Some(Ok(Sv::Fill('1'))));
        assert_eq!(sv_literal("12"), None);
        assert_eq!(
            sv_literal("4'hFF"),
            Some(Err("4'hFF: the value doesn't fit in 4 bits".into()))
        );
        assert_eq!(
            sv_literal("8'q1"),
            Some(Err(
                "8'q1: an SV number is <size>'<b|o|d|h><digits>, e.g. 8'hFF, 'd12, 4'b10x0".into()
            ))
        );
        // as values of a signal, and in arithmetic
        let bits = |s: &str| Ok(Target::Bits(s.into()));
        assert_eq!(parse("8'h40", Some(8), None), bits("01000000"));
        assert_eq!(parse("'1", Some(4), None), bits("1111"));
        assert_eq!(parse("4'b1x01", Some(6), None), bits("001x01"));
        assert_eq!(
            parse("'h1FF", Some(8), None),
            Err("'h1FF doesn't fit in 8 bits".into())
        );
        let yes = |s: &str| assert_eq!(arith(s), Ok(true), "{s}");
        yes("m == 4'b0100");
        yes("m == 'h4 && m != 1'b0");
        yes("a == '1");
        yes("a + 1'b1 == 33'h1_0000_0000");
        yes("4'sb1111 == -1");
        yes("s == 8'sd252 && s == -8'd4");
        assert_eq!(
            arith("a + '1 == 0"),
            Err("'1: '1 has no width in arithmetic; write one, as 32'hFFFFFFFF".into())
        );
        assert_eq!(
            arith("a + 4'b1x00 == 0"),
            Err("4'b1x00: arithmetic takes known numbers; compare with x instead (== x)".into())
        );
    }

    #[test]
    fn division_ternary_slices() {
        let yes = |s: &str| assert!(arith(s).expect(s), "{s}");
        let no = |s: &str| assert!(!arith(s).expect(s), "{s}");
        // `/` and `%` bind like `*`, truncate toward zero, and by zero give x
        yes("a / b == a");
        yes("m + 6 / 4 == 5");
        yes("(s - 3) / 2 == -3");
        yes("(s - 3) % 2 == -1");
        yes("m % 3 == 1");
        yes("m / 0 == x");
        yes("m % (b - 1) == x");
        // `c ? a : b`, below `||`; an unknown condition gives a if a == b, else x, and && and ||
        // know a condition one known side decides
        yes("(m == 4 ? 1 : 0) == 1");
        yes("(m == 5 || b ? a : 0) == a");
        yes("(m[2] ? -4096 : 0) + 1 == -4095");
        yes("(u ? 1 : 0) == x");
        yes("(u ? 3 : 3) == 3");
        yes("(m == 5 && u ? 1 : 2) == 2");
        yes("(m == 4 || u ? 1 : 2) == 1");
        yes("(m == 4 && u ? 1 : 2) == x");
        yes("m == 4 ? 1 : 0");
        no("m == 5 ? 1 : 0");
        // a bit of a parenthesised value, a slice of signed(x)
        yes("(m + 0)[2] == 1");
        yes("signed(s)[15:0] == 0xFFFC");
        // a field's slice on the right
        yes("m == m[3:0]");
        no("n == m[3:0]");
    }

    #[test]
    fn arithmetic_errors() {
        let err = |s: &str| arith(s).expect_err(s);
        assert_eq!(
            err("(m ? 1) == 0"),
            "? is c ? a : b, with a nested ? in parentheses: c ? a : (d ? e : f); at: ) == 0"
        );
        assert_eq!(
            err("m ? 1 : b ? 2 : 3"),
            "? is c ? a : b, with a nested ? in parentheses: c ? a : (d ? e : f); at: ? 2 : 3"
        );
        assert_eq!(err("m == nope"), "m: not a value: nope");
        assert_eq!(
            err("m & 3 == 0"),
            "m & 3 == 0: & and == need parentheses: (m & 3) == 0 or m & (3 == 0)"
        );
        assert_eq!(
            err("0 == m & 3"),
            "0 == m & 3: == and & need parentheses: 0 == (m & 3) or (0 == m) & 3"
        );
        assert_eq!(
            err("a + m & 3 == 0"),
            "a + m & 3: + and & need parentheses: (a + m) & 3 or a + (m & 3)"
        );
        assert_eq!(
            err("a << b == 0"),
            "a << b: b is 32 bits, so the result could be 2^32 bits; shift by a slice, a << b[14:0]"
        );
        assert_eq!(
            err("a << 70000 == 0"),
            "a << 70000: the result could be 70032 bits, more than 65536; slice an operand to fewer bits"
        );
        assert_eq!(
            err("a << s == 0"),
            "a << s: s can be negative; shift by an unsigned amount"
        );
        assert_eq!(
            err("~(a + b) == 0"),
            "~(a + b): ~ inverts bits at a width, so give one with a slice: ~(a + b)[31:0]"
        );
        assert_eq!(
            err("signed(a + b) == 0"),
            "signed(a + b): signed reads bits at a width, so give one with a slice: signed((a + b)[31:0])"
        );
        assert_eq!(
            err("t + 1 == 0"),
            "t is a real or string signal; arithmetic takes numbers"
        );
        assert_eq!(
            err("a + 1 < x"),
            "a + 1 < x: < compares numbers; x only with == or !="
        );
        assert_eq!(
            err("a + 1 == z"),
            "a + 1 == z: an arithmetic result is a number or x"
        );
        assert_eq!(err("m[4:0] == 0"), "m[4:0]: m has no bits 4:0 (4 bits)");
        assert_eq!(err("q[1:0] == 0"), "unknown signal: q[1:0]");
        assert_eq!(
            err("(a + b)[70000:0] == 0"),
            "(a + b)[70000:0]: a slice is [hi:lo], hi from lo to 65535"
        );
    }
}
