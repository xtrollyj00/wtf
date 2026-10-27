//! Ticks <-> real time (spec §14 "Time").

pub type Tick = u64;

const UNITS: [(&str, u128); 6] = [
    ("s", 1_000_000_000_000_000),
    ("ms", 1_000_000_000_000),
    ("us", 1_000_000_000),
    ("ns", 1_000_000),
    ("ps", 1_000),
    ("fs", 1),
];

/// Femtoseconds per tick, or None if the file has no usable timescale.
// shortcut: timescales finer than 1fs (zs, as) fall back to raw ticks; upgrade if such files show up.
pub fn fs_per_tick(ts: Option<wellen::Timescale>) -> Option<u128> {
    let ts = ts?;
    let exp = ts.unit.to_exponent()? as i32 + 15;
    (exp >= 0 && ts.factor > 0).then(|| ts.factor as u128 * 10u128.pow(exp as u32))
}

/// A time in `unit` (an index into the units, `s` first), or with None the largest unit that keeps
/// it at 1 or more (zero in the file's own unit); with `decimals` rounded to that many and trailing
/// zeros dropped (`2595.000001ns` to 3 is `2595ns`), with None exact, every decimal it takes to read
/// back to the same tick. Plain ticks without a timescale. The one formatter: the interfaces pick
/// the unit and the precision.
pub fn format_as(t: Tick, fpt: Option<u128>, unit: Option<usize>, decimals: Option<u32>) -> String {
    let Some(fpt) = fpt else { return t.to_string() };
    let fs = t as u128 * fpt;
    let (name, ufs) = UNITS[unit.unwrap_or_else(|| unit_for([t], Some(fpt)))];
    let (whole, frac) = match decimals {
        Some(d) => {
            let scale = 10u128.pow(d);
            let n = (fs * scale + ufs / 2) / ufs;
            (n / scale, format!("{:0w$}", n % scale, w = d as usize))
        }
        None => (fs / ufs, format!("{:0w$}", fs % ufs, w = ufs.ilog10() as usize)),
    };
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() { format!("{whole}{name}") } else { format!("{whole}.{frac}{name}") }
}

/// For people (spec §14 "Time"): the largest unit that keeps the value at 1 or more, up to 3
/// decimals: `12.5ns`, `1.234us`.
pub fn format(t: Tick, fpt: Option<u128>) -> String {
    format_as(t, fpt, None, Some(3))
}

/// For session files: exact, so it reads back to the same tick (`2.595000001us`).
pub fn format_exact(t: Tick, fpt: Option<u128>) -> String {
    format_as(t, fpt, None, None)
}

/// The unit for a set of times shown together: the largest that keeps the smallest nonzero one at
/// 1 or more, or the file's own unit if all are zero. An index into the units, `s` first.
pub fn unit_for(ts: impl IntoIterator<Item = Tick>, fpt: Option<u128>) -> usize {
    let Some(fpt) = fpt else { return UNITS.len() - 1 };
    let base = ts.into_iter().filter(|&t| t > 0).min().map_or(fpt, |t| t as u128 * fpt);
    UNITS.iter().position(|(_, u)| base >= *u).unwrap_or(UNITS.len() - 1)
}

/// A unit by name, e.g. `ns`.
pub fn unit_named(name: &str) -> Result<usize, String> {
    UNITS.iter().position(|(n, _)| *n == name).ok_or(format!("bad time unit: {name} (s, ms, us, ns, ps, fs)"))
}

/// Whether `text` is a time with a unit (`40ns`, `1.5us`), not a number.
pub fn has_unit(text: &str) -> bool {
    let t = text.trim();
    UNITS.iter().any(|(u, _)| t.strip_suffix(u).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.')))
}

/// Parse `2000ns`, `1.5us`, or a plain tick count. Returns ticks, possibly fractional.
pub fn parse(s: &str, fpt: Option<u128>) -> Result<f64, String> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let v: f64 = num.parse().map_err(|_| format!("bad time: {s}"))?;
    if unit.is_empty() {
        return Ok(v);
    }
    let ufs = UNITS.iter().find(|(n, _)| *n == unit).ok_or(format!("bad time unit: {unit}"))?.1;
    let fpt = fpt.ok_or("file has no timescale; use ticks")?;
    // in whole femtoseconds when it is one, so long exact times (`69.360000001us`) don't lose
    // their last digits to floating point
    let (int, frac) = num.split_once('.').unwrap_or((num, ""));
    let scale = 10u128.checked_pow(frac.len() as u32).filter(|&d| ufs.is_multiple_of(d));
    if let (Some(d), Ok(i), Ok(f)) = (scale, int.parse::<u128>(), if frac.is_empty() { Ok(0) } else { frac.parse::<u128>() }) {
        let fs = i * ufs + f * (ufs / d);
        return Ok((fs / fpt) as f64 + (fs % fpt) as f64 / fpt as f64);
    }
    Ok(v * ufs as f64 / fpt as f64)
}

