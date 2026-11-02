//! Normal mode's keys as a table of action and key (spec §3, §19 "Keys"): the config file's
//! `[keys]` changes it, and everything that names a key (the `?` box, the menus) reads it.

use crate::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A key as Normal mode tells them apart: Shift is in the character, other modifiers but Ctrl
/// don't count.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
}

impl Key {
    pub fn of(k: KeyEvent) -> Self {
        Key { code: k.code, ctrl: k.modifiers.contains(KeyModifiers::CONTROL) }
    }

    /// As the `?` box and the menus show it: `x`, `Ctrl-d`, `Space`, `Enter`, `Tab`.
    fn show(self) -> String {
        let name = match self.code {
            KeyCode::Char(' ') => "Space".into(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "Enter".into(),
            KeyCode::Tab => "Tab".into(),
            KeyCode::BackTab => "Shift-Tab".into(),
            KeyCode::Backspace => "Backspace".into(),
            c => format!("{c:?}"),
        };
        if self.ctrl { format!("Ctrl-{name}") } else { name }
    }
}

macro_rules! actions {
    ($($a:ident $name:literal $key:literal,)*) => {
        /// What a Normal mode key does; its name is the `[keys]` entry.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Action { $($a,)* }
        pub const ACTIONS: &[(Action, &str, &str)] = &[$((Action::$a, $name, $key),)*];
    };
}

// the defaults, in `--keys` notation (spec §3)
actions! {
    Left "left" "h",
    Right "right" "l",
    NextChange "next_change" "w",
    PrevChange "prev_change" "b",
    Down "down" "j",
    Up "up" "k",
    Fold "fold" "<CR>",
    Examine "examine" "e",
    Start "start" "gg",
    End "end" "G",
    HalfRight "half_right" "<C-d>",
    HalfLeft "half_left" "<C-u>",
    ScreenLeft "screen_left" "H",
    ScreenRight "screen_right" "L",
    Center "center" "c",
    PageDown "page_down" "<C-f>",
    PageUp "page_up" "<C-b>",
    ZoomIn "zoom_in" "z",
    ZoomOut "zoom_out" "x",
    Panel "panel" "a",
    Remove "remove" "dd",
    MoveDown "move_down" "J",
    MoveUp "move_up" "K",
    Range "range" "V",
    Radix "radix" "r",
    Undo "undo" "u",
    Redo "redo" "<C-r>",
    CopyValue "copy_value" "yy",
    CopyTime "copy_time" "yt",
    Search "search" "/",
    NextMatch "next_match" "n",
    PrevMatch "prev_match" "N",
    Mark "mark" "m",
    Unmark "unmark" "M",
    NextMark "next_mark" "<Tab>",
    PrevMark "prev_mark" "<S-Tab>",
    Jump "jump" "'",
    JumpBack "jump_back" "''",
    Command "command" ":",
    Help "help" "?",
    Quit "quit" "q",
}

/// `jump` takes the cursor's number after its key (`'2`); it stands for that digit in a sequence.
const DIGIT: Key = Key { code: KeyCode::Null, ctrl: false };

/// What the keys typed so far mean.
#[derive(PartialEq, Debug)]
pub enum Meaning {
    Action(Action),
    /// `jump` and the cursor's number.
    Jump(usize),
    /// The start of a longer key: wait for the next.
    Prefix,
    None,
}

pub struct Keymap {
    /// Each action's key; `jump`'s ends in `DIGIT`.
    keys: Vec<Vec<Key>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap { keys: ACTIONS.iter().map(|&(a, _, k)| seq(a, k).expect("the defaults parse")).collect() }
    }
}

fn seq(a: Action, text: &str) -> Result<Vec<Key>, String> {
    let mut keys: Vec<Key> = crate::tui::app::parse_keys(text)?.into_iter().map(Key::of).collect();
    if a == Action::Jump {
        keys.push(DIGIT);
    }
    Ok(keys)
}

/// A cursor's number, as `jump` takes it.
fn digit(k: &Key) -> bool {
    matches!(k.code, KeyCode::Char('1'..='9')) && !k.ctrl
}

/// The same key, `DIGIT` standing for any number.
fn same(a: &Key, b: &Key) -> bool {
    a == b || (*a == DIGIT && digit(b)) || (*b == DIGIT && digit(a))
}

/// `a` is the start of `b`, or the same key: one of them could never be typed.
fn clash(a: &[Key], b: &[Key]) -> bool {
    a.iter().zip(b).all(|(x, y)| same(x, y))
}

