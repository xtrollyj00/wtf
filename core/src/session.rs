//! Session files (spec §11): loading, saving, and the layout `[+]` compares.

use crate::radix;
use crate::rows::{self, Look, Row, Sig};
use crate::time::{self, Tick};
use crate::trace::Trace;
use crate::workspace::Workspace;
use serde::Deserialize;
use std::path::Path;
use toml::{Table, Value};

pub mod model;
// declare's entry types (no `Row`: that is the rows one above)
pub use model::{Decode, File, GroupRow, Protocol, SignalEntry, SignalRow};

pub const VERSION: i64 = 1;

/// The default session file next to the trace: `run1.fst` -> `run1.fst.session.toml`.
pub fn default_path(trace: &std::path::Path) -> std::path::PathBuf {
    let mut p = trace.as_os_str().to_owned();
    p.push(".session.toml");
    p.into()
}

/// Where an agent's `present` writes its session when no window is open on the trace:
/// `<trace>.agent.session.toml` (spec §15 "Present").
pub fn agent_path(trace: &std::path::Path) -> std::path::PathBuf {
    let mut p = trace.as_os_str().to_owned();
    p.push(".agent.session.toml");
    p.into()
}

/// Why a `radix` name doesn't fit a signal: it names no radix, or one that can't show its width.
pub enum RadixErr {
    /// No radix has this name; those that could show this width are the ones to pick from.
    Unknown {
        name: String,
        candidates: Vec<String>,
    },
    Misfit(String),
}

/// The `radix` rule the session rows and `declare` share: the name parses and fits the var. Each
/// side formats its own message.
pub fn check_radix(name: &str, width: u32, named: bool) -> Result<(usize, u32), RadixErr> {
    match radix::parse_name(name) {
        None => Err(RadixErr::Unknown {
            name: name.into(),
            candidates: radix::RADIXES
                .iter()
                .enumerate()
                .filter(|(r, radix)| !radix.fixed && radix::fits(*r, None, width, named))
                .map(|(_, radix)| radix.name.to_string())
                .collect(),
        }),
        Some((r, frac, need)) if radix::fits(r, need, width, named) => Ok((r, frac)),
        Some((r, frac, need)) => Err(RadixErr::Misfit(radix::name(
            r,
            frac,
            need.unwrap_or(width),
        ))),
    }
}

/// A session's text as a file, if it reads and this wtf supports its version: the errors that
/// refuse a whole session, checked before anything changes.
pub fn read(text: &str) -> Result<File, String> {
    parse(text).map(|(file, _)| file)
}

/// The file plus what misshapen sections it held: those read as missing. Scope, unit and every
/// other section warn where they sit, so a file the user typed only holds valid shapes; the typed
/// file only holds valid shapes.
#[derive(Default)]
struct Shape {
    scope_bad: bool,
    unit_bad: bool,
    signal_bad: bool,
    names_bad: bool,
    protocol_bad: bool,
    decode_bad: bool,
    rows_bad: bool,
    view_bad: bool,
    cursors_bad: bool,
    trail_bad: bool,
    trail_seen: bool,
}

fn parse(text: &str) -> Result<(File, Shape), String> {
    let mut t: Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("session: {}", e.message()))?;
    let version = t.remove("version").map_or(Ok(VERSION), |v| {
        v.as_integer().ok_or("session: version must be a number")
    })?;
    if version > VERSION {
        return Err(format!(
            "session version {version} is newer than this wtf supports ({VERSION})"
        ));
    }
    let mut shape = Shape {
        scope_bad: false,
        unit_bad: false,
        signal_bad: false,
        names_bad: false,
        protocol_bad: false,
        decode_bad: false,
        rows_bad: false,
        view_bad: false,
        cursors_bad: false,
        trail_bad: false,
        trail_seen: t.contains_key("trail"),
    };
    let strip = |t: &mut Table, key: &str, ok: fn(&Value) -> bool| -> bool {
        if matches!(t.get(key), Some(v) if !ok(v)) {
            t.remove(key);
            return true;
        }
        false
    };
    shape.scope_bad = strip(&mut t, "scope", Value::is_str);
    shape.unit_bad = strip(&mut t, "unit", Value::is_str);
    shape.signal_bad = strip(&mut t, "signal", Value::is_array);
    shape.names_bad = strip(&mut t, "names", Value::is_array);
    shape.protocol_bad = strip(&mut t, "protocol", Value::is_array);
    shape.decode_bad = strip(&mut t, "decode", Value::is_array);
    shape.rows_bad = strip(&mut t, "rows", Value::is_array);
    shape.view_bad = strip(&mut t, "view", Value::is_table);
    shape.cursors_bad = strip(&mut t, "cursors", Value::is_table);
    shape.trail_bad = strip(&mut t, "trail", Value::is_array);
    let file: File = Value::Table(t)
        .try_into()
        .map_err(|e: toml::de::Error| format!("session: {}", e.message()))?;
    Ok((file, shape))
}

/// The session's changes that are not a command of their own, read and checked but not made:
/// `apply` lands them at once, a caller that only asks (the MCP `workspace` tool) settles them
/// alone. A field is `None`/empty when the file didn't ask for it.
#[derive(Default)]
pub struct Commit {
    unit: Option<usize>,
    zoom: Option<f64>,
    start: Option<Tick>,
    to: Option<Tick>,
    cursors: Vec<CursorOp>,
    trail: Option<Vec<crate::workspace::Step>>,
}

