//! The config file (spec §19): glyph set, theme and single colours, read once at start. A bad
//! entry is skipped with a warning for the status line; nothing in it stops wtf.

use crate::draw::{self, Palette};
use crate::theme;
use std::path::{Path, PathBuf};

pub struct Config {
    pub glyphs: &'static draw::Glyphs,
    pub palette: Palette,
    pub warnings: Vec<String>,
}

/// `$XDG_CONFIG_HOME/wtf/`, or `~/.config/wtf/`: `config.toml` and the user's `themes/`. A missing
/// file means the defaults.
pub fn load() -> Config {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty());
    let base = xdg.map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    match base {
        Some(b) => load_from(&b.join("wtf")),
        None => parse("", draw::truecolor(), Path::new("")),
    }
}

fn load_from(dir: &Path) -> Config {
    let (path, themes, rgb) = (dir.join("config.toml"), dir.join("themes"), draw::truecolor());
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text, rgb, &themes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => parse("", rgb, &themes),
        Err(e) => Config { warnings: vec![format!("config: {}: {e}", path.display())], ..parse("", rgb, &themes) },
    }
}

/// The file's text as wtf reads it, with the user's themes in `themes`. Pure apart from reading
/// the theme named, so tests can run it on any text.
pub fn parse(text: &str, rgb: bool, themes: &Path) -> Config {
    let mut warnings = Vec::new();
    let dark = || theme::builtin("dark", rgb).expect("dark is built in");
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            warnings.push(format!("config: {}", theme::toml_error(&e, text)));
            return Config { glyphs: &draw::UNICODE, palette: dark(), warnings };
        }
    };
    let mut glyphs = &draw::UNICODE;
    for (key, value) in &table {
        match key.as_str() {
            "glyphs" => match value.as_str().and_then(|s| draw::SETS.iter().find(|(n, _)| *n == s)) {
                Some((_, g)) => glyphs = g,
                None => warnings.push(format!("config: glyphs = {}? (unicode, box, ascii)", theme::show(value))),
            },
            "theme" | "colors" => {}
            _ => warnings.push(format!("config: unknown key {key} (keys: glyphs, theme, colors)")),
        }
    }
    // the theme first, so the single colours override it whatever the order in the file
    let mut palette = match table.get("theme") {
        None => dark(),
        Some(v) => match v.as_str().and_then(|n| theme::find(n, themes, rgb, &mut warnings)) {
            Some(p) => p,
            None => {
                warnings.push(format!("config: theme = {}? ({})", theme::show(v), theme::names(themes).join(", ")));
                dark()
            }
        },
    };
    if let Some(v) = table.get("colors") {
        match v.as_table() {
            Some(colors) => theme::apply(&mut palette, colors, rgb, "config: ", "colors.", &mut warnings),
            None => warnings.push(format!("config: colors = {}? (a table of colours)", theme::show(v))),
        }
    }
    Config { glyphs, palette, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn dark() -> Palette {
        theme::builtin("dark", true).unwrap()
    }

    /// A config directory of its own, with these theme files.
    fn dir(name: &str, themes: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wtf-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("themes")).unwrap();
        for (n, text) in themes {
            std::fs::write(d.join("themes").join(format!("{n}.toml")), text).unwrap();
        }
        d
    }

    #[test]
    fn valid_file_applies() {
        let c = parse("glyphs = \"box\"\ntheme = \"light\"\n[colors]\ncursor = \"#ff0000\"\npanel = 17\nred = \"#5f87af\"\n", true, Path::new(""));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(c.glyphs.crossing, draw::BOX.crossing);
        assert_eq!(c.palette.line, theme::builtin("light", true).unwrap().line);
        assert_eq!(c.palette.cursor, Color::Rgb(255, 0, 0));
        assert_eq!(c.palette.panel, Color::Indexed(17));
        assert_eq!(c.palette.hues[0], Color::Rgb(0x5f, 0x87, 0xaf));
    }

    #[test]
    fn every_warning() {
        let text = "foo = 1\nglyphs = \"utf8\"\ntheme = 3\n[colors]\ncurser = \"#000000\"\ncursor = \"blue\"\nred = \"#ff0000\"\n";
        let c = parse(text, true, Path::new(""));
        assert_eq!(
            c.warnings,
            [
                "config: unknown key foo (keys: glyphs, theme, colors)",
                "config: glyphs = \"utf8\"? (unicode, box, ascii)",
                "config: theme = 3? (dark, light)",
                "config: unknown key colors.curser (keys: line, fill, unknown, highz, cursor, cursor_text, column, cursor_dim, selected, panel, hatch, dim, background, text, red, orange, yellow, green, cyan, blue, magenta, gray)",
                "config: colors.cursor = \"blue\"? (\"#rrggbb\", 0-255, [\"#rrggbb\", 0-255] or \"default\")",
            ]
        );
        // the valid entries still apply
        assert_eq!(c.palette.hues[0], Color::Rgb(255, 0, 0));
        assert_eq!(c.palette.cursor, dark().cursor);
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);
    }

    #[test]
    fn not_toml_gives_defaults() {
        let c = parse("glyphs = \"box\"\ntheme = \n", true, Path::new(""));
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
        assert!(c.warnings[0].starts_with("config: ") && c.warnings[0].ends_with("(line 2)"), "{:?}", c.warnings);
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);
        assert_eq!(c.palette.line, dark().line);
    }

    #[test]
    fn user_themes() {
        // a theme gives what it sets, dark the rest; one named as a built-in replaces it; a bad
        // entry in one is named with it; an unknown theme lists those there are
        let d = dir("themes", &[("gruvbox", "background = \"#282828\"\nline = [\"#83a598\", 109]\n"), ("light", "line = 1\n"), ("broken", "lne = 1\n")]);
        let themes = d.join("themes");
        let c = parse("theme = \"gruvbox\"\n[colors]\nline = 2\n", true, &themes);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!((c.palette.background, c.palette.line, c.palette.panel), (Color::Rgb(0x28, 0x28, 0x28), Color::Indexed(2), dark().panel));
        assert_eq!(parse("theme = \"light\"", true, &themes).palette.line, Color::Indexed(1));
        assert!(parse("theme = \"broken\"", true, &themes).warnings[0].starts_with("config: theme broken: unknown key lne (keys: line, "));
        assert_eq!(parse("theme = \"nord\"", true, &themes).warnings, ["config: theme = \"nord\"? (broken, dark, gruvbox, light)"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn load_from_missing_and_written_file() {
        let d = dir("load", &[]);
        let c = load_from(&d);
        assert!(c.warnings.is_empty());
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);
        std::fs::write(d.join("config.toml"), "theme = \"light\"\n").unwrap();
        let c = load_from(&d);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(c.palette.line, theme::builtin("light", draw::truecolor()).unwrap().line);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
