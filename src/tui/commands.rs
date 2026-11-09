//! The `:` commands as the command line lists and completes them (spec §4 "Command line", §7):
//! the core's, and the window's own (`:w`, `:q`, `:help`).

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};

pub struct Command {
    pub name: &'static str,
    pub usage: &'static str,
    pub desc: &'static str,
    pub args: bool,
}

const TIME: &str = "<time>";

pub const COMMANDS: &[Command] = &[
    Command { name: TIME, usage: ":<time>", desc: "Move the cursor to a time, e.g. :2000ns", args: false },
    Command { name: "zoom", usage: ":zoom <time>", desc: "Set the time per column, e.g. :zoom 1ns", args: true },
    Command { name: "add", usage: ":add <path|pattern>...", desc: "Add signals below the selected row (after a selected group); a scope becomes a group, e.g. :add top.cpu.*", args: true },
    Command { name: "rm", usage: ":rm [path|pattern]...", desc: "Remove matching rows, or the selected row; :rm ** removes all", args: true },
    Command { name: "signal", usage: ":signal <name> = <signal>", desc: "Name a computed signal, e.g. :signal op = cmd[7:0] or :signal hdr = {a, b}", args: true },
    Command { name: "decode", usage: ":decode <name> <protocol> <role>=<signal>...", desc: "Decode a protocol into streams, e.g. :decode rx axis clock=clk tvalid=rx_tvalid tready=rx_tready tdata=rx_tdata tlast=rx_tlast; alone, a form that fills it in", args: true },
    Command { name: "decodes", usage: ":decodes", desc: "List the decodes made: Enter opens records, a adds rows, e edits, dd removes", args: false },
    Command { name: "protocol", usage: ":protocol <name> roles=<a,b> when=<expr> <field>=<expr>...", desc: "Define a protocol on its roles, a record per cycle where when holds, e.g. :protocol cmd roles=valid,ready,data when=valid && ready op=data[7:0]; :decode puts it on signals; alone, a form with the selected rows as roles; :protocol search, one filled in from the last clocked search", args: true },
    Command { name: "names", usage: ":names <signal> <value>=<name>...", desc: "Name a signal's values, shown by the enum radix and searched by name, e.g. :names state 0=IDLE 1=BUSY; off removes them", args: true },
    Command { name: "records", usage: ":records <streams> [where <expr>]", desc: "Browse records as a table: a stream, a decode's results (:records bus), a glob; / filters, s sorts, = groups", args: true },
    Command { name: "scope", usage: ":scope [path]", desc: "Resolve names under a scope first, e.g. :scope top.cpu makes state top.cpu.state; none without a path", args: true },
    Command { name: "radix", usage: ":radix <name>", desc: "Set the radix: hex bin dec sdec oct ascii float enum, q4.12 / uq4.12 fixed point", args: true },
    Command { name: "color", usage: ":color <name|off>", desc: "Colour the selected row or group: red orange yellow green cyan blue magenta gray; off for the theme's", args: true },
    Command { name: "group", usage: ":group <name>", desc: "Group the selected rows (V selects them)", args: true },
    Command { name: "rename", usage: ":rename <name>", desc: "Rename the group the selected row is in", args: true },
    Command { name: "ungroup", usage: ":ungroup", desc: "Dissolve the group the selected row is in, keeping its signals", args: false },
    Command { name: "select", usage: ":select <row> [<last row>]", desc: "Select a signal or group by name, or the rows from one to another", args: true },
    Command { name: "pick", usage: ":pick <source>", desc: "A fuzzy list with a preview: signals, marks, records <stream>, commands, sessions, trail, interfaces; Ctrl-s goes to the next", args: true },
    Command { name: "messages", usage: ":messages", desc: "Every message so far, with its time and who caused it: you or an agent", args: false },
    Command { name: "trail", usage: ":trail", desc: "The agent's steps; Enter jumps to a step's time", args: false },
    Command { name: "cursor", usage: ":cursor <n> [time|off] [name=] [note=]", desc: "Put mark n at a time (1: the cursor), name it, remove it, or jump to it, e.g. :cursor 2 120ns name=bad write", args: true },
    Command { name: "undo", usage: ":undo", desc: "Undo the last change to the rows", args: false },
    Command { name: "redo", usage: ":redo", desc: "Redo a change undone with :undo", args: false },
    Command { name: "fit", usage: ":fit [<from> <to>]", desc: "Zoom to the whole simulation, or between cursors or times, e.g. :fit '2 '3 or :fit 10ns 20ns", args: true },
    Command { name: "agent", usage: ":agent", desc: "Show the session an agent left for this trace when no window was open", args: false },
    Command { name: "follow", usage: ":follow [<id>|off]", desc: "Follow an agent working on this trace: show its workspace and each change it makes", args: true },
    Command { name: "e", usage: ":e", desc: "Reload the trace, keeping signals, groups, cursor and zoom", args: false },
    Command { name: "w", usage: ":w [file]", desc: "Save the session", args: true },
    Command { name: "wq", usage: ":wq", desc: "Save the session and quit", args: false },
    Command { name: "help", usage: ":help [topic]", desc: "Show the keys of the current mode (also ?), or a topic: names times radix expressions decode records workspace commands", args: true },
    Command { name: "q", usage: ":q", desc: "Quit; refuses if the session has unsaved changes", args: false },
    Command { name: "q!", usage: ":q!", desc: "Quit without saving", args: false },
];

