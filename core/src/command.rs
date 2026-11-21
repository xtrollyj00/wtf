//! `:` commands (spec §7), run on a workspace.

use crate::pattern::{Pattern, Syntax};
use crate::radix;
use crate::rows::{self, Row};
use crate::session;
use crate::workspace::{Msg, Workspace};

/// What a command's argument is, so a listing can say what to type there (`<time>`) and a
/// completion can offer the values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgKind {
    Signal,
    Scope,
    Stream,
    Protocol,
    Role,
    Field,
    Radix,
    Color,
    HelpTopic,
    Agent,
    PickSource,
    FilePath,
    Row,
    CursorIndex,
    MarkRef,
    Time,
    Text,
    KeyVal,
    /// The values to pick from, `|` between them: `off|name=|note=`.
    Lit(&'static str),
}

/// One `:` command: its name, its usage line, a description for the listings, what its arguments
/// are and its handler. `run` looks the name up here instead of matching inline, so this table is
/// the only place that names what the core runs (spec §7); the window's own commands (`:w`, `:q`,
/// `:help`) are its `WINDOW` (`src/tui/commands.rs`), the only other list. A listing shows `desc`
/// and the usage line without `usage: `; handlers report their own entry's `usage`, except `add`
/// and `color`, whose usage is built at runtime (pattern syntax, color list); entries with no
/// usage error (`e`, `scope`, `names`, `ungroup`, `undo`, `redo`, `rm`) carry it for the listings.
pub struct Command {
    pub name: &'static str,
    pub usage: &'static str,
    pub desc: &'static str,
    /// The arguments, in order; empty when it takes none.
    pub args: &'static [ArgKind],
    /// The last argument takes more of the same: `...` after the placeholders.
    pub trailing: bool,
    /// The glyph a completion shows before it (`src/draw.rs`, `icon`).
    pub icon: char,
    pub run: fn(&mut Workspace, &Command, &str) -> Result<Option<Msg>, String>,
}

