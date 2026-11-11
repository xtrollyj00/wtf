//! `wtf --mcp`: an MCP server over stdin / stdout, so an AI agent can open traces, list signals,
//! read values, search with wtf's expressions, look at waveforms drawn as text, and show the user
//! what it found. Each trace it opens is a workspace, listening for `wtf --attach` like a window.

use wtf_core::workspace::{Kind, Workspace};
use wtf_core::workspace::unknown::NoStream;
use crate::draw;
use crate::live;
use wtf_core::session;
use wtf_core::protocol;
use wtf_core::radix;
use wtf_core::rows::{self, Row, Sig};
use wtf_core::query;
use wtf_core::search;
use wtf_core::time::{self, Tick};
use wtf_core::trace::Val;
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use std::sync::{Arc, Mutex};

mod args;
mod declare;
pub mod help;
mod trail;
pub use args::*;
use declare::declared;
use trail::Answer;

/// Most lines one answer gives, so a busy clock can't flood the agent.
const MAX_LINES: usize = 1000;
/// Interfaces `info` lists; more are counted.
const MAX_INTERFACES: usize = 10;
/// Most bytes one answer gives: output can't be taken back out of the agent's context.
const MAX_BYTES: usize = 24_000;

#[derive(Clone)]
pub struct Wtf {
    state: Arc<Mutex<State>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Default)]
struct State {
    workspaces: Vec<Opened>,
    current: usize,
    opened: usize,
}

/// A trace the agent opened.
struct Opened {
    id: String,
    app: Arc<Mutex<Workspace>>,
    _link: live::Listener,
}

impl Wtf {
    pub fn new() -> Self {
        Wtf { state: Default::default(), tool_router: Self::tool_router() }
    }
}

// a tool that panicked leaves things as they were before the panic, which is still usable
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The current workspace's app.
fn current(w: &Wtf) -> Result<Arc<Mutex<Workspace>>, String> {
    let st = lock(&w.state);
    let ws = st.workspaces.get(st.current).ok_or("no trace is open: `open` a VCD, FST or GHW waveform (`sessions` lists the user's windows and their traces)")?;
    Ok(ws.app.clone())
}

/// The ticks a time given stands for, first and last: a rounded time (`2595ns`) is every tick that
/// prints as it (spec §15 "Times").
fn at(app: &Workspace, s: &str) -> Result<(Tick, Tick), String> {
    let (lo, hi) = time::span(s, app.trace.fs_per_tick)?;
    Ok((lo.min(app.trace.end()), hi.min(app.trace.end())))
}

/// `to` not before `from`, as written: reversed, it is likely a unit slip (`3386665ps` for
/// 3.386665ms), so the error gives the trace's range to compare with (spec §15 "Times").
fn ordered(app: &Workspace, from: (&str, Tick), to: (&str, Tick)) -> Result<(), String> {
    if to.1 >= from.1 {
        return Ok(());
    }
    let t = &app.trace;
    let u = unit(app, &None, [t.start(), t.end()])?;
    Err(format!("`to` {} is before `from` {}; the trace runs {} - {}", to.0.trim(), from.0.trim(), show(app, t.start(), u), show(app, t.end(), u)))
}

/// The unit of an answer's times, and their precision.
#[derive(Clone, Copy)]
struct Unit {
    index: usize,
    /// None: exact.
    decimals: Option<u32>,
}

fn show(app: &Workspace, t: Tick, u: Unit) -> String {
    time::format_as(t, app.trace.fs_per_tick, Some(u.index), u.decimals)
}

/// The unit of an answer's times, so they compare at a glance: the one asked for, else the
/// workspace's, to a thousandth of it (`2595ns` for 2595.000001ns; `fs` is exact), else the one that
/// fits them all, exact.
fn unit(app: &Workspace, asked: &Option<String>, ts: impl IntoIterator<Item = Tick>) -> Result<Unit, String> {
    Ok(unit_of(app, asked.as_deref().map(time::unit_named).transpose()?, ts))
}

fn unit_of(app: &Workspace, asked: Option<usize>, ts: impl IntoIterator<Item = Tick>) -> Unit {
    match asked.or(app.time_unit()) {
        Some(index) => Unit { index, decimals: Some(3) },
        None => Unit { index: time::unit_for(ts, app.trace.fs_per_tick), decimals: None },
    }
}

/// A record's field, a time field in the answer's unit, a number in its radix if one was asked.
fn field_text(app: &Workspace, f: &wtf_core::stream::Field, v: &Option<Val>, u: Unit, radix: Option<(usize, u32)>) -> String {
    match (v, radix) {
        (Some(Val::Bits(b)), Some((r, frac))) if b.len() > 1 => (radix::RADIXES[r].format)(b, frac),
        _ => wtf_core::stream::value_text(f, v, app.trace.fs_per_tick, Some(u.index), u.decimals),
    }
}

/// The radix asked for each of a stream's fields (`dec`, `len:dec,data:hex`), None for the
/// default: a radix for every field skips those it doesn't fit, one for a field must fit it.
fn field_radixes(s: &wtf_core::stream::Set, asked: Option<&str>) -> Result<Vec<Option<(usize, u32)>>, String> {
    let mut out = vec![None; s.columns.len()];
    for item in asked.unwrap_or("").split([',', ' ']).filter(|i| !i.is_empty()) {
        let (field, r) = match with_radix(item) {
            (f, Some(r)) => (Some(f), r),
            (r, None) => (None, r),
        };
        let numbers = |f: &wtf_core::stream::Field| f.width.filter(|_| !f.time);
        match field {
            None => {
                radix::parse_name(r).ok_or(format!("unknown radix: {r}"))?;
                for (k, f) in s.columns.iter().map(|c| &c.field).enumerate() {
                    if let Some(Ok(rf)) = numbers(f).map(|w| radix_for(r, w, false, &f.name)) {
                        out[k] = Some(rf);
                    }
                }
            }
            Some(name) => {
                let Some(k) = s.columns.iter().position(|c| c.name == name) else {
                    let names: Vec<&str> = s.columns.iter().map(|c| c.name.as_str()).collect();
                    let streams: Vec<&str> = s.streams.iter().map(|s| s.name.as_str()).collect();
                    return Err(format!("{}: no field {name} (fields: {})", streams.join(", "), names.join(", ")));
                };
                let f = &s.columns[k].field;
                let w = numbers(f).ok_or(format!("{name} isn't a number: a time or text takes no radix"))?;
                out[k] = Some(radix_for(r, w, false, name)?);
            }
        }
    }
    Ok(out)
}

