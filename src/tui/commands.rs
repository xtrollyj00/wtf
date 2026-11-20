//! The `:` commands the command line lists and completes (spec §4 "Command line", §7): the
//! core's table, and the window's own (`:w`, `:q`, `:help`) with what it does before the core.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use wtf_core::command::{self, ArgKind};

use crate::draw::icon as ic;
use crate::tui::picker::Source;

/// What the window does with a `:` command before the core runs it (spec §7).
pub enum When {
    /// The window's own: it answers and the core never sees it.
    Window,
    /// With no argument: the window opens a form or a card for it.
    Empty,
    /// With this one argument: `:protocol search`.
    Is(&'static str),
    /// Never: the window answers with what it is for.
    Refuse(&'static str),
}

impl When {
    /// It claims this argument.
    fn claims(&self, arg: &str) -> bool {
        match self {
            When::Window | When::Refuse(_) => true,
            When::Empty => arg.is_empty(),
            When::Is(s) => arg == *s,
        }
    }
}

/// A command only the window has, or one it takes from the core first: name, usage, description
/// and arguments as the core's `Command` has them (spec §7).
pub struct WindowCmd {
    pub name: &'static str,
    pub usage: &'static str,
    pub desc: &'static str,
    pub args: &'static [ArgKind],
    pub trailing: bool,
    pub icon: char,
    pub when: When,
}

/// A row with nothing to list, only what the window does (`:e` reloads the window's file):
/// `WindowCmd { name, when, ..GUARD }`.
const GUARD: WindowCmd = WindowCmd {
    name: "",
    usage: "",
    desc: "",
    args: &[],
    trailing: false,
    icon: '\0',
    when: When::Window,
};

pub const WINDOW: &[WindowCmd] = &[
    WindowCmd {
        name: "records",
        usage: ":records <streams> [where <expr>]",
        desc: "Browse records as a table: a stream, a decode's results (:records bus), a glob; / filters, s sorts, = groups",
        args: &[ArgKind::Stream, ArgKind::Lit("where"), ArgKind::Field],
        trailing: true,
        icon: ic::RECORD,
        when: When::Window,
    },
    WindowCmd {
        name: "decodes",
        usage: ":decodes",
        desc: "List the decodes made: Enter opens records, a adds rows, e edits, dd removes",
        args: &[],
        trailing: false,
        icon: ic::EDIT,
        when: When::Window,
    },
    WindowCmd {
        name: "pick",
        usage: ":pick <source>",
        desc: "A fuzzy list with a preview: signals, marks, records <stream>, commands, sessions, trail, interfaces; Ctrl-s goes to the next",
        args: &[ArgKind::PickSource, ArgKind::Stream],
        trailing: false,
        icon: ic::SEARCH,
        when: When::Window,
    },
    WindowCmd {
        name: "messages",
        usage: ":messages",
        desc: "Every message so far, with its time and who caused it: you or an agent",
        args: &[],
        trailing: false,
        icon: ic::BELL,
        when: When::Window,
    },
    WindowCmd {
        name: "trail",
        usage: ":trail",
        desc: "The agent's steps; Enter jumps to a step's time",
        args: &[],
        trailing: false,
        icon: ic::TRAIL,
        when: When::Window,
    },
    WindowCmd {
        name: "agent",
        usage: ":agent",
        desc: "Show the session an agent left for this trace when no window was open",
        args: &[],
        trailing: false,
        icon: ic::AGENT,
        when: When::Window,
    },
    WindowCmd {
        name: "follow",
        usage: ":follow [<id>|off]",
        desc: "Follow an agent working on this trace: show its workspace and each change it makes",
        args: &[ArgKind::Agent],
        trailing: false,
        icon: ic::EYE,
        when: When::Window,
    },
    WindowCmd {
        name: "w",
        usage: ":w [file]",
        desc: "Save the session",
        args: &[ArgKind::FilePath],
        trailing: false,
        icon: ic::SAVE,
        when: When::Window,
    },
    WindowCmd {
        name: "wq",
        usage: ":wq",
        desc: "Save the session and quit",
        args: &[],
        trailing: false,
        icon: ic::SAVE,
        when: When::Window,
    },
    WindowCmd {
        name: "help",
        usage: ":help [topic]",
        desc: "Show the keys of the current mode (also ?), or a topic: names times radix expressions decode records workspace commands",
        args: &[ArgKind::HelpTopic],
        trailing: false,
        icon: ic::KEYS,
        when: When::Window,
    },
    WindowCmd {
        name: "q",
        usage: ":q",
        desc: "Quit; refuses if the session has unsaved changes",
        args: &[],
        trailing: false,
        icon: ic::ERROR,
        when: When::Window,
    },
    WindowCmd {
        name: "q!",
        usage: ":q!",
        desc: "Quit without saving",
        args: &[],
        trailing: false,
        icon: ic::ERROR,
        when: When::Window,
    },
    // and what the window takes from the core first, with nothing of its own to list
    WindowCmd {
        name: "e",
        when: When::Window,
        ..GUARD
    },
    WindowCmd {
        name: "decode",
        when: When::Empty,
        ..GUARD
    },
    WindowCmd {
        name: "protocol",
        when: When::Empty,
        ..GUARD
    },
    WindowCmd {
        name: "protocol",
        when: When::Is("search"),
        ..GUARD
    },
    WindowCmd {
        name: "names",
        when: When::Empty,
        ..GUARD
    },
    WindowCmd {
        name: "unit",
        when: When::Refuse(
            "unit sets the time unit of an agent's answers; the window picks what fits",
        ),
        ..GUARD
    },
];

/// What the window does with `cmd` and `arg` before the core runs it (spec §7). None: the core's.
pub fn window_for(cmd: &str, arg: &str) -> Option<&'static WindowCmd> {
    let arg = arg.trim();
    WINDOW.iter().find(|w| w.name == cmd && w.when.claims(arg))
}

/// A `:` command as a listing shows it (spec §7): the core's, or the window's own.
#[derive(Clone)]
pub struct Listed {
    pub name: &'static str,
    /// The usage line as a listing shows it: the error line without `usage: `.
    pub usage: String,
    pub desc: &'static str,
    pub args: &'static [ArgKind],
    pub trailing: bool,
    pub icon: char,
}

/// `:<time>` is line syntax, not a command, but the listings show it beside them (spec §7).
pub fn at_time() -> Listed {
    Listed {
        name: "<time>",
        usage: ":<time>".to_string(),
        desc: "Move the cursor to a time, e.g. :2000ns",
        args: &[],
        trailing: false,
        icon: ic::EDIT,
    }
}

/// The window refuses this core command (spec §7): no listing shows it.
fn refused(name: &str) -> bool {
    WINDOW
        .iter()
        .any(|w| w.name == name && matches!(w.when, When::Refuse(_)))
}

fn from_core(c: &'static command::Command) -> Listed {
    Listed {
        name: c.name,
        usage: shown(c),
        desc: c.desc,
        args: c.args,
        trailing: c.trailing,
        icon: c.icon,
    }
}

fn from_window(w: &'static WindowCmd) -> Listed {
    Listed {
        name: w.name,
        usage: w.usage.to_string(),
        desc: w.desc,
        args: w.args,
        trailing: w.trailing,
        icon: w.icon,
    }
}

/// Every `:` command a listing shows: the core's but for the one the window refuses, then the
/// window's own (spec §7).
pub fn listed() -> Vec<Listed> {
    let mut out: Vec<Listed> = command::COMMANDS
        .iter()
        .filter(|c| !refused(c.name))
        .map(from_core)
        .collect();
    out.extend(
        WINDOW
            .iter()
            .filter(|w| command::lookup(w.name).is_none())
            .map(from_window),
    );
    out
}

/// The listed command of that name, if `:` has it.
pub fn entry(name: &str) -> Option<Listed> {
    match command::lookup(name) {
        Some(c) if !refused(name) => Some(from_core(c)),
        Some(_) => None,
        None => WINDOW.iter().find(|w| w.name == name).map(from_window),
    }
}

/// The usage line as a listing shows it: the error line without `usage: `, and for the one that
/// says what to do instead (`zoom`'s, the only one) its time argument.
fn shown(c: &command::Command) -> String {
    match c.usage.strip_prefix("usage: ") {
        Some(u) => u.to_string(),
        None => format!(":{} <time>", c.name),
    }
}

/// What `Tab` can put on the command line (spec §4 "Command line"): the command being typed, or
/// an argument of the one typed.
pub struct Completion {
    /// What is listed: `command`, `protocol`, `signal`, ...
    pub what: &'static str,
    /// The argument being typed, when the items complete one: what they are, for their glyphs.
    pub kind: Option<ArgKind>,
    /// The characters of the line an item replaces: the word at the end being typed.
    pub start: usize,
    pub items: Vec<Item>,
}

pub struct Item {
    /// What replaces the word, a space after it where another argument follows.
    pub text: String,
    /// How the list shows it, and one line on it after.
    pub label: String,
    pub desc: String,
    /// What is still to type after it, shown dim after the cursor: `<time>`.
    pub then: String,
    /// The docs card beside the list; without it, the description where the list cuts it.
    pub doc: String,
    /// The trace has this protocol: an interface found in it is marked, not a plug.
    pub found_here: bool,
}

impl Completion {
    /// What the highlighted item would add after what is typed, if it starts with it.
    pub fn ghost(&self, line: &str, hl: usize) -> String {
        let Some(it) = self.items.get(hl) else {
            return String::new();
        };
        let word: String = line.chars().skip(self.start).collect();
        match it.text.strip_prefix(word.as_str()) {
            Some(rest) => format!("{rest}{}", it.then),
            None => String::new(),
        }
    }

