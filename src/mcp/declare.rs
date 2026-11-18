//! The `workspace` tool's declaration, checked and turned into session TOML (spec §15).

use super::*;
use wtf_core::rows;

/// The session TOML for a `workspace` declaration. Every name, radix, time and key is checked
/// first, and all the errors come back at once, so one retry can fix them; a declaration that
/// fails changes nothing (`Workspace::attempt`), so the retry starts clean.
pub(super) fn declared(app: &mut Workspace, a: &WorkspaceArgs) -> Result<String, String> {
    app.attempt(|app| declare(app, a))
}

fn declare(app: &mut Workspace, a: &WorkspaceArgs) -> Result<String, String> {
    use toml::{Table, Value};
    let mut errors = Vec::new();
    let f = &a.file;
    let mut file = session::File::default();
    // the scope first, so the names below resolve under it; a bad one is reported with the rest,
    // its nearest scopes listed (spec §15: every mistake at once)
    if let Some(s) = &f.scope
        && let Err(e) = app.set_scope(s)
    {
        errors.push(e);
    }
    // a TOML string, else nothing to check
    fn str_of(v: Option<&Value>) -> Option<&str> {
        match v {
            Some(Value::String(s)) => Some(s),
            _ => None,
        }
    }
    // entries the workspace doesn't take: the dead names say what took their place; `trail` and
    // `version` are the session file's own, not declared
    let own = [
        ("trail", !f.trail.is_empty()),
        ("version", f.version != session::VERSION),
    ];
    let unknown = own
        .iter()
        .filter(|o| o.1)
        .map(|o| o.0)
        .chain(f.rest.keys().map(String::as_str));
    for k in unknown {
        let hint = match k {
            "range" => "view: {\"time\": ..., \"to\": ...}",
            "signals" => "signal: [{\"name\": ..., \"expr\": ...}]",
            "protocols" => "protocol: [{\"name\": ...}]",
            _ => "signal, protocol, decode, names, rows, cursors, view, scope, unit",
        };
        errors.push(format!("unknown workspace entry: {k} (use {hint})"));
    }
    unknown_keys("view", &f.view.rest, &["time", "to"], &mut errors);
    for s in &f.signal {
        let (name, expr) = (str_of(s.name.as_ref()), str_of(s.expr.as_ref()));
        let what = name.map_or("signal".to_string(), |n| format!("signal {n}"));
        unknown_keys(&what, &s.rest, &["name", "expr"], &mut errors);
        match (name, expr) {
            (Some(n), Some(e)) => match app.define(n, e) {
                Ok(_) => file.signal.push(session::SignalEntry {
                    name: Some(Value::String(n.into())),
                    expr: Some(Value::String(e.into())),
                    ..Default::default()
                }),
                Err(e2) => errors.push(format!("signal {n}: {e2}")),
            },
            (None, _) => errors.push("signal: an entry needs a name string".into()),
            (Some(n), None) => errors.push(format!("signal: {n} needs an expr string")),
        }
    }
    for p in &f.protocol {
        let name = str_of(p.name.as_ref()).unwrap_or("");
        if name.is_empty() {
            errors.push("protocol: an entry needs a name string".into());
            continue;
        }
        unknown_keys(
            &format!("protocol {name}"),
            &p.rest,
            &["name", "when", "roles", "fields", "wasm"],
            &mut errors,
        );
        let mut roles = Vec::new();
        match &p.roles {
            Some(Value::Array(rs)) => {
                for r in rs {
                    match r {
                        Value::String(s) => roles.push(s.clone()),
                        _ => errors.push(format!("protocol {name}: a role is a string")),
                    }
                }
            }
            Some(_) => errors.push(format!(
                "protocol {name}: roles is an array of signal names"
            )),
            None => {}
        }
        let mut fields = Vec::new();
        match &p.fields {
            Some(Value::Array(_)) | Some(Value::Table(_)) => {
                fields = pair_list(
                    &format!("protocol {name}: field"),
                    p.fields.as_ref(),
                    &mut errors,
                )
            }
            Some(_) => errors.push(format!(
                "protocol {name}: fields maps field names to expressions"
            )),
            None => {}
        }
        if let Some(w) = str_of(p.wasm.as_ref()) {
            // from a file: the file's name is the protocol's, and the session keeps both (spec §11)
            let stem = std::path::Path::new(w)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem != name {
                errors.push(format!("wasm {w} holds {stem}, not {name}: name it {stem}"));
            } else if str_of(p.when.as_ref()).is_some() || !roles.is_empty() || !fields.is_empty() {
                errors.push(format!(
                    "protocol {name} from a file takes no roles, when or fields"
                ));
            } else {
                match app.loader.clone() {
                    Some(load) => match load(w) {
                        Ok(loaded) => match app.add_loaded(loaded, w) {
                            Ok(()) => file.protocol.push(session::Protocol {
                                name: Some(Value::String(name.into())),
                                wasm: Some(Value::String(w.into())),
                                ..Default::default()
                            }),
                            Err(e) => errors.push(format!("protocol {name}: {e}")),
                        },
                        Err(e) => errors.push(format!("{w}: {e}")),
                    },
                    None => errors.push(format!(
                        "{w}: this wtf has no host for protocols from files"
                    )),
                }
            }
            continue;
        }
        if p.clock.is_some() {
            errors.push(format!(
                "protocol {name} takes no clock: the decode's clock samples it"
            ));
            continue;
        }
        if p.when.is_some() && str_of(p.when.as_ref()).is_none() {
            errors.push(format!("protocol {name}: when is a string"));
            continue;
        }
        let Some(when) = str_of(p.when.as_ref()) else {
            errors.push(format!("protocol {name} needs a when expression"));
            continue;
        };
        let def = wtf_core::stream::Sampler {
            name: name.into(),
            when: when.into(),
            fields,
            roles,
            wasm: None,
        };
        let entry = session::Protocol {
            name: Some(Value::String(def.name.clone())),
            when: Some(Value::String(def.when.clone())),
            roles: Some(Value::Array(
                def.roles.iter().map(|r| Value::String(r.clone())).collect(),
            )),
            fields: Some(table_of(&def.fields)),
            ..Default::default()
        };
        match app.add_protocol(def) {
            Ok(()) => file.protocol.push(entry),
            Err(e) => errors.push(format!("protocol {name}: {e}")),
        }
    }
    for d in &f.decode {
        let name = str_of(d.name.as_ref()).unwrap_or("");
        if name.is_empty() {
            errors.push("decode: an entry needs a name string".into());
            continue;
        }
        unknown_keys(
            &format!("decode {name}"),
            &d.rest,
            &["name", "protocol", "clock", "signals", "params"],
            &mut errors,
        );
        let signals = pair_list(
            &format!("decode {name} signal:"),
            d.signals.as_ref(),
            &mut errors,
        );
        let params = pair_list(
            &format!("decode {name} param:"),
            d.params.as_ref(),
            &mut errors,
        );
        if d.protocol.is_some() && str_of(d.protocol.as_ref()).is_none() {
            errors.push(format!("decode {name}: protocol is a string"));
            continue;
        }
        let clock = match &d.clock {
            None => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => {
                errors.push(format!("decode {name}: clock is a string"));
                continue;
            }
        };
        let def = wtf_core::stream::Decode {
            name: name.into(),
            protocol: str_of(d.protocol.as_ref()).unwrap_or("").into(),
            clock,
            signals,
            params,
        };
        let mut entry = session::Decode {
            name: Some(Value::String(def.name.clone())),
            protocol: Some(Value::String(def.protocol.clone())),
            signals: Some(table_of(&def.signals)),
            ..Default::default()
        };
        if !def.params.is_empty() {
            entry.params = Some(table_of(&def.params));
        }
        if !def.clock.is_empty() {
            entry.clock = Some(Value::String(def.clock.clone()));
        }
        match app.add_decode(def) {
            Ok(_) => file.decode.push(entry),
            Err(e) => errors.push(format!("decode {e}")),
        }
    }
    for n in &f.names {
        let what = str_of(n.signal.as_ref()).map_or("names".to_string(), |s| format!("names {s}"));
        unknown_keys(&what, &n.rest, &["signal", "values"], &mut errors);
        match (str_of(n.signal.as_ref()), str_of(n.values.as_ref())) {
            (Some(s), Some(v)) => match app.name_values(s, v) {
                Ok(()) => file.names.push(n.clone()),
                Err(e) => errors.push(format!("names {s}: {e}")),
            },
            _ => errors.push("names: an entry needs signal and values strings".into()),
        }
    }
    // the unit applies with the rest, after every check (the file never keeps `auto`)
    let mut unit = None;
    if let Some(s) = &f.unit {
        match s.as_str() {
            "auto" => unit = Some(None),
            _ => match time::unit_named(s) {
                Ok(u) => unit = Some(Some(u)),
                Err(_) => errors.push(format!(
                    "unit: unknown unit: {s} (s, ms, us, ns, ps, fs or auto)"
                )),
            },
        }
    }
    let mut rows = Vec::new();
    for r in &f.rows {
        match r {
            session::model::Row::Signal(s) => {
                unknown_keys(
                    &s.signal,
                    &s.rest,
                    &["signal", "radix", "color", "open", "fields"],
                    &mut errors,
                );
                let radix = opt_string(s.radix.as_ref(), &s.signal, "radix", &mut errors);
                let color = opt_string(s.color.as_ref(), &s.signal, "color", &mut errors);
                let Some((mut row, var)) = signal_row(
                    app,
                    &mut errors,
                    &s.signal,
                    radix.as_deref(),
                    color.as_deref(),
                ) else {
                    continue;
                };
                if let Some(o) = &s.open {
                    match o {
                        Value::Array(levels) => {
                            let mut open = Vec::new();
                            for l in levels {
                                match l {
                                    Value::String(x) => open.push(x.clone()),
                                    _ => errors
                                        .push(format!("{}: an open level is a name", s.signal)),
                                }
                            }
                            for name in &open {
                                if session::open_var(app, var, name).is_none() {
                                    errors.push(format!(
                                        "{} has no {name} to open; its levels are named from {} on",
                                        s.signal,
                                        session::open_name(app, var, var)
                                    ));
                                }
                            }
                            row.open =
                                Some(Value::Array(open.into_iter().map(Value::String).collect()));
                        }
                        _ => errors.push(format!("{}: open is an array of level names", s.signal)),
                    }
                }
                if let Some(fv) = &s.fields {
                    match fv {
                        Value::Array(fs) => {
                            let mut out = Vec::new();
                            for item in fs {
                                let Value::Table(ft) = item else {
                                    errors.push(format!(
                                        "{}: a field is {{field, radix?, color?}}",
                                        s.signal
                                    ));
                                    continue;
                                };
                                let Some(Value::String(fname)) = ft.get("field") else {
                                    errors
                                        .push(format!("{}: a field needs a field name", s.signal));
                                    continue;
                                };
                                unknown_keys(fname, ft, &["field", "radix", "color"], &mut errors);
                                let fr = opt_string(ft.get("radix"), fname, "radix", &mut errors);
                                let fc = opt_string(ft.get("color"), fname, "color", &mut errors);
                                let Some(fvar) =
                                    session::open_var(app, var, fname).filter(|&fvar| fvar != var)
                                else {
                                    errors.push(format!(
                                        "{} has no field {fname}; fields are named from {} on",
                                        s.signal,
                                        session::open_name(app, var, var)
                                    ));
                                    continue;
                                };
                                let mut t = Table::from_iter([(
                                    "field".into(),
                                    Value::String(fname.into()),
                                )]);
                                let (rr, cc) = look_vals(
                                    app,
                                    fvar,
                                    fname,
                                    fr.as_deref(),
                                    fc.as_deref(),
                                    &mut errors,
                                );
                                if let Some(rr) = rr {
                                    t.insert("radix".into(), rr);
                                }
                                if let Some(cc) = cc {
                                    t.insert("color".into(), cc);
                                }
                                out.push(Value::Table(t));
                            }
                            row.fields = Some(Value::Array(out));
                        }
                        _ => {
                            errors.push(format!("{}: fields is an array of field tables", s.signal))
                        }
                    }
                }
                rows.push(session::model::Row::Signal(row));
            }
            session::model::Row::Group(g) => {
                unknown_keys(
                    &format!("group {}", g.group),
                    &g.rest,
                    &["group", "collapsed", "rows"],
                    &mut errors,
                );
                let members = match &g.rows {
                    Some(Value::Array(ms)) => ms
                        .iter()
                        .filter_map(|m| match m {
                            Value::Table(t) => {
                                let name = match t.get("signal") {
                                    Some(Value::String(s)) => s.clone(),
                                    _ => {
                                        errors.push(format!(
                                            "group {}: a row needs a signal",
                                            g.group
                                        ));
                                        return None;
                                    }
                                };
                                let what = format!("group {}", g.group);
                                unknown_keys(&what, t, &["signal", "radix", "color"], &mut errors);
                                let radix = opt_string(t.get("radix"), &what, "radix", &mut errors);
                                let color = opt_string(t.get("color"), &what, "color", &mut errors);
                                signal_row(
                                    app,
                                    &mut errors,
                                    &name,
                                    radix.as_deref(),
                                    color.as_deref(),
                                )
                                .map(|(row, _)| {
                                    Value::try_from(session::model::Row::Signal(row))
                                        .unwrap_or(Value::Table(Table::new()))
                                })
                            }
                            _ => {
                                errors.push(format!(
                                    "group {}: a row is {{signal, radix?, color?}}",
                                    g.group
                                ));
                                None
                            }
                        })
                        .collect::<Vec<_>>(),
                    Some(_) => {
                        errors.push(format!("group {}: rows is an array of rows", g.group));
                        Vec::new()
                    }
                    None => Vec::new(),
                };
                let mut gr = session::GroupRow {
                    group: g.group.clone(),
                    ..Default::default()
                };
                if let Some(c) = &g.collapsed {
                    match c {
                        Value::Boolean(b) => gr.collapsed = Some(Value::Boolean(*b)),
                        _ => errors.push(format!("group {}: collapsed is true or false", g.group)),
                    }
                }
                gr.rows = Some(Value::Array(members));
                rows.push(session::model::Row::Group(gr));
            }
            session::model::Row::Other(v) => {
                // most likely the old shape, a bare name: say the new one
                let json = rmcp::serde_json::to_string(v).unwrap_or_default();
                errors.push(match v {
                    Value::String(_) => {
                        format!("rows: {json} is not a row: write {{\"signal\": {json}}}")
                    }
                    _ => format!(
                        "rows: {json} is not a row: write {{\"signal\": ...}} or {{\"group\": ..., \"rows\": [...]}}"
                    ),
                });
            }
        }
    }
    let exact = |t: Tick| Value::String(time::format_exact(t, app.trace.fs_per_tick));
    let time_of = |s: &str, errors: &mut Vec<String>| {
        at(app, s)
            .map_err(|e| errors.push(format!("{s}: {e}")))
            .ok()
    };
    let mut cursors = Table::new();
    for (n, v) in &f.cursors {
        let (t, name, note) = match v {
            Value::String(t) => (t.as_str(), "", ""),
            Value::Table(m) => {
                unknown_keys(
                    &format!("cursors {n}"),
                    m,
                    &["time", "name", "note"],
                    &mut errors,
                );
                let t = match m.get("time") {
                    Some(Value::String(x)) => x.as_str(),
                    _ => {
                        errors.push(format!("cursors: {n} is a time or {{time, name?, note?}}"));
                        continue;
                    }
                };
                (
                    t,
                    mark_part(m, n, "name", &mut errors),
                    mark_part(m, n, "note", &mut errors),
                )
            }
            _ => {
                errors.push(format!("cursors: {n} is a time or {{time, name?, note?}}"));
                continue;
            }
        };
        if !matches!(n.parse::<u8>(), Ok(1..=9)) {
            errors.push(format!("cursors: no cursor {n} (1-9)"));
        } else if n == "1" && matches!(v, Value::Table(_)) {
            errors.push(
                "cursors: 1 is the cursor and moves, so it takes a time only; name a mark (2-9)"
                    .into(),
            );
        } else if let Some((lo, hi)) = time_of(t, &mut errors) {
            // a cursor stays at the time written, not the end of its span (spec §15 "Times")
            let at = exact((lo + hi).div_ceil(2));
            let value = match v {
                Value::String(_) => at,
                _ => {
                    let mut m = Table::new();
                    m.insert("time".into(), at);
                    m.insert("name".into(), Value::String(name.into()));
                    m.insert("note".into(), Value::String(note.into()));
                    Value::Table(m)
                }
            };
            cursors.insert(n.clone(), value);
        }
    }
    for (k, v) in [("time", &f.view.time), ("to", &f.view.to)] {
        if matches!(v, Some(x) if !matches!(x, Value::String(_))) {
            errors.push(format!("view: {k} is a time string"));
        }
    }
    let (from, to) = (
        str_of(f.view.time.as_ref())
            .and_then(|s| time_of(s, &mut errors))
            .map_or(app.trace.start(), |t| t.0),
        str_of(f.view.to.as_ref())
            .and_then(|s| time_of(s, &mut errors))
            .map_or(app.trace.end().max(1), |t| t.1),
    );
    if f.view.time.is_some() && f.view.to.is_some() && to <= from {
        errors.push("view: the end must be after the start".into());
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    if let Some(u) = unit {
        app.unit = u;
        if u.is_some() {
            file.unit = f.unit.clone();
        }
    }
    file.view.time = Some(exact(from));
    file.view.to = Some(exact(to));
    file.cursors = cursors;
    file.rows = rows;
    if let Some(s) = &app.scope {
        file.scope = Some(s.clone());
    }
    toml::to_string(&file).map_err(|e| e.to_string())
}

/// A name-to-value map as pairs, in declaration order: an array of single-key tables
/// (`[{"tdata": "data"}]`) or, for session parity, a table (sorted, as the file keeps it).
/// Tables sort (spec §11 keeps file key order, but `Table` is a `BTreeMap`), so agents
/// declare order with arrays; `adopt`ed sessions read back as tables. Wrong shapes warn and miss out.
fn pair_list(
    subject: &str,
    v: Option<&toml::Value>,
    errors: &mut Vec<String>,
) -> Vec<(String, String)> {
    match v {
        Some(toml::Value::Array(items)) => {
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                match item {
                    toml::Value::Table(t) if t.len() == 1 => {
                        let (k, v) = t.iter().next().unwrap();
                        match v {
                            toml::Value::String(s) => out.push((k.clone(), s.clone())),
                            _ => errors.push(format!("{subject} {k} is a string")),
                        }
                    }
                    _ => errors.push(format!("{subject} [{i}] is {{name = value}}")),
                }
            }
            out
        }
        Some(toml::Value::Table(t)) => {
            let mut out = Vec::new();
            for (k, v) in t {
                match v {
                    toml::Value::String(s) => out.push((k.clone(), s.clone())),
                    _ => errors.push(format!("{subject} {k} is a string")),
                }
            }
            out
        }
        Some(_) => {
            errors.push(format!("{subject} maps names to values"));
            Vec::new()
        }
        None => Vec::new(),
    }
}