pub const COMMANDS: &[Command] = &[
    Command {
        name: "add",
        usage: "usage: :add <path|pattern>...",
        desc: "Add signals below the selected row (after a selected group); a scope becomes a group, e.g. :add top.cpu.*",
        args: &[ArgKind::Signal],
        trailing: true,
        icon: '\u{eb31}',
        run: do_add,
    },
    Command {
        name: "e",
        usage: "usage: :e",
        desc: "Reload the trace, keeping signals, groups, cursor and zoom",
        args: &[],
        trailing: false,
        icon: '\u{ea7b}',
        run: do_reload,
    },
    Command {
        name: "decode",
        usage: "usage: :decode <name> <protocol> <role>=<signal>...",
        desc: "Decode a protocol into streams, e.g. :decode rx axis clock=clk tvalid=rx_tvalid tready=rx_tready tdata=rx_tdata tlast=rx_tlast aresetn=rst_n; alone, a form that fills it in",
        args: &[ArgKind::Text, ArgKind::Protocol, ArgKind::Role],
        trailing: true,
        icon: '\u{eb2d}',
        run: do_decode,
    },
    Command {
        name: "signal",
        usage: "usage: :signal <name> = <signal> | off",
        desc: "Name a computed signal, e.g. :signal op = cmd[7:0] or :signal hdr = {a, b}; :signal op off removes it",
        args: &[ArgKind::Text, ArgKind::Lit("=|off"), ArgKind::Signal],
        trailing: true,
        icon: '\u{eb31}',
        run: do_signal,
    },
    Command {
        name: "zoom",
        usage: "zoom takes one time, per column (`zoom 1ns`); to show a range: fit",
        desc: "Set the time per column, e.g. :zoom 1ns",
        args: &[ArgKind::Time],
        trailing: false,
        icon: '\u{eb81}',
        run: do_zoom,
    },
    Command {
        name: "fit",
        usage: "usage: :fit [<from> <to>]",
        desc: "Zoom to the whole simulation, or between cursors or times, e.g. :fit '2 '3 or :fit 10ns 20ns",
        args: &[ArgKind::MarkRef, ArgKind::MarkRef],
        trailing: false,
        icon: '\u{eb81}',
        run: do_fit,
    },
    Command {
        name: "scope",
        usage: "usage: :scope [path]",
        desc: "Resolve names under a scope first, e.g. :scope top.cpu makes state top.cpu.state; none without a path",
        args: &[ArgKind::Scope],
        trailing: false,
        icon: '\u{ea73}',
        run: do_scope,
    },
    Command {
        name: "unit",
        usage: "usage: :unit <s|ms|us|ns|ps|fs|auto>",
        desc: "Set the time unit an agent's answers are given in; the window picks the one that fits",
        args: &[ArgKind::Lit("s|ms|us|ns|ps|fs|auto")],
        trailing: false,
        icon: '\u{ea73}',
        run: do_unit,
    },
    Command {
        name: "radix",
        usage: "usage: :radix <name>",
        desc: "Set the radix: hex bin dec sdec oct ascii float enum, q4.12 / uq4.12 fixed point",
        args: &[ArgKind::Radix],
        trailing: false,
        icon: '\u{ea73}',
        run: do_radix,
    },
    Command {
        name: "color",
        usage: "usage: :color <color|off>",
        desc: "Colour the selected row or group: red orange yellow green cyan blue magenta gray; off for the theme's",
        args: &[ArgKind::Color],
        trailing: false,
        icon: '\u{ea73}',
        run: do_color,
    },
    Command {
        name: "group",
        usage: "usage: :group <name>",
        desc: "Group the selected rows (V selects them)",
        args: &[ArgKind::Text],
        trailing: false,
        icon: '\u{ea73}',
        run: do_group,
    },
    Command {
        name: "protocol",
        usage: "usage: :protocol <name> roles=<a,b> when=<expr> <field>=<expr>... | :protocol <name> wasm=<file>",
        desc: "Define a protocol on its roles, a record per cycle where when holds, e.g. :protocol cmd roles=valid,ready,data when=valid && ready op=data[7:0]; :decode puts it on signals; alone, a form with the selected rows as roles; :protocol search, one filled in from the last clocked search",
        args: &[ArgKind::Text, ArgKind::KeyVal],
        trailing: true,
        icon: '\u{eb2d}',
        run: do_protocol,
    },
    Command {
        name: "names",
        usage: "usage: :names <signal> <value>=<name>...",
        desc: "Name a signal's values, shown by the enum radix and searched by name, e.g. :names state 0=IDLE 1=BUSY; off removes them; alone, a card of the values the selected signal takes",
        args: &[ArgKind::Signal, ArgKind::Text],
        trailing: true,
        icon: '\u{eb2d}',
        run: do_names,
    },
    Command {
        name: "rename",
        usage: "usage: :rename <name>",
        desc: "Rename the group the selected row is in",
        args: &[ArgKind::Text],
        trailing: false,
        icon: '\u{ea73}',
        run: do_rename,
    },
    Command {
        name: "ungroup",
        usage: "usage: :ungroup",
        desc: "Dissolve the group the selected row is in, keeping its signals",
        args: &[],
        trailing: false,
        icon: '\u{ea73}',
        run: do_ungroup,
    },
    Command {
        name: "select",
        usage: "usage: :select <row> [<last row>]",
        desc: "Select a signal or group by name, or the rows from one to another",
        args: &[ArgKind::Row, ArgKind::Row],
        trailing: false,
        icon: '\u{ea73}',
        run: do_select,
    },
    Command {
        name: "cursor",
        usage: "usage: :cursor <1-9> [time|off] [name=<text>] [note=<text>]",
        desc: "Put mark n at a time (1: the cursor), name it, remove it, or jump to it, e.g. :cursor 2 120ns name=bad write",
        args: &[
            ArgKind::CursorIndex,
            ArgKind::Lit("off|name=|note="),
            ArgKind::Lit("name=|note="),
        ],
        trailing: true,
        icon: '\u{eaa5}',
        run: do_cursor,
    },
    Command {
        name: "undo",
        usage: "usage: :undo",
        desc: "Undo the last change to the rows",
        args: &[],
        trailing: false,
        icon: '\u{ea82}',
        run: do_undo,
    },
    Command {
        name: "redo",
        usage: "usage: :redo",
        desc: "Redo a change undone with :undo",
        args: &[],
        trailing: false,
        icon: '\u{ea82}',
        run: do_redo,
    },
    Command {
        name: "rm",
        usage: "usage: :rm [path|pattern]...",
        desc: "Remove matching rows, or the selected row; :rm ** removes all",
        args: &[ArgKind::Signal],
        trailing: true,
        icon: '\u{ea73}',
        run: do_rm,
    },
];

/// The table entry for `name`, if `run` answers it.
pub fn lookup(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// Run a `:` command on the workspace; some answer with a message (`:e`, or `:add` naming what
/// it couldn't add). The window's own commands (`:w`, `:q`, `:help`) are the terminal UI's.
pub fn run(app: &mut Workspace, line: &str) -> Result<Option<Msg>, String> {
    let line = line.trim();
    let (cmd, arg) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let arg = arg.trim();
    // the empty line does nothing; a bare time jumps there, as before
    if cmd.is_empty() {
        return Ok(None);
    }
    if cmd.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        let t = crate::time::parse(cmd, app.trace.fs_per_tick)?;
        app.jump(t.round() as u64, false);
        return Ok(None);
    }
    match lookup(cmd) {
        Some(c) => (c.run)(app, c, arg),
        None => Err(format!("unknown command: {cmd}")),
    }
}

fn do_add(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    add(app, arg)
}

fn do_reload(app: &mut Workspace, _: &Command, _arg: &str) -> Result<Option<Msg>, String> {
    app.reload().map(Some)
}

