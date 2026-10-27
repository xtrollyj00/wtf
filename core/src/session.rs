//! Session files (spec §11): loading, saving, and the layout `[+]` compares.

use crate::workspace::Workspace;
use crate::time;
use std::path::Path;
use crate::radix;
use crate::rows::{self, Row, Sig};
use toml::{Table, Value};

pub const VERSION: i64 = 1;

/// The default session file next to the trace: `run1.fst` -> `run1.fst.session.toml`.
pub fn default_path(trace: &std::path::Path) -> std::path::PathBuf {
    let mut p = trace.as_os_str().to_owned();
    p.push(".session.toml");
    p.into()
}

/// Apply a session to a freshly opened app. Returns warnings for the status line.
pub fn apply(app: &mut Workspace, text: &str) -> Result<Vec<String>, String> {
    let mut t: Table = text.parse().map_err(|e: toml::de::Error| format!("session: {}", e.message()))?;
    let mut warn = Vec::new();
    let version = t.remove("version").map_or(Ok(VERSION), |v| v.as_integer().ok_or("session: version must be a number"))?;
    if version > VERSION {
        return Err(format!("session version {version} is newer than this wtf supports ({VERSION})"));
    }

    // the scope first: the names after it resolve under it
    if let Some(scope) = t.remove("scope") {
        match scope.as_str().map(|s| app.set_scope(s)) {
            Some(Ok(())) => {}
            Some(Err(e)) => warn.push(e),
            None => warn.push("scope must be a string".into()),
        }
    }
    // the agent's time unit (`:unit`)
    if let Some(unit) = t.remove("unit") {
        match unit.as_str().map(time::unit_named) {
            Some(Ok(u)) => app.unit = Some(u),
            Some(Err(e)) => warn.push(e),
            None => warn.push("unit must be a string".into()),
        }
    }
    // named signals first: rows may show them
    if let Some(Value::Array(signals)) = t.remove("signal") {
        for s in signals {
            let (name, expr) = (s.get("name").and_then(Value::as_str), s.get("expr").and_then(Value::as_str));
            match (name, expr) {
                (Some(n), Some(e)) => app.define(n, e).map(drop).unwrap_or_else(|e| warn.push(format!("signal {n}: {e}"))),
                _ => warn.push("signal: needs a name and an expr".into()),
            }
        }
    }
    // names for values (`:names`), before rows pick their radix
    if let Some(Value::Array(names)) = t.remove("names") {
        for n in names {
            match (n.get("signal").and_then(Value::as_str), n.get("values").and_then(Value::as_str)) {
                (Some(sig), Some(v)) => app.name_values(sig, v).unwrap_or_else(|e| warn.push(format!("names {sig}: {e}"))),
                _ => warn.push("names: needs a signal and values".into()),
            }
        }
    }
    // protocols and their instances (spec §16), after the signals they may use
    let text = |e: &Value, k: &str| e.get(k).and_then(Value::as_str).map(String::from);
    // a table of names (`fields`, `signals`), in its order; missing is empty
    let pairs = |e: &Value, k: &str| match e.get(k) {
        None => Some(Vec::new()),
        Some(v) => v.as_table().map(|f| f.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())).collect()),
    };
    if let Some(Value::Array(protocols)) = t.remove("protocol") {
        for p in protocols {
            let roles = match p.get("roles") {
                None => Some(Vec::new()),
                Some(r) => r.as_array().map(|r| r.iter().map(|r| r.as_str().unwrap_or_default().to_string()).collect::<Vec<_>>()),
            };
            // a type's instances may give the clock
            let clock = text(&p, "clock").or(roles.as_ref().filter(|r| !r.is_empty()).map(|_| String::new()));
            match (text(&p, "name"), clock, text(&p, "when"), pairs(&p, "fields"), roles) {
                (Some(name), Some(clock), Some(when), Some(fields), Some(roles)) => {
                    let def = crate::stream::Sampler { name: name.clone(), clock, when, fields, roles };
                    app.add_protocol(def).map(drop).unwrap_or_else(|e| warn.push(format!("protocol {name}: {e}")));
                }
                _ => warn.push("protocol: needs a name, a clock (or roles), a when, fields as a table and roles as a list".into()),
            }
        }
    }
    if let Some(Value::Array(decodes)) = t.remove("decode") {
        for d in decodes {
            match (text(&d, "name"), text(&d, "protocol"), pairs(&d, "signals"), pairs(&d, "params")) {
                (Some(name), Some(protocol), Some(signals), Some(params)) => {
                    let clock = text(&d, "clock").unwrap_or_default();
                    app.add_decode(crate::stream::Decode { name, protocol, clock, signals, params }).map(drop).unwrap_or_else(|e| warn.push(format!("decode {e}")));
                }
                _ => warn.push("decode: needs a name, a protocol, signals and params as tables".into()),
            }
        }
    }
    let mut missing = Vec::new();
    let mut rows = Vec::new();
    if let Some(r) = t.remove("rows") {
        rows_of(app, r, false, &mut rows, &mut missing, &mut warn);
    }
    // groups whose signals are all missing
    rows::tidy(&mut rows);
    let vars: Vec<usize> = rows.iter().filter_map(Row::sig).map(|s| s.var).collect();
    app.trace.load(&vars);
    app.rows = rows;
    app.sel = 0;
    app.saved = rows_toml(app);
    if !missing.is_empty() {
        warn.push(format!("not in trace: {}", missing.join(", ")));
    }

    let fpt = app.trace.fs_per_tick;
    let end = app.trace.end();
    let time = |v: &Value, warn: &mut Vec<String>| -> Option<u64> {
        let r = match v {
            Value::String(s) => crate::time::parse(s, fpt),
            Value::Integer(i) => Ok(*i as f64),
            _ => Err("bad time".into()),
        };
        r.map_err(|e| warn.push(e)).ok().map(|x| (x.round() as u64).min(end))
    };
    if let Some(Value::Table(mut view)) = t.remove("view") {
        if let Some(z) = view.remove("zoom") {
            match z.as_str().map(|s| crate::time::parse(s, fpt)) {
                Some(Ok(z)) if z > 0.0 => app.view.set_zoom(z),
                _ => warn.push("view.zoom: bad time".into()),
            }
        }
        if let Some(s) = view.remove("time").and_then(|v| time(&v, &mut warn)) {
            app.view.start = s;
            app.view.pending_fit = false;
        }
        // `to`: the end of the visible range, whatever the screen's width (sent by `present`)
        if let Some(to) = view.remove("to").and_then(|v| time(&v, &mut warn)) {
            app.view.show_until(to);
        }
        // written by wtf before cursors 2 - 9 became marks (spec §9)
        view.remove("active_cursor");
        unknown("view.", &view, &mut warn);
    }
    app.view.cursor = 0;
    if let Some(Value::Table(cursors)) = t.remove("cursors") {
        for (k, v) in cursors {
            match k.parse::<usize>() {
                Ok(1) => app.view.cursor = time(&v, &mut warn).unwrap_or(0),
                Ok(n @ 2..=9) => app.marks[n - 1] = time(&v, &mut warn),
                _ => warn.push(format!("cursors: no cursor {k} (1-9)")),
            }
        }
    }
    unknown("", &t, &mut warn);
    Ok(warn)
}

