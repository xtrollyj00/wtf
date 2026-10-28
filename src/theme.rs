//! Themes (spec §19 "Themes"): palettes written in TOML. The built-in ones are the files in
//! themes/, compiled in; a user's are read from the config directory's themes/.

use crate::draw::Palette;
use ratatui::style::Color;
use std::path::Path;
use wtf_core::rows::{self, COLORS};

/// To add a theme: its file in themes/, and a line here.
pub const BUILTIN: [(&str, &str); 2] = [
    ("dark", include_str!("../themes/dark.toml")),
    ("light", include_str!("../themes/light.toml")),
];

/// An sRGB colour: the terminal's background.
pub type Rgb = (u8, u8, u8);

pub fn builtin(name: &str, rgb: bool) -> Option<Palette> {
    builtin_on(name, rgb, None)
}

/// A built-in theme, its surfaces fitted to the terminal's background `bg` if known.
pub fn builtin_on(name: &str, rgb: bool, bg: Option<Rgb>) -> Option<Palette> {
    let (_, text) = BUILTIN.iter().find(|(n, _)| *n == name)?;
    Some(parse(name, text, rgb, bg, &mut Vec::new()))
}

/// A theme by name: the user's file in `dir` (`<name>.toml`), else a built-in one.
pub fn find(name: &str, dir: &Path, rgb: bool, bg: Option<Rgb>, warnings: &mut Vec<String>) -> Option<Palette> {
    match std::fs::read_to_string(dir.join(format!("{name}.toml"))) {
        Ok(text) => Some(parse(name, &text, rgb, bg, warnings)),
        Err(_) => builtin_on(name, rgb, bg),
    }
}

/// The terminal's background colour, asked with OSC 11 (spec §19 "The terminal's background");
/// `None` when it doesn't answer.
pub fn terminal_background() -> Option<Rgb> {
    terminal_colorsaurus::background_color(terminal_colorsaurus::QueryOptions::default()).ok().map(|c| c.scale_to_8bit())
}

/// A light background: the light theme suits it.
pub fn is_light((r, g, b): Rgb) -> bool {
    // Rec. 709 luma, halfway
    0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32 > 127.5
}

/// The surfaces behind cards, the selected row and the cursor columns, as steps in OKLab
/// lightness from the terminal's background `bg`, lighter on a dark one and darker on a light one,
/// so they stand out by the same amount on any background (spec §19).
fn fit(p: &mut Palette, bg: Rgb, rgb: bool) {
    let colour = |(r, g, b): Rgb| if rgb { Color::Rgb(r, g, b) } else { Color::Indexed(nearest_256(r, g, b)) };
    let (l, a, b) = oklab(bg);
    // near black a step this small can't be seen: at least three of them from black
    let step = |d: f64| colour(srgb((if is_light(bg) { l - d } else { (l + d).max(3.0 * d) }, a, b)));
    p.panel = step(0.07);
    p.selected = step(0.10);
    p.cursor_dim = step(0.14);
    // the accent's yellow, a quarter of the way from the background
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * 0.25).round() as u8;
    p.column = colour((m(bg.0, 0xe5), m(bg.1, 0xc0), m(bg.2, 0x7b)));
}

/// sRGB to OKLab (Björn Ottosson's).
fn oklab((r, g, b): Rgb) -> (f64, f64, f64) {
    let lin = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s, 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s)
}

/// OKLab to sRGB, clipped to it.
fn srgb((l, a, b): (f64, f64, f64)) -> Rgb {
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let out = |c: f64| {
        let c = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    (
        out(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_),
        out(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_),
        out(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_),
    )
}

/// The themes there are: built in, and the user's in `dir`.
pub fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
    let files = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path());
    names.extend(files.filter(|p| p.extension().is_some_and(|e| e == "toml")).filter_map(|p| Some(p.file_stem()?.to_str()?.to_string())));
    names.sort();
    names.dedup();
    names
}