fn do_decode(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    decode(app, arg).map(|m| Some(Msg::Info(m)))
}

fn do_signal(app: &mut Workspace, cmd: &Command, arg: &str) -> Result<Option<Msg>, String> {
    // `:signal <name> off` removes it, as `:decode <name> off` does
    if word(arg).1.trim() == "off" {
        let name = word(arg).0;
        app.remove_signal(name)?;
        return Ok(Some(Msg::Info(format!("{name}: removed"))));
    }
    let (name, expr) = arg.split_once('=').ok_or(cmd.usage)?;
    app.define(name, expr).map(|_| None)
}

fn do_zoom(app: &mut Workspace, cmd: &Command, arg: &str) -> Result<Option<Msg>, String> {
    if arg.split_whitespace().count() > 1 {
        return Err(format!("{} {arg}", cmd.usage));
    }
    let t = crate::time::parse(arg, app.trace.fs_per_tick)?;
    if t <= 0.0 {
        return Err("zoom must be above zero".into());
    }
    app.view.set_zoom(t);
    Ok(None)
}

fn do_fit(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    fit(app, arg).map(|()| None)
}

fn do_scope(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    app.set_scope(arg).map(|()| None)
}

fn do_unit(app: &mut Workspace, cmd: &Command, arg: &str) -> Result<Option<Msg>, String> {
    app.unit = match arg {
        "auto" => None,
        u => Some(crate::time::unit_named(u).map_err(|_| cmd.usage.to_string())?),
    };
    Ok(None)
}

fn do_radix(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    let r = radix::parse_name(arg).ok_or_else(|| format!("unknown radix: {arg}"))?;
    app.set_radix(Some(r)).map(|()| None)
}

fn do_color(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    if arg.is_empty() {
        return Err(format!("usage: :color <{}|off>", rows::COLORS.join("|")));
    }
    if arg == "off" {
        return app.set_color(None).map(|()| None);
    }
    app.set_color(Some(rows::color_named(arg)?)).map(|()| None)
}

fn do_group(app: &mut Workspace, cmd: &Command, arg: &str) -> Result<Option<Msg>, String> {
    if arg.is_empty() {
        return Err(cmd.usage.into());
    }
    app.group(arg).map(|()| None)
}

fn do_protocol(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    protocol(app, arg).map(|()| None)
}

fn do_names(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    let (sig, spec) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    app.name_values(sig, spec).map(|()| None)
}

fn do_rename(app: &mut Workspace, cmd: &Command, arg: &str) -> Result<Option<Msg>, String> {
    if arg.is_empty() {
        return Err(cmd.usage.into());
    }
    let h = app
        .rows
        .get(app.sel)
        .and_then(|_| rows::header_of(&app.rows, app.sel))
        .ok_or("not in a group")?;
    if let Row::Group { name, .. } = &mut app.rows[h] {
        *name = arg.to_string();
    }
    Ok(None)
}

fn do_ungroup(app: &mut Workspace, _: &Command, _arg: &str) -> Result<Option<Msg>, String> {
    let h = app
        .rows
        .get(app.sel)
        .and_then(|_| rows::header_of(&app.rows, app.sel))
        .ok_or("not in a group")?;
    let end = rows::group_end(&app.rows, h);
    for r in &mut app.rows[h + 1..end] {
        if let Row::Signal(s) = r {
            s.grouped = false;
        }
    }
    app.rows.remove(h);
    app.sel = h;
    app.fix_sel();
    Ok(None)
}

fn do_select(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    select(app, arg).map(|()| None)
}

fn do_cursor(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    cursor(app, arg).map(|()| None)
}

fn do_undo(app: &mut Workspace, _: &Command, _arg: &str) -> Result<Option<Msg>, String> {
    app.undo().map(|()| None)
}

fn do_redo(app: &mut Workspace, _: &Command, _arg: &str) -> Result<Option<Msg>, String> {
    app.redo().map(|()| None)
}

fn do_rm(app: &mut Workspace, _: &Command, arg: &str) -> Result<Option<Msg>, String> {
    rm(app, arg).map(|()| None)
}