/// One `[cursors.N]` entry, in the file's order.
enum CursorOp {
    /// `1`: where the cursor moves.
    SetCursor(Tick),
    /// A mark: its time, and the label a `[cursors.N]` table gave it (a plain time keeps the one
    /// the mark had, a table's empty name and note clear it).
    PutMark {
        n: usize,
        time: Tick,
        label: Option<(String, String)>,
    },
    /// A mark the file put nowhere (a time that isn't one): it goes.
    ClearMark(usize),
}

impl Commit {
    /// Make the changes `validate` read, in the file's order.
    pub fn land(self, app: &mut Workspace) {
        if let Some(u) = self.unit {
            app.unit = Some(u);
        }
        if let Some(z) = self.zoom {
            app.view.set_zoom(z);
        }
        if let Some(s) = self.start {
            app.view.start = s;
            app.view.pending_fit = false;
        }
        // `to`: the end of the visible range, whatever the screen's width (sent by `present`)
        if let Some(to) = self.to {
            app.view.show_until(to);
        }
        app.view.cursor = 0;
        for op in self.cursors {
            match op {
                CursorOp::SetCursor(t) => app.view.cursor = t,
                CursorOp::PutMark { n, time, label } => {
                    app.put_mark(n, time);
                    if let Some((name, note)) = label {
                        app.label_mark(n, &name, &note);
                    }
                }
                CursorOp::ClearMark(n) => {
                    app.remove_mark(Some(n)).ok();
                }
            }
        }
        if let Some(trail) = self.trail {
            app.trail = trail;
        }
    }
}

/// Apply a session to a freshly opened app. Returns warnings for the status line.
pub fn apply(app: &mut Workspace, text: &str) -> Result<Vec<String>, String> {
    let (file, shape) = parse(text)?;
    let (warn, commit) = validate(app, file, shape);
    commit.land(app);
    // after the marks: `[+]` compares them too
    app.saved = layout_toml(app);
    Ok(warn)
}

/// The problems a declaration has and what it sets, for a `File` that skipped `parse`: the MCP
/// `workspace` tool's JSON is typed, so no section can arrive the wrong shape (spec §15).
pub fn validate_file(app: &mut Workspace, file: File) -> (Vec<String>, Commit) {
    validate(app, file, Shape::default())
}