fn plural(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// Lines a `signals` answer folds to: below the deepest level that fits, a scope is one line.
const FOLD_LINES: usize = 200;

/// The `signals` answer: the design's nodes a search matched, as the hierarchy sent to the agent
/// (spec §15). A signal with its width; a struct or array one line, what is in it hidden unless
/// only that matched; a scope the search names before its signals, when it doesn't name the
/// scope it is in. Over `FOLD_LINES`, the scopes below the deepest level that fits are a line
/// each counting their matches, never above the scope all matches share, so asking for one
/// scope lists it. Empty if nothing matched.
fn hierarchy(ws: &Workspace, matched: &[usize]) -> String {
    let t = &ws.trace;
    let d = t.design();
    let nodes = &d.nodes;
    let mut hit = vec![false; nodes.len()];
    matched.iter().for_each(|&i| hit[i] = true);
    let up = |mut i: usize| std::iter::from_fn(move || nodes[i].parent.inspect(|&p| i = p));
    let what = |v: &wtf_core::trace::Var| match v.width {
        Some(w) => plural(w as usize, "bit"),
        None if v.real => "real".into(),
        None => "string".into(),
    };
    // a line: (order, node, text); a var's ordered by its first signal, a scope before them
    let mut items: Vec<((usize, usize, usize), usize, String)> = Vec::new();
    for &i in matched {
        let n = &nodes[i];
        match n.var {
            // inside a struct or array that matched: that is its line
            Some(_) if up(i).take_while(|&p| nodes[p].var.is_some()).any(|p| hit[p]) => {}
            Some(v) => {
                let var = &t.vars[v];
                let text = match ws.kind(v, None) {
                    k @ (Kind::Array | Kind::Struct) => format!("{}, {}, {}", if k == Kind::Array { "array" } else { "struct" }, if var.width.is_some() { what(var) } else { "text".into() }, plural(n.signals, "signal")),
                    _ => what(var),
                };
                items.push(((n.first, 1, n.depth), i, text));
            }
            None if n.parent.is_some_and(|p| hit[p]) => {}
            None => items.push(((n.first, 0, n.depth), i, format!("scope of {}", plural(n.signals, "signal")))),
        }
    }
    if items.is_empty() {
        return String::new();
    }
    items.sort_by_key(|x| x.0);
    // the scopes a line is in, from the top; a scope's line is in itself
    let scopes = |i: usize| {
        let mut s: Vec<usize> = std::iter::once(i).chain(up(i)).filter(|&p| nodes[p].var.is_none()).collect();
        s.reverse();
        s
    };
    let chains: Vec<Vec<usize>> = items.iter().map(|x| scopes(x.1)).collect();
    // the scope every line is in: folding stops there
    let common = chains.iter().map(|c| c.iter().zip(&chains[0]).take_while(|(a, b)| a == b).count()).min().unwrap_or(0);
    let deepest = chains.iter().map(Vec::len).max().unwrap_or(0);
    // at level `k`, what is in `k` scopes or fewer is listed, the rest counted in its scope k + 1 deep
    let fold = |k: usize| -> (Vec<String>, Option<String>) {
        let mut lines: Vec<(usize, Option<String>)> = Vec::new();
        let mut folded: std::collections::HashMap<usize, (usize, usize)> = Default::default();
        for (x, chain) in items.iter().zip(&chains) {
            if chain.len() <= k {
                lines.push((x.1, Some(x.2.clone())));
                continue;
            }
            let s = chain[k];
            let count = folded.entry(s).or_insert_with(|| {
                lines.push((s, None));
                (0, 0)
            });
            match nodes[x.1].var {
                Some(_) => count.0 += nodes[x.1].signals,
                None => count.1 += 1,
            }
        }
        let first = lines.iter().find(|l| l.1.is_none()).map(|l| format!("^{}\\.", regex::escape(&nodes[l.0].canon)));
        let lines = lines.into_iter().map(|(i, text)| {
            let text = text.unwrap_or_else(|| match folded[&i] {
                (0, s) => format!("scope, {}", plural(s, "matching scope")),
                (n, _) => format!("scope, {}", plural(n, "matching signal")),
            });
            (nodes[i].path.as_str(), text)
        });
        (collapse(lines), first)
    };
    let (mut lines, mut one) = fold(deepest);
    for k in (common..deepest).rev() {
        if lines.len() <= FOLD_LINES {
            break;
        }
        (lines, one) = fold(k);
    }
    let more = lines.len().saturating_sub(MAX_LINES);
    let mut out = lines.into_iter().take(MAX_LINES).collect::<Vec<_>>().join("\n");
    if more > 0 {
        out += &format!("\n... {more} more; narrow the pattern");
    }
    if let Some(one) = one {
        out += &format!("\nfolded to fit: a scope's line counts its matches; ask for one by its path, `{one}`");
    }
    out
}

/// Paths with their widths as `signals` lines: those differing only in indices (`[3]`, `(3)`) and
/// of one width are one line, each index a range: `mux_in[0..15][0..15] (8 bits, 256 signals)`.
/// A barrel shifter's 256 elements, one line each, were most of an agent's answer.
fn collapse<'a>(vars: impl Iterator<Item = (&'a str, String)>) -> Vec<String> {
    /// Paths alike but for their indices: each index's range, how many, the first.
    struct Group<'a> {
        key: String,
        width: String,
        ranges: Vec<(u64, u64)>,
        n: usize,
        first: &'a str,
    }
    let mut groups: Vec<Group> = Vec::new();
    for (path, width) in vars {
        let (key, idx) = blank_indices(path);
        match groups.iter_mut().find(|g| g.key == key && g.width == width) {
            Some(g) => {
                for (r, i) in g.ranges.iter_mut().zip(&idx) {
                    *r = (r.0.min(*i), r.1.max(*i));
                }
                g.n += 1;
            }
            None => groups.push(Group { key, width, ranges: idx.iter().map(|&i| (i, i)).collect(), n: 1, first: path }),
        }
    }
    groups
        .into_iter()
        .map(|Group { key, width, ranges, n, first }| match n {
            1 => format!("{first} ({width})"),
            _ => {
                let mut ranges = ranges.into_iter().map(|(a, b)| if a == b { a.to_string() } else { format!("{a}..{b}") });
                let path: String = key.split('\0').enumerate().map(|(i, part)| if i == 0 { part.to_string() } else { format!("{}{part}", ranges.next().unwrap_or_default()) }).collect();
                // a group of scopes, structs or arrays says so: it counts their signals already
                let noun = ["scope", "struct", "array"].into_iter().find(|k| width.starts_with(k)).map_or("signals".into(), |k| format!("{k}s"));
                format!("{path} ({width}, {n} {noun})")
            }
        })
        .collect()
}