/// `:decode name protocol key=value...`: an instance (`[[decode]]`); `clock=` is its clock,
/// `prefix=` a param, and for `frame`, `pair` and `check` the keys that aren't roles are params. A
/// value runs to the next `key=`, so a rule can have spaces: `bad=valid && !ready`; one in quotes
/// runs to the closing quote, so it can hold a `key=` (`bad="x=1"`).
/// Answers with the streams the instance made and how many records each has, violations apart,
/// so what there is to look at shows: `bus: records bus.aw 2, ...; violations bus.errors 2, ...`.
/// A `:decode` line's argument as an instance, not made yet (the TUI's decode form checks it).
pub fn parse_decode(
    arg: &str,
    from: crate::protocol::Sources<'_>,
) -> Result<crate::stream::Decode, String> {
    let usage = lookup("decode").expect("in COMMANDS").usage;
    let (name, rest) = word(arg);
    let (protocol, rest) = word(rest);
    if protocol.is_empty() {
        return Err(usage.into());
    }
    let mut pairs = key_values(rest).ok_or(usage)?;
    let clock = pairs
        .iter()
        .position(|p| p.0 == "clock")
        .map_or(String::new(), |i| pairs.remove(i).1);
    // the card says what is a role: a signal protocol's roles, or all but `prefix` for one
    // the registry doesn't name (which fails in `add_decode`); on streams, the union of their
    // roles, so a misplaced key keeps its head error (`frame` has no role `request`)
    let any = from.get(protocol);
    let (signals, params) = pairs.into_iter().partition(|(k, _)| match &any {
        Some(crate::protocol::Any::OnStreams(_)) => {
            matches!(k.as_str(), "input" | "request" | "response")
        }
        // a session sampler takes no params: every key but `prefix` is a role
        Some(crate::protocol::Any::Sampled(_)) => k != "prefix",
        // a built-in's params (`mode=0`, `baud=115200`) aren't roles
        Some(a) => k != "prefix" && !a.params().contains(&k.as_str()),
        None => k != "prefix",
    });
    Ok(crate::stream::Decode {
        name: name.into(),
        protocol: protocol.into(),
        clock,
        signals,
        params,
    })
}