/// Read a session onto the app: every section that can be made is made, the rest is `Commit`, and
/// each problem it meets is a warning naming its subject, in the order the file holds them.
fn validate(app: &mut Workspace, file: File, shape: Shape) -> (Vec<String>, Commit) {
    let mut warn = Vec::new();
    let mut commit = Commit::default();

    // the scope first: the names after it resolve under it
    if shape.scope_bad {
        warn.push("scope: must be a string".into());
    } else if let Some(scope) = file.scope
        && let Err(e) = app.set_scope(&scope)
    {
        warn.push(e);
    }
    // the agent's time unit (`:unit`)
    if shape.unit_bad {
        warn.push("unit: must be a string".into());
    } else if let Some(unit) = file.unit.as_deref() {
        match time::unit_named(unit) {
            Ok(u) => commit.unit = Some(u),
            Err(e) => warn.push(format!("unit: {e}")),
        }
    }
    // named signals first: rows may show them
    if shape.signal_bad {
        warn.push("signal: must be a list".into());
    }
    for (i, s) in file.signal.into_iter().enumerate() {
        let (name, expr) = (
            s.name
                .as_ref()
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty()),
            s.expr.as_ref().and_then(Value::as_str),
        );
        let site = match name {
            Some(n) => format!("signal {n}"),
            None => format!("signal [{}]", i + 1),
        };
        unknown(&site, &s.rest, &["name", "expr"], &mut warn);
        match (name, expr) {
            (Some(n), Some(e)) => app
                .define(n, e)
                .map(drop)
                .unwrap_or_else(|e| warn.push(format!("signal {n}: {e}"))),
            _ => warn.push(format!("signal [{}]: needs a name and an expr", i + 1)),
        }
    }
    // names for values (`:names`), before rows pick their radix
    if shape.names_bad {
        warn.push("names: must be a list".into());
    }
    for (i, n) in file.names.into_iter().enumerate() {
        let site = match n.signal.as_ref().and_then(Value::as_str) {
            Some(sig) => format!("names {sig}"),
            None => format!("names [{}]", i + 1),
        };
        unknown(&site, &n.rest, &["signal", "values"], &mut warn);
        match (
            n.signal.as_ref().and_then(Value::as_str),
            n.values.as_ref().and_then(Value::as_str),
        ) {
            (Some(sig), Some(v)) => app
                .name_values(sig, v)
                .unwrap_or_else(|e| warn.push(format!("names {sig}: {e}"))),
            _ => warn.push(format!("names [{}]: needs a signal and values", i + 1)),
        }
    }
    // protocols and their instances (spec §16), after the signals they may use
    let text = |o: &Option<Value>| o.as_ref().and_then(Value::as_str).map(String::from);
    // a table of names (`fields`, `signals`), in its order; missing is empty
    let pairs = |o: &Option<Value>| match o {
        None => Some(Vec::new()),
        Some(v) => v.as_table().map(|f| {
            f.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        }),
    };
    // the entry's name, or where it sits in the file when it gives none to call it by
    let named = |kind: &str, name: Option<String>, i: usize| match name {
        Some(n) => format!("{kind} {n}"),
        None => format!("{kind} [{}]", i + 1),
    };
    if shape.protocol_bad {
        warn.push("protocol: must be a list".into());
    }
    for (i, p) in file.protocol.into_iter().enumerate() {
        let subject = named("protocol", text(&p.name), i);
        unknown(
            &subject,
            &p.rest,
            &["name", "roles", "when", "fields", "wasm"],
            &mut warn,
        );
        let roles = match p.roles.as_ref() {
            None => None,
            Some(r) => r.as_array().map(|r| {
                r.iter()
                    .map(|r| r.as_str().unwrap_or_default().to_string())
                    .collect::<Vec<_>>()
            }),
        };
        if p.clock.is_some() {
            warn.push(format!(
                "{subject}: a protocol has no clock; its [[decode]] gives it"
            ));
            continue;
        }
        // one from a file is that file and its name: nothing else belongs in the entry (§11)
        if let (Some(name), Some(path)) = (text(&p.name), text(&p.wasm)) {
            let kept = [
                ("when", p.when.is_some()),
                ("roles", p.roles.is_some()),
                ("fields", p.fields.is_some()),
            ]
            .into_iter()
            .find(|&(_, some)| some)
            .map(|(k, _)| k);
            if let Some(k) = kept {
                warn.push(format!(
                    "protocol {name}: wasm {path} keeps no {k}, its card is the file's"
                ));
                continue;
            }
            let def = crate::stream::Sampler {
                name: name.clone(),
                wasm: Some(path),
                ..Default::default()
            };
            app.add_protocol(def)
                .map(drop)
                .unwrap_or_else(|e| warn.push(format!("protocol {name}: {e}")));
            continue;
        }
        match (text(&p.name), text(&p.when), pairs(&p.fields), roles) {
            (Some(name), Some(when), Some(fields), Some(roles)) => {
                let def = crate::stream::Sampler {
                    name: name.clone(),
                    when,
                    fields,
                    roles,
                    wasm: None,
                };
                app.add_protocol(def)
                    .map(drop)
                    .unwrap_or_else(|e| warn.push(format!("protocol {name}: {e}")));
            }
            _ => warn.push(format!(
                "{subject}: needs a name, roles as a list, a when and fields as a table"
            )),
        }
    }
    if shape.decode_bad {
        warn.push("decode: must be a list".into());
    }
    for (i, d) in file.decode.into_iter().enumerate() {
        let subject = named("decode", text(&d.name), i);
        unknown(
            &subject,
            &d.rest,
            &["name", "protocol", "clock", "signals", "params"],
            &mut warn,
        );
        match (
            text(&d.name),
            text(&d.protocol),
            pairs(&d.signals),
            pairs(&d.params),
        ) {
            (Some(name), Some(protocol), Some(signals), Some(params)) => {
                let clock = text(&d.clock).unwrap_or_default();
                app.add_decode(crate::stream::Decode {
                    name,
                    protocol,
                    clock,
                    signals,
                    params,
                })
                .map(drop)
                .unwrap_or_else(|e| warn.push(format!("decode {e}")));
            }
            _ => warn.push(format!(
                "{subject}: needs a name, a protocol, signals and params as tables"
            )),
        }
    }
    if shape.rows_bad {
        warn.push("rows: must be a list".into());
    }
    let mut missing = Vec::new();
    let mut rows = Vec::new();
    // the rows read back through the value they serialize to, so `rows_of` keeps its shape
    let rv = Value::try_from(file.rows).unwrap_or(Value::Array(Vec::new()));
    rows_of(app, rv, false, "rows", &mut rows, &mut missing, &mut warn);
    // groups whose signals are all missing
    rows::tidy(&mut rows);
    let vars: Vec<usize> = rows.iter().filter_map(Row::sig).map(|s| s.var).collect();
    app.trace.load(&vars);
    app.rows = rows;
    app.sel = 0;
    if !missing.is_empty() {
        warn.push(format!("not in trace: {}", missing.join(", ")));
    }

    // a time at `site`, in file ticks; a trace's end is as far as the view goes
    let fpt = app.trace.fs_per_tick;
    let end = app.trace.end();
    let time = |v: &Value, site: &str, warn: &mut Vec<String>| -> Option<Tick> {
        let r = match v {
            Value::String(s) => crate::time::parse(s, fpt).map_err(String::from),
            Value::Integer(i) => Ok(*i as f64),
            _ => Err("bad time".into()),
        };
        r.map_err(|e| warn.push(format!("{site}: {e}")))
            .ok()
            .map(|x| (x.round() as u64).min(end))
    };
    if shape.view_bad {
        warn.push("view: must be a table".into());
    }
    let view = file.view;
    if let Some(z) = view.zoom {
        match z.as_str().map(|s| crate::time::parse(s, fpt)) {
            Some(Ok(z)) if z > 0.0 => commit.zoom = Some(z),
            _ => warn.push("view.zoom: bad time".into()),
        }
    }
    commit.start = view.time.and_then(|v| time(&v, "view.time", &mut warn));
    commit.to = view.to.and_then(|v| time(&v, "view.to", &mut warn));
    unknown("view", &view.rest, &["time", "to", "zoom"], &mut warn);
    if shape.cursors_bad {
        warn.push("cursors: must be a table".into());
    }
    for (k, v) in file.cursors {
        match k.parse::<usize>() {
            Ok(1) if v.is_table() => warn
                .push("cursors.1: the cursor moves and has no name or note; give it a time".into()),
            Ok(1) => {
                let site = format!("cursors.{k}");
                let t = time(&v, &site, &mut warn).unwrap_or(0);
                commit.cursors.push(CursorOp::SetCursor(t));
            }
            // a mark is a time, or a table with its time, name and note (spec §9)
            Ok(n @ 2..=9) => match v {
                Value::Table(mut m) => {
                    let text =
                        |m: &mut Table, key: &str, warn: &mut Vec<String>| match m.remove(key) {
                            None => String::new(),
                            Some(Value::String(s)) => s,
                            Some(_) => {
                                warn.push(format!("cursors.{k}.{key} must be a string"));
                                String::new()
                            }
                        };
                    let (name, note) = (
                        text(&mut m, "name", &mut warn),
                        text(&mut m, "note", &mut warn),
                    );
                    match m.remove("time") {
                        Some(t) => {
                            let site = format!("cursors.{k}.time");
                            match time(&t, &site, &mut warn) {
                                Some(time) => commit.cursors.push(CursorOp::PutMark {
                                    n: n - 1,
                                    time,
                                    label: Some((name, note)),
                                }),
                                // a bad time clears the mark, as assigning None did; a name
                                // without a time has nowhere to live — the writer only writes put marks
                                None => commit.cursors.push(CursorOp::ClearMark(n - 1)),
                            }
                        }
                        None => warn.push(format!("cursors.{k}: needs a time")),
                    }
                    unknown(
                        &format!("cursors.{k}"),
                        &m,
                        &["time", "name", "note"],
                        &mut warn,
                    );
                }
                v => {
                    let site = format!("cursors.{k}");
                    commit.cursors.push(match time(&v, &site, &mut warn) {
                        Some(time) => CursorOp::PutMark {
                            n: n - 1,
                            time,
                            label: None,
                        },
                        None => CursorOp::ClearMark(n - 1),
                    });
                }
            },
            _ => warn.push(format!("cursors: no cursor {k} (1-9)")),
        }
    }
    // the agent's steps, in a session another window shares (spec §15 "Trail")
    // a session without steps (the agent's own `workspace`) leaves the trail as it is
    if shape.trail_bad {
        warn.push("trail: must be a list".into());
    } else if shape.trail_seen {
        let mut trail = Vec::new();
        steps(app, &file.trail, &mut trail, &mut warn);
        commit.trail = Some(trail);
    }
    unknown(
        "session",
        &file.rest,
        &[
            "version", "scope", "unit", "signal", "names", "protocol", "decode", "rows", "view",
            "cursors", "trail",
        ],
        &mut warn,
    );
    (warn, commit)
}