/// The keys an entry doesn't take (serde's `rest`, or a table's), each an error naming those it
/// does: a misspelt key is a mistake to fix, not one dropped without a word (spec §15).
fn unknown_keys(what: &str, t: &toml::Table, known: &[&str], errors: &mut Vec<String>) {
    for k in t.keys().filter(|k| !known.contains(&k.as_str())) {
        errors.push(format!("{what}: unknown entry: {k} ({})", known.join(", ")));
    }
}

/// An optional string value; wrong shapes warn and miss out.
fn opt_string(
    v: Option<&toml::Value>,
    what: &str,
    k: &str,
    errors: &mut Vec<String>,
) -> Option<String> {
    match v {
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(_) => {
            errors.push(format!("{what}: {k} is a string"));
            None
        }
        None => None,
    }
}

/// Pairs as a TOML table value, for session entries.
fn table_of(pairs: &[(String, String)]) -> toml::Value {
    toml::Value::Table(
        pairs
            .iter()
            .map(|(f, s)| (f.clone(), toml::Value::String(s.clone())))
            .collect(),
    )
}

/// A row from its parts, checked as `:add` checks them: the row and its variable.
/// A free fn (not a closure): the rows loop and the group members each reborrow per call.
fn signal_row(
    app: &mut Workspace,
    errors: &mut Vec<String>,
    name: &str,
    radix: Option<&str>,
    color: Option<&str>,
) -> Option<(session::SignalRow, usize)> {
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
    let mut row = session::SignalRow {
        signal: app.trace.vars[var].path.clone(),
        ..Default::default()
    };
    let (radix, color) = look_vals(app, var, name, radix, color, errors);
    row.radix = radix;
    row.color = color;
    Some((row, var))
}

