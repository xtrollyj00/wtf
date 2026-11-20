//! The `workspace` tool's declaration, checked by the session validator (spec §15).

use super::*;
use toml::{Table, Value};

/// A `workspace` declaration applied to the workspace: the tool's own keys checked and stripped,
/// then the session validator's, every problem at once, so one retry can fix them; a declaration
/// that fails changes nothing (`Workspace::attempt`), so the retry starts clean.
pub(super) fn declare(app: &mut Workspace, a: &WorkspaceArgs) -> Result<(), String> {
    let mut errors = Vec::new();
    let mut file = a.file.clone();
    // entries the workspace doesn't take: the dead names say what took their place; `trail` and
    // `version` are the session file's own, not declared
    let own = [
        ("trail", !file.trail.is_empty()),
        ("version", file.version != session::VERSION),
    ];
    let unknown = own
        .iter()
        .filter(|o| o.1)
        .map(|o| o.0.to_string())
        .chain(file.rest.keys().cloned());
    for k in unknown {
        let hint = match k.as_str() {
            "range" => "view: {\"time\": ..., \"to\": ...}",
            "signals" => "signal: [{\"name\": ..., \"expr\": ...}]",
            "protocols" => "protocol: [{\"name\": ...}]",
            _ => "signal, protocol, decode, names, rows, cursors, view, scope, unit",
        };
        errors.push(format!("unknown workspace entry: {k} (use {hint})"));
    }
    file.rest = Table::new();
    // the scope first, so the rows below resolve under it; a bad one is reported with the rest,
    // its nearest scopes listed (spec §15: every mistake at once), and taken out so the validator
    // doesn't say it twice
    if let Some(s) = file.scope.take()
        && let Err(e) = app.set_scope(&s)
    {
        errors.push(e);
    }
    // `auto` is the tool's word and no unit the file knows: the one that fits, which is no unit
    let auto = file.unit.as_deref() == Some("auto");
    if auto {
        file.unit = None;
    }
    // so a unit the validator would refuse is refused here, `auto` among the units listed
    if let Some(u) = file.unit.take_if(|u| time::unit_named(u).is_err()) {
        errors.push(format!(
            "unit: bad time unit: {u} (s, ms, us, ns, ps, fs or auto)"
        ));
    }
    // a protocol from a file is loaded here — only the MCP has the host — and its card is the
    // file's, so roles, `when` and `fields` are refused; the stem is the protocol's name (spec §19)
    let mut kept = Vec::with_capacity(file.protocol.len());
    for mut p in std::mem::take(&mut file.protocol) {
        let subject = match p.name.as_ref().and_then(Value::as_str) {
            Some(n) => format!("protocol {n}: field"),
            None => "protocol: field".into(),
        };
        pair_table(&subject, &mut p.fields, &mut errors);
        if p.wasm.is_none() {
            kept.push(p);
            continue;
        }
        let w = p.wasm.as_ref().and_then(Value::as_str).unwrap_or_default();
        let name = p.name.as_ref().and_then(Value::as_str).unwrap_or_default();
        let stem = std::path::Path::new(w)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        // whether the entry says what only a file's protocol can't (an empty list says nothing)
        let says = |v: &Option<Value>| match v {
            None => false,
            Some(Value::Array(items)) => !items.is_empty(),
            Some(Value::Table(t)) => !t.is_empty(),
            Some(_) => true,
        };
        if name.is_empty() {
            errors.push("protocol: an entry needs a name string".into());
        } else if stem != name {
            errors.push(format!("wasm {w} holds {stem}, not {name}: name it {stem}"));
        } else if says(&p.when) || says(&p.roles) || says(&p.fields) {
            errors.push(format!(
                "protocol {name} from a file takes no roles, when or fields"
            ));
        } else {
            // the error names the file already
            match crate::wasm::load_one(std::path::Path::new(w)) {
                Ok(loaded) => match app.add_loaded(loaded, w) {
                    Ok(()) => kept.push(session::Protocol {
                        name: Some(Value::String(name.into())),
                        wasm: Some(Value::String(w.into())),
                        ..Default::default()
                    }),
                    Err(e) => errors.push(format!("protocol {name}: {e}")),
                },
                Err(e) => errors.push(e),
            }
        }
    }
    file.protocol = kept;
    for d in &mut file.decode {
        let subject = d.name.as_ref().and_then(Value::as_str).unwrap_or("");
        pair_table(
            &format!("decode {subject} signal:"),
            &mut d.signals,
            &mut errors,
        );
        pair_table(
            &format!("decode {subject} param:"),
            &mut d.params,
            &mut errors,
        );
    }
    // what the declaration itself makes, unknown until the validator makes it: a decode's stream
    // and its fields, a computed signal — a row may show one (the rows come after them)
    let made: Vec<String> = file
        .decode
        .iter()
        .filter_map(|d| d.name.as_ref().and_then(Value::as_str).map(String::from))
        .chain(
            file.signal
                .iter()
                .filter_map(|s| s.name.as_ref().and_then(Value::as_str).map(String::from)),
        )
        .collect();
    resolve_rows(app, &mut errors, &mut file.rows, &made);
    // no `view` is the whole trace: a declaration replaces the view, it doesn't inherit the last
    // one's zoom (spec §15)
    let exact = |t: Tick| Value::String(time::format_exact(t, app.trace.fs_per_tick));
    if file.view.time.is_none() {
        file.view.time = Some(exact(app.trace.start()));
    }
    if file.view.to.is_none() {
        file.view.to = Some(exact(app.trace.end().max(1)));
    }
    // the validator takes each view time on its own; a range that runs backwards is a unit slip
    // (`3386665ps` for 3.386665ms), spec §15 "Times"
    let span = |v: &Option<Value>| {
        let s = v.as_ref().and_then(Value::as_str);
        s.and_then(|s| at(app, s).ok())
    };
    if let (Some((from, _)), Some((_, to))) = (span(&file.view.time), span(&file.view.to))
        && to <= from
    {
        errors.push("view: the end must be after the start".into());
    }
    let (warn, commit) = session::validate_file(app, file);
    if !errors.is_empty() || !warn.is_empty() {
        return Err(errors
            .into_iter()
            .chain(warn)
            .collect::<Vec<_>>()
            .join("\n"));
    }
    commit.land(app);
    if auto {
        app.unit = None;
    }
    Ok(())
}

