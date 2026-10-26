//! Radixes (spec §10). Adding one = adding an entry to RADIXES.
//! Values come in as wellen bit strings, MSB first: 0 1 x z u w l h -.

pub struct Radix {
    pub name: &'static str,
    /// Value bits to the label. The number is the fraction bits of fixed point, 0 otherwise.
    pub format: fn(&str, u32) -> String,
    /// Search text in this notation to bits (MSB first) for a signal of the given width, or None
    /// if the text isn't in this notation. More bits than the width means the value doesn't fit.
    pub parse: Option<fn(&str, u32) -> Option<String>>,
    /// Signal widths it can show; `r` skips it for others.
    pub fits: fn(u32) -> bool,
    /// Fixed point: written with its integer and fraction bits (`q4.12`), set with `:radix`,
    /// skipped by `r`.
    pub fixed: bool,
}

const ANY: fn(u32) -> bool = |_| true;

pub const RADIXES: &[Radix] = &[
    Radix { name: "hex", format: |b, _| hex(b), parse: Some(parse_hex), fits: ANY, fixed: false },
    Radix { name: "bin", format: |b, _| bin(b), parse: Some(parse_bin), fits: ANY, fixed: false },
    Radix { name: "dec", format: |b, _| dec(b), parse: Some(parse_dec), fits: ANY, fixed: false },
    Radix { name: "sdec", format: |b, _| sdec(b), parse: Some(parse_sdec), fits: ANY, fixed: false },
    Radix { name: "oct", format: |b, _| grouped(b, 3, "0o"), parse: Some(parse_oct), fits: ANY, fixed: false },
    Radix { name: "ascii", format: |b, _| ascii(b), parse: None, fits: |w| w >= 8, fixed: false },
    Radix { name: "float", format: |b, _| float(b), parse: None, fits: |w| matches!(w, 16 | 32 | 64), fixed: false },
    Radix { name: "q", format: |b, f| fixed(b, f, true), parse: None, fits: |w| w <= 128, fixed: true },
    Radix { name: "uq", format: |b, f| fixed(b, f, false), parse: None, fits: |w| w <= 128, fixed: true },
    // the names come from the var (`rows::label`); a value without one is shown in hex. It fits
    // signals with names, whatever their width (`fits`).
    Radix { name: "enum", format: |b, _| hex(b), parse: None, fits: |_| false, fixed: false },
];

/// Bits of digits in a power-of-two base, leading zeros dropped.
fn digits(s: &str, base: u32, per: usize) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let mut bits = String::new();
    for c in s.chars() {
        bits += &format!("{:0per$b}", c.to_digit(base)?);
    }
    Some(trim(&bits))
}

fn trim(bits: &str) -> String {
    let t = bits.trim_start_matches('0');
    if t.is_empty() { "0".into() } else { t.into() }
}

fn parse_hex(s: &str, _: u32) -> Option<String> {
    digits(s.strip_prefix("0x")?, 16, 4)
}

fn parse_oct(s: &str, _: u32) -> Option<String> {
    digits(s.strip_prefix("0o")?, 8, 3)
}

fn parse_bin(s: &str, _: u32) -> Option<String> {
    digits(s.strip_prefix("0b")?, 2, 1)
}

/// Decimal digits of any length to bits, by repeated halving.
fn parse_dec(s: &str, _: u32) -> Option<String> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut d: Vec<u8> = s.bytes().map(|b| b - b'0').collect();
    let mut bits = Vec::new();
    while d.iter().any(|&x| x > 0) {
        let mut rem = 0;
        for x in d.iter_mut() {
            let v = rem * 10 + *x;
            *x = v / 2;
            rem = v % 2;
        }
        bits.push(b'0' + rem);
    }
    bits.reverse();
    Some(trim(&String::from_utf8(bits).unwrap_or_default()))
}

/// `-12`: two's complement over the signal's width (spec §10).
fn parse_sdec(s: &str, width: u32) -> Option<String> {
    let m = parse_dec(s.strip_prefix('-')?, width)?;
    let w = width as usize;
    if m == "0" {
        return Some("0".into());
    }
    // the most negative value is 1 followed by zeros; anything larger doesn't fit
    if m.len() > w || (m.len() == w && m != format!("1{}", "0".repeat(w - 1))) {
        return Some("1".repeat(w + 1));
    }
    let inverted: Vec<u8> = format!("{m:0>w$}").bytes().map(|b| if b == b'0' { 1 } else { 0 }).collect();
    // add one
    let mut out = inverted;
    for b in out.iter_mut().rev() {
        *b ^= 1;
        if *b == 1 {
            break;
        }
    }
    Some(out.iter().map(|b| char::from(b'0' + b)).collect())
}