/// A path with each index (`[12]`, `(4)`) blanked to its brackets around a NUL, and the indices.
fn blank_indices(path: &str) -> (String, Vec<u64>) {
    let (mut key, mut idx, mut rest) = (String::new(), Vec::new(), path);
    while let Some(open) = rest.find(['[', '(']) {
        let close = if rest.as_bytes()[open] == b'[' { ']' } else { ')' };
        let inner = rest[open + 1..].find(close).map(|c| &rest[open + 1..open + 1 + c]);
        match inner.and_then(|n| n.parse::<u64>().ok().filter(|_| n.bytes().all(|b| b.is_ascii_digit()))) {
            Some(n) => {
                let len = inner.map_or(0, str::len);
                key += &rest[..=open];
                key.push('\0');
                key.push(close);
                idx.push(n);
                rest = &rest[open + len + 2..];
            }
            None => {
                key += &rest[..=open];
                rest = &rest[open + 1..];
            }
        }
    }
    key += rest;
    (key, idx)
}

/// A field's text for a table of records: a frame's bytes past the first 8 as their count
/// (`f7 88 0c 5c 25 8f 30 3a ... (580 bytes)`), other text past 40 characters likewise. 140 frames'
/// data filled an agent's answer that wanted their lengths.
fn shorten(t: String) -> String {
    let words: Vec<&str> = t.split(' ').collect();
    if words.len() > 8 && words.iter().all(|w| w.len() == 2 && w.bytes().all(|b| b.is_ascii_hexdigit())) {
        return format!("{} ... ({} bytes)", words[..8].join(" "), words.len());
    }
    match t.char_indices().nth(40) {
        Some((at, _)) => format!("{} ... ({} chars)", &t[..at], t.chars().count()),
        None => t,
    }
}

/// Cut an answer at a line boundary once it is over `MAX_BYTES`.
fn budget(s: String) -> String {
    if s.len() <= MAX_BYTES {
        return s;
    }
    let cut = s[..MAX_BYTES].rfind('\n').unwrap_or(MAX_BYTES);
    let rest = s[cut..].lines().filter(|l| !l.is_empty()).count();
    format!("{}\n... cut at {MAX_BYTES} bytes, {rest} more lines; ask for less (a shorter range, fewer signals, `count`)", &s[..cut])
}

/// A radix by name for signal `n` of `width` bits, as (radix, fraction bits).
fn radix_for(r: &str, width: u32, named: bool, n: &str) -> Result<(usize, u32), String> {
    let (r, frac, need) = radix::parse_name(r).ok_or(format!("unknown radix: {r}"))?;
    if r == radix::ENUM && !named {
        return Err(format!("enum: the file names no values of {n}"));
    }
    if !radix::fits(r, need, width, named) {
        return Err(format!("{} doesn't fit {n} ({width} bits)", radix::name(r, frac, need.unwrap_or(width))));
    }
    Ok((r, frac))
}

/// `name:radix` split; the `:` of a slice (`data[7:4]`) is not a radix's.
fn with_radix(spec: &str) -> (&str, Option<&str>) {
    match spec.rsplit_once(':') {
        Some((n, r)) if !r.contains(']') => (n, Some(r)),
        _ => (spec, None),
    }
}

/// Resolve names (`name` or `name:radix`), load their values, and pick each one's radix.
fn probes(app: &mut Workspace, names: &[String], radix_name: &Option<String>) -> Result<Vec<(String, search::Probe, Sig)>, String> {
    let mut out = Vec::new();
    for n in names {
        let (n, own) = with_radix(n);
        let p = app.resolve(n)?;
        let width = app.trace.probe_width(p).unwrap_or(0);
        let named = p.1.is_none() && app.trace.vars[p.0].names.is_some();
        let (r, frac) = match own.or(radix_name.as_deref()) {
            None => (radix::default_for(named, p.1.is_none() && app.trace.vars[p.0].signed), 0),
            Some(r) => radix_for(r, width, named, n)?,
        };
        let mut s = Sig::of(p.0, &app.trace.vars[p.0]);
        (s.look.radix, s.look.frac) = (r, frac);
        out.push((n.to_string(), p, s));
    }
    let vars: Vec<usize> = out.iter().map(|(_, p, _)| p.0).collect();
    app.trace.load(&vars);
    Ok(out)
}

fn value(app: &Workspace, p: search::Probe, s: &Sig, t: Tick) -> String {
    match app.trace.value_of(p.0, p.1, t) {
        Some(Val::Bits(b)) if b.len() == 1 => b.to_uppercase(),
        Some(v) => wtf_core::rows::label(&app.trace.vars[s.var], &s.look, &v),
        None => "-".into(),
    }
}

#[tool_router]
impl Wtf {
    #[tool(description = "The open sessions: the user's wtf windows and agents' workspaces, with ids and traces, yours marked. `open` starts one or switches between yours.", annotations(read_only_hint = true, open_world_hint = false))]
    fn sessions(&self) -> String {
        let (ours, cur) = {
            let st = lock(&self.state);
            (st.workspaces.iter().map(|w| w.id.clone()).collect::<Vec<_>>(), st.workspaces.get(st.current).map(|w| w.id.clone()))
        };
        let all = match live::sessions() {
            Ok(all) => all,
            Err(e) => return e,
        };
        let lines: Vec<String> = all
            .iter()
            .map(|s| {
                let mark = match (ours.contains(&s.id), cur.as_ref() == Some(&s.id)) {
                    (true, true) => "  (yours, current)",
                    (true, false) => "  (yours)",
                    _ => "",
                };
                format!("{}  {}  {}{mark}", s.id, s.kind, s.trace.display())
            })
            .collect();
        if lines.is_empty() { "no sessions: the user has no wtf window open; `open` a trace".into() } else { lines.join("\n") }
    }

    #[tool(description = "Choose the trace the other tools work on: `trace` opens a waveform (VCD, FST or GHW file) in a new workspace; `session` goes back to one of your workspaces (`sessions`); neither re-reads the current one's file, after the simulation ran again, keeping its rows. Answers with `info`, which lists the interfaces to decode.", annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false))]
    fn open(&self, Parameters(a): Parameters<OpenArgs>) -> Result<String, String> {
        let trace = match (a.trace, a.session) {
            (Some(_), Some(_)) => return Err("give `trace` (a new workspace) or `session` (one of yours), not both".into()),
            (Some(t), None) => t,
            (None, Some(id)) => {
                let mut st = lock(&self.state);
                st.current = st.workspaces.iter().position(|w| w.id == id).ok_or(format!("{id} is not one of your workspaces; see `sessions`"))?;
                drop(st);
                return self.info();
            }
            (None, None) => {
                let w = current(self)?;
                let msg = {
                    let mut app = lock(&w);
                    let name = app.file_name.clone();
                    // its message lists the signals no longer in the file
                    app.reload()?.0.replacen("reloaded", &format!("reloaded {name}"), 1)
                };
                return Ok(format!("{msg}\n{}", self.info()?));
            }
        };
        let mut ws = Workspace::open(std::path::Path::new(&trace))?;
        // an agent writes patterns as regexes, everywhere (spec §15)
        ws.patterns = wtf_core::pattern::Syntax::Regex;
        let app = Arc::new(Mutex::new(ws));
        // the user's start screen shows what agents opened (spec §4 "Start screen")
        crate::history::opened(std::path::Path::new(&trace), None, true);
        let mut st = lock(&self.state);
        st.opened += 1;
        let id = format!("{}-{}", std::process::id(), st.opened);
        let shared_app = app.clone();
        let link = live::listen(&id, "agent", std::path::Path::new(&trace), move |req| match req {
            live::Request::Get => Ok(link_answer(&lock(&shared_app))),
            live::Request::Put(_) => Err("an agent's workspace takes no sessions".into()),
        })
        .map_err(|e| format!("can't listen for the user's wtf windows: {e}"))?;
        st.workspaces.push(Opened { id: id.clone(), app, _link: link });
        st.current = st.workspaces.len() - 1;
        drop(st);
        Ok(format!("workspace {id} (the user's wtf windows on this trace see your steps; :follow there shows the workspace)\n{}", self.info()?))
    }