/// The session's tables in the shape agents write them (`{"signals": [{"valid": "cmd_valid"}]}`),
/// as the file keeps them (spec §11): every list item is one `{name: value}` pair. Anything else is
/// left as it is, for the validator to want a table; a value that isn't a string is a mistake now.
fn pair_table(subject: &str, v: &mut Option<Value>, errors: &mut Vec<String>) {
    let Some(Value::Array(items)) = v else {
        return;
    };
    let mut t = Table::new();
    for item in items.iter() {
        match item {
            Value::Table(pair) if pair.len() == 1 => {
                let (k, value) = pair.iter().next().unwrap();
                match value {
                    Value::String(_) => t.extend(pair.clone()),
                    _ => errors.push(format!("{subject} {k} is a string")),
                }
            }
            _ => return,
        }
    }
    *v = Some(Value::Table(t));
}

/// Each row's signal as the trace spells it: the MCP's rows are names, resolved under the
/// declared scope (spec §15 "A name ambiguous in the trace"), so the errors are the ones a name
/// gets. `made` names what the declaration itself makes (`decode`, `signal`): no path yet, since
/// the validator makes them, so a row showing one is no mistake. A row whose name doesn't resolve
/// is reported here, its candidates listed, and taken out so the validator doesn't say it twice.
fn resolve_rows(
    app: &mut Workspace,
    errors: &mut Vec<String>,
    rows: &mut Vec<session::model::Row>,
    made: &[String],
) {
    rows.retain_mut(|row| {
        match row {
            session::model::Row::Signal(s) => match row_path(app, errors, &s.signal, made) {
                Ok(Some(path)) => s.signal = path,
                Ok(None) => {}
                Err(()) => return false,
            },
            session::model::Row::Group(g) => {
                let Some(Value::Array(items)) = &mut g.rows else {
                    return true;
                };
                let mut members: Vec<session::model::Row> = std::mem::take(items)
                    .into_iter()
                    .filter_map(|v| v.try_into().ok())
                    .collect();
                resolve_rows(app, errors, &mut members, made);
                *items = members
                    .into_iter()
                    .filter_map(|r| Value::try_from(r).ok())
                    .collect();
            }
            // neither a signal nor a group, the one wrong shape the typed file still carries, in
            // a group too: say how to write it (spec §15)
            session::model::Row::Other(v) => {
                let json = rmcp::serde_json::to_string(v).unwrap_or_default();
                errors.push(if v.is_str() {
                    format!("rows: {json} is not a row: write {{\"signal\": {json}}}")
                } else {
                    format!(
                        "rows: {json} is not a row: write {{\"signal\": ...}} or {{\"group\": ..., \"rows\": [...]}}"
                    )
                });
                return false;
            }
        }
        true
    });
}