fn decode(app: &mut Workspace, arg: &str) -> Result<String, String> {
    // `:decode <name> off` removes it, as `:names <signal> off` does
    if let (name, "off") = (word(arg).0, word(arg).1.trim()) {
        app.remove_decode(name)?;
        return Ok(format!("{name}: removed"));
    }
    let d = parse_decode(arg, app.sources())?;
    let name = d.name.clone();
    let name = name.as_str();
    app.add_decode(d)?;
    // its results first, then violations, then the streams they are made from (spec §16 "Results")
    let made = app.streams_of_decode(name);
    let list = |kind: u8| {
        let of = |i: usize| match (app.intermediate(i), app.streams()[i].violations) {
            (true, _) => 2,
            (_, true) => 1,
            _ => 0,
        };
        made.iter()
            .filter(|&&i| of(i) == kind)
            .map(|&i| {
                format!(
                    "{} {}",
                    app.streams()[i].name,
                    app.streams()[i].records.len()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let parts: Vec<String> = [
        ("records", list(0)),
        ("violations", list(1)),
        ("made from", list(2)),
    ]
    .into_iter()
    .filter(|(_, l)| !l.is_empty())
    .map(|(w, l)| format!("{w} {l}"))
    .collect();
    Ok(format!("{name}: {}", parts.join("; ")))
}

/// `:protocol name roles=a,b when=expr field=expr...`: defines a sampled protocol (`[[protocol]]`)
/// on its roles, no signals; `:decode` puts it on signals at a clock. The other keys are its
/// fields in order.
fn protocol(app: &mut Workspace, arg: &str) -> Result<(), String> {
    let usage = lookup("protocol").expect("in COMMANDS").usage;
    let (name, rest) = word(arg);
    if name.is_empty() {
        return Err(usage.into());
    }
    let mut fields = key_values(rest).ok_or(usage)?;
    let mut take = |k: &str| {
        fields
            .iter()
            .position(|p| p.0 == k)
            .map(|i| fields.remove(i).1)
    };
    if let Some(c) = take("clock") {
        return Err(format!(
            "a protocol names no signals, its clock neither: :decode gives it (:decode x {name} clock={c} ...)"
        ));
    }
    if let Some(path) = take("wasm") {
        // the file's name is the protocol's, and the session keeps both (spec §11)
        let stem = std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem != name {
            return Err(format!(
                "wasm {path} holds {stem}, not {name}: :protocol {stem} wasm={path}"
            ));
        }
        if !fields.is_empty() {
            return Err(format!(
                "a protocol from a file has no roles, when or fields of its own: :protocol {name} wasm={path}"
            ));
        }
        let loader = app
            .loader
            .clone()
            .ok_or_else(|| format!("{path}: this wtf has no host for protocols from files"))?;
        let loaded = loader(&path)?;
        return app.add_loaded(loaded, &path);
    }
    let (when, roles) = (take("when").ok_or(usage)?, take("roles").ok_or(usage)?);
    let roles = roles
        .split(',')
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect();
    let p = crate::stream::Sampler {
        name: name.into(),
        when,
        fields,
        roles,
        wasm: None,
    };
    app.add_protocol(p)
        .map(drop)
        .map_err(|e| format!("protocol {name} not made: {e}"))
}

/// A value as a `key=value` line writes it: as it is when it reads back so, else in quotes
/// (`bad="x=1"`).
pub fn quote(v: &str) -> String {
    match key_values(&format!("k={v}")) {
        Some(p) if p.len() == 1 && p[0].1 == v => v.to_string(),
        _ => format!("\"{v}\""),
    }
}

/// The first word, and the text after it.
fn word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    text.split_at(text.find(char::is_whitespace).unwrap_or(text.len()))
}

/// `key=value` words in order; a value runs to the next `key=`, so it can have spaces
/// (`when=valid && ready`), or in quotes to the closing one (`when="x=1"`). None if the first word
/// has no key, or a quote isn't closed.
fn key_values(text: &str) -> Option<Vec<(String, String)>> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut rest = text;
    loop {
        let (w, after) = word(rest);
        if w.is_empty() {
            return Some(pairs);
        }
        let key = w.split_once('=').filter(|(k, v)| {
            !k.is_empty()
                && k.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !v.starts_with('=')
        });
        match (key, pairs.last_mut()) {
            (Some((k, v)), _) if v.starts_with('"') => {
                // from after `key="` to the closing quote, spaces and all
                let from = &rest.trim_start()[k.len() + 2..];
                let close = from.find('"')?;
                pairs.push((k.into(), from[..close].into()));
                rest = &from[close + 1..];
                continue;
            }
            (Some((k, v)), _) => pairs.push((k.into(), v.into())),
            (None, Some(last)) => last.1 = format!("{} {w}", last.1),
            (None, None) => return None,
        }
        rest = after;
    }
}

/// `:select a [b]`: the row named `a` (a signal as `:add` names it, or a group), and with `b` the
/// rows from `a` to `b`, as `V` would select them.
fn select(app: &mut Workspace, arg: &str) -> Result<(), String> {
    let mut row = |name: &str| -> Result<usize, String> {
        if let Some(i) = app
            .rows
            .iter()
            .position(|r| matches!(r, Row::Group { name: n, .. } if n == name))
        {
            return Ok(i);
        }
        let (var, _) = app.resolve(name)?;
        app.rows
            .iter()
            .position(|r| r.sig().is_some_and(|s| s.var == var))
            .ok_or(format!("not shown: {name}"))
    };
    let mut names = arg.split_whitespace();
    let usage = lookup("select").expect("in COMMANDS").usage;
    let first = row(names.next().ok_or(usage)?)?;
    let last = names.next().map(row).transpose()?;
    app.sel = last.unwrap_or(first);
    app.bit = None;
    // a row hidden in a collapsed group: its group is shown and selected instead
    let hidden = !rows::visible(&app.rows).contains(&app.sel);
    app.fix_sel();
    app.anchor = last.filter(|_| !hidden).map(|_| first);
    Ok(())
}

/// `:cursor n time` puts mark n there (`:cursor 1 time` moves the cursor), `:cursor n off` removes
/// mark n, and `:cursor n` jumps to it (spec §9).
fn cursor(app: &mut Workspace, arg: &str) -> Result<(), String> {
    let usage = lookup("cursor").expect("in COMMANDS").usage;
    let (n, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let i = n
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=9).contains(n))
        .ok_or(usage)?
        - 1;
    // the keys start at the first word with `=`; a name or note runs to the next key
    let rest = rest.trim();
    let at = rest
        .split_whitespace()
        .find(|w| w.contains('='))
        .map_or(rest.len(), |w| w.as_ptr() as usize - rest.as_ptr() as usize);
    let (what, keys) = (rest[..at].trim(), key_values(&rest[at..]).ok_or(usage)?);
    if let Some((k, _)) = keys.iter().find(|(k, _)| k != "name" && k != "note") {
        return Err(format!(":cursor has no key {k} (name, note)"));
    }
    if i == 0 && !keys.is_empty() {
        return Err("cursor 1 is the cursor and moves: name a mark (2-9)".into());
    }
    match what {
        "" if keys.is_empty() => return app.goto_mark(i),
        "" if app.marks()[i].is_none() => {
            return Err(format!(
                "no cursor {}: give it a time first (:cursor {} <time> ...)",
                i + 1,
                i + 1
            ));
        }
        "" => {}
        "off" if keys.is_empty() => return app.remove_mark(Some(i)),
        "off" => return Err(usage.into()),
        t => {
            let t = (crate::time::parse(t, app.trace.fs_per_tick)?
                .max(0.0)
                .round() as u64)
                .min(app.trace.end());
            match i {
                0 => app.jump(t, false),
                i => app.put_mark(i, t),
            }
        }
    }
    for (k, v) in keys {
        let old = app.marks()[i].as_ref().expect("checked above").clone();
        match k.as_str() {
            "name" => app.label_mark(i, &v, &old.note),
            _ => app.label_mark(i, &old.name, &v),
        }
    }
    Ok(())
}

/// `:fit [from to]`: the whole simulation, or the range between two cursors (`'2 '3`, `'1` is
/// the cursor) or times, in either order. The cursor stays.
fn fit(app: &mut Workspace, arg: &str) -> Result<(), String> {
    let at = |a: &str| -> Result<crate::time::Tick, String> {
        match a.strip_prefix('\'') {
            Some(n) => {
                let i = n
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=9).contains(n))
                    .ok_or(format!("no cursor {n}: cursors are '1 to '9"))?
                    - 1;
                app.cursor_list()
                    .into_iter()
                    .find(|c| c.0 == i)
                    .map(|c| c.1)
                    .ok_or(format!("no cursor {n}"))
            }
            None => Ok((crate::time::parse(a, app.trace.fs_per_tick)?
                .max(0.0)
                .round() as u64)
                .min(app.trace.end())),
        }
    };
    match arg.split_whitespace().collect::<Vec<_>>()[..] {
        [] => app.view.fit(),
        [a, b] => {
            let (a, b) = (at(a)?, at(b)?);
            if a == b {
                return Err("nothing between them: the same time".into());
            }
            app.view.start = a.min(b);
            app.view.show_until(a.max(b));
        }
        _ => {
            let usage = lookup("fit").expect("in COMMANDS").usage;
            return Err(format!("{usage}, as cursors ('2 '3) or times"));
        }
    }
    Ok(())
}

