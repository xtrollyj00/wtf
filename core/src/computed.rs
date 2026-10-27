//! Signals computed from names (spec §16): bit slices `data[63:32]` and
//! concatenations `{a, b}` (most significant first, as in Verilog). Their values are worked out
//! once, from the changes of the signals they are made of, into a computed var (`Trace`). Also
//! expressions (`pc + 4`, `signed(insn[31:20])`, spec §16), evaluated at each change.

use crate::time::Tick;
use crate::search::Term;
use crate::trace::{Trace, Val};

/// What a name computes.
#[derive(Debug, PartialEq)]
pub enum Form<'a> {
    /// `base[hi:lo]`
    Slice(&'a str, u32, u32),
    /// `{a, b, c}`
    Concat(Vec<&'a str>),
    /// `pc + 4`: an expression of section 8
    Expr(&'a str),
}

/// The form of a name, if it computes anything. `data[3]` is a bit, not a slice, and computes
/// nothing; names that aren't well formed compute nothing and are looked up as they are.
pub fn form(name: &str) -> Option<Form<'_>> {
    let name = name.trim();
    if let Some(inner) = name.strip_prefix('{').and_then(|n| n.strip_suffix('}'))
        && depth_ok(inner)
    {
        let mut parts = Vec::new();
        let (mut depth, mut start) = (0, 0);
        for (i, c) in inner.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(inner[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
        parts.push(inner[start..].trim());
        return Some(Form::Concat(parts));
    }
    if is_expr(name) {
        return Some(Form::Expr(name));
    }
    let (base, range) = name.strip_suffix(']')?.rsplit_once('[')?;
    let (hi, lo) = range.split_once(':')?;
    Some(Form::Slice(base.trim(), hi.trim().parse().ok()?, lo.trim().parse().ok()?))
}

/// An operator outside brackets and braces, or a leading `(`, `~` or `signed(`. A `*` at an end
/// or next to `.` or `*` is a pattern's (`top.cpu.*`, `**.valid`), not a product.
fn is_expr(name: &str) -> bool {
    let mut depth = 0i32;
    let b = name.as_bytes();
    let glob = |i: usize| i == 0 || i + 1 == b.len() || b".*".contains(&b[i - 1]) || b".*".contains(&b[i + 1]);
    name.starts_with(['(', '~'])
        || name.starts_with("signed(")
        || name.char_indices().any(|(i, c)| {
            depth += match c {
                '[' | '{' => 1,
                ']' | '}' => -1,
                _ => 0,
            };
            depth == 0 && ("+-&|^~<>=!/%?".contains(c) || (c == '*' && !glob(i)))
        })
}

/// Braces balance, so `{a}, {b}` isn't read as one concatenation.
fn depth_ok(s: &str) -> bool {
    let mut depth = 0i32;
    s.chars().all(|c| {
        depth += match c {
            '{' => 1,
            '}' => -1,
            _ => 0,
        };
        depth >= 0
    }) && depth == 0
}

/// The times a var has a value from: its first, then each change.
fn times(trace: &Trace, var: usize) -> Vec<Tick> {
    let Some(first) = trace.first_time(var) else { return vec![] };
    std::iter::once(first).chain(trace.walk(var, None, first, true).map(|c| c.0)).collect()
}

/// A var's bits at `t`, as wide as it is; `x`s where it has no value (yet).
fn bits(trace: &Trace, var: usize, w: usize, t: Tick) -> String {
    match trace.value_at(var, t) {
        Some(Val::Bits(b)) if b.len() == w => b,
        _ => "x".repeat(w),
    }
}

fn width(trace: &Trace, var: usize) -> Result<u32, String> {
    trace.vars[var].width.ok_or(format!("{} is a real or string signal, not bits", trace.vars[var].path))
}

/// All of a var's values, its first then each change.
pub fn values(trace: &mut Trace, var: usize) -> Vec<(Tick, Val)> {
    trace.load(&[var]);
    times(trace, var).into_iter().filter_map(|t| Some((t, trace.value_at(var, t)?))).collect()
}

/// The values of bits `hi` down to `lo` of a bus.
pub fn slice(trace: &mut Trace, var: usize, hi: u32, lo: u32) -> Result<Vec<(Tick, Val)>, String> {
    let w = width(trace, var)?;
    if hi < lo || hi >= w {
        return Err(format!("{} has no bits {hi}:{lo} ({w} bits)", trace.vars[var].path));
    }
    trace.load(&[var]);
    let (a, b) = ((w - 1 - hi) as usize, (w - lo) as usize);
    Ok(times(trace, var).into_iter().map(|t| (t, Val::Bits(bits(trace, var, w as usize, t)[a..b].to_string()))).collect())
}

/// The values of signals side by side, the first one's bits most significant.
pub fn concat(trace: &mut Trace, vars: &[usize]) -> Result<(u32, Vec<(Tick, Val)>), String> {
    let widths = vars.iter().map(|&v| width(trace, v)).collect::<Result<Vec<u32>, String>>()?;
    trace.load(vars);
    let mut all: Vec<Tick> = vars.iter().flat_map(|&v| times(trace, v)).collect();
    all.sort();
    all.dedup();
    let value = |t| Val::Bits(vars.iter().zip(&widths).map(|(&v, &w)| bits(trace, v, w as usize, t)).collect());
    Ok((widths.iter().sum(), all.into_iter().map(|t| (t, value(t))).collect()))
}

/// The values of an expression, at each change of the signals it reads, as `shape` bits (two's
/// complement); `x`s where it is unknown.
pub fn term(trace: &mut Trace, t: &Term) -> Vec<(Tick, Val)> {
    let mut probes = Vec::new();
    t.probes(&mut probes);
    let vars: Vec<usize> = probes.iter().map(|p| p.0.0).collect();
    trace.load(&vars);
    let mut all: Vec<Tick> = vars.iter().flat_map(|&v| times(trace, v)).collect();
    all.sort();
    all.dedup();
    let w = t.shape().bits as usize;
    let bits = |at| match t.value(&|p, _| Some(trace.value_of(p.0, p.1, at))) {
        Some(n) => format!("{:0w$b}", (n as u128) & ((1u128 << w) - 1)),
        None => "x".repeat(w),
    };
    all.into_iter().map(|at| (at, Val::Bits(bits(at)))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms() {
        assert_eq!(form("tdata[63:32]"), Some(Form::Slice("tdata", 63, 32)));
        assert_eq!(form(" top.x [ 7 : 4 ] "), Some(Form::Slice("top.x", 7, 4)));
        assert_eq!(form("{a, b[3:0], {c, d}}"), Some(Form::Concat(vec!["a", "b[3:0]", "{c, d}"])));
        assert_eq!(form("{a,b}[3:0]"), Some(Form::Slice("{a,b}", 3, 0)));
        assert_eq!(form("{a}, {b}"), None);
        assert_eq!(form("data[3]"), None);
        assert_eq!(form("data[x:1]"), None);
        assert_eq!(form("top.clk"), None);
        assert_eq!(form("pc + 4"), Some(Form::Expr("pc + 4")));
        assert_eq!(form("signed(insn[31:20])"), Some(Form::Expr("signed(insn[31:20])")));
        assert_eq!(form("{a, b - 1}"), Some(Form::Concat(vec!["a", "b - 1"])));
        assert_eq!(form("x[7:4]"), Some(Form::Slice("x", 7, 4)));
        assert_eq!(form("pc * 4"), Some(Form::Expr("pc * 4")));
        assert_eq!(form("top.cpu.*"), None);
        assert_eq!(form("**.s_axi_*"), None);
    }
}