/// A theme file's text over dark, which gives the keys it leaves out (dark itself starts from
/// the terminal's colours, its surfaces fitted to the background `bg` if known).
pub fn parse(name: &str, text: &str, rgb: bool, bg: Option<Rgb>, warnings: &mut Vec<String>) -> Palette {
    let dark = BUILTIN[0].1;
    let mut p = Palette { mono: false, ..Palette::mono() };
    apply(&mut p, &dark.parse().expect("dark parses"), rgb, "", "", &mut Vec::new());
    if let Some(bg) = bg {
        fit(&mut p, bg, rgb);
    }
    if text != dark {
        let who = format!("config: theme {name}: ");
        match text.parse::<toml::Table>() {
            Ok(t) => apply(&mut p, &t, rgb, &who, "", warnings),
            Err(e) => warnings.push(format!("{who}{}", toml_error(&e, text))),
        }
    }
    p
}

/// Set the palette's colours from a table of them: a theme file, or the config's `[colors]`.
/// `who` starts each warning, `path` comes before each key in them (`colors.`).
pub fn apply(p: &mut Palette, colors: &toml::Table, rgb: bool, who: &str, path: &str, warnings: &mut Vec<String>) {
    for (key, value) in colors {
        match slot(p, key) {
            None => warnings.push(format!("{who}unknown key {path}{key} (keys: {})", keys())),
            Some(c) => match colour(value, rgb) {
                Some(new) => *c = new,
                None => warnings.push(format!("{who}{path}{key} = {}? (\"#rrggbb\", 0-255, [\"#rrggbb\", 0-255] or \"default\")", show(value))),
            },
        }
    }
}

/// A TOML error on one line, with the line it is on.
pub fn toml_error(e: &toml::de::Error, text: &str) -> String {
    let line = e.span().map_or(1, |s| text[..s.start].matches('\n').count() + 1);
    format!("{} (line {line})", e.message().replace('\n', " "))
}

/// The value as TOML shows it in a warning: strings quoted, numbers as they are.
pub fn show(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

/// The palette's colour called `key`: a field, or a signal colour's shade.
fn slot<'a>(p: &'a mut Palette, key: &str) -> Option<&'a mut Color> {
    Some(match key {
        "line" => &mut p.line,
        "bus" => &mut p.bus,
        "fill" => &mut p.fill,
        "unknown" => &mut p.unknown,
        "highz" => &mut p.highz,
        "cursor" => &mut p.cursor,
        "cursor_text" => &mut p.cursor_text,
        "column" => &mut p.column,
        "cursor_dim" => &mut p.cursor_dim,
        "selected" => &mut p.selected,
        "panel" => &mut p.panel,
        "hatch" => &mut p.hatch,
        "dim" => &mut p.dim,
        "background" => &mut p.background,
        "text" => &mut p.text,
        _ => return rows::color_named(key).ok().map(|i| &mut p.hues[i]),
    })
}

fn keys() -> String {
    const FIELDS: [&str; 15] = ["line", "bus", "fill", "unknown", "highz", "cursor", "cursor_text", "column", "cursor_dim", "selected", "panel", "hatch", "dim", "background", "text"];
    FIELDS.iter().chain(COLORS.iter()).copied().collect::<Vec<_>>().join(", ")
}

/// `"#rrggbb"` (the nearest 256-colour shade without truecolor), an index 0..=255,
/// `["#rrggbb", index]` (the index without truecolor), or `"default"`, the terminal's.
fn colour(v: &toml::Value, rgb: bool) -> Option<Color> {
    let hex = |s: &str| {
        let h = s.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
        let n = u32::from_str_radix(h, 16).ok()?;
        Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
    };
    let index = |v: &toml::Value| v.as_integer().and_then(|i| u8::try_from(i).ok());
    match v {
        toml::Value::String(s) if s == "default" => Some(Color::Reset),
        toml::Value::String(s) => {
            let (r, g, b) = hex(s)?;
            Some(if rgb { Color::Rgb(r, g, b) } else { Color::Indexed(nearest_256(r, g, b)) })
        }
        toml::Value::Integer(_) => index(v).map(Color::Indexed),
        toml::Value::Array(a) if a.len() == 2 => {
            let (r, g, b) = hex(a[0].as_str()?)?;
            let i = index(&a[1])?;
            Some(if rgb { Color::Rgb(r, g, b) } else { Color::Indexed(i) })
        }
        _ => None,
    }
}