fn rows_of(
    app: &mut Workspace,
    rows: Value,
    grouped: bool,
    site: &str,
    out: &mut Vec<Row>,
    missing: &mut Vec<String>,
    warn: &mut Vec<String>,
) {
    let Value::Array(rows) = rows else {
        warn.push(format!("{site}: must be a list"));
        return;
    };
    for (i, row) in rows.into_iter().enumerate() {
        let Value::Table(mut row) = row else { continue };
        let mut site = format!("{site} [{}]", i + 1);
        // the keys the entry takes, for one it doesn't: a signal's or a group's
        let mut keys: &[&str] = &["signal", "group"];
        if let Some(Value::String(path)) = row.remove("signal") {
            keys = &["signal", "radix", "color", "open", "fields"];
            site = format!("rows: {path}");
            let levels: Vec<String> = match row.remove("open") {
                Some(Value::Array(a)) => a
                    .iter()
                    .filter_map(|n| n.as_str().map(str::to_string))
                    .collect(),
                Some(_) => {
                    warn.push(format!("open of {path}: must be a list of names"));
                    Vec::new()
                }
                None => Vec::new(),
            };
            let fields = row.remove("fields");
            // its path, else a name: a row may name what a `decode` above it made, which no path
            // has until the decode is on the workspace (the decodes are made before the rows)
            let found = app.var_named(&path).or_else(|| match app.resolve(&path) {
                Ok((var, None)) => Some(var),
                _ => None,
            });
            match found {
                Some(var) if !out.iter().any(|r| r.sig().is_some_and(|s| s.var == var)) => {
                    let mut look_warn = Vec::new();
                    let look = look_keys(
                        app,
                        &mut row,
                        var,
                        &path,
                        Look::of(&app.trace.vars[var]),
                        &mut look_warn,
                    );
                    warn.extend(look_warn.into_iter().map(|w| format!("rows: {path}: {w}")));
                    let mut open: Vec<usize> = Vec::new();
                    for name in levels {
                        match open_var(app, var, &name) {
                            Some(v) if !open.contains(&v) => open.push(v),
                            Some(_) => {}
                            None => warn.push(format!("{path} has no {name} to open")),
                        }
                    }
                    let mut sig = Sig {
                        var,
                        look,
                        grouped,
                        open,
                        looks: Vec::new(),
                    };
                    if let Some(f) = fields {
                        fields_of(app, &mut sig, f, &path, warn);
                    }
                    out.push(Row::Signal(sig));
                }
                found => {
                    // shown already, or missing: its keys go with it
                    row.remove("radix");
                    row.remove("color");
                    if found.is_none() {
                        missing.push(path);
                    }
                }
            }
        } else if let Some(Value::String(name)) = row.remove("group") {
            keys = &["group", "collapsed", "rows"];
            let collapsed = row
                .remove("collapsed")
                .and_then(|c| c.as_bool())
                .unwrap_or(false);
            // shortcut: groups have one level until subgroups exist (ROADMAP.md "Later"); nested ones are flattened.
            if grouped {
                warn.push(format!("nested group flattened: {name}"));
            } else {
                out.push(Row::Group { name, collapsed });
            }
            if let Some(sub) = row.remove("rows") {
                rows_of(app, sub, true, &format!("{site}.rows"), out, missing, warn);
            }
        } else {
            warn.push(format!("{site}: entry without signal or group"));
        }
        unknown(&site, &row, keys, warn);
    }
}