/// What `Tab` can put on the command line (spec §4 "Command line"): the command being typed, or
/// an argument of the one typed.
pub struct Completion {
    /// What is listed: `command`, `protocol`, `signal`, ...
    pub what: &'static str,
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
}

impl Completion {
    /// What the highlighted item would add after what is typed, if it starts with it.
    pub fn ghost(&self, line: &str, hl: usize) -> String {
        let Some(it) = self.items.get(hl) else { return String::new() };
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
    Pattern::parse(word, CaseMatching::Ignore, Normalization::Smart).match_list(names, &mut Matcher::new(Config::DEFAULT)).into_iter().map(|(n, _)| n).collect()
}

fn item(text: impl Into<String>, desc: impl Into<String>) -> Item {
    let text = text.into();
    Item { label: text.trim_end().to_string(), text, desc: desc.into(), then: String::new(), doc: String::new() }
}

/// What the line completes to: while the first word is typed (or isn't a command) the commands,
/// then the typed command's arguments.
pub fn complete(line: &str, app: &crate::tui::app::App) -> Completion {
    let first = line.split_whitespace().next().unwrap_or("");
    let lead = line.len() - line.trim_start().len();
    let in_first = !line[lead..].contains(char::is_whitespace);
    let cmd = COMMANDS.iter().find(|c| c.name == first && c.args);
    match cmd {
        Some(c) if !in_first => arguments(c, line, app),
        _ => commands(line, lead + first.len(), in_first),
    }
}

/// The commands matching the first word.
fn commands(line: &str, end: usize, alone: bool) -> Completion {
    let word = &line[..end];
    let found: Vec<&Command> = if word.trim().starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        vec![&COMMANDS[0]]
    } else {
        let names = fuzzy(word.trim(), COMMANDS.iter().map(|c| c.name).filter(|n| *n != TIME));
        let mut v: Vec<&Command> = names.iter().filter_map(|n| COMMANDS.iter().find(|c| c.name == *n)).collect();
        if word.trim().is_empty() {
            v.insert(0, &COMMANDS[0]);
        }
        v
    };
    let items = found
        .into_iter()
        .map(|c| {
            // the arguments still to type, from the usage: `:zoom <time>` -> ` <time>`
            let then = if alone { c.usage.trim_start_matches(':').strip_prefix(c.name).unwrap_or("").trim_start().to_string() } else { String::new() };
            let text = if alone && c.args { format!("{} ", c.name) } else { c.name.to_string() };
            Item { text, label: c.usage.into(), desc: c.desc.into(), then, doc: String::new() }
        })
        .collect();
    Completion { what: "command", start: 0, items }
}

