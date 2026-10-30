//! `:` commands (spec §7), run on a workspace.

use crate::workspace::{Msg, Workspace};
use crate::rows::{self, Row};
use crate::radix;
use crate::session;


/// Run a `:` command on the workspace; some answer with a message (`:e`, or `:add` naming what
/// it couldn't add). The window's own commands (`:w`, `:q`, `:help`) are the terminal UI's.
pub fn run(app: &mut Workspace, line: &str) -> Result<Option<Msg>, String> {
    let line = line.trim();
    let (cmd, arg) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let arg = arg.trim();
    let fpt = app.trace.fs_per_tick;
    match cmd {
        "add" => return add(app, arg),
        "e" => return app.reload().map(Some),
        "decode" => return decode(app, arg).map(|m| Some((m, false))),
        _ => {}
    }
    match cmd {
        "" => Ok(()),
        _ if cmd.starts_with(|c: char| c.is_ascii_digit() || c == '.') => {
            let t = crate::time::parse(cmd, fpt)?;
            app.jump(t.round() as u64, false);
            Ok(())
        }
        "zoom" => {
            let t = crate::time::parse(arg, fpt)?;
            if t <= 0.0 {
                return Err("zoom must be above zero".into());
            }
            app.view.set_zoom(t);
            Ok(())
        }
        "fit" => fit(app, arg),
        "scope" => app.set_scope(arg),
        "unit" => {
            app.unit = match arg {
                "auto" => None,
                u => Some(crate::time::unit_named(u).map_err(|_| "usage: :unit <s|ms|us|ns|ps|fs|auto>")?),
            };
            Ok(())
        }
        "radix" => {
            let r = radix::parse_name(arg).ok_or_else(|| format!("unknown radix: {arg}"))?;
            app.set_radix(Some(r))
        }
        "color" if arg.is_empty() => Err(format!("usage: :color <{}|off>", rows::COLORS.join("|"))),
        "color" if arg == "off" => app.set_color(None),
        "color" => app.set_color(Some(rows::color_named(arg)?)),
        "group" if arg.is_empty() => Err("usage: :group <name>".into()),
        "group" => app.group(arg),
        "signal" => {
            let (name, expr) = arg.split_once('=').ok_or("usage: :signal <name> = <signal>")?;
            app.define(name, expr).map(drop)
        }
        "protocol" => protocol(app, arg),
        "names" => {
            let (sig, spec) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
            app.name_values(sig, spec)
        }
        "rename" if arg.is_empty() => Err("usage: :rename <name>".into()),
        "rename" => {
            let h = app.rows.get(app.sel).and_then(|_| rows::header_of(&app.rows, app.sel)).ok_or("not in a group")?;
            if let Row::Group { name, .. } = &mut app.rows[h] {
                *name = arg.to_string();
            }
            Ok(())
        }
        "ungroup" => {
            let h = app.rows.get(app.sel).and_then(|_| rows::header_of(&app.rows, app.sel)).ok_or("not in a group")?;
            let end = rows::group_end(&app.rows, h);
            for r in &mut app.rows[h + 1..end] {
                if let Row::Signal(s) = r {
                    s.grouped = false;
                }
            }
            app.rows.remove(h);
            app.sel = h;
            app.fix_sel();
            Ok(())
        }
        "select" => select(app, arg),
        "cursor" => cursor(app, arg),
        "undo" => app.undo(),
        "redo" => app.redo(),
        "rm" => rm(app, arg),
        _ => Err(format!("unknown command: {cmd}")),
    }
    .map(|()| None)
}

