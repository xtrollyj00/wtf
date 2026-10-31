//! The `workspace` tool's declaration, checked and turned into session TOML (spec §15).

use super::*;

/// The session TOML for a `workspace` declaration. Every name, radix and time is checked first, and
/// all the errors come back at once, so one retry can fix them.
pub(super) fn declared(app: &mut Workspace, a: &WorkspaceArgs) -> Result<String, String> {
    let scope = app.scope.clone();
    if let Some(s) = &a.scope {
        app.set_scope(s)?;
    }
    let text = declare(app, a);
    if text.is_err() {
        app.scope = scope;
    }
    text
}

fn declare(app: &mut Workspace, a: &WorkspaceArgs) -> Result<String, String> {
    use toml::{Table, Value};
    let mut errors = Vec::new();
    let mut signals = Vec::new();
    for s in &a.signals {
        let Some((name, expr)) = s.split_once('=') else {
            errors.push(format!("signals: {s}: not `name = expr`"));
            continue;
        };
        match app.define(name, expr) {
            Ok(_) => signals.push(Value::Table(Table::from_iter([("name".into(), name.trim().into()), ("expr".into(), expr.trim().into())]))),
            Err(e) => errors.push(format!("signal {}: {e}", name.trim())),
        }
    }
    // `"name = signal"` lists, as (name, signal)
    let split = |what: &str, list: &[String], errors: &mut Vec<String>| -> Vec<(String, String)> {
        list.iter()
            .filter_map(|f| match f.split_once('=') {
                Some((f, sig)) => Some((f.trim().to_string(), sig.trim().to_string())),
                None => {
                    errors.push(format!("{what}: {f}: not `name = signal`"));
                    None
                }
            })
            .collect()
    };
    let table = |pairs: &[(String, String)]| Value::Table(pairs.iter().map(|(f, s)| (f.clone(), Value::from(s.clone()))).collect());
    let mut protocols = Vec::new();
    for p in &a.protocols {
        let fields = split(&format!("protocol {}", p.name), &p.fields, &mut errors);
        let def = wtf_core::stream::Sampler { name: p.name.clone(), clock: p.clock.clone(), when: p.when.clone(), fields, roles: p.roles.clone() };
        let mut t = Table::from_iter([("name".into(), Value::from(def.name.clone())), ("when".into(), def.when.clone().into()), ("fields".into(), table(&def.fields))]);
        if !def.clock.is_empty() {
            t.insert("clock".into(), def.clock.clone().into());
        }
        if !def.roles.is_empty() {
            t.insert("roles".into(), Value::Array(def.roles.iter().map(|r| Value::from(r.clone())).collect()));
        }
        match app.add_protocol(def) {
            Ok(_) => protocols.push(Value::Table(t)),
            Err(e) => errors.push(format!("protocol {}: {e}", p.name)),
        }
    }
    let mut decodes = Vec::new();
    for d in &a.decode {
        let signals = split(&format!("decode {}", d.name), &d.signals, &mut errors);
        let params = split(&format!("decode {}", d.name), &d.params, &mut errors);
        let def = wtf_core::stream::Decode { name: d.name.clone(), protocol: d.protocol.clone(), clock: d.clock.clone(), signals, params };
        let mut t = Table::from_iter([("name".into(), Value::from(def.name.clone())), ("protocol".into(), def.protocol.clone().into()), ("signals".into(), table(&def.signals))]);
        if !def.params.is_empty() {
            t.insert("params".into(), table(&def.params));
        }
        if !def.clock.is_empty() {
            t.insert("clock".into(), def.clock.clone().into());
        }
        match app.add_decode(def) {
            Ok(_) => decodes.push(Value::Table(t)),
            Err(e) => errors.push(format!("decode {e}")),
        }
    }
    let mut signal = |spec: &str| -> Option<Value> {
        let (name, radix, color) = look_spec(spec);
        let var = match app.resolve(name) {
            Ok((v, None)) => v,
            Ok(_) => {
                errors.push(format!("{name}: a bit can't be a row; add its bus"));
                return None;
            }
            Err(e) => {
                errors.push(e);
                return None;
            }
        };
        let mut row = Table::new();
        row.insert("signal".into(), Value::String(app.trace.vars[var].path.clone()));
        look_keys(app, var, name, radix, color, &mut row, &mut errors);
        Some(Value::Table(row))
    };
    let (mut rows, mut opens, mut fields_of) = (Vec::new(), Vec::new(), Vec::new());
    for spec in &a.rows {
        match spec {
            RowSpec::Signal(n) => rows.extend(signal(n)),
            RowSpec::Open { signal: n, open, fields } => {
                let Some(Value::Table(mut row)) = signal(n) else { continue };
                opens.push((n.clone(), row["signal"].as_str().unwrap_or_default().to_string(), open.clone()));
                if !fields.is_empty() {
                    fields_of.push((rows.len(), n.clone(), row["signal"].as_str().unwrap_or_default().to_string(), fields.clone()));
                }
                row.insert("open".into(), Value::Array(open.iter().map(|o| Value::String(o.clone())).collect()));
                rows.push(Value::Table(row));
            }
            RowSpec::Group { group, rows: members, collapsed } => {
                let mut g = Table::new();
                g.insert("group".into(), Value::String(group.clone()));
                if *collapsed {
                    g.insert("collapsed".into(), Value::Boolean(true));
                }
                g.insert("rows".into(), Value::Array(members.iter().filter_map(|n| signal(n)).collect()));
                rows.push(Value::Table(g));
            }
        }
    }
    // each field as a row is written, named as `open` names its levels
    for (k, n, path, fields) in fields_of {
        let var = app.trace.by_path[&path];
        let mut out = Vec::new();
        for spec in &fields {
            let (name, radix, color) = look_spec(spec);
            let Some(f) = session::open_var(app, var, name).filter(|&f| f != var) else {
                errors.push(format!("{n} has no field {name}; fields are named from {} on", session::open_name(app, var, var)));
                continue;
            };
            let mut t = Table::from_iter([("field".into(), Value::String(name.into()))]);
            look_keys(app, f, name, radix, color, &mut t, &mut errors);
            out.push(Value::Table(t));
        }
        if let Value::Table(row) = &mut rows[k] {
            row.insert("fields".into(), Value::Array(out));
        }
    }
    for (n, path, open) in opens {
        let var = app.trace.by_path[&path];
        for name in open.iter().filter(|o| session::open_var(app, var, o).is_none()) {
            errors.push(format!("{n} has no {name} to open; its levels are named from {} on", session::open_name(app, var, var)));
        }
    }
    let exact = |t: Tick| Value::String(time::format_exact(t, app.trace.fs_per_tick));
    let time_of = |s: &str, errors: &mut Vec<String>| at(app, s).map_err(|e| errors.push(format!("{s}: {e}"))).ok();
    let mut cursors = Table::new();
    for (n, t) in &a.cursors {
        if !matches!(n.parse::<u8>(), Ok(1..=9)) {
            errors.push(format!("cursors: no cursor {n} (1-9)"));
        } else if let Some((lo, hi)) = time_of(t, &mut errors) {
            // a cursor stays at the time written, not the end of its span (spec §15 "Times")
            cursors.insert(n.clone(), exact((lo + hi).div_ceil(2)));
        }
    }
    let (from, to) = match &a.range {
        Some([f, t]) => (time_of(f, &mut errors).map_or(0, |s| s.0), time_of(t, &mut errors).map_or(0, |s| s.1)),
        None => (app.trace.start(), app.trace.end().max(1)),
    };
    if a.range.is_some() && to <= from {
        errors.push("range: the end must be after the start".into());
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let mut view = Table::new();
    view.insert("time".into(), exact(from));
    view.insert("to".into(), exact(to));
    let mut t = Table::new();
    t.insert("version".into(), Value::Integer(session::VERSION));
    if let Some(s) = &app.scope {
        t.insert("scope".into(), s.clone().into());
    }
    t.insert("view".into(), Value::Table(view));
    t.insert("cursors".into(), Value::Table(cursors));
    t.insert("signal".into(), Value::Array(signals));
    t.insert("protocol".into(), Value::Array(protocols));
    t.insert("decode".into(), Value::Array(decodes));
    t.insert("rows".into(), Value::Array(rows));
    Ok(t.to_string())
}


/// A row's or field's spec: its name, radix and colour (`awaddr:hex:red`, `wvalid:red`). A colour
/// comes last; its names are no radix's.
fn look_spec(spec: &str) -> (&str, Option<&str>, Option<&str>) {
    let (spec, color) = match spec.rsplit_once(':') {
        Some((n, c)) if rows::COLORS.contains(&c) => (n, Some(c)),
        _ => (spec, None),
    };
    let (name, radix) = with_radix(spec);
    (name, radix, color)
}

/// A line's `radix` and `color` keys, the radix checked against `var`.
fn look_keys(app: &Workspace, var: usize, name: &str, radix: Option<&str>, color: Option<&str>, t: &mut toml::Table, errors: &mut Vec<String>) {
    use toml::Value;
    let v = &app.trace.vars[var];
    if let Some(r) = radix {
        match radix_for(r, v.width.unwrap_or(0), v.names.is_some(), name) {
            Ok((r, frac)) => _ = t.insert("radix".into(), Value::String(radix::name(r, frac, v.width.unwrap_or(0)))),
            Err(e) => errors.push(e),
        }
    }
    if let Some(c) = color {
        t.insert("color".into(), Value::String(c.into()));
    }
}