/// The typed command's argument being typed: the word at the end of the line.
fn arguments(c: &Command, line: &str, app: &crate::tui::app::App) -> Completion {
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
    let (what, items): (&'static str, Vec<Item>) = match (c.name, n) {
        ("add" | "rm", _) => ("signal", signals(word)),
        ("radix", 1) => ("radix", fuzzy(word, wtf_core::radix::RADIXES.iter().map(|r| r.name)).into_iter().map(|r| item(r, "")).collect()),
        ("color", 1) => ("colour", fuzzy(word, wtf_core::rows::COLORS.iter().copied().chain(["off"])).into_iter().map(|r| item(r, "")).collect()),
        // a decode's name first: its results
        ("records", 1) => ("stream", fuzzy(word, ws.decodes.iter().map(|d| d.name.as_str()).chain(ws.streams.iter().map(|s| s.name.as_str()))).into_iter().map(|s| item(s, "")).collect()),
        ("help", 1) => {
            let topics = crate::mcp::help::TOPICS;
            ("topic", fuzzy(word, topics.iter().map(|t| t.0)).into_iter().filter_map(|t| topics.iter().find(|x| x.0 == t)).map(|t| item(t.0, t.1)).collect())
        }
        ("decode", 2) => ("protocol", protocols(word, ws)),
        ("names", 1) | ("signal", 3..) => ("signal", signals(word)),
        ("records", 2) => ("", fuzzy(word, ["where"]).into_iter().map(|w| item(format!("{w} "), "only the records it holds for")).collect()),
        ("records", 3..) => {
            // where's expression: the stream's fields
            let st = line.split_whitespace().nth(1).and_then(|n| ws.streams.iter().find(|s| s.name == n));
            let skip = word.len() - word.trim_start_matches(['!', '(', '~']).len();
            let fields: Vec<&str> = st.map(|s| s.schema.iter().map(|f| f.name.as_str()).collect()).unwrap_or_default();
            let items = fuzzy(&word[skip..], fields).into_iter().map(|f| item(format!("{}{f} ", &word[..skip]), "")).collect();
            return Completion { what: "field", start, items };
        }
        ("scope", 1) => {
            let mut scopes: Vec<String> = ws.trace.vars.iter().filter(|v| !v.path.is_empty()).flat_map(|v| {
                let parts = wtf_core::trace::parts(&v.path);
                (1..parts.len()).map(move |k| parts[..k].join(".")).collect::<Vec<_>>()
            }).collect();
            scopes.sort();
            scopes.dedup();
            ("scope", fuzzy(word, scopes.iter().map(|s| s.as_str())).into_iter().take(200).map(|s| item(s, "")).collect())
        }
        ("select", 1 | 2) => {
            let rows: Vec<String> = ws.rows.iter().map(|r| match r {
                wtf_core::rows::Row::Signal(s) => ws.trace.vars[s.var].path.clone(),
                wtf_core::rows::Row::Group { name, .. } => name.clone(),
            }).collect();
            ("row", fuzzy(word, rows.iter().map(|r| r.as_str())).into_iter().map(|r| item(format!("{r} "), "")).collect())
        }
        ("cursor", 1) => {
            let marks: Vec<Item> = (0..9).map(|i| {
                let what = match (i, ws.marks[i]) {
                    (0, _) => "the cursor".to_string(),
                    (_, None) => "not put".into(),
                    (_, Some(t)) => format!("{} {}", wtf_core::time::format(t, ws.trace.fs_per_tick), ws.labels[i].name),
                };
                item(format!("{} ", i + 1), what)
            }).collect();
            ("mark", marks.into_iter().filter(|m| m.text.starts_with(word)).collect())
        }
        ("cursor", 2..) => ("", fuzzy(word, ["off", "name=", "note="].into_iter().filter(|k| n == 2 || *k != "off")).into_iter().map(|k| item(k, "")).collect()),
        ("fit", 1 | 2) => {
            // the marks put, as '2
            let marks: Vec<Item> = (1..9).filter_map(|i| ws.marks[i].map(|t| item(format!("'{} ", i + 1), format!("{} {}", wtf_core::time::format(t, ws.trace.fs_per_tick), ws.labels[i].name)))).collect();
            ("mark", marks.into_iter().filter(|m| m.text.starts_with(word)).collect())
        }
        ("follow", 1) => ("agent", fuzzy(word, app.agents.iter().map(|a| a.0.as_str()).chain(["off"])).into_iter().map(|a| item(a, "")).collect()),
        ("pick", 1) => ("source", fuzzy(word, crate::tui::picker::SOURCES).into_iter().map(|s| item(if s == "records" { format!("{s} ") } else { s.to_string() }, "")).collect()),
        ("pick", 2) if line.split_whitespace().nth(1) == Some("records") => ("stream", fuzzy(word, ws.streams.iter().map(|s| s.name.as_str())).into_iter().map(|s| item(s, "")).collect()),
        ("w", 1) => ("file", files(word)),
        ("protocol", 2..) => {
            // a definition names only its roles: in when= and the fields, the roles given so far
            let roles: Vec<String> = line.split_whitespace().find_map(|w| w.strip_prefix("roles=")).map_or(Vec::new(), |r| r.split(',').filter(|r| !r.is_empty()).map(String::from).collect());
            let in_expr = line[..at].split_whitespace().rev().find(|w| key(w).is_some()).is_some_and(|w| !w.starts_with("roles="));
            let (k, v, from) = match key(word) {
                Some(k) => (Some(k), &word[k.len() + 1..], start + k.chars().count() + 1),
                None => (None, word, start),
            };
            match (k, in_expr) {
                (Some("roles"), _) => ("", Vec::new()),
                (Some(_), _) | (None, true) => {
                    let skip = v.len() - v.trim_start_matches(['!', '(', '~']).len();
                    let items = fuzzy(&v[skip..], roles.iter().map(String::as_str)).into_iter().map(|r| item(format!("{}{r} ", &v[..skip]), "role")).collect();
                    return Completion { what: "role", start: from, items };
                }
                (None, false) => ("key", fuzzy(word, ["roles=", "when="]).into_iter().map(|k| item(k, "")).collect()),
            }
        }
        ("decode", _) => {
            let proto = line.split_whitespace().nth(2).unwrap_or("");
            match word.split_once('=') {
                // a role's signal, from its start
                Some((role, v)) => {
                    let at = start + role.chars().count() + 1;
                    let items = if role == "prefix" {
                        let found = protocol::detect(&ws.trace);
                        fuzzy(v, found.iter().filter(|f| f.protocol == proto).map(|f| f.prefix.as_str())).into_iter().map(|p| item(format!("{p} "), "")).collect()
                    } else {
                        signals(v).into_iter().map(|mut it| { it.text.push(' '); it }).collect()
                    };
                    return Completion { what: if role == "prefix" { "prefix" } else { "signal" }, start: at, items };
                }
                None => {
                    let roles: Vec<String> = match protocol::builtin(proto) {
                        Some(b) => ["clock", "prefix"].iter().copied().filter(|r| *r != "clock" || b.clocked).chain(b.required.iter().copied()).chain(b.optional.iter().copied()).chain(b.params.iter().copied()).map(String::from).collect(),
                        None => ws.protocols.iter().find(|p| p.name == proto).map(|p| p.roles.clone()).unwrap_or_default(),
                    };
                    ("role", fuzzy(word, roles.iter().map(|r| r.as_str())).into_iter().map(|r| item(format!("{r}="), "")).collect())
                }
            }
        }
        _ => ("", Vec::new()),
    };
    Completion { what, start, items }
}

