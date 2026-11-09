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
    pub fn show(self) -> String {
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

    pub const fn ch(c: char) -> Self {
        Key { code: KeyCode::Char(c), ctrl: false }
    }

    pub const fn ctrl(c: char) -> Self {
        Key { code: KeyCode::Char(c), ctrl: true }
    }

    pub const fn code(code: KeyCode) -> Self {
        Key { code, ctrl: false }
    }
}

/// The keys of a mode whose keys are fixed (the records browser, the picker; spec §5): a line of
/// the keys page each, its keys with what each does and the line's text. The handler looks a key
/// up here and the page prints the same lines, so a key the page leaves out does nothing.
pub type Fixed<A> = [(&'static [(Key, A)], &'static str)];

/// What `k` does in a `Fixed` table, if anything.
pub fn fixed<A: Copy>(table: &Fixed<A>, k: KeyEvent) -> Option<A> {
    let k = Key::of(k);
    table.iter().flat_map(|(keys, _)| keys.iter()).find(|(key, _)| *key == k).map(|&(_, a)| a)
}

/// A `Fixed` line's keys as the keys page shows them: `j k Down Up`.
#[cfg(test)]
pub fn fixed_keys<A>(keys: &[(Key, A)]) -> String {
    keys.iter().map(|(k, _)| k.show()).collect::<Vec<_>>().join(" ")
}

macro_rules! actions {
    ($($a:ident $name:literal $key:literal $desc:literal $count:literal,)*) => {
        /// What a Normal mode key does; its name is the `[keys]` entry.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Action { $($a,)* }
        /// Each action: its name, default key, what which-key says it does, and whether it takes a
        /// count (repeated, or a mark's number).
        pub const ACTIONS: &[(Action, &str, &str, &str, bool)] = &[$((Action::$a, $name, $key, $desc, $count),)*];
    };
}

// the defaults, in `--keys` notation (spec §3)
actions! {
    Left "left" "h" "cursor left" true,
    Right "right" "l" "cursor right" true,
    NextChange "next_change" "w" "next change" true,
    PrevChange "prev_change" "b" "previous change" true,
    Down "down" "j" "next row" true,
    Up "up" "k" "previous row" true,
    Fold "fold" "<CR>" "open / fold the row" false,
    Examine "examine" "e" "the whole value" false,
    Start "start" "gg" "start of the trace" false,
    End "end" "G" "end of the trace" false,
    HalfRight "half_right" "<C-d>" "half a screen right" true,
    HalfLeft "half_left" "<C-u>" "half a screen left" true,
    ScreenLeft "screen_left" "H" "screen left" true,
    ScreenRight "screen_right" "L" "screen right" true,
    Center "center" "c" "center the cursor" false,
    PageDown "page_down" "<C-f>" "a screen of rows down" true,
    PageUp "page_up" "<C-b>" "a screen of rows up" true,
    ZoomIn "zoom_in" "z" "zoom in" true,
    ZoomOut "zoom_out" "x" "zoom out" true,
    Panel "panel" "a" "add signals" false,
    Remove "remove" "dd" "remove the row" false,
    MoveDown "move_down" "J" "move the row down" true,
    MoveUp "move_up" "K" "move the row up" true,
    Range "range" "V" "select rows" false,
    Radix "radix" "r" "next radix" false,
    Group "group" ">" "group the rows" false,
    Ungroup "ungroup" "<" "out of the group" false,
    Rename "rename" "R" "rename the group" false,
    Put "put" "p" "put removed rows below" false,
    PutAbove "put_above" "P" "put removed rows above" false,
    Undo "undo" "u" "undo" false,
    Redo "redo" "<C-r>" "redo" false,
    CopyValue "copy_value" "yy" "copy the value" false,
    CopyTime "copy_time" "yt" "copy the cursor's time" false,
    Search "search" "/" "search a value" false,
    NextMatch "next_match" "n" "next match" true,
    PrevMatch "prev_match" "N" "previous match" true,
    Mark "mark" "m" "put mark N" true,
    Unmark "unmark" "M" "remove mark N" true,
    NextMark "next_mark" "<Tab>" "next mark" false,
    PrevMark "prev_mark" "<S-Tab>" "previous mark" false,
    Jump "jump" "'" "jump to a cursor" false,
    JumpBack "jump_back" "''" "back before the jump" false,
    Command "command" ":" "command" false,
    Help "help" "?" "keys" false,
    Quit "quit" "q" "quit" false,
    Leader "leader" "<Space>" "the leader menu" false,
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
    /// The leader menu (spec §19 "Leader"): a group named "".
    pub leader: Entry,
}

/// An entry of the leader menu: a `:` command, or a group of entries.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// What it does, the command (run, or with a trailing space put on the command line), and
    /// a longer word on what happens, if any.
    Run { label: String, line: String, why: String },
    Group { name: String, items: Vec<(Key, Entry)> },
}