    /// The line with the highlighted item in place of the word.
    pub fn take(&self, line: &str, hl: usize) -> Option<String> {
        let it = self.items.get(hl).filter(|it| !it.text.starts_with('<'))?;
        Some(line.chars().take(self.start).collect::<String>() + &it.text)
    }
}

/// The candidates matching `word`, best first.
fn fuzzy<'a>(word: &str, names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let names: Vec<&str> = names.into_iter().collect();
    if word.is_empty() {
        return names;
    }
    Pattern::parse(word, CaseMatching::Ignore, Normalization::Smart)
        .match_list(names, &mut Matcher::new(Config::DEFAULT))
        .into_iter()
        .map(|m| m.0)
        .collect()
}

fn item(text: impl Into<String>, desc: impl Into<String>) -> Item {
    let text = text.into();
    Item {
        label: text.trim_end().to_string(),
        text,
        desc: desc.into(),
        then: String::new(),
        doc: String::new(),
        found_here: false,
    }
}

/// What the line completes to: while the first word is typed (or isn't a command) the commands,
/// then the typed command's arguments.
pub fn complete(line: &str, app: &crate::tui::app::App) -> Completion {
    let first = line.split_whitespace().next().unwrap_or("");
    let lead = line.len() - line.trim_start().len();
    let in_first = !line[lead..].contains(char::is_whitespace);
    let cmd = entry(first).filter(|c| !c.args.is_empty());
    match cmd {
        Some(c) if !in_first => arguments(&c, line, app),
        _ => commands(line, lead + first.len(), in_first),
    }
}

