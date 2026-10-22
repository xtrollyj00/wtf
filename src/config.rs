//! The config file (spec §19): glyph set, theme and single colours, read once at start. A bad
//! entry is skipped with a warning for the status line; nothing in it stops wtf.

use crate::draw::{self, Palette};
use ratatui::style::Color;
use std::path::{Path, PathBuf};
use wtf_core::rows::{self, COLORS};

pub struct Config {
    pub glyphs: &'static draw::Glyphs,
    pub palette: Palette,
    pub warnings: Vec<String>,
}

/// `$XDG_CONFIG_HOME/wtf/config.toml`, or `~/.config/wtf/config.toml`; a missing file means the
/// defaults. `rgb` says whether the terminal has truecolor (spec §14 "Colours").
pub fn load() -> Config {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty());
    let path = xdg.map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    match path {
        Some(p) => load_from(&p.join("wtf/config.toml")),
        None => parse("", draw::truecolor()),
    }
}

fn load_from(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, draw::truecolor()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => parse("", draw::truecolor()),
        Err(e) => Config { warnings: vec![format!("config: {}: {e}", path.display())], ..parse("", draw::truecolor()) },
    }
}

/// The file's text as wtf reads it. Pure, so tests can run it on any text.
pub fn parse(text: &str, rgb: bool) -> Config {
    let mut warnings = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map_or(1, |s| text[..s.start].matches('\n').count() + 1);
            warnings.push(format!("config: {} (line {line})", e.message()));
            return Config { glyphs: &draw::UNICODE, palette: Palette::dark(rgb), warnings };
        }
    };
    let mut glyphs = &draw::UNICODE;
    for (key, value) in &table {
        match key.as_str() {
            "glyphs" => match value.as_str().and_then(|s| draw::SETS.iter().find(|(n, _)| *n == s)) {
                Some((_, g)) => glyphs = g,
                None => warnings.push(format!("config: glyphs = {}? (unicode, box, ascii)", show(value))),
            },
            "theme" | "colors" => {}
            _ => warnings.push(format!("config: unknown key {key} (keys: glyphs, theme, colors)")),
        }
    }
    // the theme first, so the single colours override it whatever the order in the file
    let mut palette = match table.get("theme") {
        None => Palette::dark(rgb),
        Some(v) => match v.as_str() {
            Some("dark") => Palette::dark(rgb),
            Some("light") => Palette::light(rgb),
            _ => {
                warnings.push(format!("config: theme = {}? (dark, light)", show(v)));
                Palette::dark(rgb)
            }
        },
    };
    if let Some(v) = table.get("colors") {
        match v.as_table() {
            Some(colors) => {
                for (key, value) in colors {
                    match slot(&mut palette, key) {
                        None => warnings.push(format!("config: unknown key colors.{key} (keys: {})", keys())),
                        Some(c) => match colour(value, rgb) {
                            Some(new) => *c = new,
                            None => warnings.push(format!("config: colors.{key} = {}? (#rrggbb or 0-255)", show(value))),
                        },
                    }
                }
            }
            None => warnings.push(format!("config: colors = {}? (a table of colours)", show(v))),
        }
    }
    Config { glyphs, palette, warnings }
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

/// `"#rrggbb"` or an integer 0..=255; the nearest 256-colour index where there is no truecolor.
fn colour(v: &toml::Value, rgb: bool) -> Option<Color> {
    match v {
        toml::Value::String(s) => {
            let hex = s.strip_prefix('#')?;
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let n = u32::from_str_radix(hex, 16).ok()?;
            let (r, g, b) = ((n >> 16) as u8, (n >> 8) as u8, n as u8);
            Some(if rgb { Color::Rgb(r, g, b) } else { Color::Indexed(nearest_256(r, g, b)) })
        }
        toml::Value::Integer(i) => u8::try_from(*i).ok().map(Color::Indexed),
        _ => None,
    }
}

