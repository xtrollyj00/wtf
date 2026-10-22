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

pub fn builtin(name: &str, rgb: bool) -> Option<Palette> {
    let (_, text) = BUILTIN.iter().find(|(n, _)| *n == name)?;
    Some(parse(name, text, rgb, &mut Vec::new()))
}

/// A theme by name: the user's file in `dir` (`<name>.toml`), else a built-in one.
pub fn find(name: &str, dir: &Path, rgb: bool, warnings: &mut Vec<String>) -> Option<Palette> {
    match std::fs::read_to_string(dir.join(format!("{name}.toml"))) {
        Ok(text) => Some(parse(name, &text, rgb, warnings)),
        Err(_) => builtin(name, rgb),
    }
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
/// the terminal's colours).
pub fn parse(name: &str, text: &str, rgb: bool, warnings: &mut Vec<String>) -> Palette {
    let mut p = match name {
        "dark" => Palette { mono: false, ..Palette::mono() },
        _ => builtin("dark", rgb).expect("dark is built in"),
    };
    let who = format!("config: theme {name}: ");
    match text.parse::<toml::Table>() {
        Ok(t) => apply(&mut p, &t, rgb, &who, "", warnings),
        Err(e) => warnings.push(format!("{who}{}", toml_error(&e, text))),
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
        "fill" => &mut p.fill,
        "unknown" => &mut p.unknown,
        "highz" => &mut p.highz,
        "cursor" => &mut p.cursor,
        "cursor_text" => &mut p.cursor_text,
        "cursor_dim" => &mut p.cursor_dim,
        "selected" => &mut p.selected,
        "hatch" => &mut p.hatch,
        "dim" => &mut p.dim,
        "grid" => &mut p.grid,
        "background" => &mut p.background,
        "text" => &mut p.text,
        _ => return rows::color_named(key).ok().map(|i| &mut p.hues[i]),
    })
}

fn keys() -> String {
    const FIELDS: [&str; 13] = ["line", "fill", "unknown", "highz", "cursor", "cursor_text", "cursor_dim", "selected", "hatch", "dim", "grid", "background", "text"];
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
            parse(name, text, true, &mut warnings);
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
        }
        let dark: toml::Table = BUILTIN[0].1.parse().unwrap();
        assert_eq!(dark.keys().map(String::as_str).collect::<Vec<_>>().join(", "), keys());
        let p = builtin("dark", false).unwrap();
        assert_eq!((p.line, p.background, p.hues[3]), (Color::Indexed(73), Color::Reset, Color::Indexed(114)));
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