/// The leader menu's defaults, as a `[leader]` table in the config file would write them.
pub const LEADER: &str = r#"
f = ["find a signal", ":pick signals"]
m = ["marks", ":pick marks"]
r = ["records", ":pick records"]
":" = ["commands", ":pick commands"]
n = ["messages", ":messages"]
"?" = ["keys", ":help"]

[v]
name = "view"
g = ["group the selection…", ":group "]
r = ["rename the group…", ":rename "]
u = ["ungroup", ":ungroup"]
c = ["colour…", ":color "]
x = ["radix…", ":radix "]

[t]
name = "time"
f = ["zoom to fit", ":fit"]
z = ["zoom to…", ":zoom "]
m = ["mark at a time…", ":cursor "]

[d]
name = "decode"
d = ["decode an interface", ":pick interfaces", "the ones found in the trace, each with its line"]
l = ["decodes", ":decodes", "the ones made: records, rows, edit, remove"]
D = ["decode with a form…", ":decode", "name, protocol, clock and roles as fields, checked as you fill them in; on a V range, its rows play the roles"]
p = ["protocol with a form…", ":protocol", "roles from the selected rows, when and the fields checked as you type"]
m = ["records of the matches…", ":protocol search", "the last clocked search as a protocol: its when, the shown signals as fields"]
s = ["computed signal…", ":signal "]
n = ["name values…", ":names", "the values the selected signal takes, each named"]
r = ["records where…", ":records "]

[a]
name = "agent"
f = ["follow…", ":follow ", "the window shows its workspace as it works"]
F = ["stop following", ":follow off", "your own view back"]
t = ["trail", ":pick trail", "its steps; Enter jumps to one's time"]
j = ["join a session…", ":pick sessions", "the agents working on this machine"]
s = ["the agent's session", ":agent", "what it left while no window was open"]

[q]
name = "session"
w = ["save", ":w", "rows, marks and decodes, beside the trace"]
e = ["reload the trace", ":e", "after the simulation ran again"]
q = ["quit", ":q", "refused with unsaved changes"]
x = ["save and quit", ":wq"]
Q = ["quit without saving", ":q!", "unsaved changes are lost"]
"#;

impl Default for Keymap {
    fn default() -> Self {
        let keys = ACTIONS.iter().map(|&(a, _, k, ..)| seq(a, k).expect("the defaults parse")).collect();
        let mut leader = Entry::Group { name: String::new(), items: Vec::new() };
        let mut warnings = Vec::new();
        merge(&mut leader, &LEADER.parse().expect("the leader's defaults are TOML"), "leader", &mut warnings);
        assert!(warnings.is_empty(), "{warnings:?}");
        Keymap { keys, leader }
    }
}

/// `table` over the group `into`: an entry replaces the one on its key, a group merges into the
/// group there, `""` removes it. A bad entry is skipped with a warning naming it by `at`.
fn merge(into: &mut Entry, table: &toml::Table, at: &str, warnings: &mut Vec<String>) {
    let Entry::Group { name, items } = into else { return };
    for (k, v) in table {
        if k == "name" {
            match v.as_str() {
                Some(n) => *name = n.into(),
                None => warnings.push(format!("config: {at}.name = {}? (a string)", theme::show(v))),
            }
            continue;
        }
        let key = match crate::tui::app::parse_keys(k).map(|ks| ks.into_iter().map(Key::of).collect::<Vec<_>>()).as_deref() {
            Ok([key]) if !matches!(key.code, KeyCode::Esc | KeyCode::Backspace) => *key,
            _ => {
                warnings.push(format!("config: {at}.{k}? (one key, not Esc or Backspace: \"x\", \"<C-x>\")"));
                continue;
            }
        };
        let i = items.iter().position(|(ik, _)| *ik == key);
        let bad = |w: &mut Vec<String>| w.push(format!("config: {at}.{k} = {}? ([\"what it does\", \":command\"] and maybe what happens, a table for a group, or \"\" to remove)", theme::show(v)));
        match v {
            toml::Value::String(t) if t.is_empty() => _ = i.map(|i| items.remove(i)),
            toml::Value::Array(a) => match a.as_slice() {
                [toml::Value::String(label), toml::Value::String(line), why @ ..] if line.starts_with(':') && why.len() <= 1 && why.iter().all(|w| w.is_str()) => {
                    let why = why.first().and_then(|w| w.as_str()).unwrap_or("").to_string();
                    let e = Entry::Run { label: label.clone(), line: line.clone(), why };
                    match i {
                        Some(i) => items[i].1 = e,
                        None => items.push((key, e)),
                    }
                }
                _ => bad(warnings),
            },
            toml::Value::Table(t) => {
                let i = match i {
                    Some(i) if matches!(items[i].1, Entry::Group { .. }) => i,
                    _ => {
                        let g = (key, Entry::Group { name: k.clone(), items: Vec::new() });
                        match i {
                            Some(i) => items[i] = g,
                            None => items.push(g),
                        }
                        i.unwrap_or(items.len() - 1)
                    }
                };
                merge(&mut items[i].1, t, &format!("{at}.{k}"), warnings);
            }
            _ => bad(warnings),
        }
    }
}