/// A line's `radix` and `color` keys (a row's or a field's), checked against its signal: `base`
/// where one is missing or wrong.
fn look_keys(
    app: &Workspace,
    t: &mut Table,
    var: usize,
    path: &str,
    base: Look,
    warn: &mut Vec<String>,
) -> Look {
    let mut l = base;
    if let Some(r) = t.remove("radix") {
        let v = &app.trace.vars[var];
        match check_radix(
            r.as_str().unwrap_or(""),
            v.width.unwrap_or(0),
            v.names.is_some(),
        ) {
            Ok((r, frac)) => (l.radix, l.frac) = (r, frac),
            Err(RadixErr::Unknown { name, candidates }) => warn.push(format!(
                "unknown radix: {name} (radixes: {})",
                candidates.join(", ")
            )),
            // enum fits any width: it misfits a signal no value names were given for
            Err(RadixErr::Misfit(_))
                if v.names.is_none()
                    && radix::parse_name(r.as_str().unwrap_or(""))
                        .is_some_and(|(r, _, _)| r == radix::ENUM) =>
            {
                warn.push(format!("enum: the file names no values of {path}"))
            }
            Err(RadixErr::Misfit(shown)) => warn.push(format!("radix {shown} doesn't fit {path}")),
        }
    }
    if let Some(c) = t.remove("color") {
        match rows::color_named(c.as_str().unwrap_or("")) {
            Ok(c) => l.color = Some(c),
            Err(e) => warn.push(e),
        }
    }
    l
}

/// A row's `fields`: the lines under it with a look of their own, each named as `open` names them
/// (`pkt.pair.hi`), with `radix` and `color` keys as a row has.
fn fields_of(app: &Workspace, sig: &mut Sig, fields: Value, path: &str, warn: &mut Vec<String>) {
    let Value::Array(fields) = fields else {
        warn.push(format!("fields of {path}: must be a list"));
        return;
    };
    for f in fields {
        let Value::Table(mut f) = f else { continue };
        let Some(Value::String(name)) = f.remove("field") else {
            warn.push(format!("fields of {path}: entry without field"));
            continue;
        };
        match open_var(app, sig.var, &name).filter(|&v| v != sig.var) {
            Some(v) if !sig.looks.iter().any(|(x, _)| *x == v) => {
                let base = app.line_default(sig, v);
                let l = look_keys(app, &mut f, v, &name, base, warn);
                if l != base {
                    sig.looks.push((v, l));
                }
            }
            Some(_) => {}
            None => warn.push(format!("{path} has no field {name}")),
        }
        f.remove("radix");
        f.remove("color");
        unknown(
            &format!("rows: {path}: field {name}"),
            &f,
            &["field", "radix", "color"],
            warn,
        );
    }
}

/// The keys an entry doesn't take, each a warning naming where it sits and the keys it does take
/// (spec §11): a misspelt key is a mistake to fix, not one dropped without a word.
fn unknown(site: &str, t: &Table, keys: &[&str], warn: &mut Vec<String>) {
    warn.extend(
        t.keys()
            .map(|k| format!("{site}: unknown key: {k} (keys: {})", keys.join(", "))),
    );
}

/// A level open under a row, named from the row's name on (`pkts`, `pkts[1]`, `pkts[1].pair`; a
/// bus or stream opened is its own name, spec §11).
pub fn open_name(app: &Workspace, row: usize, var: usize) -> String {
    let base = &app.trace.vars[row].path;
    rows::child_name(base) + &app.trace.vars[var].path[base.len()..]
}

/// The var a level's name in a row's `open` names: the row's own or one inside it.
pub fn open_var(app: &Workspace, row: usize, name: &str) -> Option<usize> {
    let base = &app.trace.vars[row].path;
    let rest = name.strip_prefix(&rows::child_name(base))?;
    if !(rest.is_empty() || rest.starts_with(['.', '['])) {
        return None;
    }
    app.trace.by_path.get(&format!("{base}{rest}")).copied()
}

/// The steps a shared session holds.
pub fn read_trail(app: &Workspace, text: &str) -> Vec<crate::workspace::Step> {
    let mut out = Vec::new();
    if let Ok((file, _shape)) = parse(text) {
        steps(app, &file.trail, &mut out, &mut Vec::new());
    }
    out
}

/// `[[trail]]` tables as steps, into `out`.
fn steps(
    app: &Workspace,
    steps: &[model::TrailStep],
    out: &mut Vec<crate::workspace::Step>,
    warn: &mut Vec<String>,
) {
    let (fpt, end) = (app.trace.fs_per_tick, app.trace.end());
    let opt = |v: &Option<Value>| v.as_ref().and_then(Value::as_str).map(String::from);
    for (i, s) in steps.iter().enumerate() {
        unknown(
            &format!("trail [{}]", i + 1),
            &s.rest,
            &["tool", "ask", "answer", "time"],
            warn,
        );
        match (opt(&s.tool), opt(&s.ask), opt(&s.answer)) {
            (Some(tool), Some(ask), Some(answer)) => {
                let time = opt(&s.time)
                    .and_then(|t| time::parse(&t, fpt).ok())
                    .map(|t| (t.max(0.0).round() as u64).min(end));
                out.push(crate::workspace::Step {
                    tool,
                    ask,
                    answer,
                    time,
                });
            }
            _ => warn.push(format!(
                "trail [{}]: a step needs a tool, an ask and an answer",
                i + 1
            )),
        }
    }
}

/// A line's `radix` and `color` values where its look differs from `base`.
fn look_vals(app: &Workspace, var: usize, l: &Look, base: &Look) -> (Option<Value>, Option<Value>) {
    let v = &app.trace.vars[var];
    let radix = (v.width != Some(1) && (l.radix, l.frac) != (base.radix, base.frac))
        .then(|| Value::String(radix::name(l.radix, l.frac, v.width.unwrap_or(0))));
    let color = l.color.map(|c| Value::String(rows::COLORS[c].into()));
    (radix, color)
}