/// `:decode name protocol key=value...`: an instance (`[[decode]]`); `clock=` is its clock,
/// `prefix=` a param, and for `frame`, `pair` and `check` the keys that aren't roles are params. A
/// value runs to the next `key=`, so a rule can have spaces: `bad=valid && !ready`; one in quotes
/// runs to the closing quote, so it can hold a `key=` (`bad="x=1"`).
/// Answers with the streams the instance made and how many records each has, violations apart,
/// so what there is to look at shows: `bus: records bus.aw 2, ...; violations bus.errors 2, ...`.
fn decode(app: &mut Workspace, arg: &str) -> Result<String, String> {
    let usage = "usage: :decode <name> <protocol> <role>=<signal>...";
    let (name, rest) = word(arg);
    let (protocol, rest) = word(rest);
    if protocol.is_empty() {
        return Err(usage.into());
    }
    let mut pairs = key_values(rest).ok_or(usage)?;
    let clock = pairs.iter().position(|p| p.0 == "clock").map_or(String::new(), |i| pairs.remove(i).1);
    let on_streams = matches!(protocol, "frame" | "pair" | "check");
    let (signals, params) = pairs.into_iter().partition(|(k, _)| match on_streams {
        true => matches!(k.as_str(), "input" | "request" | "response"),
        // a built-in's params (`mode=0`, `baud=115200`) aren't roles
        false => k != "prefix" && !crate::protocol::builtin(protocol).is_some_and(|b| b.params.contains(&k.as_str())),
    });
    let d = crate::stream::Decode { name: name.into(), protocol: protocol.into(), clock, signals, params };
    app.add_decode(d)?;
    let inner = format!("{name}.");
    let made: Vec<&crate::stream::Stream> = app.streams.iter().filter(|s| s.name == name || s.name.starts_with(&inner)).collect();
    let list = |violations: bool| made.iter().filter(|s| s.violations == violations).map(|s| format!("{} {}", s.name, s.records.len())).collect::<Vec<_>>().join(", ");
    let parts: Vec<String> = [("records", list(false)), ("violations", list(true))].into_iter().filter(|(_, l)| !l.is_empty()).map(|(w, l)| format!("{w} {l}")).collect();
    Ok(format!("{name}: {}", parts.join("; ")))
}

/// `:protocol name when=expr field=signal...`: a stateless protocol (`[[protocol]]`); `clock=` is
/// its clock, `roles=a,b,c` makes it a type for `:decode`, the other keys are its fields in order.
fn protocol(app: &mut Workspace, arg: &str) -> Result<(), String> {
    let usage = "usage: :protocol <name> [clock=<clk>] when=<expr> [roles=<a,b>] <field>=<signal>...";
    let (name, rest) = word(arg);
    if name.is_empty() {
        return Err(usage.into());
    }
    let mut fields = key_values(rest).ok_or(usage)?;
    let mut take = |k: &str| fields.iter().position(|p| p.0 == k).map(|i| fields.remove(i).1);
    let (clock, when, roles) = (take("clock").unwrap_or_default(), take("when").ok_or(usage)?, take("roles"));
    let roles = roles.map_or(Vec::new(), |r| r.split(',').map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect());
    let p = crate::stream::Sampler { name: name.into(), clock, when, fields, roles };
    app.add_protocol(p).map(drop).map_err(|e| format!("protocol {name} not made: {e}"))
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
        let key = w.split_once('=').filter(|(k, v)| !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_') && !v.starts_with('='));
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
        if let Some(i) = app.rows.iter().position(|r| matches!(r, Row::Group { name: n, .. } if n == name)) {
            return Ok(i);
        }
        let (var, _) = app.resolve(name)?;
        app.rows.iter().position(|r| r.sig().is_some_and(|s| s.var == var)).ok_or(format!("not shown: {name}"))
    };
    let mut names = arg.split_whitespace();
    let first = row(names.next().ok_or("usage: :select <row> [<last row>]")?)?;
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
    let usage = "usage: :cursor <1-9> [time|off]";
    let (n, what) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
    let i = n.parse::<usize>().ok().filter(|n| (1..=9).contains(n)).ok_or(usage)? - 1;
    match what.trim() {
        "" => app.goto_mark(i),
        "off" => app.remove_mark(Some(i)),
        t => {
            let t = (crate::time::parse(t, app.trace.fs_per_tick)?.max(0.0).round() as u64).min(app.trace.end());
            match i {
                0 => app.jump(t, false),
                i => app.marks[i] = Some(t),
            }
            Ok(())
        }
    }
}