/// The ticks that print as `s`, first and last: those `format_as` rounds to it, to a thousandth of
/// its unit or its last digit if finer (`2595ns` is 2594.9995ns up to 2595.0005ns). The inverse of
/// the formatter, so a rounded time read back stands for every tick it was. A plain tick count, or
/// a span holding no tick, is the nearest tick.
pub fn span(s: &str, fpt: Option<u128>) -> Result<(Tick, Tick), String> {
    let point = parse(s, fpt)?.max(0.0).round() as Tick;
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len()));
    let (Some(fpt), Some(&(_, ufs))) = (fpt, UNITS.iter().find(|(n, _)| *n == unit)) else { return Ok((point, point)) };
    let (int, frac) = num.split_once('.').unwrap_or((num, ""));
    let span = || -> Option<(Tick, Tick)> {
        // in halves of the last place (ufs / 10^p / 2), the span is [2n - 1, 2n + 1)
        let p = frac.len().max(3) as u32;
        let frac = if frac.is_empty() { 0 } else { frac.parse::<u128>().ok()? * 10u128.pow(p - frac.len() as u32) };
        let n = int.parse::<u128>().ok()?.checked_mul(10u128.checked_pow(p)?)?.checked_add(frac)?;
        let tick = fpt.checked_mul(2)?.checked_mul(10u128.pow(p))?;
        let lo = (2 * n).saturating_sub(1).checked_mul(ufs)?.div_ceil(tick);
        let hi = (2 * n + 1).checked_mul(ufs)?.div_ceil(tick).checked_sub(1)?;
        let (lo, hi) = (Tick::try_from(lo).ok()?, Tick::try_from(hi).ok()?);
        (lo <= hi).then_some((lo, hi))
    };
    Ok(span().unwrap_or((point, point)))
}

#[cfg(test)]
mod tests {
    use super::*;
    const NS: Option<u128> = Some(1_000_000);

    #[test]
    fn formats() {
        assert_eq!(format(0, NS), "0ns");
        assert_eq!(format(0, Some(1000)), "0ps");
        assert_eq!(format(12, NS), "12ns");
        assert_eq!(format(1234, NS), "1.234us");
        assert_eq!(format(2000, NS), "2us");
        assert_eq!(format(12_500, Some(1000)), "12.5ns");
        assert_eq!(format(77, None), "77");
    }

    #[test]
    fn in_a_unit() {
        let (fs, ns) = (Some(1), Some(3));
        // to a thousandth: a femtosecond of skew goes, a half cycle stays
        assert_eq!(format_as(2_595_000_001, fs, ns, Some(3)), "2595ns");
        assert_eq!(format_as(39_895_000_001, fs, ns, Some(3)), "39895ns");
        assert_eq!(format_as(2_500_000, fs, ns, Some(3)), "2.5ns");
        assert_eq!(format_as(80_000_000, fs, Some(2), Some(3)), "0.08us");
        // exact
        assert_eq!(format_as(2_595_000_001, fs, ns, None), "2595.000001ns");
        assert_eq!(format_as(0, fs, ns, None), "0ns");
        assert!(has_unit("40ns") && has_unit("1.5us") && !has_unit("40") && !has_unit("ns") && !has_unit("0x40"));
    }

    #[test]
    fn exact() {
        assert_eq!(format_exact(16_432, NS), "16.432us");
        assert_eq!(format_exact(2_000, NS), "2us");
        assert_eq!(format_exact(460, NS), "460ns");
        assert_eq!(format_exact(0, NS), "0ns");
        assert_eq!(format_exact(7, None), "7");
        assert_eq!(parse(&format_exact(16_432, NS), NS), Ok(16_432.0));
        // 1fs ticks, as real traces have: every digit kept, and read back to the same tick
        let fs = Some(1);
        assert_eq!(format_exact(2_595_000_001, fs), "2.595000001us");
        assert_eq!(format_exact(480_000_015, fs), "480.000015ns");
        // times shown together share the unit of the smallest
        assert_eq!(unit_for([80_000_000, 2_595_000_001, 0], fs), 3);
        assert_eq!(unit_for([0], NS), 3);
        for t in [2_595_000_001, 69_360_000_001, 3_600_000_000_000_000_001] {
            assert_eq!(parse(&format_exact(t, fs), fs), Ok(t as f64), "{t}");
        }
    }

    #[test]
    fn spans() {
        let (fs, ns) = (Some(1), Some(3));
        // the ticks that print as the text, and no others
        let (lo, hi) = span("2595ns", fs).unwrap();
        assert_eq!((lo, hi), (2_594_999_500, 2_595_000_499));
        assert_eq!(format_as(lo, fs, ns, Some(3)), "2595ns");
        assert_eq!(format_as(hi, fs, ns, Some(3)), "2595ns");
        assert_eq!(format_as(lo - 1, fs, ns, Some(3)), "2594.999ns");
        assert_eq!(format_as(hi + 1, fs, ns, Some(3)), "2595.001ns");
        // more digits, narrower; a femtosecond or a tick count is one tick
        assert_eq!(span("2595.0004ns", fs).unwrap(), (2_595_000_350, 2_595_000_449));
        assert_eq!(span("20000002fs", fs).unwrap(), (20_000_002, 20_000_002));
        assert_eq!(span("42", fs).unwrap(), (42, 42));
        assert_eq!(span("0ns", fs).unwrap(), (0, 499));
        // coarser ticks than the span: the nearest
        assert_eq!(span("12.5ns", NS).unwrap(), (13, 13));
        assert_eq!(span("12ns", NS).unwrap(), (12, 12));
        assert!(span("1furlong", fs).is_err());
    }

    #[test]
    fn parses() {
        assert_eq!(parse("2000ns", NS).unwrap(), 2000.0);
        assert_eq!(parse("1.5us", NS).unwrap(), 1500.0);
        assert_eq!(parse("500ps", NS).unwrap(), 0.5);
        assert_eq!(parse("42", None).unwrap(), 42.0);
        assert!(parse("1ns", None).is_err());
        assert!(parse("1furlong", NS).is_err());
        assert!(parse("abc", NS).is_err());
    }
}