/// A signal row as the file holds it: its path, and its radix, expansion and fields where they
/// differ from the defaults.
fn signal_row(app: &Workspace, s: &Sig) -> model::SignalRow {
    let var = &app.trace.vars[s.var];
    let (radix, color) = look_vals(app, s.var, &s.look, &Look::of(var));
    let mut r = model::SignalRow {
        signal: var.path.clone(),
        radix,
        color,
        ..Default::default()
    };
    if !s.open.is_empty() {
        r.open = Some(Value::Array(
            s.open
                .iter()
                .map(|&v| Value::String(open_name(app, s.var, v)))
                .collect(),
        ));
    }
    if !s.looks.is_empty() {
        r.fields = Some(Value::Array(
            s.looks
                .iter()
                .map(|(v, l)| {
                    let mut f = Table::new();
                    f.insert("field".into(), Value::String(open_name(app, s.var, *v)));
                    // a field always names its radix: a stream row's default needs the row's
                    f.insert(
                        "radix".into(),
                        Value::String(radix::name(
                            l.radix,
                            l.frac,
                            app.trace.vars[*v].width.unwrap_or(0),
                        )),
                    );
                    if let Some(c) = l.color {
                        f.insert("color".into(), Value::String(rows::COLORS[c].into()));
                    }
                    Value::Table(f)
                })
                .collect(),
        ));
    }
    r
}

/// The layout part of a session: named signals, value names, protocols, decodes and rows.
fn rows_file(app: &Workspace) -> File {
    let mut f = File::default();
    for (name, expr) in &app.signals {
        f.signal.push(model::SignalEntry {
            name: Some(Value::String(name.clone())),
            expr: Some(Value::String(expr.clone())),
            ..Default::default()
        });
    }
    for (path, spec) in &app.value_names {
        f.names.push(model::NamesEntry {
            signal: Some(Value::String(path.clone())),
            values: Some(Value::String(spec.clone())),
            ..Default::default()
        });
    }
    for p in app.protocols() {
        let mut e = model::Protocol {
            name: Some(Value::String(p.name.clone())),
            ..Default::default()
        };
        // one from a file is that file: read back, it loads it again (spec §11)
        match &p.wasm {
            Some(path) => e.wasm = Some(Value::String(path.clone())),
            None => {
                e.roles = Some(Value::Array(
                    p.roles.iter().map(|r| Value::String(r.clone())).collect(),
                ));
                e.when = Some(Value::String(p.when.clone()));
                if !p.fields.is_empty() {
                    e.fields = Some(Value::Table(
                        p.fields
                            .iter()
                            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                            .collect(),
                    ));
                }
            }
        }
        f.protocol.push(e);
    }
    for d in app.decodes() {
        let mut e = model::Decode {
            name: Some(Value::String(d.name.clone())),
            protocol: Some(Value::String(d.protocol.clone())),
            ..Default::default()
        };
        if !d.clock.is_empty() {
            e.clock = Some(Value::String(d.clock.clone()));
        }
        if !d.signals.is_empty() {
            e.signals = Some(Value::Table(
                d.signals
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            ));
        }
        if !d.params.is_empty() {
            e.params = Some(Value::Table(
                d.params
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            ));
        }
        f.decode.push(e);
    }
    let mut i = 0;
    while i < app.rows.len() {
        match &app.rows[i] {
            Row::Group { name, collapsed } => {
                let end = rows::group_end(&app.rows, i);
                let mut g = model::GroupRow {
                    group: name.clone(),
                    ..Default::default()
                };
                if *collapsed {
                    g.collapsed = Some(Value::Boolean(true));
                }
                g.rows = Some(Value::Array(
                    app.rows[i + 1..end]
                        .iter()
                        .filter_map(Row::sig)
                        .map(|s| {
                            Value::try_from(signal_row(app, s))
                                .unwrap_or(Value::Table(Table::new()))
                        })
                        .collect(),
                ));
                f.rows.push(model::Row::Group(g));
                i = end;
            }
            Row::Signal(s) => {
                f.rows.push(model::Row::Signal(signal_row(app, s)));
                i += 1;
            }
        }
    }
    f
}

/// The whole file: layout, view and cursors; with `shared` the visible end (`to`); with `trail`
/// the agent's steps last (spec §15 "Trail").
fn file_of(app: &Workspace, shared: bool, trail: bool) -> File {
    let fpt = app.trace.fs_per_tick;
    let v = &app.view;
    let mut f = rows_file(app);
    f.scope = app.scope.clone();
    f.unit = app.unit.map(|u| time::unit_name(u).to_string());
    // below one tick per column the zoom is a fraction of a tick, written as plain ticks
    let zoom = if v.cols == 1 {
        time::format_exact(v.ticks, fpt)
    } else {
        (1.0 / v.cols as f64).to_string()
    };
    f.view.zoom = Some(Value::String(zoom));
    f.view.time = Some(Value::String(time::format_exact(v.start, fpt)));
    if shared {
        f.view.to = Some(Value::String(time::format_exact(
            v.last_visible().min(app.trace.end()),
            fpt,
        )));
    }
    for (i, t) in app.cursor_list() {
        let t = Value::String(time::format_exact(t, fpt));
        if i == 0 {
            f.cursors.insert("1".into(), t);
        } else if let Some(m) = app.marks()[i].as_ref() {
            if m.name.is_empty() && m.note.is_empty() {
                f.cursors.insert((i + 1).to_string(), t);
            } else {
                let mut mark = Table::new();
                mark.insert("time".into(), t);
                if !m.name.is_empty() {
                    mark.insert("name".into(), Value::String(m.name.clone()));
                }
                if !m.note.is_empty() {
                    mark.insert("note".into(), Value::String(m.note.clone()));
                }
                f.cursors.insert((i + 1).to_string(), Value::Table(mark));
            }
        }
    }
    if trail && !app.trail.is_empty() {
        f.trail = app
            .trail
            .iter()
            .map(|s| model::TrailStep {
                tool: Some(Value::String(s.tool.clone())),
                ask: Some(Value::String(s.ask.clone())),
                answer: Some(Value::String(s.answer.clone())),
                time: s.time.map(|t| Value::String(time::format_exact(t, fpt))),
                ..Default::default()
            })
            .collect();
    }
    f
}