/// `:w [file]`: the file given becomes the session file in use.
pub fn save(app: &mut Workspace, arg: &str) -> Result<Msg, String> {
    let path = if arg.is_empty() {
        app.session_path.clone()
    } else {
        arg.into()
    };
    session::write(&path, &session::to_toml(app))?;
    app.saved = session::layout_toml(app);
    let msg = format!("written: {}", path.display());
    app.session_path = path;
    Ok(Msg::Info(msg))
}

/// Vars an `:add` / `:rm` argument names: an exact path, a pattern in the workspace's syntax, or
/// a scope's direct signals; a name or glob under the workspace's scope first.
pub fn resolve(app: &Workspace, arg: &str) -> Result<Vec<usize>, String> {
    Ok(match app.patterns.pattern(arg)? {
        Some(Pattern::Regex(re)) => regex_matches(app, &re, false),
        _ => paths(app, &scoped(app, arg)),
    })
}

/// The vars a regex finds in their canonical path (spec §15); with `leaves`, the signals only,
/// not the structs and arrays they make up.
fn regex_matches(app: &Workspace, re: &regex::Regex, leaves: bool) -> Vec<usize> {
    let canon = app.trace.canon_paths();
    (0..app.trace.vars.len())
        .filter(|&v| !(leaves && app.trace.is_composite(v)) && re.is_match(&canon[v]))
        .collect()
}

/// `arg` under the workspace's scope (`:scope`) if it names something there, else as it is.
fn scoped(app: &Workspace, arg: &str) -> String {
    match &app.scope {
        Some(s) if !paths(app, &format!("{s}.{arg}")).is_empty() => format!("{s}.{arg}"),
        _ => arg.to_string(),
    }
}

/// As written, else in either HDL's form (spec §8: any case, `gen(4)` for `gen[4]`).
fn paths(app: &Workspace, arg: &str) -> Vec<usize> {
    if let Some(&v) = app.trace.by_path.get(arg) {
        return vec![v];
    }
    // a struct, record or array in either HDL's form is one signal, not the signals in it
    let c = crate::trace::canon(arg);
    if let Some(v) = app
        .trace
        .vars
        .iter()
        .position(|v| !v.children.is_empty() && crate::trace::canon(&v.path) == c)
    {
        return vec![v];
    }
    let found = matching(app, arg, &|p| p.to_string());
    if !found.is_empty() {
        return found;
    }
    matching(app, &crate::trace::canon(arg), &crate::trace::canon)
}

/// The vars whose path, in `form`, is `arg`, matches it as a pattern, or is in it: directly in a
/// scope, anywhere in a struct, record or array (part of one signal's HDL name).
fn matching(app: &Workspace, arg: &str, form: &dyn Fn(&str) -> String) -> Vec<usize> {
    let a = crate::trace::parts(arg);
    let hit = |v: &crate::trace::Var| {
        let path = form(&v.path);
        if arg.contains('*') {
            // a pattern takes the signals, not the composites they make up
            return v.children.is_empty() && glob(arg, &path);
        }
        let p = crate::trace::parts(&path);
        let rest = p.len().saturating_sub(a.len());
        p.starts_with(&a) && (rest <= 1 || a.len() > p.len() - v.decl.min(p.len()))
    };
    app.trace
        .vars
        .iter()
        .enumerate()
        .filter(|(_, v)| hit(v))
        .map(|(i, _)| i)
        .collect()
}