/// The icon before a completion (spec §14 `nerd`): a command's own, a protocol's plug, an
/// interface found in the trace a check.
pub fn item_icon(what: &str, it: &Item) -> char {
    use crate::draw::icon as ic;
    match what {
        "command" => self::icon(&it.text),
        "protocol" if it.desc.starts_with("found here") => ic::OK,
        "protocol" => ic::PLUG,
        "signal" | "role" => ic::WIRE,
        "stream" => ic::STREAM,
        "mark" => ic::MARK,
        "agent" => ic::AGENT,
        "file" => ic::FILE,
        "scope" | "row" => ic::FOLDER,
        _ => ic::CMD,
    }
}

/// Files and directories starting with what is typed, for `:w`: the session files (`.toml`) and
/// directories to go into.
fn files(word: &str) -> Vec<Item> {
    let (dir, base) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let Ok(rd) = std::fs::read_dir(if dir.is_empty() { "." } else { dir }) else { return Vec::new() };
    let mut out: Vec<Item> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            (name.starts_with(base) && !name.starts_with('.') && (is_dir || name.ends_with(".toml"))).then(|| item(format!("{dir}{name}{}", if is_dir { "/" } else { "" }), ""))
        })
        .collect();
    out.sort_by(|a, b| a.text.cmp(&b.text));
    out
}

/// The key of a `key=value` argument; none for a comparison (`a==b`, `a<=3`).
fn key(word: &str) -> Option<&str> {
    let (k, rest) = word.split_once('=')?;
    let ok = !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_') && !rest.starts_with('=');
    ok.then_some(k)
}