impl Entry {
    /// The group at `path` (keys from the top), if each key is one.
    pub fn at(&self, path: &[Key]) -> Option<&Entry> {
        path.iter().try_fold(self, |e, k| match e {
            Entry::Group { items, .. } => items.iter().find(|(ik, _)| ik == k).map(|(_, e)| e),
            Entry::Run { .. } => None,
        })
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

    /// The `[leader]` table over the defaults.
    pub fn set_leader(&mut self, table: &toml::Table, warnings: &mut Vec<String>) {
        merge(&mut self.leader, table, "leader", warnings);
    }

    /// What can follow `typed` (the start of a longer key), for which-key: the rest of each key,
    /// and its action. `jump`'s rest is the cursors, which the caller lists.
    pub fn next(&self, typed: &[Key]) -> Vec<(String, Action)> {
        let mut out: Vec<(String, Action)> = self
            .keys
            .iter()
            .enumerate()
            .filter(|(_, k)| k.len() > typed.len() && clash(typed, k))
            .map(|(i, k)| (k[typed.len()..].iter().filter(|&&k| k != DIGIT).map(|k| k.show()).collect::<Vec<_>>().join(" "), ACTIONS[i].0))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// What which-key calls the keys a prefix leads to, from one of them: `' jump to`.
    pub fn group(a: Action) -> &'static str {
        match a {
            Action::Jump | Action::JumpBack => "jump to",
            Action::CopyValue | Action::CopyTime => "copy",
            Action::Remove => "remove",
            _ => "go to",
        }
    }

    /// What which-key says `a` does.
    pub fn desc(a: Action) -> &'static str {
        ACTIONS[a as usize].3
    }

