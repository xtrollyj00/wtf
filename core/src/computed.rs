//! Signals computed from names (spec §16): bit slices `data[63:32]` and
//! concatenations `{a, b}` (most significant first, as in Verilog). Their values are worked out
//! once, from the changes of the signals they are made of, into a computed var (`Trace`).

use crate::time::Tick;
use crate::trace::{Trace, Val};

/// What a name computes.
#[derive(Debug, PartialEq)]
pub enum Form<'a> {
    /// `base[hi:lo]`
    Slice(&'a str, u32, u32),
    /// `{a, b, c}`
    Concat(Vec<&'a str>),
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
    let (base, range) = name.strip_suffix(']')?.rsplit_once('[')?;
    let (hi, lo) = range.split_once(':')?;
    Some(Form::Slice(base.trim(), hi.trim().parse().ok()?, lo.trim().parse().ok()?))
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
    let mut out: Vec<Tick> = trace.first_time(var).into_iter().collect();
    while let Some(t) = out.last().and_then(|&t| trace.next_change(var, t)) {
        out.push(t);
    }
    out
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
    }
}