/// A radix as `:radix` and session files write it: its name, or for fixed point the name with
/// integer and fraction bits (`q4.12`, `uq0.8`). Returns the radix, its fraction bits, and the
/// width fixed point needs (integer + fraction bits; the integer bits of `q` include the sign).
pub fn parse_name(text: &str) -> Option<(usize, u32, Option<u32>)> {
    if let Some(r) = RADIXES.iter().position(|r| !r.fixed && r.name == text) {
        return Some((r, 0, None));
    }
    let (r, rest) = RADIXES.iter().enumerate().filter(|(_, r)| r.fixed).find_map(|(i, r)| Some((i, text.strip_prefix(r.name)?)))?;
    let (m, n) = rest.split_once('.')?;
    let (m, n) = (m.parse::<u32>().ok()?, n.parse::<u32>().ok()?);
    // up to 38 fraction bits, so the decimal digits stay exact in a u128
    (n <= 38).then_some((r, n, Some(m.checked_add(n)?)))
}

/// The text `parse_name` reads back.
pub fn name(r: usize, frac: u32, width: u32) -> String {
    match RADIXES[r].fixed {
        true => format!("{}{}.{frac}", RADIXES[r].name, width.saturating_sub(frac)),
        false => RADIXES[r].name.into(),
    }
}

/// Whether radix `r` can show a signal of this width, whose values the file names or not.
pub fn fits(r: usize, width: u32, named: bool) -> bool {
    if r == ENUM { named } else { (RADIXES[r].fits)(width) }
}

/// The radix `r` moves to from `r`: the next one that fits, without fixed point.
pub fn next(r: usize, width: u32, named: bool) -> usize {
    (1..=RADIXES.len()).map(|k| (r + k) % RADIXES.len()).find(|&n| !RADIXES[n].fixed && fits(n, width, named)).unwrap_or(r)
}

/// Hex, the radix of values without a row (a record's fields).
pub const DEFAULT: usize = 0;
/// Names from the file.
pub const ENUM: usize = 9;

/// Two's complement decimal.
pub const SDEC: usize = 3;

/// The radix new rows start in: `enum` if the file names the signal's values, `sdec` if they are
/// signed (spec §16), otherwise hex.
pub fn default_for(named: bool, signed: bool) -> usize {
    if named { ENUM } else if signed { SDEC } else { DEFAULT }
}

/// One output digit from a group of bits; unknown digits are X, U or Z.
fn digit(bits: &str, base_digit: fn(u32) -> char) -> char {
    let mut v = 0;
    let mut odd = None;
    for c in bits.chars() {
        v <<= 1;
        let class = match c {
            '0' | 'l' => None,
            '1' | 'h' => {
                v |= 1;
                None
            }
            'z' => Some('Z'),
            'u' => Some('U'),
            _ => Some('X'),
        };
        if let Some(k) = class {
            odd = Some(match odd {
                None => k,
                Some(prev) if prev == k => k,
                _ => 'X',
            });
        }
    }
    odd.unwrap_or_else(|| base_digit(v))
}

fn grouped(bits: &str, n: usize, prefix: &str) -> String {
    let pad = (n - bits.len() % n) % n;
    let padded: String = "0".repeat(pad) + bits;
    let digits: String = padded
        .as_bytes()
        .chunks(n)
        .map(|g| digit(std::str::from_utf8(g).unwrap(), |v| char::from_digit(v, 16).unwrap().to_ascii_uppercase()))
        .collect();
    let trimmed = digits.trim_start_matches('0');
    format!("{prefix}{}", if trimmed.is_empty() { "0" } else { trimmed })
}

fn hex(bits: &str) -> String {
    grouped(bits, 4, "0x")
}

fn bin(bits: &str) -> String {
    grouped(bits, 1, "0b")
}

/// A decimal value with any unknown bit is `x`, with any high-impedance bit `z` (spec §10).
fn unknown(bits: &str) -> Option<String> {
    if bits.contains(['x', 'u', 'w', '-']) {
        Some("x".into())
    } else if bits.contains('z') {
        Some("z".into())
    } else {
        None
    }
}