/// A mark's optional string; wrong shapes warn and miss out.
fn mark_part<'m>(m: &'m toml::Table, n: &str, k: &str, errors: &mut Vec<String>) -> &'m str {
    match m.get(k) {
        Some(toml::Value::String(x)) => x.as_str(),
        Some(_) => {
            errors.push(format!("cursors: {n} {k} is a string"));
            ""
        }
        None => "",
    }
}

/// A line's `radix` and `color` values, checked against `var`: the rule lives in the core,
/// `radix_for` only words it.
fn look_vals(
    app: &Workspace,
    var: usize,
    name: &str,
    radix: Option<&str>,
    color: Option<&str>,
    errors: &mut Vec<String>,
) -> (Option<toml::Value>, Option<toml::Value>) {
    use toml::Value;
    let v = &app.trace.vars[var];
    let width = v.width.unwrap_or(0);
    let radix = radix.and_then(|r| match radix_for(r, width, v.names.is_some(), name) {
        Ok((r, frac)) => Some(Value::String(radix::name(r, frac, width))),
        Err(e) => {
            errors.push(e);
            None
        }
    });
    let color = color.and_then(|c| match rows::color_named(c) {
        Ok(_) => Some(Value::String(c.into())),
        Err(e) => {
            errors.push(e);
            None
        }
    });
    (radix, color)
}