    #[tool(description = "What can be decoded; with a name, its roles, params, streams and fields (`help decode`).", annotations(read_only_hint = true, open_world_hint = false))]
    fn protocols(&self, Parameters(a): Parameters<ProtocolsArgs>) -> Result<String, String> {
        // works before a trace is open, without the session's own protocols
        let own = current(self).ok().map(|w| lock(&w).protocols.clone()).unwrap_or_default();
        match a.name {
            Some(n) => protocol::describe(&n, &own),
            None => Ok(protocol::list(&own)),
        }
    }

    #[tool(description = "The reference for one concept: its form, recipes and details. Topics: names, patterns, times, radix, expressions, decode, records, workspace, commands; without a topic, lists them. Call it when an argument's doc or an error isn't enough.", annotations(read_only_hint = true, open_world_hint = false))]
    fn help(&self, Parameters(a): Parameters<HelpArgs>) -> Result<String, String> {
        help::answer(a.topic.as_deref(), wtf_core::pattern::Syntax::Regex)
    }

    #[tool(description = "The open waveform: file, time range, signals, and the interfaces a built-in protocol decodes, each with its `decode` line.", annotations(read_only_hint = true, open_world_hint = false))]
    fn info(&self) -> Result<String, String> {
        let a = current(self)?;
        let app = lock(&a);
        let t = &app.trace;
        let tick = t.fs_per_tick.map_or("no timescale (plain ticks)".into(), |f| format!("1 tick = {}", time::format_exact(1, Some(f))));
        let u = unit(&app, &None, [t.start(), t.end()])?;
        let mut out = format!("{}\n{} - {}\n{tick}\n{} signals", app.file_name, show(&app, t.start(), u), show(&app, t.end(), u), t.vars.len());
        // the interfaces a decoder reads, so the agent decodes them instead of writing searches
        let found = protocol::detect(t);
        if !found.is_empty() {
            out += "\ninterfaces a built-in protocol decodes (decode them: exact records, handshake checks built in):";
            for f in found.iter().take(MAX_INTERFACES) {
                out += &format!("\n  {}* {}: {}", f.prefix, f.protocol, f.decode_line());
            }
            if found.len() > MAX_INTERFACES {
                out += &format!("\n  and {} more", found.len() - MAX_INTERFACES);
            }
        }
        Ok(out)
    }

    #[tool(description = "Search the waveform's design, scopes (instances) and signals, by a regex in slashes found anywhere in the path, as grep does (`/uart/`, `/_valid$/`); none lists it all, folded to fit.", annotations(read_only_hint = true, open_world_hint = false))]
    fn signals(&self, Parameters(a): Parameters<SignalsArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        // not the slices earlier queries made (spec §16)
        app.drop_unused();
        // a pattern as everywhere else an agent writes one, in slashes; none lists everything
        let re = match a.pattern.trim() {
            "" => wtf_core::pattern::regex("").map_err(|e| e.to_string())?,
            p => match wtf_core::pattern::Syntax::Regex.pattern(p)? {
                Some(wtf_core::pattern::Pattern::Regex(re)) => re,
                _ => return Err(format!("{p} is no pattern: a pattern is a regex in slashes, /{p}/")),
            },
        };
        let t = &app.trace;
        let d = t.design();
        // a scope's count is the file's signals: the computed slices are rows, not the file's
        let matched: Vec<usize> = d.matching(|path| re.is_match(path)).into_iter().filter(|&i| d.nodes[i].file).collect();
        Ok(match hierarchy(&app, &matched) {
            out if out.is_empty() => wtf_core::command::no_match(&app, &a.pattern),
            out => budget(out),
        })
    }