fn toml(app: &Workspace, shared: bool, trail: bool) -> String {
    format!(
        "# wtf session\n{}",
        toml::to_string(&file_of(app, shared, trail)).unwrap_or_default()
    )
}

/// The whole session: view, cursors and layout.
pub fn to_toml(app: &Workspace) -> String {
    toml(app, false, false)
}

/// The session as another window takes it (`present`): with `to`, the end of the visible range,
/// so a window of another width shows the same stretch.
pub fn to_shared_toml(app: &Workspace) -> String {
    toml(app, true, false)
}

/// The session a window reads from an agent's workspace (spec §15 "Joining"): shared, then the
/// whole trail.
pub fn to_link_toml(app: &Workspace) -> String {
    toml(app, true, true)
}

/// A shared session without its trail, which comes last: what a window following an agent
/// compares, so a new step alone doesn't touch the user's view (spec §15 "Following").
///
/// The file ends with the steps, each a `[[trail]]` section after a blank line: this cuts at the
/// first `[[trail]]` header line whose text before it is whole TOML. A note may hold the same
/// line inside a multi-line string, and the text up to there is then an open string
/// (`trail_comes_last`, `trail_split_skips_strings`, `trail_split_one_line_quotes`).
pub fn without_trail(text: &str) -> &str {
    let mut from = 0;
    while let Some(i) = text[from..].find("\n[[trail]]\n").map(|i| from + i) {
        // up to the newline before the header: the blank separator line's goes too
        if toml::from_str::<Table>(&text[..i]).is_ok() {
            return &text[..i];
        }
        from = i + 1;
    }
    text
}

/// What `[+]` compares (spec §5 "Undo"): the rows and the marks, as the session writes them.
pub fn layout_toml(app: &Workspace) -> String {
    let mut out = rows_toml(app);
    // the marks inline: an internal `[+]` string only, so the `Option<Mark>` Debug shape is
    // harmless — both sides come from this function
    for (i, t) in app.cursor_list().into_iter().filter(|c| c.0 > 0) {
        out += &format!("{i} {t} {:?}\n", app.marks()[i]);
    }
    out
}

/// The layout part of a session. `[+]` compares it with the one last saved or loaded; it names
/// signals by path, so it still compares after `:e` reopens the trace.
pub fn rows_toml(app: &Workspace) -> String {
    let f = rows_file(app);
    let text = toml::to_string(&f).unwrap_or_default();
    // the version is the file's, not the layout's: a fresh window is not modified
    let head = format!("version = {}\n", f.version);
    text.strip_prefix(&head)
        .map_or_else(|| text.clone(), str::to_string)
}

/// What the text view adds to the file: display names, the selection, time formatting and the
/// visible range. The trace gives default looks and the tick rate; `show` formats ticks in the
/// caller's unit — the core picks no display unit.
pub struct Describe<'a> {
    pub names: &'a [String],
    pub sel: usize,
    pub show: &'a dyn Fn(time::Tick) -> String,
    pub visible: (String, String),
    pub trace: &'a Trace,
}

/// The workspace in words: the file's rows, looks and marks as text (`view`).
pub fn describe(f: &File, d: &Describe) -> String {
    let mut out = String::from("rows (> selected):\n");
    if f.rows.is_empty() {
        out += "  (none)\n";
    }
    let mut flat = 0;
    let mut names = d.names.iter();
    for row in &f.rows {
        flat = describe_row(d, row, flat, &mut names, &mut out);
    }
    // cursor times: integers stay exact; strings re-parse exactly as `apply` parses them
    // (the `f64` path is shared, so `view` shows what `present` placed)
    let fpt = d.trace.fs_per_tick;
    let ticks = |v: &Value| match v {
        Value::Integer(i) => (*i).try_into().ok(),
        Value::String(s) => time::parse(s, fpt).ok().map(|t| t.max(0.0).round() as u64),
        _ => None,
    };
    let mut cursors = Vec::new();
    for (k, v) in &f.cursors {
        let Ok(n) = k.parse::<usize>() else { continue }; // `apply` warns
        if !(1..=9).contains(&n) {
            continue;
        }
        let (t, name, note) = match v {
            Value::Table(m) if n > 1 => (
                m.get("time").and_then(ticks),
                m.get("name").and_then(Value::as_str).unwrap_or_default(),
                m.get("note").and_then(Value::as_str).unwrap_or_default(),
            ),
            // a table on 1 warns on apply; the cursor stays 0
            Value::Table(_) => (Some(0), "", ""),
            v => (ticks(v), "", ""),
        };
        let Some(t) = t else { continue }; // unparseable times are cleared on apply
        cursors.push((n, t, name.to_string(), note.to_string()));
    }
    if !cursors.iter().any(|&(n, _, _, _)| n == 1) {
        cursors.push((1, 0, String::new(), String::new())); // the cursor is always listed
    }
    cursors.sort_by_key(|&(n, t, _, _)| (t, n));
    let mut list = Vec::with_capacity(cursors.len());
    for (k, &(n, t, ref name, ref note)) in cursors.iter().enumerate() {
        let dt = if k > 0 {
            let (pn, pt, _, _) = cursors[k - 1];
            format!(", +{} after {}", (d.show)(t.saturating_sub(pt)), pn)
        } else {
            String::new()
        };
        let name = if name.is_empty() {
            String::new()
        } else {
            format!(", \"{name}\"")
        };
        let note = if note.is_empty() {
            String::new()
        } else {
            format!(": {note}")
        };
        list.push(format!("  {n}: {}{dt}{name}{note}", (d.show)(t)));
    }
    out += &format!(
        "cursors, in time:\n{}\nvisible: {} - {}\n",
        list.join("\n"),
        d.visible.0,
        d.visible.1
    );
    out
}