impl Keymap {
    /// The defaults with the config file's `[keys]` table over them; a bad entry is skipped with a
    /// warning, as the rest of the file (spec §19).
    pub fn from_table(table: &toml::Table, warnings: &mut Vec<String>) -> Self {
        let mut map = Keymap::default();
        let names = || ACTIONS.iter().map(|a| a.1).collect::<Vec<_>>().join(", ");
        for (name, value) in table {
            let Some(i) = ACTIONS.iter().position(|a| a.1 == name) else {
                warnings.push(format!("config: unknown key keys.{name} (actions: {})", names()));
                continue;
            };
            let a = ACTIONS[i].0;
            let new = match value.as_str().ok_or_else(|| "a string".to_string()).and_then(|t| seq(a, t)) {
                Ok(k) if k.is_empty() || k.len() > 2 + (a == Action::Jump) as usize => Err("one key or two, as \"x\", \"gx\", \"<C-x>\"".into()),
                // a count comes first, so a digit can't start a key
                Ok(k) if matches!(k[0].code, KeyCode::Char('0'..='9')) && !k[0].ctrl => Err("not a digit: digits are counts".into()),
                Ok(k) if matches!(k[0].code, KeyCode::Esc) => Err("not Esc: it cancels".into()),
                r => r,
            };
            match new {
                Ok(k) => match (0..ACTIONS.len()).find(|&j| j != i && clash(&map.keys[j], &k)) {
                    Some(j) => warnings.push(format!("config: keys.{name} = {}? {} is {}'s key", theme::show(value), map.show(ACTIONS[j].0), ACTIONS[j].1)),
                    None => map.keys[i] = k,
                },
                Err(e) => warnings.push(format!("config: keys.{name} = {}? ({e})", theme::show(value))),
            }
        }
        map
    }

    /// What `typed` (the keys pending, then the new one) means.
    pub fn hit(&self, typed: &[Key]) -> Meaning {
        for (i, k) in self.keys.iter().enumerate() {
            if k.len() > typed.len() && clash(typed, k) {
                return Meaning::Prefix;
            }
            if k.len() == typed.len() && clash(typed, k) {
                return match ACTIONS[i].0 {
                    Action::Jump => match typed.last().unwrap().code {
                        KeyCode::Char(c) => Meaning::Jump(c as usize - '1' as usize),
                        _ => Meaning::None,
                    },
                    a => Meaning::Action(a),
                };
            }
        }
        Meaning::None
    }

    /// An action's key as the `?` box and the menus show it: `x`, `gg`, `Ctrl-r`; `jump`'s without
    /// the number.
    pub fn show(&self, a: Action) -> String {
        let k = &self.keys[a as usize];
        let parts: Vec<String> = k.iter().filter(|&&k| k != DIGIT).map(|k| k.show()).collect();
        // a named key in a sequence gets a space: `g Enter`, not `gEnter`
        parts.join(if parts.iter().any(|p| p.chars().count() > 1) { " " } else { "" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<Key> {
        crate::tui::app::parse_keys(text).unwrap().into_iter().map(Key::of).collect()
    }

    #[test]
    fn defaults() {
        let m = Keymap::default();
        assert_eq!(m.hit(&keys("g")), Meaning::Prefix);
        assert_eq!(m.hit(&keys("gg")), Meaning::Action(Action::Start));
        assert_eq!(m.hit(&keys("gx")), Meaning::None);
        assert_eq!(m.hit(&keys("'")), Meaning::Prefix);
        assert_eq!(m.hit(&keys("'3")), Meaning::Jump(2));
        assert_eq!(m.hit(&keys("''")), Meaning::Action(Action::JumpBack));
        assert_eq!(m.hit(&keys("'0")), Meaning::None);
        assert_eq!(m.hit(&keys("<CR>")), Meaning::Action(Action::Fold));
        assert_eq!(m.hit(&keys("<C-r>")), Meaning::Action(Action::Redo));
        assert_eq!(m.hit(&keys("r")), Meaning::Action(Action::Radix));
        assert_eq!((m.show(Action::Redo), m.show(Action::Start), m.show(Action::Jump), m.show(Action::Fold)), ("Ctrl-r".into(), "gg".into(), "'".into(), "Enter".into()));
        // every default is typeable: none is the start of another
        let d = &m.keys;
        for i in 0..d.len() {
            for j in 0..d.len() {
                assert!(i == j || !clash(&d[i], &d[j]), "{} {}", ACTIONS[i].1, ACTIONS[j].1);
            }
        }
    }

    #[test]
    fn config_table() {
        let mut w = Vec::new();
        let t: toml::Table = "zoom_in = \"i\"\nfold = \"<Space>\"\nzom = \"z\"\nquit = \"x\"\nundo = \"gg\"\nstart = \"5\"\nup = 3\nleft = \"abc\"\njump = \"<C-j>\"".parse().unwrap();
        let m = Keymap::from_table(&t, &mut w);
        assert_eq!(m.hit(&keys("i")), Meaning::Action(Action::ZoomIn));
        assert_eq!(m.hit(&keys("z")), Meaning::None);
        assert_eq!(m.hit(&keys("<Space>")), Meaning::Action(Action::Fold));
        assert_eq!(m.hit(&keys("<C-j>4")), Meaning::Jump(3));
        assert_eq!(m.show(Action::Fold), "Space");
        let names = ACTIONS.iter().map(|a| a.1).collect::<Vec<_>>().join(", ");
        assert_eq!(
            w,
            [
                format!("config: unknown key keys.zom (actions: {names})"),
                "config: keys.quit = \"x\"? x is zoom_out's key".into(),
                "config: keys.undo = \"gg\"? gg is start's key".into(),
                "config: keys.start = \"5\"? (not a digit: digits are counts)".into(),
                "config: keys.up = 3? (a string)".into(),
                "config: keys.left = \"abc\"? (one key or two, as \"x\", \"gx\", \"<C-x>\")".into(),
            ][..]
        );
    }
}