/// The path a row's name resolves to: a bus, as a row shows one (a bit is a `values` column).
/// `None` for a name the declaration makes; `Err` once the mistake is in `errors`.
fn row_path(
    app: &mut Workspace,
    errors: &mut Vec<String>,
    name: &str,
    made: &[String],
) -> Result<Option<String>, ()> {
    // a decode names its stream and its fields under it (`rx.frame.len`); the rows come after it.
    // Checked first: the trace may hold another var whose path ends the same way
    if made.iter().any(|n| {
        name == n
            || name
                .strip_prefix(n.as_str())
                .is_some_and(|r| r.starts_with('.'))
    }) {
        return Ok(None);
    }
    match app.resolve(name) {
        Ok((v, None)) => Ok(Some(app.trace.vars[v].path.clone())),
        Ok(_) => {
            errors.push(format!("{name}: a bit can't be a row; add its bus"));
            Err(())
        }
        Err(e) => {
            errors.push(e);
            Err(())
        }
    }
}

/// The kind each `workspace` key takes: a key of another kind is an error in wtf's words, naming
/// the key, the kind it takes and an example, before serde's own text (spec §15).
pub(super) fn shape_error(args: &rmcp::model::JsonObject) -> Option<String> {
    /// A JSON value's kind, as an error names it.
    fn kind_of(v: &rmcp::serde_json::Value) -> &'static str {
        use rmcp::serde_json::Value;
        match v {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Number(_) => "a number",
            Value::String(_) => "a string",
            Value::Array(_) => "a list",
            Value::Object(_) => "a table",
        }
    }
    // the session's keys in its own order (spec §11), each with the kind it takes and one example
    let expected = [
        ("scope", "a string", "\"top\""),
        ("unit", "a string", "\"ns\""),
        ("view", "a table", "{\"time\": \"60ns\", \"to\": \"480ns\"}"),
        ("cursors", "a table", "{\"1\": \"80ns\"}"),
        (
            "signal",
            "a list",
            "[{\"name\": \"op\", \"expr\": \"cmd[7:0]\"}]",
        ),
        (
            "names",
            "a list",
            "[{\"signal\": \"state\", \"values\": \"0=zero\"}]",
        ),
        (
            "protocol",
            "a list",
            "[{\"name\": \"cmd\", \"roles\": [\"valid\", \"data\"]}]",
        ),
        (
            "decode",
            "a list",
            "[{\"name\": \"c0\", \"protocol\": \"cmd\", \"signals\": [{\"valid\": \"cmd_valid\"}]}]",
        ),
        ("rows", "a list", "[{\"signal\": \"top.clk\"}]"),
    ];
    for (key, kind, example) in expected {
        let Some(v) = args.get(key) else { continue };
        // null is no value for the two strings (serde takes it), not a wrong kind
        if v.is_null() && (key == "scope" || key == "unit") {
            continue;
        }
        let fits = match v {
            rmcp::serde_json::Value::String(_) => kind == "a string",
            rmcp::serde_json::Value::Object(_) => kind == "a table",
            rmcp::serde_json::Value::Array(_) => kind == "a list",
            _ => false,
        };
        if !fits {
            return Some(format!(
                "{key} must be {kind}, not {}: write it as {example}; see `help workspace`",
                kind_of(v)
            ));
        }
    }
    None
}