fn describe_row(
    d: &Describe,
    row: &model::Row,
    mut flat: usize,
    names: &mut std::slice::Iter<String>,
    out: &mut String,
) -> usize {
    match row {
        model::Row::Signal(s) => {
            let mark = if flat == d.sel { ">" } else { " " };
            flat += 1;
            // machine files always resolve; a missing var keeps its name slot and shows bare
            let name = names.next().map(String::as_str).unwrap_or("?");
            let extra = match d.trace.by_path.get(&s.signal) {
                None => format!("  {}", s.signal),
                Some(&v) => {
                    let var = &d.trace.vars[v];
                    let width = var.width.unwrap_or(0);
                    let mut l = Look::of(var);
                    if let Some(r) = s.radix.as_ref().and_then(Value::as_str) {
                        // invalid stored values never come from our writer; keep the default
                        if let Ok((r, frac)) = check_radix(r, width, var.names.is_some()) {
                            (l.radix, l.frac) = (r, frac);
                        }
                    }
                    if let Some(c) = s.color.as_ref().and_then(Value::as_str) {
                        l.color = rows::color_named(c).ok();
                    }
                    let radix = if width > 1 {
                        format!("  {}", radix::name(l.radix, l.frac, width))
                    } else {
                        String::new()
                    };
                    let color = l
                        .color
                        .map(|c| format!("  {}", rows::COLORS[c]))
                        .unwrap_or_default();
                    let bits = match s.open.as_ref().and_then(Value::as_array) {
                        Some(a) if !a.is_empty() => format!(
                            "  (open: {})",
                            a.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        _ => String::new(),
                    };
                    let fields = match s.fields.as_ref().and_then(Value::as_array) {
                        Some(a) if !a.is_empty() => {
                            let one = |t: &Value| -> Option<String> {
                                let t = t.as_table()?;
                                let name = t.get("field").and_then(Value::as_str)?;
                                // the field's var, named from the row's name on (as `open` names it)
                                let base = &d.trace.vars[v].path;
                                let rest = name.strip_prefix(&rows::child_name(base))?;
                                if !(rest.is_empty() || rest.starts_with(['.', '['])) {
                                    return None;
                                }
                                let fv = d.trace.by_path.get(&format!("{base}{rest}"))?;
                                let fw = d.trace.vars[*fv].width.unwrap_or(0);
                                let fl = Look::of(&d.trace.vars[*fv]);
                                let r = match t.get("radix").and_then(Value::as_str) {
                                    // our writer always stores it; hand files fall back
                                    Some(r) => {
                                        match check_radix(r, fw, d.trace.vars[*fv].names.is_some())
                                        {
                                            Ok((r, frac)) => radix::name(r, frac, fw),
                                            Err(_) => radix::name(fl.radix, fl.frac, fw),
                                        }
                                    }
                                    None => radix::name(fl.radix, fl.frac, fw),
                                };
                                let r = if fw > 1 {
                                    format!(":{r}")
                                } else {
                                    String::new()
                                };
                                let c = t
                                    .get("color")
                                    .and_then(Value::as_str)
                                    .and_then(|c| rows::color_named(c).ok())
                                    .map(|c| format!(":{}", rows::COLORS[c]))
                                    .unwrap_or_default();
                                Some(format!("{name}{r}{c}"))
                            };
                            let list: Vec<String> = a.iter().filter_map(one).collect();
                            if list.is_empty() {
                                String::new()
                            } else {
                                format!("  (fields: {})", list.join(", "))
                            }
                        }
                        _ => String::new(),
                    };
                    format!("  {}{}{}{}{}", s.signal, radix, color, bits, fields)
                }
            };
            *out += &format!("{mark} {name}{extra}\n");
        }
        model::Row::Group(g) => {
            let mark = if flat == d.sel { ">" } else { " " };
            flat += 1;
            // names arrive pre-indented (`display_names`); use them verbatim
            let name = names.next().map(String::as_str).unwrap_or("?");
            let extra = if g
                .collapsed
                .as_ref()
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "  (collapsed)"
            } else {
                ""
            };
            *out += &format!("{mark} {name}{extra}\n");
            if let Some(Value::Array(members)) = g.rows.as_ref() {
                for m in members {
                    // untagged dispatch never fails (`Other`); garbage members stay silent
                    if let Ok(sub) = model::Row::deserialize(m.clone()) {
                        flat = describe_row(d, &sub, flat, names, out);
                    }
                }
            }
        }
        // apply skips non-table entries too
        model::Row::Other(_) => {}
    }
    flat
}

/// Write through a temporary file and a rename, so a failed save leaves the old file whole.
pub fn write(path: &Path, text: &str) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let r = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map_err(|e| format!("{}: {e}", path.display()))
}
