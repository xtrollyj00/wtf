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
        // a colour comes last (`awaddr:hex:red`, `wvalid:red`); its names are no radix's
        let (spec, color) = match spec.rsplit_once(':') {
            Some((n, c)) if rows::COLORS.contains(&c) => (n, Some(c)),
            _ => (spec, None),
        };
        let (name, radix) = with_radix(spec);
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
        if let Some(r) = radix {
            match radix_for(r, app.trace.vars[var].width.unwrap_or(0), app.trace.vars[var].names.is_some(), name) {
                Ok((r, frac)) => _ = row.insert("radix".into(), Value::String(radix::name(r, frac, app.trace.vars[var].width.unwrap_or(0)))),
                Err(e) => errors.push(e),
            }
        }
        if let Some(c) = color {
            row.insert("color".into(), Value::String(c.into()));
        }
        Some(Value::Table(row))
    };
    let mut rows = Vec::new();
    for spec in &a.rows {
        match spec {
            RowSpec::Signal(n) => rows.extend(signal(n)),
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
    let exact = |t: Tick| Value::String(time::format_exact(t, app.trace.fs_per_tick));
    let time_of = |s: &str, errors: &mut Vec<String>| at(app, s).map_err(|e| errors.push(format!("{s}: {e}"))).ok();
    let mut cursors = Table::new();
    for (n, t) in &a.cursors {
        if !matches!(n.parse::<u8>(), Ok(1..=9)) {
            errors.push(format!("cursors: no cursor {n} (1-9)"));
        } else if let Some(t) = time_of(t, &mut errors) {
            cursors.insert(n.clone(), exact(t));
        }
    }
    let (from, to) = match &a.range {
        Some([f, t]) => (time_of(f, &mut errors).unwrap_or(0), time_of(t, &mut errors).unwrap_or(0)),
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