/// The name of a group of a scope's signals (spec §5, §7): the scope's tail from its last part
/// that isn't an index, indices kept (`top.cpu` → `cpu`, `dut.GEN_STAGES[0]` → `gen_stages[0]`),
/// or from where its HDL name starts if inside a struct, record or array (`grid[1]`, `pkt.pair`);
/// `group` for no scope. `scope` in either HDL's form; `var` is a signal inside it.
pub fn group_name(ws: &Workspace, scope: &str, var: usize) -> String {
    if crate::trace::canon(scope).is_empty() {
        return "group".into();
    }
    let v = &ws.trace.vars[var];
    let p = crate::trace::parts(&v.path);
    // the parts of the path `scope` names, in whichever form it was written
    let key = crate::trace::canon(scope);
    let n = (1..=p.len())
        .find(|&k| crate::trace::canon(&crate::trace::join(p[..k].iter().copied())) == key)
        .unwrap_or(p.len().saturating_sub(1))
        .max(1);
    let mut start = (p.len() - v.decl.min(p.len())).min(n - 1);
    while start > 0 && p[start].starts_with('[') {
        start -= 1;
    }
    crate::trace::join(p[start..n.min(p.len())].iter().copied())
}

/// The scope the paths share, part by part: `top.cpu.a`, `top.cpu.sub.b` → `top.cpu`.
pub fn common_scope(paths: &[&str]) -> String {
    let scopes: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| crate::trace::parts(p))
        .map(|mut v| {
            v.pop();
            v
        })
        .collect();
    let first = scopes.first().cloned().unwrap_or_default();
    let n = (0..first.len())
        .take_while(|&i| scopes.iter().all(|s| s.get(i) == Some(&first[i])))
        .count();
    crate::trace::join(first[..n].iter().copied())
}

/// `:add a b ...`: each in turn, below the previous. Names that fail are reported (as an error
/// if all do).
pub fn add(app: &mut Workspace, arg: &str) -> Result<Option<Msg>, String> {
    if arg.is_empty() {
        return Err(match app.patterns {
            Syntax::Glob => "usage: :add <path|pattern>...".into(),
            Syntax::Regex => "usage: add <path|/regex/>...".into(),
        });
    }
    let words = words(arg);
    let errors: Vec<String> = words.iter().filter_map(|w| add_one(app, w).err()).collect();
    match errors.len() {
        0 => Ok(None),
        n if n == words.len() => Err(errors.join("; ")),
        _ => Ok(Some(Msg::Warn(errors.join("; ")))),
    }
}

/// The error for a name or pattern that matches nothing, in the workspace's syntax: why a glob
/// missed (it matches the whole path, `*` is one level of it, braces are no pattern), and the
/// nearest scopes and signals.
pub fn no_match(app: &Workspace, arg: &str) -> String {
    let trace = &app.trace;
    let core = arg.trim_matches(['*', '.']);
    let mut hints = Vec::new();
    let glob_syntax = app.patterns == Syntax::Glob;
    if glob_syntax && arg.contains(['{', '}']) {
        hints.push("braces are no pattern: one name or pattern each".to_string());
    } else if glob_syntax && arg.contains('*') && core.is_empty() {
        hints.push(
            "`*` is one level of the path, `**` any number: `**` matches every signal".to_string(),
        );
    } else if glob_syntax && arg.contains('*') && !arg.starts_with("**.") {
        // the pattern that does match: signals named so, or the signals in a scope named so
        let any = |p: &str| {
            trace
                .vars
                .iter()
                .any(|v| v.children.is_empty() && glob(p, &v.path))
        };
        let fix = [format!("**.*{core}*"), format!("**.*{core}*.*")]
            .into_iter()
            .find(|p| any(p));
        let fix = fix.map_or(String::new(), |p| format!(": {p} for {core} anywhere"));
        hints.push(format!("a pattern matches the whole path{fix}"));
    }
    let near = trace.nearest(arg, 3);
    if !near.is_empty() {
        hints.push(format!("nearest: {}", near.join(", ")));
    }
    match hints.is_empty() {
        true => format!("no match: {arg}"),
        false => format!("no match: {arg} ({})", hints.join("; ")),
    }
}