    #[tool(description = "Values of signals from `from` to `to`: the value at `from`, then each change, as `time value` lines; omit `to` for one time. A struct or array also lists its fields. For when something happens, `find`.", annotations(read_only_hint = true, open_world_hint = false))]
    fn values(&self, Parameters(a): Parameters<ValuesArgs>) -> Result<Answer, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        // one time is a span too: `20ns` covers what prints as 20ns, its changes listed
        let to_text = a.to.as_ref().unwrap_or(&a.from);
        let (from, to) = (at(&app, &a.from)?.0, at(&app, to_text)?.1);
        ordered(&app, (&a.from, from), (to_text, to))?;
        let asked = probes(&mut app, &a.signals, &a.radix)?;
        // a struct or array: its whole value, then each signal in it (spec §15)
        let asked: Vec<_> = asked
            .into_iter()
            .flat_map(|(n, p, s)| {
                let mut out = vec![(n.clone(), p, s)];
                if p.1.is_none() && app.trace.is_composite(p.0) {
                    for f in app.trace.fields(p.0).into_iter().filter(|f| !app.trace.is_composite(f.var)) {
                        out.push((f.under(&n), (f.var, None), Sig::of(f.var, &app.trace.vars[f.var])));
                    }
                }
                out
            })
            .collect();
        // the cap is for the whole answer: one busy signal mustn't hide behind a quiet one
        let (mut lists, mut budget_left, mut cut) = (Vec::new(), MAX_LINES, false);
        for (name, p, s) in asked {
            if cut {
                break;
            }
            let (times, stopped) = query::changes(&mut app.trace, p, from, to, &mut budget_left);
            cut = stopped;
            let list: Vec<(Tick, String)> = std::iter::once(from).chain(times).map(|t| (t, value(&app, p, &s, t))).collect();
            lists.push((name, list));
        }
        let u = unit(&app, &a.unit, lists.iter().flat_map(|(_, l)| l.iter().map(|x| x.0)).chain([to]))?;
        let mut out = String::new();
        for (name, list) in &lists {
            out += &format!("{name}:\n");
            for (t, v) in list {
                out += &format!("  {} {v}\n", show(&app, *t, u));
            }
        }
        if cut {
            out += &format!("  ... more changes until {}; narrow the range, or `find` the events\n", show(&app, to, u));
        }
        Ok(Answer { text: budget(out), time: Some(from) })
    }

    #[tool(description = "Records of a decoded stream, or of several in start order (a decode's name: its results), as a table: n, start (the rising edge that starts the record's first clock cycle), end (the edge that samples its last) and the fields; `where`, `group_by`, `count_only` (`help records`).", annotations(read_only_hint = true, open_world_hint = false))]
    fn records(&self, Parameters(a): Parameters<RecordsArgs>) -> Result<Answer, String> {
        let w = current(self)?;
        let app = lock(&w);
        let ids = match app.stream_set(&a.stream) {
            Ok(ids) => ids,
            Err(e) => {
                return Err(match e {
                    // an interface `open` listed, not decoded yet (`top.s_axi.wr`, `s_axi.wr`): its line
                    NoStream::NotDecoded { name, part, found } => {
                        let how: Vec<String> = found
                            .iter()
                            .take(3)
                            .map(|f| match part.as_str() {
                                "" => format!("`{}` makes its streams (`protocols {}`)", f.decode_line(), f.protocol),
                                _ => format!("`{}` makes {}.{part}", f.decode_line(), f.name()),
                            })
                            .collect();
                        format!("unknown stream: {name}: its interface isn't decoded yet; with `command`, {}", how.join("; or "))
                    }
                    NoStream::NoneYet { name } => format!("unknown stream: {name} (none yet; make one with `command`: `decode <name> <protocol> ...`)"),
                    e => e.to_string(),
                });
            }
        };
        let set = wtf_core::stream::Set::new(ids.iter().map(|&i| &app.streams[i]).collect());
        // several streams: a column says whose each record is
        let multi = set.streams.len() > 1;
        let from = a.from.as_deref().map(|f| at(&app, f)).transpose()?.map_or(0, |s| s.0);
        let to = a.to.as_deref().map(|t| at(&app, t)).transpose()?.map_or(app.trace.end(), |s| s.1);
        if let (Some(f), Some(t)) = (&a.from, &a.to) {
            ordered(&app, (f, from), (t, to))?;
        }
        let (picked, of) = set.select(from, to, a.filter.as_deref(), app.trace.fs_per_tick)?;
        let radixes = field_radixes(&set, a.radix.as_deref())?;
        // with `where`: how many match, of how many there are
        let (matching, of) = match a.filter {
            Some(_) => (" matching", format!(", of {of}")),
            None => ("", String::new()),
        };
        let (Some(&first), Some(&last)) = (picked.first(), picked.last()) else {
            // a check's empty answer is its result, not a sign it didn't run
            let text = match (matching, set.streams.iter().all(|s| s.violations)) {
                ("", true) => "no records: no violations".to_string(),
                ("", false) => "no records".to_string(),
                _ => format!("no matching records{of}"),
            };
            return Ok(Answer { text, time: None });
        };
        let count = a.count.unwrap_or(20).min(MAX_LINES);
        if !a.group_by.is_empty() {
            let groups = set.group_by(&picked, &a.group_by)?;
            let what = format!("{} distinct, of {}{matching} records{of}", groups.len(), picked.len());
            if a.count_only {
                return Ok(Answer { text: what, time: None });
            }
            let page = &groups[..groups.len().min(count)];
            let u = unit(&app, &a.unit, page.iter().flat_map(|g| [g.first, g.last]))?;
            let mut lines = vec![format!("{} of {what}", page.len())];
            let cols: Vec<usize> = a.group_by.iter().filter_map(|n| set.columns.iter().position(|c| c.name == *n)).collect();
            for g in page {
                let values: Vec<String> = cols.iter().zip(&g.values).map(|(&k, v)| format!("{}={}", set.columns[k].name, field_text(&app, &set.columns[k].field, v, u, radixes[k]))).collect();
                lines.push(format!("{} × {} ({} .. {})", g.count, values.join(" "), show(&app, g.first, u), show(&app, g.last, u)));
            }
            // the first group listed, and the record that starts it
            return Ok(Answer { text: budget(lines.join("\n")), time: Some(page[0].first) });
        }
        let (first, last) = (set.record(first), set.record(last));
        if a.count_only {
            let u = unit(&app, &a.unit, [first.start, last.start])?;
            return Ok(Answer { text: format!("{}{matching} records{of}, first {}, last {}", picked.len(), show(&app, first.start, u), show(&app, last.start, u)), time: Some(first.start) });
        }
        let page = &picked[..picked.len().min(count)];
        let u = unit(&app, &a.unit, page.iter().flat_map(|&at| [set.record(at).start, set.record(at).end]))?;
        // several records: long values shortened, as the rest of the row is what's compared
        let whole = page.len() == 1;
        let texts: Vec<Vec<String>> = page
            .iter()
            .map(|&(s, i)| {
                let r = &set.streams[s].records[i];
                // a field its stream hasn't is empty
                set.columns.iter().zip(&radixes).map(|(c, &rf)| c.at[s].map_or(String::new(), |k| field_text(&app, &c.field, &r.fields[k], u, rf))).map(|t| if whole { t } else { shorten(t) }).collect()
            })
            .collect();
        let cut = texts.iter().flatten().any(|t| t.ends_with(" bytes)") || t.ends_with(" chars)"));
        let mut lines = vec![format!("{} of {}{matching}{of}{}", page.len(), picked.len(), if cut { " (long values shortened; one record, `count: 1` with `from`, shows them whole)" } else { "" })];
        let heads = ["stream", "n", "start", "end"];
        lines.push(heads[!multi as usize..].iter().copied().chain(set.columns.iter().map(|c| c.name.as_str())).collect::<Vec<_>>().join("\t"));
        for (&(s, n), texts) in page.iter().zip(texts) {
            let r = &set.streams[s].records[n];
            let stream = multi.then(|| set.streams[s].name.clone());
            lines.push(stream.into_iter().chain([format!("{n}\t{}\t{}", show(&app, r.start, u), show(&app, r.end, u))]).chain(texts).collect::<Vec<_>>().join("\t"));
        }
        // the record that starts the first row
        Ok(Answer { text: budget(lines.join("\n")), time: Some(set.record(page[0]).start) })
    }

    #[tool(description = "When a search expression holds, as a table of times; `show` adds values, `count_only` counts. With `@<clock> ` first, a match is a cycle whose ending edge samples it true (`@clk x`: the cycles x is high), reported at the cycle's start; without, where it becomes true (`rose(x)`: x's edges). A bus: decode it (`help decode`).", annotations(read_only_hint = true, open_world_hint = false))]
    fn find(&self, Parameters(a): Parameters<FindArgs>) -> Result<Answer, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let q = app.query(&a.expr)?;
        let shown = probes(&mut app, &a.show, &None)?;
        let forward = !a.backward;
        // backward, the search runs from the later time down to the earlier
        if let (Some(f), Some(t)) = (&a.from, &a.to) {
            let (lo, hi) = (at(&app, f)?.0, at(&app, t)?.0);
            match forward {
                true => ordered(&app, (f, lo), (t, hi))?,
                false => ordered(&app, (t, hi), (f, lo))?,
            }
        }
        let search = query::Search {
            // after (before) every tick the time stands for, so searching on from a match goes past it
            from: match &a.from {
                Some(f) => at(&app, f).map(|(lo, hi)| if forward { hi } else { lo })?,
                None if a.backward => app.trace.end() + 1,
                None => 0,
            },
            to: a.to.as_deref().map(|s| at(&app, s)).transpose()?.map(|(lo, hi)| if forward { hi } else { lo }),
            forward,
            merge: a.show.is_empty(),
            runs: if a.count_only { usize::MAX } else { a.count.unwrap_or(20).min(MAX_LINES) },
            matches: if a.count_only { 1_000_000 } else { u64::MAX },
        };
        let query::Found { runs, total, more } = query::find(&app.trace, &q, &search);
        let merge = q.clock.is_some() && search.merge;
        if runs.is_empty() {
            return Ok(Answer { text: "no match".into(), time: None });
        }
        if a.count_only {
            let first = runs.iter().map(|r| r.span().0).min().unwrap_or(0);
            let last = runs.iter().map(|r| r.span().1).max().unwrap_or(0);
            let cycles = match runs.len() {
                _ if !merge => String::new(),
                1 => " in 1 run of consecutive cycles".into(),
                r => format!(" in {r} runs of consecutive cycles"),
            };
            let more = if more { " (stopped counting there)" } else { "" };
            let u = unit(&app, &a.unit, [first, last])?;
            return Ok(Answer { text: format!("{total} matches{cycles}, first {}, last {}{more}", show(&app, first, u), show(&app, last, u)), time: Some(first) });
        }
        let u = unit(&app, &a.unit, runs.iter().flat_map(|r| [r.first, r.last]))?;
        let mut head = if merge { vec!["from", "last", "cycles"] } else { vec!["time"] };
        head.extend(shown.iter().map(|(n, _, _)| n.as_str()));
        let mut lines = vec![head.join("\t")];
        for r in &runs {
            let (lo, hi) = r.span();
            let mut line = show(&app, lo, u);
            if merge {
                line += &format!("\t{}\t{}", show(&app, hi, u), r.cycles);
            }
            let sampled = query::sampled(&app.trace, &q, lo);
            for (_, p, s) in &shown {
                line += &format!("\t{}", value(&app, *p, s, sampled));
            }
            lines.push(line);
        }
        if more {
            lines.push("(more may follow; search on from the last time)".into());
        }
        // a table of events per clock cycle is what `protocol` makes, with records to decode on
        if q.clock.is_some() && !shown.is_empty() {
            let names: Vec<String> = shown.iter().map(|(n, _, _)| n.clone()).collect();
            let (p, d) = app.protocol_of("ev", "s", &a.expr, &names)?;
            lines.push(format!("as records (`help decode`): `{}` then `{}`", p.line(), d.line()));
        }
        // the first run listed, which backward is the latest
        Ok(Answer { text: budget(lines.join("\n")), time: Some(runs[0].span().0) })
    }

    #[tool(description = "Draw signals over a time range as text, as wtf's screen does; without signals, the workspace.", annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    fn render(&self, Parameters(a): Parameters<RenderArgs>) -> Result<Answer, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let w = a.width.unwrap_or(120).clamp(40, 1000);
        let range = |app: &Workspace| -> Result<Option<(Tick, Tick)>, String> {
            match (&a.from, &a.to) {
                (None, None) => Ok(None),
                (f, t) => {
                    let from = f.as_deref().map_or(Ok(app.trace.start()), |f| at(app, f).map(|s| s.0))?;
                    let to = t.as_deref().map_or(Ok(app.trace.end()), |t| at(app, t).map(|s| s.1))?;
                    if to <= from { Err("`to` must be after `from`".into()) } else { Ok(Some((from, to))) }
                }
            }
        };
        if a.signals.is_empty() {
            if app.rows.is_empty() {
                return Err("the workspace has no signals yet: set it up with `workspace` or `command`".into());
            }
            let time = match range(&app)? {
                Some((from, to)) => {
                    app.view.start = from;
                    app.view.show_until(to);
                    a.from.as_ref().map(|_| from)
                }
                None => None,
            };
            return Ok(Answer { text: budget(screen(&mut app, w, &a.unit)?), time });
        }
        let (from, to) = range(&app)?.unwrap_or((app.trace.start(), app.trace.end().max(1)));
        let rows: Vec<Row> = probes(&mut app, &a.signals, &None)?.into_iter().map(|(_, _, s)| Row::Signal(s)).collect();
        let saved = (std::mem::replace(&mut app.rows, rows), app.sel, app.view.clone());
        // only the start marked: the workspace's marks don't belong to this drawing
        let saved_marks = std::mem::take(&mut app.marks);
        app.view.start = from;
        app.view.show_until(to);
        app.view.cursor = from;
        app.sel = 0;
        let text = screen(&mut app, w, &a.unit);
        (app.rows, app.sel, app.view) = saved;
        app.marks = saved_marks;
        // the asked start, not the trace's own
        Ok(Answer { text: budget(text?), time: a.from.as_ref().map(|_| from) })
    }

    #[tool(description = "Run one of wtf's `:` commands on the workspace: `decode`, `protocol`, rows, cursors (`help commands`). Answers with the rows it added or removed; `undo` takes a step back.", annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false))]
    fn command(&self, Parameters(a): Parameters<CommandArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let line = a.line.trim().trim_start_matches(':');
        let cmd = line.split_whitespace().next().unwrap_or("");
        if matches!(cmd, "w" | "wq" | "q" | "q!" | "e" | "help") {
            return Err(format!(":{cmd} belongs to the user's window; use `present` to show the workspace, `open` with no arguments to reopen the trace"));
        }
        let rows = |app: &Workspace| -> Vec<String> {
            let row = |r: &Row| match r {
                Row::Signal(s) => app.trace.vars[s.var].path.clone(),
                Row::Group { name, .. } => format!("group {name}"),
            };
            app.rows.iter().map(row).collect()
        };
        let before = rows(&app);
        let msg = app.undoable(|ws| wtf_core::command::run(ws, line))?;
        let after = rows(&app);
        // what changed, so a pattern that caught more (or less) than meant shows
        let mut out = Vec::new();
        for (what, from, to) in [("added", &before, &after), ("removed", &after, &before)] {
            let d: Vec<&str> = to.iter().filter(|r| !from.contains(r)).map(String::as_str).collect();
            if !d.is_empty() {
                let more = if d.len() > 20 { format!(", and {} more", d.len() - 20) } else { String::new() };
                out.push(format!("{what} {}: {}{more}", d.len(), d[..d.len().min(20)].join(", ")));
            }
        }
        out.extend(msg.map(|m| m.0));
        if cmd == "decode" {
            let protocol = line.split_whitespace().nth(2).unwrap_or("<protocol>");
            let pair = if protocol == "pair" { "" } else { "; `pair` matches a stream with another (`help decode`)" };
            out.push(format!("`records <stream>` lists one, `records <name>` its records and violations in order; `protocols {protocol}` says what each stream and field is{pair}"));
        }
        Ok(if out.is_empty() { "done".into() } else { out.join("\n") })
    }

    #[tool(description = "Describe the workspace in words: rows, cursors with the time between them, visible range. Drawn: `render`; the user's window: `user_view`.", annotations(read_only_hint = true, open_world_hint = false))]
    fn view(&self, Parameters(a): Parameters<ViewArgs>) -> Result<String, String> {
        let w = current(self)?;
        let app = lock(&w);
        let u = a.unit.as_deref().map(time::unit_named).transpose()?;
        Ok(budget(describe(&app, u)))
    }

    #[tool(description = "Set up the whole workspace in one call: rows, cursors, range, decodes (`help workspace`). Answers as `view` does.", annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false))]
    fn workspace(&self, Parameters(a): Parameters<WorkspaceArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let text = declared(&mut app, &a)?;
        // decodes made before with `command` stay (spec §15)
        let (msg, warned) = app.present(&text, true)?;
        let warn = if warned { format!("({msg})\n") } else { String::new() };
        Ok(budget(format!("{}{warn}", describe(&app, None))))
    }

    #[tool(description = "Show your workspace to the user as evidence: it replaces the view of every wtf window on this trace (theirs is one `u` away), or with none open writes `<trace>.agent.session.toml` for them to open.", annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    fn present(&self) -> Result<String, String> {
        let w = current(self)?;
        let app = lock(&w);
        if app.rows.is_empty() {
            return Err("the workspace has no signals yet: set it up with `workspace` or `command`".into());
        }
        let text = session::to_shared_toml(&app);
        // every window on the trace: they are all the user's (spec §15 "Present")
        let shown: Vec<Result<String, String>> = live::on_trace("window", &app.trace_path).iter().map(|id| live::request(id, live::Request::Put(text.clone()))).collect();
        match shown.len() {
            0 => {
                // with the trail, so the user sees how the agent got there (spec §15 "Present")
                let path = session::agent_path(&app.trace_path);
                session::write(&path, &session::to_link_toml(&app))?;
                Ok(format!(
                    "no wtf window is open on this trace; written to {}. Tell the user: `wtf {}` shows it (their own view is one `u` away)",
                    path.display(),
                    app.trace_path.display()
                ))
            }
            1 => shown.into_iter().next().unwrap().map(|m| format!("shown in the user's window: {m}")),
            n => {
                let answers: Vec<String> = shown.into_iter().map(|r| r.unwrap_or_else(|e| format!("error: {e}"))).collect();
                Ok(format!("shown in the user's {n} windows on this trace: {}", answers.join("; ")))
            }
        }
    }

    #[tool(description = "What the user is looking at in their wtf window on this trace, worded as `view` words the workspace, to start from what they see. Changes nothing unless `adopt`: that replaces the workspace's rows, cursors and range with the user's, dropping yours; `command` `undo` brings them back.", annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false))]
    fn user_view(&self, Parameters(a): Parameters<UserViewArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let id = live::window_on(&app.trace_path).ok_or("no wtf window is open on this trace")?;
        let (described, toml) = live::get(&id)?;
        if a.adopt {
            // the window's copy of the trail isn't the agent's own
            let trail = std::mem::take(&mut app.trail);
            let r = app.present(&toml, true);
            app.trail = trail;
            let (msg, warned) = r?;
            let warn = if warned { format!("; {msg}") } else { String::new() };
            return Ok(format!("{described}\n(adopted into the workspace{warn})"));
        }
        Ok(budget(described))
    }
}

