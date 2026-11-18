//! Session files (spec §11): loading, saving, and the layout `[+]` compares.

use crate::radix;
use crate::rows::{self, Look, Row, Sig};
use crate::time;
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
    Unknown(String),
    Misfit(String),
}

/// The `radix` rule the session rows and `declare` share: the name parses and fits the var. Each
/// side formats its own message.
pub fn check_radix(name: &str, width: u32, named: bool) -> Result<(usize, u32), RadixErr> {
    match radix::parse_name(name) {
        None => Err(RadixErr::Unknown(name.into())),
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

/// The file plus what misshapen sections it held: those read as missing. `rows` and `trail` warn
/// where they sit (a `trail` that isn't a list leaves the steps as they are; a list, even an empty
/// one, replaces them), scope and unit warn, the other sections are left out without a word, so
/// the typed file only holds valid shapes.
struct Shape {
    scope_bad: bool,
    unit_bad: bool,
    rows_bad: bool,
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
        rows_bad: false,
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
    shape.rows_bad = strip(&mut t, "rows", Value::is_array);
    shape.trail_bad = strip(&mut t, "trail", Value::is_array);
    for key in ["signal", "names", "protocol", "decode"] {
        strip(&mut t, key, Value::is_array);
    }
    for key in ["view", "cursors"] {
        strip(&mut t, key, Value::is_table);
    }
    let file: File = Value::Table(t)
        .try_into()
        .map_err(|e: toml::de::Error| format!("session: {}", e.message()))?;
    Ok((file, shape))
}

/// Apply a session to a freshly opened app. Returns warnings for the status line.
pub fn apply(app: &mut Workspace, text: &str) -> Result<Vec<String>, String> {
    let (file, shape) = parse(text)?;
    let mut warn = Vec::new();

    // the scope first: the names after it resolve under it
    if shape.scope_bad {
        warn.push("scope must be a string".into());
    } else if let Some(scope) = file.scope
        && let Err(e) = app.set_scope(&scope)
    {
        warn.push(e);
    }
    // the agent's time unit (`:unit`)
    if shape.unit_bad {
        warn.push("unit must be a string".into());
    } else if let Some(unit) = file.unit.as_deref() {
        match time::unit_named(unit) {
            Ok(u) => app.unit = Some(u),
            Err(e) => warn.push(e),
        }
    }
    // named signals first: rows may show them
    for s in file.signal {
        unknown("signal.", &s.rest, &mut warn);
        let (name, expr) = (
            s.name.as_ref().and_then(Value::as_str),
            s.expr.as_ref().and_then(Value::as_str),
        );
        match (name, expr) {
            (Some(n), Some(e)) => app
                .define(n, e)
                .map(drop)
                .unwrap_or_else(|e| warn.push(format!("signal {n}: {e}"))),
            _ => warn.push("signal: needs a name and an expr".into()),
        }
    }
    // names for values (`:names`), before rows pick their radix
    for n in file.names {
        unknown("names.", &n.rest, &mut warn);
        match (
            n.signal.as_ref().and_then(Value::as_str),
            n.values.as_ref().and_then(Value::as_str),
        ) {
            (Some(sig), Some(v)) => app
                .name_values(sig, v)
                .unwrap_or_else(|e| warn.push(format!("names {sig}: {e}"))),
            _ => warn.push("names: needs a signal and values".into()),
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
    for p in file.protocol {
        unknown("protocol.", &p.rest, &mut warn);
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
                "protocol {}: a protocol has no clock; its [[decode]] gives it",
                text(&p.name).unwrap_or_default()
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
            _ => warn.push(
                "protocol: needs a name, roles as a list, a when and fields as a table".into(),
            ),
        }
    }
    for d in file.decode {
        unknown("decode.", &d.rest, &mut warn);
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
            _ => warn.push("decode: needs a name, a protocol, signals and params as tables".into()),
        }
    }
    if shape.rows_bad {
        warn.push("rows: must be a list".into());
    }
    let mut missing = Vec::new();
    let mut rows = Vec::new();
    // the rows read back through the value they serialize to, so `rows_of` keeps its shape
    let rv = Value::try_from(file.rows).unwrap_or(Value::Array(Vec::new()));
    rows_of(app, rv, false, &mut rows, &mut missing, &mut warn);
    // groups whose signals are all missing
    rows::tidy(&mut rows);
    let vars: Vec<usize> = rows.iter().filter_map(Row::sig).map(|s| s.var).collect();
    app.trace.load(&vars);
    app.rows = rows;
    app.sel = 0;
    if !missing.is_empty() {
        warn.push(format!("not in trace: {}", missing.join(", ")));
    }

    let fpt = app.trace.fs_per_tick;
    let end = app.trace.end();
    let time = |v: &Value, warn: &mut Vec<String>| -> Option<u64> {
        let r = match v {
            Value::String(s) => crate::time::parse(s, fpt).map_err(String::from),
            Value::Integer(i) => Ok(*i as f64),
            _ => Err("bad time".into()),
        };
        r.map_err(|e| warn.push(e))
            .ok()
            .map(|x| (x.round() as u64).min(end))
    };
    let view = file.view;
    if let Some(z) = view.zoom {
        match z.as_str().map(|s| crate::time::parse(s, fpt)) {
            Some(Ok(z)) if z > 0.0 => app.view.set_zoom(z),
            _ => warn.push("view.zoom: bad time".into()),
        }
    }
    if let Some(s) = view.time.and_then(|v| time(&v, &mut warn)) {
        app.view.start = s;
        app.view.pending_fit = false;
    }
    // `to`: the end of the visible range, whatever the screen's width (sent by `present`)
    if let Some(to) = view.to.and_then(|v| time(&v, &mut warn)) {
        app.view.show_until(to);
    }
    unknown("view.", &view.rest, &mut warn);
    app.view.cursor = 0;
    for (k, v) in file.cursors {
        match k.parse::<usize>() {
            Ok(1) if v.is_table() => warn
                .push("cursors.1: the cursor moves and has no name or note; give it a time".into()),
            Ok(1) => app.view.cursor = time(&v, &mut warn).unwrap_or(0),
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
                        Some(t) => match time(&t, &mut warn) {
                            Some(t) => {
                                app.put_mark(n - 1, t);
                                app.label_mark(n - 1, &name, &note);
                            }
                            // a bad time clears the mark, as assigning None did; a name
                            // without a time has nowhere to live — the writer only writes put marks
                            None => {
                                app.remove_mark(Some(n - 1)).ok();
                            }
                        },
                        None => warn.push(format!("cursors.{k}: needs a time")),
                    }
                    unknown(&format!("cursors.{k}."), &m, &mut warn);
                }
                v => match time(&v, &mut warn) {
                    Some(t) => app.put_mark(n - 1, t),
                    None => {
                        app.remove_mark(Some(n - 1)).ok();
                    }
                },
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
        app.trail = trail;
    }
    unknown("", &file.rest, &mut warn);
    // after the marks: `[+]` compares them too
    app.saved = layout_toml(app);
    Ok(warn)
}

fn rows_of(
    app: &mut Workspace,
    rows: Value,
    grouped: bool,
    out: &mut Vec<Row>,
    missing: &mut Vec<String>,
    warn: &mut Vec<String>,
) {
    let Value::Array(rows) = rows else {
        warn.push("rows: must be a list".into());
        return;
    };
    for row in rows {
        let Value::Table(mut row) = row else { continue };
        if let Some(Value::String(path)) = row.remove("signal") {
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
            match app.var_named(&path) {
                Some(var) if !out.iter().any(|r| r.sig().is_some_and(|s| s.var == var)) => {
                    let look = look_keys(
                        app,
                        &mut row,
                        var,
                        &path,
                        Look::of(&app.trace.vars[var]),
                        warn,
                    );
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
                rows_of(app, sub, true, out, missing, warn);
            }
        } else {
            warn.push("rows: entry without signal or group".into());
        }
        unknown("rows.", &row, warn);
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
            Err(RadixErr::Unknown(n)) => warn.push(format!("unknown radix: {n}")),
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
        unknown("rows.fields.", &f, warn);
    }
}

fn unknown(prefix: &str, t: &Table, warn: &mut Vec<String>) {
    warn.extend(t.keys().map(|k| format!("unknown key: {prefix}{k}")));
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
    if let Ok((file, _)) = parse(text) {
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
    for s in steps {
        unknown("trail.", &s.rest, warn);
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
            _ => warn.push("trail: a step needs a tool, an ask and an answer".into()),
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