fn add_one(app: &mut Workspace, arg: &str) -> Result<(), String> {
    let pattern = app.patterns.pattern(arg)?;
    if let Some(Pattern::Regex(re)) = &pattern {
        let found = regex_matches(app, re, true);
        if found.is_empty() {
            return Err(no_match(app, arg));
        }
        let new: Vec<usize> = found.into_iter().filter(|v| !app.shown(*v)).collect();
        if new.is_empty() {
            return Err(format!("already shown: {arg}"));
        }
        app.add_rows(&new);
        return Ok(());
    }
    // a bit of a bus (`state[1]`) is a one-bit slice, as in a concatenation
    let bit =
        paths(app, &scoped(app, arg)).is_empty() && matches!(app.resolve(arg), Ok((_, Some(_))));
    if crate::computed::form(arg).is_some() || bit {
        let v = app.whole(arg)?;
        if app.shown(v) {
            return Err(format!("already shown: {arg}"));
        }
        app.add_rows(&[v]);
        return Ok(());
    }
    let mut arg = scoped(app, arg);
    let mut found = paths(app, &arg);
    if found.is_empty() && pattern.is_none() {
        // the end of a scope's path, as a signal can be named by the end of its path
        let scopes: Vec<String> = {
            let d = app.trace.design();
            d.ending(&arg)
                .into_iter()
                .map(|i| d.nodes[i].path.clone())
                .collect()
        };
        match &scopes[..] {
            [one] => {
                arg = one.clone();
                found = paths(app, &arg);
            }
            [] => {}
            many => {
                return Err(format!(
                    "ambiguous: {arg} ({} scopes: {})",
                    many.len(),
                    many.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    let arg = &arg;
    if found.is_empty() {
        let ca = crate::trace::canon(arg);
        let a = crate::trace::parts(&ca);
        let (mut here, mut inner) = (arg.clone(), Vec::new());
        // compared in either HDL's form, named as in the trace: nvc's `gen(1)` is one part
        for v in &app.trace.vars {
            let p = crate::trace::parts(&v.path);
            let k = (1..p.len()).find(|&k| {
                crate::trace::parts(&crate::trace::canon(&crate::trace::join(
                    p[..k].iter().copied(),
                ))) == a
            });
            if let Some(k) = k.filter(|&k| p.len() > k + 1) {
                here = crate::trace::join(p[..k].iter().copied());
                let scope = crate::trace::join(p[..=k].iter().copied());
                if !inner.contains(&scope) {
                    inner.push(scope);
                }
            }
        }
        if !inner.is_empty() {
            // named as the trace writes it
            let list = inner.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
            let all = match app.patterns {
                Syntax::Glob => format!("{here}.**"),
                Syntax::Regex => format!("/^{}\\./", regex::escape(&crate::trace::canon(&here))),
            };
            return Err(format!(
                "no signals directly in {here}: add {all} for all of it, or one of its scopes: {list}"
            ));
        }
        return Err(no_match(app, arg));
    }
    let new: Vec<usize> = found.into_iter().filter(|v| !app.shown(*v)).collect();
    if new.is_empty() {
        return Err(format!("already shown: {arg}"));
    }
    // a scope (neither a signal nor a pattern) becomes a group named after it
    let signal = matches!(new[..], [v] if crate::trace::canon(&app.trace.vars[v].path) == crate::trace::canon(arg));
    if signal || pattern.is_some() {
        app.add_rows(&new);
    } else {
        // named as in the trace, whichever form it was named in
        let name = group_name(app, arg, new[0]);
        app.add_group(&name, &new);
    }
    Ok(())
}

fn rm(app: &mut Workspace, arg: &str) -> Result<(), String> {
    if app.rows.is_empty() {
        return Err("no rows".into());
    }
    if arg.is_empty() {
        return app.delete();
    } else {
        let mut found = Vec::new();
        for w in words(arg) {
            match resolve(app, w)?[..] {
                // a name as search names it: as shown, a slice, the end of a path
                [] => found.extend(app.resolve(w).ok().filter(|p| p.1.is_none()).map(|p| p.0)),
                ref f => found.extend(f),
            }
        }
        let before = app.rows.len();
        app.rows
            .retain(|r| r.sig().is_none_or(|s| !found.contains(&s.var)));
        if app.rows.len() == before {
            return Err(format!("no matching rows: {arg}"));
        }
    }
    rows::tidy(&mut app.rows);
    app.fix_sel();
    Ok(())
}

/// The words of an argument; a concatenation `{a, b}` is one, spaces and all.
fn words(arg: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0, None);
    for (i, c) in arg.char_indices() {
        depth += matches!(c, '{' | '(') as i32 - matches!(c, '}' | ')') as i32;
        if !c.is_whitespace() || depth > 0 {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            out.push(&arg[s..i]);
        }
    }
    out.extend(start.map(|s| &arg[s..]));
    out
}

/// Full-path pattern: `*` within one scope level, `**` across levels.
pub fn glob(pat: &str, path: &str) -> bool {
    fn segs(p: &[&str], s: &[&str]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((&"**", rest)) => (0..=s.len()).any(|i| segs(rest, &s[i..])),
            Some((first, rest)) => {
                !s.is_empty() && wild(first.as_bytes(), s[0].as_bytes()) && segs(rest, &s[1..])
            }
        }
    }
    fn wild(p: &[u8], s: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'*', rest)) => (0..=s.len()).any(|i| wild(rest, &s[i..])),
            Some((c, rest)) => s.first() == Some(c) && wild(rest, &s[1..]),
        }
    }
    let p = crate::trace::parts(pat);
    let s = crate::trace::parts(path);
    segs(&p, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob("top.cpu.*", "top.cpu.state"));
        assert!(!glob("top.cpu.*", "top.cpu.sub.x"));
        assert!(glob("top.**", "top.cpu.sub.x"));
        assert!(glob("**.state", "top.dma.state"));
        assert!(glob("top.*.st*", "top.cpu.state"));
        assert!(!glob("top.*.st*", "top.clk"));
    }
}