    /// The actions a count repeats, with their keys, for which-key after a count.
    pub fn counted(&self) -> Vec<(String, Action)> {
        ACTIONS.iter().filter(|a| a.4).map(|a| (self.show(a.0), a.0)).collect()
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

    /// The fixed tables (spec §5 "Records", "Picker"): no key on two lines, and a key in none
    /// does nothing.
    #[test]
    fn fixed_tables() {
        use crate::tui::app::App;
        fn unique<A>(table: &Fixed<A>) {
            let all: Vec<Key> = table.iter().flat_map(|(k, _)| k.iter().map(|(k, _)| *k)).collect();
            for (i, k) in all.iter().enumerate() {
                assert!(!all[..i].contains(k), "{} is on two lines", k.show());
            }
        }
        unique(App::BROWSER_KEYS);
        unique(App::PICKER_KEYS);
        let ev = |s: &str| crate::tui::app::parse_keys(s).unwrap()[0];
        assert_eq!(fixed(App::BROWSER_KEYS, ev("G")), Some(crate::tui::browser::Browse::Last));
        assert_eq!(fixed(App::BROWSER_KEYS, ev("<C-f>")), Some(crate::tui::browser::Browse::PageDown));
        assert_eq!(fixed(App::BROWSER_KEYS, ev("f")), None);
        assert_eq!(fixed(App::BROWSER_KEYS, ev("x")), None);
        assert_eq!(fixed(App::PICKER_KEYS, ev("<C-s>")), Some(crate::tui::picker::Pick::Source));
        assert_eq!(fixed(App::PICKER_KEYS, ev("s")), None);
    }

    /// The const constructors the fixed tables are written with build the keys parsing gives.
    #[test]
    fn key_constructors() {
        assert_eq!(Key::ch('G'), keys("G")[0]);
        assert_eq!(Key::ctrl('f'), keys("<C-f>")[0]);
        assert_eq!(Key::code(KeyCode::Tab), keys("<Tab>")[0]);
    }

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
    fn leader_table() {
        // an entry replaces the one on its key, a group merges, "" removes; bad ones are named
        let mut m = Keymap::default();
        let mut w = Vec::new();
        let t: toml::Table = "u = \"\"\nf = [\"find\", \":pick signals\"]\nab = [\"x\", \":q\"]\nx = [\"x\", \"q\"]\n[a]\nname = \"ai\"\nt = [\"steps\", \":trail\"]\n[g]\nname = \"go\"\ns = [\"start\", \":0\"]".parse().unwrap();
        m.set_leader(&t, &mut w);
        let k = |t: &str| keys(t);
        assert_eq!(m.leader.at(&k("u")), None);
        assert_eq!(m.leader.at(&k("f")), Some(&Entry::Run { label: "find".into(), line: ":pick signals".into(), why: String::new() }));
        assert!(matches!(m.leader.at(&k("a")), Some(Entry::Group { name, .. }) if name == "ai"));
        assert_eq!(m.leader.at(&k("at")), Some(&Entry::Run { label: "steps".into(), line: ":trail".into(), why: String::new() }));
        // the rest of the agent group stays
        assert!(m.leader.at(&k("aF")).is_some());
        assert!(m.leader.at(&k("gs")).is_some());
        assert_eq!(
            w,
            [
                "config: leader.ab? (one key, not Esc or Backspace: \"x\", \"<C-x>\")",
                "config: leader.x = [\"x\", \"q\"]? ([\"what it does\", \":command\"] and maybe what happens, a table for a group, or \"\" to remove)",
            ]
        );
    }

    #[test]
    fn config_table() {
        let mut w = Vec::new();
        let t: toml::Table = "zoom_in = \"i\"\nfold = \"<C-o>\"\nhelp = \"<Space>\"\nzom = \"z\"\nquit = \"x\"\nundo = \"gg\"\nstart = \"5\"\nup = 3\nleft = \"abc\"\njump = \"<C-j>\"".parse().unwrap();
        let m = Keymap::from_table(&t, &mut w);
        assert_eq!(m.hit(&keys("i")), Meaning::Action(Action::ZoomIn));
        assert_eq!(m.hit(&keys("z")), Meaning::None);
        assert_eq!(m.hit(&keys("<C-o>")), Meaning::Action(Action::Fold));
        assert_eq!(m.hit(&keys("<C-j>4")), Meaning::Jump(3));
        assert_eq!(m.show(Action::Fold), "Ctrl-o");
        let names = ACTIONS.iter().map(|a| a.1).collect::<Vec<_>>().join(", ");
        assert_eq!(
            w,
            [
                "config: keys.help = \"<Space>\"? Space is leader's key".to_string(),
                format!("config: unknown key keys.zom (actions: {names})"),
                "config: keys.quit = \"x\"? x is zoom_out's key".into(),
                "config: keys.undo = \"gg\"? gg is start's key".into(),
                "config: keys.start = \"5\"? (not a digit: digits are counts)".into(),
                "config: keys.up = 3? (a string)".into(),
                "config: keys.left = \"abc\"? (one key or two, as \"x\", \"gx\", \"<C-x>\")".into(),
            ][..]
        );
    }

    #[test]
    fn shown_keys_and_groups() {
        let show = |t: &str| keys(t)[0].show();
        assert_eq!((show("<BS>"), show("<S-Tab>"), Key { code: KeyCode::Up, ctrl: false }.show()), ("Backspace".into(), "Shift-Tab".into(), "Up".into()));
        assert_eq!((Keymap::group(Action::JumpBack), Keymap::group(Action::CopyTime), Keymap::group(Action::Remove), Keymap::group(Action::Start)), ("jump to", "copy", "remove", "go to"));
        // a jump key followed by something that is no digit means nothing
        let mut w = Vec::new();
        let m = Keymap::from_table(&"jump = \"<C-j>\"".parse().unwrap(), &mut w);
        assert_eq!(m.hit(&keys("<C-j><CR>")), Meaning::None);
    }

    #[test]
    fn leader_table_replaces() {
        // a non-string name is named; a table over a command replaces it; a path through a command is none
        let mut m = Keymap::default();
        let mut w = Vec::new();
        let t: toml::Table = "[f]\nname = 3\n[u]\nname = \"x\"\ny = [\"y\", \":q\"]\n[n]\nz = [\"z\", \":q\"]".parse().unwrap();
        m.set_leader(&t, &mut w);
        assert_eq!(w, ["config: leader.f.name = 3? (a string)"]);
        let mut w = Vec::new();
        m.set_leader(&"q = 3".parse().unwrap(), &mut w);
        assert_eq!(w.len(), 1);
        // a new group on a free key, and over a command key
        assert!(matches!(m.leader.at(&keys("uy")), Some(Entry::Run { .. })));
        assert!(matches!(m.leader.at(&keys("nz")), Some(Entry::Run { .. })));
        let Entry::Group { items, .. } = &m.leader else { panic!() };
        let run = items.iter().find(|(_, e)| matches!(e, Entry::Run { .. })).map(|(k, _)| *k);
        if let Some(k) = run {
            assert_eq!(m.leader.at(&[k, k]), None);
            let mut m2 = m;
            let key = k.show();
            m2.set_leader(&format!("[{key}]\nname = \"g\"").parse().unwrap(), &mut Vec::new());
            assert!(matches!(m2.leader.at(&[k]), Some(Entry::Group { .. })));
        }
    }
}