/// Decimal digits (least significant first) of a known bit string of any width.
fn decimal(bits: impl Iterator<Item = bool>) -> Vec<u8> {
    let mut d = vec![0u8];
    for bit in bits {
        let mut carry = bit as u8;
        for x in d.iter_mut() {
            let v = *x * 2 + carry;
            *x = v % 10;
            carry = v / 10;
        }
        if carry > 0 {
            d.push(carry);
        }
    }
    d
}

fn text(d: &[u8]) -> String {
    d.iter().rev().map(|x| char::from(b'0' + x)).collect()
}

fn dec(bits: &str) -> String {
    unknown(bits).unwrap_or_else(|| text(&decimal(bits.chars().map(|c| matches!(c, '1' | 'h')))))
}

/// Two's complement over the signal's width: a set top bit is negative, -(inverted + 1).
fn sdec(bits: &str) -> String {
    if let Some(u) = unknown(bits) {
        return u;
    }
    if !bits.starts_with(['1', 'h']) {
        return dec(bits);
    }
    let mut d = decimal(bits.chars().map(|c| !matches!(c, '1' | 'h')));
    let mut i = 0;
    loop {
        if i == d.len() {
            d.push(0);
        }
        d[i] += 1;
        if d[i] < 10 {
            break;
        }
        d[i] = 0;
        i += 1;
    }
    format!("-{}", text(&d))
}

fn known(bits: &str) -> impl Iterator<Item = bool> + '_ {
    bits.chars().map(|c| matches!(c, '1' | 'h'))
}

/// Bytes as characters, `.` where not printable, `?` where unknown; leading zero bytes dropped.
fn ascii(bits: &str) -> String {
    let pad = (8 - bits.len() % 8) % 8;
    let padded = "0".repeat(pad) + bits;
    let chars: Vec<char> = padded
        .as_bytes()
        .chunks(8)
        .map(|b| std::str::from_utf8(b).unwrap_or_default())
        .map(|b| match unknown(b) {
            Some(_) => '?',
            None => match known(b).fold(0u8, |v, x| v << 1 | x as u8) {
                c @ 0x20..=0x7e => c as char,
                0 => '\0',
                _ => '.',
            },
        })
        .collect();
    let text: String = chars.iter().skip_while(|&&c| c == '\0').map(|&c| if c == '\0' { '.' } else { c }).collect();
    if text.is_empty() { ".".into() } else { text }
}

/// IEEE 754 half, single or double precision, by the signal's width.
fn float(bits: &str) -> String {
    if let Some(u) = unknown(bits) {
        return u;
    }
    let v = known(bits).fold(0u64, |v, x| v << 1 | x as u64);
    // plain notation where it reads well, exponent notation for very large and small values
    fn show<T: std::fmt::Display + std::fmt::LowerExp + Into<f64> + Copy>(x: T) -> String {
        let a = x.into().abs();
        if a.is_finite() && a != 0.0 && !(1e-4..1e7).contains(&a) { format!("{x:e}") } else { format!("{x}") }
    }
    match bits.len() {
        16 => show(half(v as u16)),
        32 => show(f32::from_bits(v as u32)),
        64 => show(f64::from_bits(v)),
        _ => hex(bits),
    }
}

/// A half-precision float as an f32 (which holds every half value exactly).
fn half(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let (e, m) = ((h >> 10) & 0x1f, (h & 0x3ff) as f32);
    sign * match e {
        0 => m * 2f32.powi(-24),
        31 if m == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        e => (1.0 + m / 1024.0) * 2f32.powi(e as i32 - 15),
    }
}