/// The commands matching the first word.
fn commands(line: &str, end: usize, alone: bool) -> Completion {
    let word = &line[..end];
    let all = listed();
    let found: Vec<Listed> = if word
        .trim()
        .starts_with(|c: char| c.is_ascii_digit() || c == '.')
    {
        vec![at_time()]
    } else {
        let names = fuzzy(word.trim(), all.iter().map(|c| c.name));
        let mut v: Vec<Listed> = names
            .into_iter()
            .filter_map(|n| all.iter().find(|c| c.name == n).cloned())
            .collect();
        if word.trim().is_empty() {
            v.insert(0, at_time());
        }
        v
    };
    let items = found
        .into_iter()
        .map(|c| {
            // the arguments still to type, from the usage: `:zoom <time>` -> `<time>`
            let then = if alone {
                c.usage
                    .trim_start_matches(':')
                    .strip_prefix(c.name)
                    .unwrap_or("")
                    .trim_start()
                    .to_string()
            } else {
                String::new()
            };
            let text = if alone && !c.args.is_empty() {
                format!("{} ", c.name)
            } else {
                c.name.to_string()
            };
            Item {
                text,
                label: c.usage,
                desc: c.desc.into(),
                then,
                doc: String::new(),
                found_here: false,
            }
        })
        .collect();
    Completion {
        what: "command",
        kind: None,
        start: 0,
        items,
    }
}