/// `:decode <name> <protocol>`: the interfaces found in the trace first, each with its whole
/// line, then every protocol with what it makes.
fn protocols(word: &str, ws: &wtf_core::workspace::Workspace) -> Vec<Item> {
    use wtf_core::protocol;
    let about = |n: &str| -> String {
        protocol::builtin(n).map(|b| b.about.to_string()).or_else(|| protocol::ON_STREAMS.iter().find(|g| g.0 == n).map(|g| g.1.to_string())).unwrap_or_else(|| "in this session".into())
    };
    let found = protocol::detect(&ws.trace);
    let names: Vec<&str> = protocol::BUILTINS.iter().copied().chain(protocol::ON_STREAMS.iter().map(|g| g.0)).chain(ws.protocols.iter().map(|p| p.name.as_str())).collect();
    let (mut items, mut rest) = (Vec::new(), Vec::new());
    for n in fuzzy(word, names) {
        let doc = protocol::brief(n, &ws.protocols).unwrap_or_default() + "\n\nTab takes it.  :help decode has the rest.";
        for f in found.iter().filter(|f| f.protocol == n) {
            // the line `decode_line` gives, from the protocol on
            let line = f.decode_line();
            let args = line.splitn(3, ' ').nth(2).unwrap_or("").to_string();
            let rest = args.strip_prefix(n).unwrap_or(&args).trim();
            items.push(Item { label: format!("{n}  {}", f.prefix), desc: format!("found here: {rest}"), text: format!("{args} "), then: String::new(), doc: format!("This trace: {rest}\n\n{doc}") });
        }
        let then = match protocol::builtin(n) {
            Some(b) if b.clocked => "clock=<clk> prefix=<prefix>",
            Some(_) => "prefix=<prefix>",
            None => "",
        };
        rest.push(Item { text: format!("{n} "), label: n.into(), desc: about(n), then: then.into(), doc });
    }
    // the trace's own interfaces first, each protocol after them in the order matched
    items.extend(rest);
    items
}