/// `:fit [from to]`: the whole simulation, or the range between two cursors (`'2 '3`, `'1` is
/// the cursor) or times, in either order. The cursor stays.
fn fit(app: &mut Workspace, arg: &str) -> Result<(), String> {
    let at = |a: &str| -> Result<crate::time::Tick, String> {
        match a.strip_prefix('\'') {
            Some(n) => {
                let i = n.parse::<usize>().ok().filter(|n| (1..=9).contains(n)).ok_or(format!("no cursor {n}: cursors are '1 to '9"))? - 1;
                app.cursor_list().into_iter().find(|c| c.0 == i).map(|c| c.1).ok_or(format!("no cursor {n}"))
            }
            None => Ok((crate::time::parse(a, app.trace.fs_per_tick)?.max(0.0).round() as u64).min(app.trace.end())),
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
        _ => return Err("usage: :fit, or :fit <from> <to> as cursors ('2 '3) or times".into()),
    }
    Ok(())
}

/// `:w [file]`: the file given becomes the session file in use.
pub fn save(app: &mut Workspace, arg: &str) -> Result<Msg, String> {
    let path = if arg.is_empty() { app.session_path.clone() } else { arg.into() };
    session::write(&path, &session::to_toml(app))?;
    app.saved = session::rows_toml(app);
    let msg = format!("written: {}", path.display());
    app.session_path = path;
    Ok((msg, false))
}

/// Vars an `:add` / `:rm` argument names: an exact path, a pattern, or a scope's direct signals;
/// under the workspace's scope first.
pub fn resolve(app: &Workspace, arg: &str) -> Vec<usize> {
    paths(app, &scoped(app, arg))
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
    let found = matching(app, arg, &|p| p.to_string());
    if !found.is_empty() {
        return found;
    }
    matching(app, &crate::trace::canon(arg), &crate::trace::canon)
}

/// The vars whose path, in `form`, is `arg`, matches it as a pattern, or is directly in it.
fn matching(app: &Workspace, arg: &str, form: &dyn Fn(&str) -> String) -> Vec<usize> {
    let prefix = format!("{arg}.");
    let hit = |p: &str| match arg.contains('*') {
        true => glob(arg, p),
        false => p == arg || p.strip_prefix(&prefix).is_some_and(|leaf| !leaf.contains('.')),
    };
    app.trace.vars.iter().enumerate().filter(|(_, v)| hit(&form(&v.path))).map(|(i, _)| i).collect()
}

/// `:add a b ...`: each in turn, below the previous. Names that fail are reported (as an error
/// if all do).
pub fn add(app: &mut Workspace, arg: &str) -> Result<Option<Msg>, String> {
    if arg.is_empty() {
        return Err("usage: :add <path|pattern>...".into());
    }
    let words = words(arg);
    let errors: Vec<String> = words.iter().filter_map(|w| add_one(app, w).err()).collect();
    match errors.len() {
        0 => Ok(None),
        n if n == words.len() => Err(errors.join("; ")),
        _ => Ok(Some((errors.join("; "), true))),
    }
}

/// The error for a name or pattern that matches nothing, with a hint for patterns: they match
/// the whole path.
pub fn no_match(arg: &str) -> String {
    let core = arg.trim_matches(['*', '.']);
    if arg.contains('*') && !arg.starts_with("**.") && !core.is_empty() {
        format!("no match: {arg} (a pattern matches the whole path: **.*{core}* for {core} anywhere)")
    } else {
        format!("no match: {arg}")
    }
}

fn add_one(app: &mut Workspace, arg: &str) -> Result<(), String> {
    if crate::computed::form(arg).is_some() {
        let (v, _) = app.resolve(arg)?;
        if app.shown(v) {
            return Err(format!("already shown: {arg}"));
        }
        app.add_rows(&[v]);
        return Ok(());
    }
    let arg = &scoped(app, arg);
    let found = paths(app, arg);
    if found.is_empty() {
        let scope = format!("{arg}.");
        if app.trace.vars.iter().any(|v| v.path.starts_with(&scope)) {
            return Err(format!("no signals directly in {arg}"));
        }
        return Err(no_match(arg));
    }
    let new: Vec<usize> = found.into_iter().filter(|v| !app.shown(*v)).collect();
    if new.is_empty() {
        return Err(format!("already shown: {arg}"));
    }
    // a scope (neither a signal nor a pattern) becomes a group named after it
    let signal = matches!(new[..], [v] if crate::trace::canon(&app.trace.vars[v].path) == crate::trace::canon(arg));
    if signal || arg.contains('*') {
        app.add_rows(&new);
    } else {
        // named as in the trace, whichever form it was named in
        let path = app.trace.vars[new[0]].path.clone();
        let scope = path.rsplit_once('.').map_or(arg.as_str(), |(s, _)| s);
        app.add_group(scope.rsplit('.').next().unwrap_or(scope), &new);
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
            match resolve(app, w)[..] {
                // a name as search names it: as shown, a slice, the end of a path
                [] => found.extend(app.resolve(w).ok().filter(|p| p.1.is_none()).map(|p| p.0)),
                ref f => found.extend(f),
            }
        }
        let before = app.rows.len();
        app.rows.retain(|r| r.sig().is_none_or(|s| !found.contains(&s.var)));
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