/// The typed command's argument being typed: the word at the end of the line.
fn arguments(c: &Listed, line: &str, app: &crate::tui::app::App) -> Completion {
    use wtf_core::protocol;
    let ws = &app.ws;
    let at = line.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let word = &line[at..];
    // which argument, counting the command as 0
    let n = line[..at].split_whitespace().count();
    let start = line[..at].chars().count();
    let signals = |word: &str| -> Vec<Item> {
        let paths = fuzzy(word, ws.trace.vars.iter().map(|v| v.path.as_str()));
        paths.into_iter().take(200).map(|p| item(p, "")).collect()
    };
    let last = if c.trailing { c.args.last() } else { None };
    let Some(&kind) = c.args.get(n.saturating_sub(1)).or(last) else {
        return Completion {
            what: "",
            kind: None,
            start,
            items: Vec::new(),
        };
    };
    let (what, items): (&'static str, Vec<Item>) = match kind {
        ArgKind::Signal => ("signal", signals(word)),
        // the design's scopes: the paths it holds under itself (spec §15)
        ArgKind::Scope => {
            let mut scopes: Vec<&str> = ws
                .trace
                .design()
                .nodes
                .iter()
                .filter(|n| !n.children.is_empty())
                .map(|n| n.path.as_str())
                .collect();
            scopes.sort();
            scopes.dedup();
            (
                "scope",
                fuzzy(word, scopes)
                    .into_iter()
                    .take(200)
                    .map(|s| item(s, ""))
                    .collect(),
            )
        }
        // a decode's name: its results, streams and all; `:pick records` names a stream only
        ArgKind::Stream if c.args.first() == Some(&ArgKind::PickSource) => {
            let names: Vec<&str> = if line.split_whitespace().nth(1) == Some("records") {
                ws.streams().iter().map(|s| s.name.as_str()).collect()
            } else {
                Vec::new()
            };
            (
                "stream",
                fuzzy(word, names)
                    .into_iter()
                    .map(|s| item(s, ""))
                    .collect(),
            )
        }
        ArgKind::Stream => (
            "stream",
            fuzzy(
                word,
                ws.decodes()
                    .iter()
                    .map(|d| d.name.as_str())
                    .chain(ws.streams().iter().map(|s| s.name.as_str())),
            )
            .into_iter()
            .map(|s| item(s, ""))
            .collect(),
        ),
        ArgKind::Protocol => ("protocol", protocols(word, ws)),
        // a protocol's roles, and after a role's `=`, its signal
        ArgKind::Role => {
            let proto = line.split_whitespace().nth(2).unwrap_or("");
            match word.split_once('=') {
                Some((role, v)) => {
                    let at = start + role.chars().count() + 1;
                    let items = if role == "prefix" {
                        let found = protocol::detect(&ws.trace);
                        fuzzy(
                            v,
                            found
                                .iter()
                                .filter(|f| f.protocol == proto)
                                .map(|f| f.prefix.as_str()),
                        )
                        .into_iter()
                        .map(|p| item(format!("{p} "), ""))
                        .collect()
                    } else {
                        signals(v)
                            .into_iter()
                            .map(|mut it| {
                                it.text.push(' ');
                                it
                            })
                            .collect()
                    };
                    return Completion {
                        what: if role == "prefix" { "prefix" } else { "signal" },
                        kind: Some(kind),
                        start: at,
                        items,
                    };
                }
                None => {
                    let roles: Vec<String> = match ws.sources().get(proto) {
                        Some(any) => match any.def() {
                            Some(def) => {
                                let (required, optional) = def.roles();
                                ["clock", "prefix"]
                                    .iter()
                                    .copied()
                                    .filter(|r| *r != "clock" || def.clocked())
                                    .chain(required)
                                    .chain(optional)
                                    .chain(def.params().iter().copied())
                                    .map(String::from)
                                    .collect()
                            }
                            // a session sampler or an on-stream protocol: no prefix,
                            // and a clock only where one is read
                            None => ["clock"]
                                .into_iter()
                                .filter(|_| any.clocked())
                                .chain(any.roles().0)
                                .chain(any.params())
                                .map(String::from)
                                .collect(),
                        },
                        // a word typed past a listed name completes to nothing, as the
                        // session-miss did; an unloaded session file falls back to the session
                        None => ws
                            .protocols()
                            .iter()
                            .find(|p| p.name == proto)
                            .map(|p| p.roles.clone())
                            .unwrap_or_default(),
                    };
                    (
                        "role",
                        fuzzy(word, roles.iter().map(|r| r.as_str()))
                            .into_iter()
                            .map(|r| item(format!("{r}="), ""))
                            .collect(),
                    )
                }
            }
        }
        // where's expression: the fields of the stream named at word 1
        ArgKind::Field => {
            let st = line
                .split_whitespace()
                .nth(1)
                .and_then(|n| ws.streams().iter().find(|s| s.name == n));
            let skip = word.len() - word.trim_start_matches(['!', '(', '~']).len();
            let fields: Vec<&str> = st
                .map(|s| s.schema.iter().map(|f| f.name.as_str()).collect())
                .unwrap_or_default();
            let items = fuzzy(&word[skip..], fields)
                .into_iter()
                .map(|f| item(format!("{}{f} ", &word[..skip]), ""))
                .collect();
            return Completion {
                what: "field",
                kind: Some(kind),
                start,
                items,
            };
        }
        ArgKind::Radix => (
            "radix",
            fuzzy(word, wtf_core::radix::RADIXES.iter().map(|r| r.name))
                .into_iter()
                .map(|r| item(r, ""))
                .collect(),
        ),
        ArgKind::Color => (
            "colour",
            fuzzy(word, wtf_core::rows::COLORS.iter().copied().chain(["off"]))
                .into_iter()
                .map(|r| item(r, ""))
                .collect(),
        ),
        ArgKind::HelpTopic => {
            let topics = crate::guide::TOPICS;
            (
                "topic",
                fuzzy(word, topics.iter().map(|t| t.0))
                    .into_iter()
                    .filter_map(|t| topics.iter().find(|x| x.0 == t))
                    .map(|t| item(t.0, t.1))
                    .collect(),
            )
        }
        ArgKind::Agent => (
            "agent",
            fuzzy(word, app.agents.iter().map(|a| a.0.as_str()).chain(["off"]))
                .into_iter()
                .map(|a| item(a, ""))
                .collect(),
        ),
        ArgKind::PickSource => (
            "source",
            fuzzy(word, Source::ALL.map(Source::name))
                .into_iter()
                .map(|s| {
                    item(
                        if s == "records" {
                            format!("{s} ")
                        } else {
                            s.to_string()
                        },
                        "",
                    )
                })
                .collect(),
        ),
        ArgKind::FilePath => ("file", files(word)),
        ArgKind::Row => {
            let rows: Vec<String> = ws
                .rows
                .iter()
                .map(|r| match r {
                    wtf_core::rows::Row::Signal(s) => ws.trace.vars[s.var].path.clone(),
                    wtf_core::rows::Row::Group { name, .. } => name.clone(),
                })
                .collect();
            (
                "row",
                fuzzy(word, rows.iter().map(|r| r.as_str()))
                    .into_iter()
                    .map(|r| item(format!("{r} "), ""))
                    .collect(),
            )
        }
        ArgKind::CursorIndex => {
            let marks: Vec<Item> = (0..9)
                .map(|i| {
                    let what = match (i, ws.marks()[i].as_ref().map(|m| m.time)) {
                        (0, _) => "the cursor".to_string(),
                        (_, None) => "not put".into(),
                        (_, Some(t)) => format!(
                            "{} {}",
                            wtf_core::time::format(t, ws.trace.fs_per_tick),
                            ws.marks()[i]
                                .as_ref()
                                .map(|m| m.name.as_str())
                                .unwrap_or("")
                        ),
                    };
                    item(format!("{} ", i + 1), what)
                })
                .collect();
            (
                "mark",
                marks
                    .into_iter()
                    .filter(|m| m.text.starts_with(word))
                    .collect(),
            )
        }
        ArgKind::MarkRef => {
            // the marks put, as '2
            let marks: Vec<Item> = (1..9)
                .filter_map(|i| {
                    ws.marks()[i].as_ref().map(|m| {
                        let t = m.time;
                        item(
                            format!("'{} ", i + 1),
                            format!(
                                "{} {}",
                                wtf_core::time::format(t, ws.trace.fs_per_tick),
                                m.name
                            ),
                        )
                    })
                })
                .collect();
            (
                "mark",
                marks
                    .into_iter()
                    .filter(|m| m.text.starts_with(word))
                    .collect(),
            )
        }
        // a definition's or a mark's keys: `key=value`
        ArgKind::KeyVal => {
            // a definition names only its roles: in when= and the fields, the roles given so far
            let roles: Vec<String> = line
                .split_whitespace()
                .find_map(|w| w.strip_prefix("roles="))
                .map_or(Vec::new(), |r| {
                    r.split(',')
                        .filter(|r| !r.is_empty())
                        .map(String::from)
                        .collect()
                });
            let in_expr = line[..at]
                .split_whitespace()
                .rev()
                .find(|w| key(w).is_some())
                .is_some_and(|w| !w.starts_with("roles="));
            let (k, v, from) = match key(word) {
                Some(k) => (Some(k), &word[k.len() + 1..], start + k.chars().count() + 1),
                None => (None, word, start),
            };
            match (k, in_expr) {
                (Some("roles"), _) => ("", Vec::new()),
                (Some(_), _) | (None, true) => {
                    let skip = v.len() - v.trim_start_matches(['!', '(', '~']).len();
                    let items = fuzzy(&v[skip..], roles.iter().map(String::as_str))
                        .into_iter()
                        .map(|r| item(format!("{}{r} ", &v[..skip]), "role"))
                        .collect();
                    return Completion {
                        what: "role",
                        kind: Some(kind),
                        start: from,
                        items,
                    };
                }
                (None, false) => (
                    "key",
                    fuzzy(word, ["roles=", "when="])
                        .into_iter()
                        .map(|k| item(k, ""))
                        .collect(),
                ),
            }
        }
        // the values of a fixed set
        ArgKind::Lit(set) => (
            "",
            fuzzy(word, set.split('|'))
                .into_iter()
                // `name=` goes on with its value, no space; `=` is a word of its own
                .map(|v| {
                    let sp = if v != "=" && v.ends_with('=') {
                        ""
                    } else {
                        " "
                    };
                    let desc = match v {
                        "=" => "define it",
                        "off" => "remove it",
                        "where" => "only the records it holds for",
                        _ => "",
                    };
                    item(format!("{v}{sp}"), desc)
                })
                .collect(),
        ),
        // a name, a time, the rest of a line: the user's to write
        ArgKind::Time | ArgKind::Text => ("", Vec::new()),
    };
    Completion {
        what,
        kind: Some(kind),
        start,
        items,
    }
}