/// A `:` command's icon, for the leader menu: by its name (spec §14 `nerd`).
pub fn icon(line: &str) -> char {
    use crate::draw::icon;
    let mut words = line.trim_start_matches(':').split_whitespace();
    match (words.next().unwrap_or(""), words.next().unwrap_or("")) {
        ("pick", "signals") | ("add", _) | ("signal", _) => icon::WIRE,
        ("pick", "marks") | ("cursor", _) => icon::MARK,
        ("pick", "records") | ("records", _) => icon::RECORD,
        ("pick", "commands") => icon::CMD,
        ("pick", "trail") | ("trail", _) => icon::TRAIL,
        ("pick", "sessions") | ("follow", _) => icon::EYE,
        ("pick", "interfaces") => icon::PLUG,
        ("pick", _) => icon::SEARCH,
        ("agent", _) => icon::AGENT,
        ("decode", _) | ("protocol", _) | ("names", _) => icon::PLUG,
        ("undo", _) | ("redo", _) => icon::HISTORY,
        ("help", _) => icon::KEYS,
        ("messages", _) => icon::BELL,
        ("w", _) | ("wq", _) => icon::SAVE,
        ("e", _) => icon::FILE,
        ("q", _) | ("q!", _) => icon::ERROR,
        ("fit", _) | ("zoom", _) => icon::ZOOM,
        _ => icon::EDIT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes() {
        let ws = wtf_core::workspace::Workspace::open(std::path::Path::new("tests/data/axilite.vcd")).unwrap();
        let mut app = crate::tui::app::App::new(ws);
        let names = |line: &str| complete(line, &app).items.into_iter().map(|i| i.text).collect::<Vec<_>>();
        assert_eq!(names("").len(), COMMANDS.len());
        assert_eq!(names("20")[0], TIME);
        assert_eq!(names("zo")[0], "zoom ");
        assert_eq!(names("ad top")[0], "add");
        let c = complete("zo", &app);
        assert_eq!(c.ghost("zo", 0), "om <time>");
        assert_eq!(names("radix he"), ["hex"]);
        // the interface found in the trace, with its whole line, before the protocol alone
        let c = complete("decode bus axil", &app);
        assert_eq!(c.what, "protocol");
        assert!(c.items[0].text.starts_with("axilite clock="), "{}", c.items[0].text);
        assert!(complete("decode bus ax", &app).items[0].text.starts_with("axilite clock="));
        assert!(c.items[0].doc.starts_with("This trace: clock="), "{}", c.items[0].doc);
        assert_eq!(c.take("decode bus axil", 1).unwrap(), "decode bus axilite ");
        assert_eq!(c.ghost("decode bus axil", 1), "ite clock=<clk> prefix=<prefix>");
        assert_eq!(complete("decode bus axilite awv", &app).items[0].text, "awvalid=");
        let c = complete("decode bus axilite awvalid=awv", &app);
        assert_eq!((c.what, c.start), ("signal", "decode bus axilite awvalid=".len()));
        // :protocol: its keys, a key's signal, and the signals of when='s expression
        assert_eq!(names("protocol cmd wh"), ["when="]);
        // its expressions name its roles, never signals
        let c = complete("protocol cmd roles=valid,ready,data when=valid && !re", &app);
        assert_eq!((c.what, c.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>()), ("role", vec!["!ready "]));
        assert_eq!(names("protocol cmd roles=valid,data when=valid op=da"), ["data "]);
        assert!(complete("protocol cmd roles=a when=a==b", &app).items.iter().all(|i| !i.text.starts_with("when")));
        assert_eq!(names("scope to"), ["top"]);
        assert_eq!(names("cursor "), ["1 ", "2 ", "3 ", "4 ", "5 ", "6 ", "7 ", "8 ", "9 "]);
        assert_eq!(names("cursor 2 n"), ["name=", "note="]);
        assert_eq!(names("pick rec"), ["records "]);
        assert_eq!(names("follow "), ["off"]);
        // what exists once a decode has run: its streams, their fields after where, the rows
        app.run("decode bus axilite clock=aclk prefix=s_axi_").unwrap();
        app.run("add bus.rd").unwrap();
        let names = |line: &str| complete(line, &app).items.into_iter().map(|i| i.text).collect::<Vec<_>>();
        assert_eq!(names("pick records bus.r")[..2], ["bus.r", "bus.rd"]);
        assert_eq!(names("records bus.rd w"), ["where "]);
        assert!(names("records bus.rd where !(add").contains(&"!(addr ".to_string()));
        assert_eq!(names("select bus.r"), ["bus.rd "]);
    }

    #[test]
    fn completes_more() {
        let ws = wtf_core::workspace::Workspace::open(std::path::Path::new("tests/data/axilite.vcd")).unwrap();
        let mut app = crate::tui::app::App::new(ws);
        // a ghost shows only what continues the typed word
        let c = complete("zo", &app);
        assert_eq!(c.ghost("xx", 0), "");
        assert_eq!(c.ghost("zo", 99), "");
        let names = |app: &crate::tui::app::App, line: &str| complete(line, app).items.into_iter().map(|i| i.text).collect::<Vec<_>>();
        // help topics, the trace's prefixes, a protocol's roles
        assert!(names(&app, "help de").contains(&"decode".to_string()));
        assert!(complete("help ", &app).what == "topic");
        let p = names(&app, "decode bus axilite prefix=");
        assert!(!p.is_empty() && p[0].ends_with(' '), "{p:?}");
        assert!(names(&app, "decode bus nosuch ").is_empty());
        assert!(names(&app, "zoom 5 6 7").is_empty());
        // what a protocol adds: its clock and prefix, only a prefix where it has no clock, nothing for a stream function
        let then = |app: &crate::tui::app::App, p: &str| complete(&format!("decode x {p}"), app).items.iter().find(|i| i.label == p).map(|i| i.then.clone());
        assert_eq!(then(&app, "axilite").unwrap(), "clock=<clk> prefix=<prefix>");
        assert_eq!(then(&app, "spi").unwrap(), "prefix=<prefix>");
        assert_eq!(then(&app, "pair").unwrap(), "");
        app.run("protocol cmd roles=valid,data when=valid").unwrap();
        assert_eq!(names(&app, "decode x cmd da"), ["data="]);
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
        assert!(names(&app, "w tests/data/simple.s").contains(&"tests/data/simple.session.toml".to_string()));
        assert!(names(&app, "w tests/d").contains(&"tests/data/".to_string()));
        assert!(names(&app, "w nosuchdir/x").is_empty());
        assert!(names(&app, "w ").iter().all(|f| !f.starts_with('.')));
    }

    #[test]
    fn icons() {
        use crate::draw::icon as ic;
        let it = |desc: &str| Item { desc: desc.into(), ..item("x", "") };
        assert_eq!(item_icon("protocol", &it("found here: x")), ic::OK);
        assert_eq!(item_icon("protocol", &it("")), ic::PLUG);
        for (what, c) in [("signal", ic::WIRE), ("role", ic::WIRE), ("stream", ic::STREAM), ("file", ic::FILE), ("scope", ic::FOLDER), ("row", ic::FOLDER), ("topic", ic::CMD)] {
            assert_eq!(item_icon(what, &it("")), c, "{what}");
        }
        assert_eq!(icon(":pick interfaces"), ic::PLUG);
        assert_eq!(icon("pick signals"), ic::WIRE);
        assert_eq!(icon("pick nothing-known"), ic::SEARCH);
    }
}