fn rows_of(app: &mut Workspace, rows: Value, grouped: bool, out: &mut Vec<Row>, missing: &mut Vec<String>, warn: &mut Vec<String>) {
    let Value::Array(rows) = rows else {
        warn.push("rows: must be a list".into());
        return;
    };
    for row in rows {
        let Value::Table(mut row) = row else { continue };
        if let Some(Value::String(path)) = row.remove("signal") {
            let radix = row.remove("radix").and_then(|r| {
                let name = r.as_str().unwrap_or("");
                radix::parse_name(name).or_else(|| {
                    warn.push(format!("unknown radix: {name}"));
                    None
                })
            });
            let expanded = row.remove("expanded").and_then(|e| e.as_bool()).unwrap_or(false);
            let color = row.remove("color").and_then(|c| rows::color_named(c.as_str().unwrap_or("")).map_err(|e| warn.push(e)).ok());
            match app.var_named(&path) {
                Some(var) if !out.iter().any(|r| r.sig().is_some_and(|s| s.var == var)) => {
                    let width = app.trace.vars[var].width.unwrap_or(0);
                    let named = app.trace.vars[var].names.is_some();
                    let (radix, frac) = match radix {
                        Some((r, frac, need)) if radix::fits(r, width, named) && need.is_none_or(|n| n == width) => (r, frac),
                        Some((r, frac, need)) => {
                            warn.push(format!("radix {} doesn't fit {path}", radix::name(r, frac, need.unwrap_or(width))));
                            (radix::default_for(named, app.trace.vars[var].signed), 0)
                        }
                        None => (radix::default_for(named, app.trace.vars[var].signed), 0),
                    };
                    let bits = if expanded { app.sub_lines(var) } else { 0 };
                    out.push(Row::Signal(Sig { var, radix, frac, grouped, bits, color }));
                }
                Some(_) => {}
                None => missing.push(path),
            }
        } else if let Some(Value::String(name)) = row.remove("group") {
            let collapsed = row.remove("collapsed").and_then(|c| c.as_bool()).unwrap_or(false);
            // shortcut: groups have one level until subgroups exist (spec §18); nested ones are flattened.
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

fn unknown(prefix: &str, t: &Table, warn: &mut Vec<String>) {
    warn.extend(t.keys().map(|k| format!("unknown key: {prefix}{k}")));
}

fn quote(s: &str) -> String {
    Value::String(s.into()).to_string()
}

/// A signal's keys: its path, and its radix and expansion where they differ from the defaults.
fn signal_keys(app: &Workspace, s: &Sig) -> Vec<String> {
    let var = &app.trace.vars[s.var];
    let mut keys = vec![format!("signal = {}", quote(&var.path))];
    if var.width != Some(1) && s.radix != radix::default_for(var.names.is_some(), var.signed) {
        keys.push(format!("radix = {}", quote(&radix::name(s.radix, s.frac, var.width.unwrap_or(0)))));
    }
    if let Some(c) = s.color {
        keys.push(format!("color = {}", quote(rows::COLORS[c])));
    }
    if s.bits > 0 {
        keys.push("expanded = true".into());
    }
    keys
}

/// The layout part of a session. `[+]` compares it with the one last saved or loaded; it names
/// signals by path, so it still compares after `:e` reopens the trace.
pub fn rows_toml(app: &Workspace) -> String {
    let rows = &app.rows;
    let mut out = String::new();
    for (name, expr) in &app.signals {
        out += &format!("\n[[signal]]\nname = {}\nexpr = {}\n", quote(name), quote(expr));
    }
    for (path, spec) in &app.value_names {
        out += &format!("\n[[names]]\nsignal = {}\nvalues = {}\n", quote(path), quote(spec));
    }
    let table = |pairs: &[(String, String)]| pairs.iter().map(|(k, v)| format!("{k} = {}", quote(v))).collect::<Vec<_>>().join(", ");
    for p in &app.protocols {
        out += &format!("\n[[protocol]]\nname = {}\n", quote(&p.name));
        if !p.roles.is_empty() {
            out += &format!("roles = [{}]\n", p.roles.iter().map(|r| quote(r)).collect::<Vec<_>>().join(", "));
        }
        if !p.clock.is_empty() {
            out += &format!("clock = {}\n", quote(&p.clock));
        }
        out += &format!("when = {}\nfields = {{ {} }}\n", quote(&p.when), table(&p.fields));
    }
    for d in &app.decodes {
        out += &format!("\n[[decode]]\nname = {}\nprotocol = {}\n", quote(&d.name), quote(&d.protocol));
        if !d.clock.is_empty() {
            out += &format!("clock = {}\n", quote(&d.clock));
        }
        if !d.signals.is_empty() {
            out += &format!("signals = {{ {} }}\n", table(&d.signals));
        }
        if !d.params.is_empty() {
            out += &format!("params = {{ {} }}\n", table(&d.params));
        }
    }
    let mut i = 0;
    while i < rows.len() {
        out += "\n[[rows]]\n";
        match &rows[i] {
            Row::Group { name, collapsed } => {
                let end = rows::group_end(rows, i);
                out += &format!("group = {}\n", quote(name));
                if *collapsed {
                    out += "collapsed = true\n";
                }
                out += "rows = [\n";
                for s in rows[i + 1..end].iter().filter_map(Row::sig) {
                    out += &format!("  {{ {} }},\n", signal_keys(app, s).join(", "));
                }
                out += "]\n";
                i = end;
            }
            Row::Signal(s) => {
                out += &signal_keys(app, s).join("\n");
                out += "\n";
                i += 1;
            }
        }
    }
    out
}

/// The whole session: view, cursors and layout.
pub fn to_toml(app: &Workspace) -> String {
    let fpt = app.trace.fs_per_tick;
    let v = &app.view;
    // below one tick per column the zoom is a fraction of a tick, written as plain ticks
    let zoom = if v.cols == 1 { time::format_exact(v.ticks, fpt) } else { (1.0 / v.cols as f64).to_string() };
    let cursors: String =
        app.cursor_list().iter().map(|&(i, t)| format!("{} = {}\n", i + 1, quote(&time::format_exact(t, fpt)))).collect();
    format!(
        "# wtf session\nversion = {VERSION}\n{}{}\n[view]\nzoom = {}\ntime = {}\n\n[cursors]\n{cursors}{}",
        app.scope.as_ref().map_or(String::new(), |s| format!("scope = {}\n", quote(s))),
        app.unit.map_or(String::new(), |u| format!("unit = {}\n", quote(time::unit_name(u)))),
        quote(&zoom),
        quote(&time::format_exact(v.start, fpt)),
        rows_toml(app)
    )
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