/// The icon before a completion (spec §14 `nerd`): a command's own, a protocol's plug, an
/// interface found in the trace a check.
pub fn item_icon(kind: Option<ArgKind>, it: &Item) -> char {
    match kind {
        None => self::icon(&it.text),
        Some(ArgKind::Protocol) if it.found_here => ic::OK,
        Some(ArgKind::Protocol) => ic::PLUG,
        Some(ArgKind::Signal | ArgKind::Role) => ic::WIRE,
        Some(ArgKind::Stream) => ic::STREAM,
        Some(ArgKind::CursorIndex | ArgKind::MarkRef) => ic::MARK,
        Some(ArgKind::Agent) => ic::AGENT,
        Some(ArgKind::FilePath) => ic::FILE,
        Some(ArgKind::Scope | ArgKind::Row) => ic::FOLDER,
        Some(_) => ic::CMD,
    }
}

/// Files and directories starting with what is typed, for `:w`: the session files (`.toml`) and
/// directories to go into.
fn files(word: &str) -> Vec<Item> {
    let (dir, base) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let Ok(rd) = std::fs::read_dir(if dir.is_empty() { "." } else { dir }) else {
        return Vec::new();
    };
    let mut out: Vec<Item> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            (name.starts_with(base)
                && !name.starts_with('.')
                && (is_dir || name.ends_with(".toml")))
            .then(|| item(format!("{dir}{name}{}", if is_dir { "/" } else { "" }), ""))
        })
        .collect();
    out.sort_by(|a, b| a.text.cmp(&b.text));
    out
}

/// The key of a `key=value` argument; none for a comparison (`a==b`, `a<=3`).
fn key(word: &str) -> Option<&str> {
    let (k, rest) = word.split_once('=')?;
    let ok = !k.is_empty()
        && k.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !rest.starts_with('=');
    ok.then_some(k)
}

/// `:decode <name> <protocol>`: the interfaces found in the trace first, each with its whole
/// line, then every protocol with what it makes.
fn protocols(word: &str, ws: &wtf_core::workspace::Workspace) -> Vec<Item> {
    use wtf_core::protocol;
    let sources = ws.sources();
    let about = |n: &str| -> String {
        match sources.get(n) {
            Some(protocol::Any::OnStreams(d)) => d.about.to_string(),
            // a session sampler, without a card, reads like the session's own
            any => any
                .as_ref()
                .and_then(|a| a.def())
                .map_or("in this session".into(), |d| d.about().to_string()),
        }
    };
    let found = protocol::detect(&ws.trace);
    let names: Vec<&str> = sources.names();
    let (mut items, mut rest) = (Vec::new(), Vec::new());
    for n in fuzzy(word, names) {
        let doc = protocol::brief(n, sources).unwrap_or_default()
            + "\n\nTab takes it.  :help decode has the rest.";
        for f in found.iter().filter(|f| f.protocol == n) {
            // the line `decode_line` gives, from the protocol on
            let line = f.decode_line();
            let args = line.splitn(3, ' ').nth(2).unwrap_or("").to_string();
            let rest = args.strip_prefix(n).unwrap_or(&args).trim();
            items.push(Item {
                label: format!("{n}  {}", f.prefix),
                desc: format!("found here: {rest}"),
                text: format!("{args} "),
                then: String::new(),
                doc: format!("This trace: {rest}\n\n{doc}"),
                found_here: true,
            });
        }
        let then = match sources.get(n) {
            Some(p) => match p.def() {
                Some(def) if def.clocked() => "clock=<clk> prefix=<prefix>",
                Some(_) => "prefix=<prefix>",
                // a session sampler and `check` read a clock; `frame` and `pair` name streams
                None if p.clocked() => "clock=<clk>",
                None => "",
            },
            None => "",
        };
        rest.push(Item {
            text: format!("{n} "),
            label: n.into(),
            desc: about(n),
            then: then.into(),
            doc,
            found_here: false,
        });
    }
    // the trace's own interfaces first, each protocol after them in the order matched
    items.extend(rest);
    items
}

