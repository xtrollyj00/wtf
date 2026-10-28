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
/// `bg` is the terminal's background, if it answered: the theme's surfaces are fitted to it.
pub fn load(bg: Option<theme::Rgb>) -> Config {
    match dir() {
        Some(d) => load_from(&d, bg),
        None => parse("", draw::truecolor(), Path::new(""), bg),
    }
}

pub fn dir() -> Option<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty());
    let base = xdg.map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|b| b.join("wtf"))
}

/// The first start on this machine: there is no config file yet (spec §19 "First start").
pub fn first_start() -> bool {
    dir().is_some_and(|d| !d.join("config.toml").exists())
}

/// Writes the setup's answers into the config file in `dir`, made if missing; the file's other
/// keys and comments stay.
pub fn save(dir: &Path, keys: &[(&str, &str)]) -> Result<(), String> {
    let path = dir.join("config.toml");
    let old = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "# wtf's settings (glyphs, theme, colors); `wtf --setup` asks again\n".into(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, set_keys(&old, keys))).map_err(|e| format!("{}: {e}", path.display()))
}

/// `text` with each top-level `key = "value"` set: an existing line replaced, a missing one added
/// before the first table.
fn set_keys(text: &str, keys: &[(&str, &str)]) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let top = lines.iter().position(|l| l.trim_start().starts_with('[')).unwrap_or(lines.len());
    let mut end = top;
    for (k, v) in keys {
        let line = format!("{k} = \"{v}\"");
        let is_key = |l: &String| l.split_once('=').is_some_and(|(a, _)| a.trim() == *k);
        match lines[..end].iter().position(is_key) {
            Some(i) => lines[i] = line,
            None => {
                // after the last top-level line that isn't blank, so a table keeps its blank line
                let at = lines[..end].iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1);
                lines.insert(at, line);
                end += 1;
            }
        }
    }
    lines.join("\n") + "\n"
}

fn load_from(dir: &Path, bg: Option<theme::Rgb>) -> Config {
    let (path, themes, rgb) = (dir.join("config.toml"), dir.join("themes"), draw::truecolor());
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text, rgb, &themes, bg),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => parse("", rgb, &themes, bg),
        Err(e) => Config { warnings: vec![format!("config: {}: {e}", path.display())], ..parse("", rgb, &themes, bg) },
    }
}

/// The file's text as wtf reads it, with the user's themes in `themes`. Pure apart from reading
/// the theme named, so tests can run it on any text.
pub fn parse(text: &str, rgb: bool, themes: &Path, bg: Option<theme::Rgb>) -> Config {
    let mut warnings = Vec::new();
    let dark = || theme::builtin_on("dark", rgb, bg).expect("dark is built in");
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
        // none chosen: the one that suits the terminal's background
        None if bg.is_some_and(theme::is_light) => theme::builtin("light", rgb).expect("light is built in"),
        None => dark(),
        Some(v) => match v.as_str().and_then(|n| theme::find(n, themes, rgb, bg, &mut warnings)) {
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
    fn setup_keeps_the_rest() {
        let text = "# mine\nglyphs = \"ascii\"\n\n[colors]\ncursor = 1\n";
        assert_eq!(set_keys(text, &[("glyphs", "box"), ("theme", "light")]), "# mine\nglyphs = \"box\"\ntheme = \"light\"\n\n[colors]\ncursor = 1\n");
        assert_eq!(set_keys("# new\n", &[("glyphs", "box")]), "# new\nglyphs = \"box\"\n");
        // a key of the same name in a table is not the top-level one
        assert_eq!(set_keys("[colors]\ntheme = 1\n", &[("theme", "dark")]), "theme = \"dark\"\n[colors]\ntheme = 1\n");
        let d = dir("save", &[]);
        save(&d, &[("glyphs", "box")]).unwrap();
        assert_eq!(parse(&std::fs::read_to_string(d.join("config.toml")).unwrap(), true, Path::new(""), None).glyphs.crossing, draw::BOX.crossing);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn valid_file_applies() {
        let c = parse("glyphs = \"box\"\ntheme = \"light\"\n[colors]\ncursor = \"#ff0000\"\npanel = 17\nred = \"#5f87af\"\n", true, Path::new(""), None);
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
        let c = parse(text, true, Path::new(""), None);
        assert_eq!(
            c.warnings,
            [
                "config: unknown key foo (keys: glyphs, theme, colors)",
                "config: glyphs = \"utf8\"? (unicode, box, ascii)",
                "config: theme = 3? (dark, light)",
                "config: unknown key colors.curser (keys: line, bus, fill, unknown, highz, cursor, cursor_text, column, cursor_dim, selected, panel, hatch, dim, background, text, red, orange, yellow, green, cyan, blue, magenta, gray)",
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
        let c = parse("glyphs = \"box\"\ntheme = \n", true, Path::new(""), None);
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
        assert!(c.warnings[0].starts_with("config: ") && c.warnings[0].ends_with("(line 2)"), "{:?}", c.warnings);
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);
        assert_eq!(c.palette.line, dark().line);
    }

    #[test]
    fn theme_by_background() {
        // no theme chosen: light on a light terminal, dark fitted to a dark one; a chosen one stays
        let light = |text| parse(text, true, Path::new(""), Some((0xff, 0xff, 0xf0))).palette;
        assert_eq!(light("").background, theme::builtin("light", true).unwrap().background);
        assert_eq!(light("theme = \"dark\"").panel, Color::Rgb(232, 232, 217));
        assert_eq!(parse("", true, Path::new(""), Some((0, 0, 0))).palette.panel, Color::Rgb(24, 24, 24));
    }

    #[test]
    fn user_themes() {
        // a theme gives what it sets, dark the rest; one named as a built-in replaces it; a bad
        // entry in one is named with it; an unknown theme lists those there are
        let d = dir("themes", &[("gruvbox", "background = \"#282828\"\nline = [\"#83a598\", 109]\n"), ("light", "line = 1\n"), ("broken", "lne = 1\n")]);
        let themes = d.join("themes");
        let c = parse("theme = \"gruvbox\"\n[colors]\nline = 2\n", true, &themes, None);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!((c.palette.background, c.palette.line, c.palette.panel), (Color::Rgb(0x28, 0x28, 0x28), Color::Indexed(2), dark().panel));
        assert_eq!(parse("theme = \"light\"", true, &themes, None).palette.line, Color::Indexed(1));
        assert!(parse("theme = \"broken\"", true, &themes, None).warnings[0].starts_with("config: theme broken: unknown key lne (keys: line, "));
        assert_eq!(parse("theme = \"nord\"", true, &themes, None).warnings, ["config: theme = \"nord\"? (broken, dark, gruvbox, light)"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn load_from_missing_and_written_file() {
        let d = dir("load", &[]);
        let c = load_from(&d, None);
        assert!(c.warnings.is_empty());
        assert_eq!(c.glyphs.crossing, draw::UNICODE.crossing);
        std::fs::write(d.join("config.toml"), "theme = \"light\"\n").unwrap();
        let c = load_from(&d, None);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        assert_eq!(c.palette.line, theme::builtin("light", draw::truecolor()).unwrap().line);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