/// The value as TOML shows it in a warning: strings quoted, numbers as they are.
fn show(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

fn keys() -> String {
    const FIELDS: [&str; 13] = ["line", "fill", "unknown", "highz", "cursor", "cursor_text", "cursor_dim", "selected", "hatch", "dim", "grid", "background", "text"];
    FIELDS.iter().chain(COLORS.iter()).copied().collect::<Vec<_>>().join(", ")
}

/// The xterm 256-colour index closest to an RGB colour (squared distance), from the 6x6x6 cube
/// and the grays.
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
    fn valid_file_applies() {
        let c = parse("glyphs = \"box\"\ntheme = \"light\"\n[colors]\ncursor = \"#ff0000\"\ngrid = 17\nred = \"#5f87af\"\n", true);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(c.glyphs.crossing, draw::BOX.crossing);
        let light = Palette::light(true);
        assert_eq!(c.palette.line, light.line);
        assert_eq!(c.palette.cursor, Color::Rgb(255, 0, 0));
        assert_eq!(c.palette.grid, Color::Indexed(17));
        assert_eq!(c.palette.hues[0], Color::Rgb(0x5f, 0x87, 0xaf));
        // without truecolor the hex colour is its nearest 256 shade
        let c = parse("[colors]\ncursor = \"#5f87af\"\n", false);
        assert_eq!(c.palette.cursor, Color::Indexed(67));
    }

    #[test]
    fn every_warning() {
        let text = "foo = 1\nglyphs = \"utf8\"\ntheme = 3\n[colors]\ncurser = \"#000000\"\ncursor = \"blue\"\nred = \"#ff0000\"\n";
        let c = parse(text, true);
        assert_eq!(
            c.warnings,
            [
                "config: unknown key foo (keys: glyphs, theme, colors)",
                "config: glyphs = \"utf8\"? (unicode, box, ascii)",
                "config: theme = 3? (dark, light)",
                "config: unknown key colors.curser (keys: line, fill, unknown, highz, cursor, cursor_text, cursor_dim, selected, hatch, dim, grid, background, text, red, orange, yellow, green, cyan, blue, magenta, gray)",
                "config: colors.cursor = \"blue\"? (#rrggbb or 0-255)",
            ]
        );
        // the valid entries still apply
        assert_eq!(c.palette.hues[0], Color::Rgb(255, 0, 0));
        assert_eq!(c.palette.cursor, Palette::dark(true).cursor);
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);
    }

    #[test]
    fn bad_values_are_skipped() {
        let c = parse("[colors]\nline = 256\nfill = -1\n", true);
        assert_eq!(c.warnings, ["config: colors.line = 256? (#rrggbb or 0-255)", "config: colors.fill = -1? (#rrggbb or 0-255)"]);
        let c = parse("[colors]\nline = \"#12345\"\n", true);
        assert_eq!(c.warnings.len(), 1);
        assert_eq!(c.palette.line, Palette::dark(true).line);
    }

    #[test]
    fn not_toml_gives_defaults() {
        let c = parse("glyphs = \"box\"\ntheme = \n", true);
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
        assert!(c.warnings[0].starts_with("config: ") && c.warnings[0].ends_with("(line 2)"), "{:?}", c.warnings);
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);
        assert_eq!(c.palette.line, Palette::dark(true).line);
    }

    #[test]
    fn nearest_is_the_closest() {
        assert_eq!(nearest_256(255, 0, 0), 196);
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(128, 128, 128), 244);
        assert_eq!(nearest_256(95, 135, 175), 67);
    }

    #[test]
    fn load_from_missing_and_written_file() {
        let missing = std::env::temp_dir().join(format!("wtf-config-missing-{}", std::process::id()));
        let c = load_from(&missing);
        assert!(c.warnings.is_empty());
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);

        let path = std::env::temp_dir().join(format!("wtf-config-test-{}.toml", std::process::id()));
        std::fs::write(&path, "theme = \"light\"\n").unwrap();
        let c = load_from(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(c.palette.line, Palette::light(draw::truecolor()).line);
    }
}