/// The screen as text, without the status line (it belongs to the interactive screen), headed by
/// the range it shows.
fn screen(app: &mut Workspace, w: u16, asked: &Option<String>) -> Result<String, String> {
    let text = draw::waveform(app, w);
    let (from, to) = (app.view.start, app.view.last_visible().min(app.trace.end()));
    let u = unit(app, asked, [from, to])?;
    // the end is where a zoom step happens to stop: rounded, unlike the times the agent set
    let rounded = Unit { decimals: Some(3), ..u };
    Ok(format!("{} - {}:\n{}", show(app, from, u), show(app, to, rounded), text.trim_end_matches('\n')))
}

/// The answer to `GET` on the link: the session in words, `---`, then as another window takes it,
/// with its trail (spec §15 "Joining").
pub fn link_answer(app: &Workspace) -> String {
    live::get_answer(&describe(app, None), &session::to_link_toml(app))
}

/// The workspace in words, for `view`, its times in `unit` (default: the one that fits them).
pub fn describe(app: &Workspace, unit: Option<usize>) -> String {
    let names = wtf_core::rows::display_names(&app.trace, &app.rows);
    let mut out = String::from("rows (> selected):\n");
    if app.rows.is_empty() {
        out += "  (none)\n";
    }
    for (i, (r, n)) in app.rows.iter().zip(&names).enumerate() {
        let mark = if i == app.sel { ">" } else { " " };
        let extra = match r {
            Row::Signal(s) => {
                let var = &app.trace.vars[s.var];
                let w = var.width.unwrap_or(0);
                let radix = if w > 1 { format!("  {}", radix::name(s.look.radix, s.look.frac, w)) } else { String::new() };
                let color = s.look.color.map(|c| format!("  {}", rows::COLORS[c])).unwrap_or_default();
                // the lines under it with a look of their own, as `fields` declares them
                let fields = match &s.looks[..] {
                    [] => String::new(),
                    looks => {
                        let one = |&(v, l): &(usize, rows::Look)| {
                            let w = app.trace.vars[v].width.unwrap_or(0);
                            let r = if w > 1 { format!(":{}", radix::name(l.radix, l.frac, w)) } else { String::new() };
                            let c = l.color.map(|c| format!(":{}", rows::COLORS[c])).unwrap_or_default();
                            format!("{}{r}{c}", wtf_core::session::open_name(app, s.var, v))
                        };
                        format!("  (fields: {})", looks.iter().map(one).collect::<Vec<_>>().join(", "))
                    }
                };
                let bits = match &s.open[..] {
                    [] => String::new(),
                    open => format!("  (open: {})", open.iter().map(|&v| wtf_core::session::open_name(app, s.var, v)).collect::<Vec<_>>().join(", ")),
                };
                format!("  {}{radix}{color}{bits}{fields}", var.path)
            }
            Row::Group { collapsed: true, .. } => "  (collapsed)".into(),
            Row::Group { .. } => String::new(),
        };
        out += &format!("{mark} {n}{extra}\n");
    }
    let mut cursors = app.cursor_list();
    cursors.sort_by_key(|&(i, t)| (t, i));
    let end = app.view.last_visible().min(app.trace.end());
    let u = unit_of(app, unit, cursors.iter().map(|c| c.1).chain([app.view.start, end]));
    let list: Vec<String> = cursors
        .iter()
        .enumerate()
        .map(|(k, &(i, t))| {
            let dt = if k > 0 { format!(", +{} after {}", show(app, t - cursors[k - 1].1, u), cursors[k - 1].0 + 1) } else { String::new() };
            let l = &app.labels[i];
            let name = if l.name.is_empty() { String::new() } else { format!(", \"{}\"", l.name) };
            let note = if l.note.is_empty() { String::new() } else { format!(": {}", l.note) };
            format!("  {}: {}{dt}{name}{note}", i + 1, show(app, t, u))
        })
        .collect();
    // the end is where a zoom step happens to stop: rounded, unlike the times the agent set
    let rounded = Unit { decimals: Some(3), ..u };
    format!("{out}cursors, in time:\n{}\nvisible: {} - {}\n", list.join("\n"), show(app, app.view.start, u), show(app, end, rounded))
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Wtf {
    /// Every call as the router makes it, then a step on the current workspace's trail (spec §15
    /// "Trail"), so the user's window can show what the agent did.
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let (tool, args) = (request.name.to_string(), request.arguments.clone());
        let mut r = self.tool_router.call(rmcp::handler::server::tool::ToolCallContext::new(self, request, context)).await;
        if let Ok(rmcp::model::CallToolResponse::Complete(res)) = &mut r {
            // the time is the trail's: out of `_meta` before the client sees the result
            let time = trail::take_time(res);
            if let Ok(w) = current(self) {
                let text: String = res.content.iter().filter_map(|c| c.as_text()).map(|t| t.text.as_str()).collect();
                let mut app = lock(&w);
                if let Some(step) = trail::step(&tool, args.as_ref(), &text, res.is_error == Some(true), time) {
                    app.add_step(step);
                }
            }
        }
        r
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("wtf", env!("CARGO_PKG_VERSION")))
            .with_instructions(include_str!("mcp/guide/instructions.md").trim_end())
    }
}