/// A `:` command's icon, for the leader menu: by its name (spec §14 `nerd`).
pub fn icon(line: &str) -> char {
    use crate::draw::icon;
    let mut words = line.trim_start_matches(':').split_whitespace();
    let name = words.next().unwrap_or("");
    let arg = words.next().unwrap_or("");
    // `:pick` is listed as one thing, but its rows are as many
    match name {
        "pick" => Source::from_name(arg).map_or(icon::SEARCH, Source::icon),
        _ => entry(name).map_or(icon::EDIT, |c| c.icon),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes() {
        let ws =
            wtf_core::workspace::Workspace::open(std::path::Path::new("tests/data/axilite.vcd"))
                .unwrap();
        let mut app = crate::tui::app::App::new(ws);
        let names = |line: &str| {
            complete(line, &app)
                .items
                .into_iter()
                .map(|i| i.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("").len(), listed().len() + 1);
        assert_eq!(names("20")[0], "<time>");
        assert_eq!(names("zo")[0], "zoom ");
        assert_eq!(names("ad top")[0], "add");
        let c = complete("zo", &app);
        assert_eq!(c.ghost("zo", 0), "om <time>");
        assert_eq!(names("radix he"), ["hex"]);
        assert_eq!(names("signal op "), ["= ", "off "]);
        assert_eq!(names("signal op of"), ["off "]);
        assert_eq!(names("cursor 2 na"), ["name="]);
        let descs = |line: &str| complete(line, &app).items.into_iter().map(|i| i.desc);
        assert!(descs("signal op ").eq(["define it", "remove it"]));
        assert!(descs("records bus wh").eq(["only the records it holds for"]));
        // the interface found in the trace, with its whole line, before the protocol alone
        let c = complete("decode bus axil", &app);
        assert_eq!(c.what, "protocol");
        assert!(
            c.items[0].text.starts_with("axilite clock="),
            "{}",
            c.items[0].text
        );
        assert!(
            complete("decode bus ax", &app).items[0]
                .text
                .starts_with("axilite clock=")
        );
        assert!(
            c.items[0].doc.starts_with("This trace: clock="),
            "{}",
            c.items[0].doc
        );
        assert_eq!(c.take("decode bus axil", 1).unwrap(), "decode bus axilite ");
        assert_eq!(
            c.ghost("decode bus axil", 1),
            "ite clock=<clk> prefix=<prefix>"
        );
        assert_eq!(
            complete("decode bus axilite awv", &app).items[0].text,
            "awvalid="
        );
        let c = complete("decode bus axilite awvalid=awv", &app);
        assert_eq!(
            (c.what, c.start),
            ("signal", "decode bus axilite awvalid=".len())
        );
        // :protocol: its keys, a key's signal, and the signals of when='s expression
        assert_eq!(names("protocol cmd wh"), ["when="]);
        // its expressions name its roles, never signals
        let c = complete(
            "protocol cmd roles=valid,ready,data when=valid && !re",
            &app,
        );
        assert_eq!(
            (
                c.what,
                c.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>()
            ),
            ("role", vec!["!ready "])
        );
        assert_eq!(
            names("protocol cmd roles=valid,data when=valid op=da"),
            ["data "]
        );
        assert!(
            complete("protocol cmd roles=a when=a==b", &app)
                .items
                .iter()
                .all(|i| !i.text.starts_with("when"))
        );
        assert_eq!(names("scope to"), ["top"]);
        assert_eq!(
            names("cursor "),
            ["1 ", "2 ", "3 ", "4 ", "5 ", "6 ", "7 ", "8 ", "9 "]
        );
        assert_eq!(names("cursor 2 n"), ["name=", "note="]);
        assert_eq!(names("pick rec"), ["records "]);
        assert_eq!(names("follow "), ["off"]);
        // what exists once a decode has run: its streams, their fields after where, the rows
        app.run("decode bus axilite clock=aclk prefix=s_axi_")
            .unwrap();
        app.run("add bus.rd").unwrap();
        let names = |line: &str| {
            complete(line, &app)
                .items
                .into_iter()
                .map(|i| i.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("pick records bus.r")[..2], ["bus.r", "bus.rd"]);
        assert_eq!(names("records bus.rd w"), ["where "]);
        assert!(names("records bus.rd where !(add").contains(&"!(addr ".to_string()));
        assert_eq!(names("select bus.r"), ["bus.rd "]);
        // a stream for :records; :pick names one for records only; a time is the user's to write
        assert_eq!(names("records bus.r")[..2], ["bus.r", "bus.rd"]);
        assert_eq!(names("add top.acl"), ["top.aclk"]);
        assert_eq!(names("color re"), ["red", "green", "orange"]);
        assert_eq!(names("pick sig"), ["signals"]);
        assert_eq!(names("pick signals bus"), [""; 0]);
        assert_eq!(names("zoom 5"), [""; 0]);
    }

    /// `:scope` offers what the design says has something under it (spec §15).
    #[test]
    fn scope_completions_walk_the_design() {
        let app = |file: &str| {
            crate::tui::app::App::new(
                wtf_core::workspace::Workspace::open(std::path::Path::new(file)).unwrap(),
            )
        };
        let names = |app: &crate::tui::app::App, line: &str| {
            complete(line, app)
                .items
                .into_iter()
                .map(|i| i.text)
                .collect::<Vec<_>>()
        };
        let simple = app("tests/data/simple.vcd");
        assert!(names(&simple, "scope ").contains(&"top.cpu".to_string()));
        // a struct inside an array: the last part before it, not a dot before the index
        let types = app("tests/data/types-nvc.fst");
        assert!(names(&types, "scope ").contains(&"types.gen[0]".to_string()));
    }

    #[test]
    fn completes_more() {
        let ws =
            wtf_core::workspace::Workspace::open(std::path::Path::new("tests/data/axilite.vcd"))
                .unwrap();
        let mut app = crate::tui::app::App::new(ws);
        // a ghost shows only what continues the typed word
        let c = complete("zo", &app);
        assert_eq!(c.ghost("xx", 0), "");
        assert_eq!(c.ghost("zo", 99), "");
        let names = |app: &crate::tui::app::App, line: &str| {
            complete(line, app)
                .items
                .into_iter()
                .map(|i| i.text)
                .collect::<Vec<_>>()
        };
        // help topics, the trace's prefixes, a protocol's roles
        assert!(names(&app, "help de").contains(&"decode".to_string()));
        assert!(complete("help ", &app).what == "topic");
        let p = names(&app, "decode bus axilite prefix=");
        assert!(!p.is_empty() && p[0].ends_with(' '), "{p:?}");
        assert!(names(&app, "decode bus nosuch ").is_empty());
        assert!(names(&app, "zoom 5 6 7").is_empty());
        // what a protocol adds: its clock and prefix, only a prefix where it has no clock, nothing for a stream function
        let then = |app: &crate::tui::app::App, p: &str| {
            complete(&format!("decode x {p}"), app)
                .items
                .iter()
                .find(|i| i.label == p)
                .map(|i| i.then.clone())
        };
        assert_eq!(
            then(&app, "axilite").unwrap(),
            "clock=<clk> prefix=<prefix>"
        );
        assert_eq!(then(&app, "spi").unwrap(), "prefix=<prefix>");
        assert_eq!(then(&app, "pair").unwrap(), "");
        app.run("protocol cmd roles=valid,data when=valid").unwrap();
        assert_eq!(names(&app, "decode x cmd da"), ["data="]);
        let c = complete("decode x cm", &app);
        assert_eq!(
            (c.items[0].label.as_str(), c.items[0].desc.as_str()),
            ("cmd", "in this session")
        );
        // rows: signals and groups; marks with their times once put
        app.run("add top.aclk").unwrap();
        app.run("group g").unwrap();
        let rows = names(&app, "select ");
        assert!(rows.iter().any(|r| r.starts_with("g ")), "{rows:?}");
        assert!(rows.iter().any(|r| r.contains("aclk")), "{rows:?}");
        assert!(complete("cursor 2", &app).items[0].desc == "not put");
        app.run("cursor 2 40ns").unwrap();
        let c = complete("cursor 2", &app);
        assert!(c.items[0].desc.contains("40"), "{:?}", c.items[0].desc);
        assert_eq!(names(&app, "fit '"), ["'2 "]);
        // :w lists session files and directories to go into
        assert!(
            names(&app, "w tests/data/simple.s")
                .contains(&"tests/data/simple.session.toml".to_string())
        );
        assert!(names(&app, "w tests/d").contains(&"tests/data/".to_string()));
        assert!(names(&app, "w nosuchdir/x").is_empty());
        assert!(names(&app, "w ").iter().all(|f| !f.starts_with('.')));
    }

    /// The window's own commands are listed with the core's, once each, and the one it refuses
    /// is not (spec §7).
    /// What `:<C-Space>` shows still to type after each command: its usage after `:name`, as
    /// the listing shows it (spec §4 "Command line").
    #[test]
    fn hints_follow_the_usage() {
        let hints: Vec<String> = listed()
            .iter()
            .map(|c| {
                let it = commands(c.name, c.name.len(), true).items;
                let it = it.iter().find(|i| i.text.trim_end() == c.name).unwrap();
                format!("{} {}", c.name, it.then).trim_end().to_string()
            })
            .collect();
        assert_eq!(
            hints,
            [
                "add <path|pattern>...",
                "e",
                "decode <name> <protocol> <role>=<signal>...",
                "signal <name> = <signal> | off",
                "zoom <time>",
                "fit [<from> <to>]",
                "scope [path]",
                "radix <name>",
                "color <color|off>",
                "group <name>",
                "protocol <name> roles=<a,b> when=<expr> <field>=<expr>... | :protocol <name> wasm=<file>",
                "names <signal> <value>=<name>...",
                "rename <name>",
                "ungroup",
                "select <row> [<last row>]",
                "cursor <1-9> [time|off] [name=<text>] [note=<text>]",
                "undo",
                "redo",
                "rm [path|pattern]...",
                "records <streams> [where <expr>]",
                "decodes",
                "pick <source>",
                "messages",
                "trail",
                "agent",
                "follow [<id>|off]",
                "w [file]",
                "wq",
                "help [topic]",
                "q",
                "q!",
            ]
        );
    }

    #[test]
    fn window_rows_are_listed() {
        let names: Vec<&str> = listed().iter().map(|c| c.name).collect();
        for own in [
            "w", "wq", "q", "q!", "help", "records", "pick", "follow", "trail", "agent", "decodes",
            "messages",
        ] {
            assert_eq!(names.iter().filter(|n| **n == own).count(), 1, "{own} once");
        }
        assert!(!names.contains(&"unit"), "unit is refused by the window");
        assert!(entry("unit").is_none());
        assert_eq!(entry("add").unwrap().usage, ":add <path|pattern>...");
        // a usage that does not start `usage: ` is shown as the form the command takes
        assert_eq!(entry("zoom").unwrap().usage, ":zoom <time>");
        assert_eq!(entry("undo").unwrap().usage, ":undo");
    }

    /// The window answers its own commands and the core's that it opens a form or a card for;
    /// for everything else the core answers (spec §7).
    #[test]
    fn window_wins_where_it_answers() {
        assert_eq!(window_for("w", "").map(|w| w.name), Some("w"));
        assert_eq!(window_for("q", "!").map(|w| w.name), Some("q"));
        assert_eq!(window_for("q!", "").map(|w| w.name), Some("q!"));
        assert_eq!(window_for("decode", "").map(|w| w.name), Some("decode"));
        assert_eq!(
            window_for("decode", "bus axilite clock=aclk").map(|w| w.name),
            None
        );
        assert_eq!(
            window_for("protocol", "search").map(|w| w.name),
            Some("protocol")
        );
        assert_eq!(window_for("protocol", "cmd roles=a").map(|w| w.name), None);
        assert_eq!(window_for("unit", "ns").map(|w| w.name), Some("unit"));
        assert_eq!(window_for("undo", "").map(|w| w.name), None);
        let refused = window_for("unit", "ns").unwrap();
        assert!(matches!(refused.when, When::Refuse(_)), "unit is refused");
        assert!(refused.desc.is_empty() && refused.usage.is_empty());
    }

    #[test]
    fn icons() {
        use crate::draw::icon as ic;
        let it = |desc: &str| Item {
            desc: desc.into(),
            ..item("x", "")
        };
        let found = Item {
            found_here: true,
            ..item("x", "")
        };
        assert_eq!(item_icon(Some(ArgKind::Protocol), &found), ic::OK);
        assert_eq!(item_icon(Some(ArgKind::Protocol), &it("")), ic::PLUG);
        // on the command list, the command's own glyph
        assert_eq!(item_icon(None, &item("zoom ", "")), ic::ZOOM);
        for (kind, c) in [
            (ArgKind::Signal, ic::WIRE),
            (ArgKind::Role, ic::WIRE),
            (ArgKind::Stream, ic::STREAM),
            (ArgKind::FilePath, ic::FILE),
            (ArgKind::Scope, ic::FOLDER),
            (ArgKind::Row, ic::FOLDER),
            (ArgKind::HelpTopic, ic::CMD),
            (ArgKind::CursorIndex, ic::MARK),
            (ArgKind::MarkRef, ic::MARK),
            (ArgKind::Agent, ic::AGENT),
        ] {
            assert_eq!(item_icon(Some(kind), &it("")), c, "{kind:?}");
        }
        assert_eq!(icon(":pick interfaces"), ic::PLUG);
        assert_eq!(icon("pick signals"), ic::WIRE);
        assert_eq!(icon("pick nothing-known"), ic::SEARCH);
    }

    /// The core table's glyphs are the window's `draw::icon` ones: one set of glyphs for both
    /// lists (spec §7, §14 `nerd`).
    #[test]
    fn core_icons_match_draw() {
        use crate::draw::icon as ic;
        for (name, glyph) in [
            ("add", ic::WIRE),
            ("e", ic::FILE),
            ("decode", ic::PLUG),
            ("signal", ic::WIRE),
            ("zoom", ic::ZOOM),
            ("fit", ic::ZOOM),
            ("scope", ic::EDIT),
            ("unit", ic::EDIT),
            ("radix", ic::EDIT),
            ("color", ic::EDIT),
            ("group", ic::EDIT),
            ("protocol", ic::PLUG),
            ("names", ic::PLUG),
            ("rename", ic::EDIT),
            ("ungroup", ic::EDIT),
            ("select", ic::EDIT),
            ("cursor", ic::MARK),
            ("undo", ic::HISTORY),
            ("redo", ic::HISTORY),
            ("rm", ic::EDIT),
        ] {
            assert_eq!(
                wtf_core::command::lookup(name).expect(name).icon,
                glyph,
                "{name}"
            );
        }
        // a listed window row carries one too; a guard lists nothing and carries none
        for w in WINDOW {
            assert_eq!(w.icon == '\0', w.usage.is_empty(), "{}", w.name);
        }
    }
}