/// The xterm 256-colour index closest to an RGB colour (squared distance), from the 6x6x6 cube
/// and the grays. Muted colours can come out gray: a theme gives those their shade by hand.
fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let dist = |x: i32, y: i32, z: i32| (r - x).pow(2) + (g - y).pow(2) + (b - z).pow(2);
    let mut best = (i32::MAX, 0u8);
    for (ri, &lr) in LEVELS.iter().enumerate() {
        for (gi, &lg) in LEVELS.iter().enumerate() {
            for (bi, &lb) in LEVELS.iter().enumerate() {
                let d = dist(lr, lg, lb);
                if d < best.0 {
                    best = (d, (16 + 36 * ri + 6 * gi + bi) as u8);
                }
            }
        }
    }
    for i in 0..24 {
        let v = 8 + 10 * i;
        let d = dist(v, v, v);
        if d < best.0 {
            best = (d, 232 + i as u8);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_themes_are_whole() {
        // every theme parses without a warning; dark, the base of the others, sets every key
        for (name, text) in BUILTIN {
            let mut warnings = Vec::new();
            parse(name, text, true, None, &mut warnings);
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
        }
        let dark: toml::Table = BUILTIN[0].1.parse().unwrap();
        assert_eq!(dark.keys().map(String::as_str).collect::<Vec<_>>().join(", "), keys());
        let p = builtin("dark", false).unwrap();
        assert_eq!((p.line, p.background, p.hues[3]), (Color::Indexed(73), Color::Reset, Color::Indexed(114)));
    }

    #[test]
    fn surfaces_fit_the_background() {
        // dark's surfaces step away from the terminal's background, toward white or black
        let on = |bg| builtin_on("dark", true, Some(bg)).unwrap();
        let p = on((0x2e, 0x34, 0x40));
        assert_eq!((p.panel, p.selected), (Color::Rgb(64, 70, 83), Color::Rgb(72, 78, 91)));
        let p = on((0xff, 0xff, 0xff));
        assert_eq!((p.panel, p.cursor_dim, p.line), (Color::Rgb(232, 232, 232), Color::Rgb(209, 209, 209), Color::Rgb(0x56, 0xb6, 0xc2)));
        // light sets its own background and surfaces; a background unknown keeps dark's shades
        assert_eq!(builtin_on("light", true, Some((0, 0, 0))).unwrap().panel, builtin("light", true).unwrap().panel);
        assert_eq!(builtin_on("dark", true, None).unwrap().panel, Color::Rgb(0x28, 0x2c, 0x34));
        assert!(is_light((0xfd, 0xf6, 0xe3)) && !is_light((0x2e, 0x34, 0x40)));
        for c in [(0, 0, 0), (0x2e, 0x34, 0x40), (0xfd, 0xf6, 0xe3), (255, 255, 255)] {
            assert_eq!(srgb(oklab(c)), c);
        }
    }

    #[test]
    fn colours() {
        let c = |s: &str, rgb| colour(&format!("v = {s}").parse::<toml::Table>().unwrap()["v"], rgb);
        assert_eq!(c("\"#5f87af\"", true), Some(Color::Rgb(0x5f, 0x87, 0xaf)));
        assert_eq!(c("\"#5f87af\"", false), Some(Color::Indexed(67)));
        assert_eq!(c("[\"#2e7d32\", 28]", false), Some(Color::Indexed(28)));
        assert_eq!(c("[\"#2e7d32\", 28]", true), Some(Color::Rgb(0x2e, 0x7d, 0x32)));
        assert_eq!(c("17", true), Some(Color::Indexed(17)));
        assert_eq!(c("\"default\"", true), Some(Color::Reset));
        for bad in ["256", "-1", "\"#12345\"", "\"blue\"", "[\"#2e7d32\"]", "[\"#2e7d32\", 300]"] {
            assert_eq!(c(bad, true), None, "{bad}");
        }
    }

    #[test]
    fn nearest_is_the_closest() {
        assert_eq!(nearest_256(255, 0, 0), 196);
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(128, 128, 128), 244);
        assert_eq!(nearest_256(95, 135, 175), 67);
    }
}