/// Serve on stdin / stdout until the client disconnects.
pub fn serve() -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let server = Wtf::new().serve(rmcp::transport::stdio()).await.map_err(|e| e.to_string())?;
        server.waiting().await.map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_live_and_headless() {
        // a trace of its own, so its window socket and session file belong to this test
        let dir = std::env::temp_dir().join(format!("wtf-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // sessions show the full path, links resolved (macOS's temp dir is under the /var link)
        let dir = dir.canonicalize().unwrap();
        let trace = dir.join("simple.vcd");
        std::fs::copy("tests/data/simple.vcd", &trace).unwrap();
        let agent = Wtf::new();
        let cmd = |l: &str| agent.command(Parameters(CommandArgs { line: l.into() }));
        assert!(agent.info().unwrap_err().starts_with("no trace is open"));
        let opened = agent.open(Parameters(OpenArgs { trace: Some(trace.display().to_string()), session: None })).unwrap();
        let id = format!("{}-1", std::process::id());
        assert!(opened.starts_with(&format!("workspace {id} (the user's wtf windows on this trace see your steps; :follow there shows the workspace)\nsimple.vcd\n")), "{opened}");
        assert!(agent.sessions().contains(&format!("{id}  agent  {}  (yours, current)", trace.display())), "{}", agent.sessions());
        assert!(agent.open(Parameters(OpenArgs { trace: None, session: Some("nope".into()) })).is_err());

        // headless: no window, so present writes a session file
        cmd("add top.cpu.data").unwrap();
        assert_eq!(cmd("q").unwrap_err(), ":q belongs to the user's window; use `present` to show the workspace, `open` with no arguments to reopen the trace");
        assert_eq!(cmd("add nope").unwrap_err(), "no match: nope");
        let r = agent.present().unwrap();
        assert!(r.starts_with("no wtf window is open") && dir.join("simple.vcd.agent.session.toml").exists(), "{r}");
        assert_eq!(agent.user_view(Parameters(UserViewArgs { adopt: false })).unwrap_err(), "no wtf window is open on this trace");

        // live: a window on the same trace, answering as its main loop does
        let mut window = crate::tui::app::App::new(Workspace::open(&trace).unwrap());
        wtf_core::command::run(&mut window.ws, "add top.clk").unwrap();
        let (_link, rx) = live::window(&format!("test-mcp-{}", std::process::id()), &trace).unwrap();
        let window = std::thread::spawn(move || {
            for _ in 0..2 {
                let inc = rx.recv().unwrap();
                let answer = match inc.request {
                    live::Request::Get => Ok(link_answer(&window.ws)),
                    live::Request::Put(text) => window.present(&text),
                };
                inc.reply.send(answer).unwrap();
            }
            window
        });
        cmd("add top.cpu.busy").unwrap();
        cmd("select data busy").unwrap();
        cmd("group evidence").unwrap();
        cmd("cursor 2 20ns").unwrap();
        cmd("radix dec").unwrap();
        let view = agent.view(Parameters(ViewArgs { unit: None })).unwrap();
        assert!(view.contains("2: 20ns, +20ns after 1"), "{view}");
        // what a window on the trace reads from the workspace (spec §15 "Joining")
        let attached = live::request(&id, live::Request::Get).unwrap();
        assert!(attached.starts_with(&view) && attached.contains("\nto = \"79ns\"\n"), "{attached}");
        assert!(agent.render(Parameters(RenderArgs { signals: vec![], from: None, to: None, width: Some(80), unit: None })).unwrap().text.contains("▾ evidence"));
        assert_eq!(agent.present().unwrap(), "shown in the user's window: agent: presented 2 signals, 2 cursors");
        // adopting drops a row added since, and `undo` brings it back
        cmd("add top.dma.state").unwrap();
        let seen = agent.user_view(Parameters(UserViewArgs { adopt: true })).unwrap();
        assert!(seen.contains("▾ evidence") && seen.contains("2: 20ns") && seen.contains("adopted"), "{seen}");
        assert!(!agent.view(Parameters(ViewArgs { unit: None })).unwrap().contains("top.dma.state"));
        cmd("undo").unwrap();
        assert!(agent.view(Parameters(ViewArgs { unit: None })).unwrap().contains("top.dma.state"));
        // the user's own view comes back with u; the session file wasn't touched
        let mut window = window.join().unwrap();
        assert!(window.ws.modified());
        window.ws.undo().unwrap();
        assert_eq!(wtf_core::rows::display_names(&window.ws.trace, &window.ws.rows), ["clk"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