/// Fixed point with `frac` fraction bits, exactly: every binary fraction has a finite decimal one.
fn fixed(bits: &str, frac: u32, signed: bool) -> String {
    if let Some(u) = unknown(bits) {
        return u;
    }
    let v = known(bits).fold(0u128, |v, x| v << 1 | x as u128);
    let w = bits.len() as u32;
    let negative = signed && w > 0 && v >> (w - 1) & 1 == 1;
    // the magnitude of a negative two's complement value
    let m = if negative { (!v).wrapping_add(1) & (u128::MAX >> (128 - w)) } else { v };
    let (int, f) = (m >> frac, m & ((1u128 << frac) - 1));
    // f / 2^frac = f * 5^frac / 10^frac: exact decimal digits (frac <= 38, see parse_name)
    let digits = format!("{:0>width$}", f * 5u128.pow(frac), width = frac as usize);
    let digits = digits.trim_end_matches('0');
    format!("{}{int}.{}", if negative { "-" } else { "" }, if digits.is_empty() { "0" } else { digits })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_bin() {
        assert_eq!(hex("0100000000000000"), "0x4000");
        assert_eq!(hex("00000000"), "0x0");
        assert_eq!(hex("0011101x"), "0x3X");
        assert_eq!(hex("zzzzzzzz"), "0xZZ");
        assert_eq!(hex("uuuuzzzz"), "0xUZ");
        assert_eq!(hex("00zx0000"), "0xX0");
        assert_eq!(hex("101"), "0x5");
        assert_eq!(bin("100"), "0b100");
        assert_eq!(bin("0001"), "0b1");
        assert_eq!(bin("0x1"), "0bX1");
    }

    #[test]
    fn decimals() {
        assert_eq!(dec("0100000000000000"), "16384");
        assert_eq!(dec("0"), "0");
        assert_eq!(dec("1".repeat(128).as_str()), u128::MAX.to_string());
        assert_eq!(dec("0x1"), "x");
        assert_eq!(dec("zz0u"), "x");
        assert_eq!(dec("zz01"), "z");
        assert_eq!(sdec("11110100"), "-12");
        assert_eq!(sdec("10000000"), "-128");
        assert_eq!(sdec("1"), "-1");
        assert_eq!(sdec("01111111"), "127");
        assert_eq!(sdec(&format!("1{}", "0".repeat(127))), i128::MIN.to_string());
    }

    #[test]
    fn more_radixes() {
        assert_eq!(grouped("001111", 3, "0o"), "0o17");
        assert_eq!(ascii("0000000001001000" ), "H");
        assert_eq!(ascii("0100100001101001"), "Hi");
        assert_eq!(ascii("01001000000000010110100x"), "H.?");
        assert_eq!(ascii("00000000"), ".");
        assert_eq!(float("0011110000000000"), "1"); // half 1.0
        assert_eq!(float("1100000000000000"), "-2");
        assert_eq!(float(&format!("{:032b}", 0.1f32.to_bits())), "0.1");
        assert_eq!(float(&format!("{:064b}", 1e300f64.to_bits())), "1e300");
        assert_eq!(float("0x111111111111111"), "x");
        assert_eq!(fixed("0001010000000000", 12, true), "1.25"); // q4.12
        assert_eq!(fixed("1110110000000000", 12, true), "-1.25");
        assert_eq!(fixed("1000000000000000", 12, true), "-8.0");
        assert_eq!(fixed("1000000000000000", 12, false), "8.0");
        assert_eq!(fixed("00000001", 8, false), "0.00390625");
        assert_eq!(fixed("0101", 0, true), "5.0");
        assert_eq!(parse_name("q4.12"), Some((7, 12, Some(16))));
        assert_eq!(parse_name("uq0.8"), Some((8, 8, Some(8))));
        assert_eq!((parse_name("dec"), parse_name("q4"), parse_name("xyz")), (Some((2, 0, None)), None, None));
        assert_eq!(parse_name("q2.39"), None);
        assert_eq!(name(7, 12, 16), "q4.12");
        // r: float only for 16, 32 and 64 bits, fixed point never, enum only with names
        assert_eq!(next(5, 32, false), 6);
        assert_eq!(next(5, 12, false), 0);
        assert_eq!(next(6, 32, false), 0);
        assert_eq!(next(6, 32, true), ENUM);
        assert_eq!(next(ENUM, 32, true), 0);
        assert_eq!((RADIXES[ENUM].name, parse_name("enum")), ("enum", Some((ENUM, 0, None))));
        assert_eq!(parse_oct("0o17", 8).as_deref(), Some("1111"));
    }

    #[test]
    fn parses() {
        assert_eq!(parse_hex("0x0040", 8).as_deref(), Some("1000000"));
        assert_eq!(parse_bin("0b1000000", 8).as_deref(), Some("1000000"));
        assert_eq!(parse_dec("64", 8).as_deref(), Some("1000000"));
        assert_eq!(parse_dec(&u128::MAX.to_string(), 128), Some("1".repeat(128)));
        assert_eq!(parse_hex("40", 8), None);
        assert_eq!(parse_dec("0x40", 8), None);
        assert_eq!(parse_sdec("-12", 8).as_deref(), Some("11110100"));
        assert_eq!(parse_sdec("-128", 8).as_deref(), Some("10000000"));
        assert_eq!(parse_sdec("-129", 8).map(|b| b.len()), Some(9));
        assert_eq!(parse_sdec("-1", 1).as_deref(), Some("1"));
    }
}
