//! `wtf --mcp`: an MCP server over stdin / stdout, so an AI agent can open traces, list signals,
//! read values, search with wtf's expressions, look at waveforms drawn as text, and show the user
//! what it found. Each trace it opens is a workspace, listening for `wtf --attach` like a window.

use wtf_core::workspace::Workspace;
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
pub use args::*;
use declare::declared;

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
    let ws = st.workspaces.get(st.current).ok_or("no trace is open: `open` one (`sessions` lists the user's windows and their traces)")?;
    Ok(ws.app.clone())
}

/// The ticks a time given stands for, first and last: a rounded time (`2595ns`) is every tick that
/// prints as it (spec §15 "Times").
fn at(app: &Workspace, s: &str) -> Result<(Tick, Tick), String> {
    let (lo, hi) = time::span(s, app.trace.fs_per_tick)?;
    Ok((lo.min(app.trace.end()), hi.min(app.trace.end())))
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
fn field_radixes(s: &wtf_core::stream::Stream, asked: Option<&str>) -> Result<Vec<Option<(usize, u32)>>, String> {
    let mut out = vec![None; s.schema.len()];
    for item in asked.unwrap_or("").split([',', ' ']).filter(|i| !i.is_empty()) {
        let (field, r) = match with_radix(item) {
            (f, Some(r)) => (Some(f), r),
            (r, None) => (None, r),
        };
        let numbers = |f: &wtf_core::stream::Field| f.width.filter(|_| !f.time);
        match field {
            None => {
                radix::parse_name(r).ok_or(format!("unknown radix: {r}"))?;
                for (k, f) in s.schema.iter().enumerate() {
                    if let Some(Ok(rf)) = numbers(f).map(|w| radix_for(r, w, false, &f.name)) {
                        out[k] = Some(rf);
                    }
                }
            }
            Some(name) => {
                let Some(k) = s.schema.iter().position(|f| f.name == name) else {
                    let names: Vec<&str> = s.schema.iter().map(|f| f.name.as_str()).collect();
                    return Err(format!("{}: no field {name} (fields: {})", s.name, names.join(", ")));
                };
                let f = &s.schema[k];
                let w = numbers(f).ok_or(format!("{name} isn't a number: a time or text takes no radix"))?;
                out[k] = Some(radix_for(r, w, false, name)?);
            }
        }
    }
    Ok(out)
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
                format!("{path} ({width}, {n} signals)")
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
    if !radix::fits(r, width, named) || need.is_some_and(|n| n != width) {
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
        let width = if p.1.is_some() { 1 } else { app.trace.vars[p.0].width.unwrap_or(0) };
        let named = p.1.is_none() && app.trace.vars[p.0].names.is_some();
        let (r, frac) = match own.or(radix_name.as_deref()) {
            None => (radix::default_for(named, p.1.is_none() && app.trace.vars[p.0].signed), 0),
            Some(r) => radix_for(r, width, named, n)?,
        };
        out.push((n.to_string(), p, Sig { var: p.0, radix: r, frac, grouped: false, bits: 0, color: None }));
    }
    let vars: Vec<usize> = out.iter().map(|(_, p, _)| p.0).collect();
    app.trace.load(&vars);
    Ok(out)
}

fn value(app: &Workspace, p: search::Probe, s: &Sig, t: Tick) -> String {
    match app.trace.value_of(p.0, p.1, t) {
        Some(Val::Bits(b)) if b.len() == 1 => b.to_uppercase(),
        Some(v) => wtf_core::rows::label(&app.trace.vars[s.var], s, &v),
        None => "-".into(),
    }
}

#[tool_router]
impl Wtf {
    #[tool(description = "The open sessions: the user's wtf windows and agents' workspaces, with ids and traces.")]
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

    #[tool(description = "Open a trace in a new workspace, the current one from then on. Answers with `info`, which lists the interfaces to decode.")]
    fn open(&self, Parameters(a): Parameters<OpenArgs>) -> Result<String, String> {
        let app = Arc::new(Mutex::new(Workspace::open(std::path::Path::new(&a.trace))?));
        let mut st = lock(&self.state);
        st.opened += 1;
        let id = format!("{}-{}", std::process::id(), st.opened);
        let shared_app = app.clone();
        let link = live::listen(&id, "agent", std::path::Path::new(&a.trace), move |req| match req {
            live::Request::Get => Ok(link_answer(&lock(&shared_app))),
            live::Request::Put(_) => Err("an agent's workspace takes no sessions".into()),
        })
        .map_err(|e| format!("can't listen for wtf --attach: {e}"))?;
        st.workspaces.push(Opened { id: id.clone(), app, _link: link });
        st.current = st.workspaces.len() - 1;
        drop(st);
        Ok(format!("workspace {id} (wtf --attach {id})\n{}", self.info()?))
    }

    #[tool(name = "use", description = "Make another of this server's workspaces the current one.")]
    fn use_session(&self, Parameters(a): Parameters<UseArgs>) -> Result<String, String> {
        let mut st = lock(&self.state);
        let i = st.workspaces.iter().position(|w| w.id == a.session).ok_or(format!("{} is not one of your workspaces; see `sessions`", a.session))?;
        st.current = i;
        drop(st);
        self.info()
    }

    #[tool(description = "What can be decoded; with a name, its roles, params, streams and fields (`help decode`).")]
    fn protocols(&self, Parameters(a): Parameters<ProtocolsArgs>) -> Result<String, String> {
        // works before a trace is open, without the session's own protocols
        let own = current(self).ok().map(|w| lock(&w).protocols.clone()).unwrap_or_default();
        match a.name {
            Some(n) => protocol::describe(&n, &own),
            None => Ok(protocol::list(&own)),
        }
    }

    #[tool(description = "The reference: a topic per concept (names, times, radix, expressions, decode, records, workspace, commands); none lists them.")]
    fn help(&self, Parameters(a): Parameters<HelpArgs>) -> Result<String, String> {
        help::answer(a.topic.as_deref())
    }

    #[tool(description = "The trace: file, time range, signals, and the interfaces a built-in protocol decodes, each with its `decode` line.")]
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

    #[tool(description = "List signals with their widths; arrays and generate instances fold to one line (`gen(0..4).x`).")]
    fn signals(&self, Parameters(a): Parameters<SignalsArgs>) -> Result<String, String> {
        let w = current(self)?;
        let app = lock(&w);
        let p = a.pattern.to_lowercase();
        let found = collapse(
            app.trace
                .vars
                .iter()
                .filter(|v| if p.contains('*') { wtf_core::command::glob(&a.pattern, &v.path) } else { v.path.to_lowercase().contains(&p) })
                .map(|v| (v.path.as_str(), v.width.map_or("real or string".into(), |w| format!("{w} bit{}", if w == 1 { "" } else { "s" })))),
        );
        let more = found.len().saturating_sub(MAX_LINES);
        let mut out = found.into_iter().take(MAX_LINES).collect::<Vec<_>>().join("\n");
        if more > 0 {
            out += &format!("\n... {more} more; narrow the pattern");
        }
        Ok(if out.is_empty() { wtf_core::command::no_match(&a.pattern) } else { budget(out) })
    }

    #[tool(description = "Values of signals over a time range: the value at `from`, then each change, as `time value` lines.")]
    fn values(&self, Parameters(a): Parameters<ValuesArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let (from, to) = (at(&app, &a.from)?.0, at(&app, &a.to)?.1);
        // the cap is for the whole answer: one busy signal mustn't hide behind a quiet one
        let (mut lists, mut budget_left, mut cut) = (Vec::new(), MAX_LINES, false);
        for (name, p, s) in probes(&mut app, &a.signals, &a.radix)? {
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
        Ok(budget(out))
    }

    #[tool(description = "A decoded stream's records as a table: n, start (the rising edge that starts the record's first clock cycle), end (the edge that samples its last) and the fields; `where`, `group_by`, `count_only` (`help records`).")]
    fn records(&self, Parameters(a): Parameters<RecordsArgs>) -> Result<String, String> {
        let w = current(self)?;
        let app = lock(&w);
        let Some(s) = app.streams.iter().find(|s| s.name == a.stream) else {
            // an interface `open` listed, not decoded yet (`top.s_axi.wr`, `s_axi.wr`): its line
            let (base, part) = a.stream.rsplit_once('.').unwrap_or((&a.stream, ""));
            let named = |f: &protocol::Found| [a.stream.as_str(), base].iter().any(|&n| n == f.name() || n == f.prefix.trim_end_matches(['_', '.']));
            let found: Vec<protocol::Found> = protocol::detect(&app.trace).into_iter().filter(named).collect();
            if !found.is_empty() {
                let how: Vec<String> = found
                    .iter()
                    .take(3)
                    .map(|f| match part {
                        "" => format!("`{}` makes its streams (`protocols {}`)", f.decode_line(), f.protocol),
                        _ => format!("`{}` makes {}.{part}", f.decode_line(), f.name()),
                    })
                    .collect();
                return Err(format!("unknown stream: {}: its interface isn't decoded yet; with `command`, {}", a.stream, how.join("; or ")));
            }
            let names: Vec<&str> = app.streams.iter().map(|s| s.name.as_str()).collect();
            return Err(match names.len() {
                0 => format!("unknown stream: {} (none yet; make one with `command`: `decode <name> <protocol> ...`)", a.stream),
                _ => format!("unknown stream: {} (streams: {})", a.stream, names.join(", ")),
            });
        };
        let from = a.from.as_deref().map(|f| at(&app, f)).transpose()?.map_or(0, |s| s.0);
        let to = a.to.as_deref().map(|t| at(&app, t)).transpose()?.map_or(app.trace.end(), |s| s.1);
        let (numbers, of) = s.select(from, to, a.filter.as_deref(), app.trace.fs_per_tick)?;
        let radixes = field_radixes(s, a.radix.as_deref())?;
        let picked: Vec<&wtf_core::stream::Record> = numbers.iter().map(|&i| &s.records[i]).collect();
        // with `where`: how many match, of how many there are
        let (matching, of) = match a.filter {
            Some(_) => (" matching", format!(", of {of}")),
            None => ("", String::new()),
        };
        let (Some(first), Some(last)) = (picked.first(), picked.last()) else {
            // a check's empty answer is its result, not a sign it didn't run
            return Ok(match (matching, s.violations) {
                ("", true) => "no records: no violations".into(),
                ("", false) => "no records".into(),
                _ => format!("no matching records{of}"),
            });
        };
        let count = a.count.unwrap_or(20).min(MAX_LINES);
        if !a.group_by.is_empty() {
            let groups = s.group_by(&picked, &a.group_by)?;
            let what = format!("{} distinct, of {}{matching} records{of}", groups.len(), picked.len());
            if a.count_only {
                return Ok(what);
            }
            let page = &groups[..groups.len().min(count)];
            let u = unit(&app, &a.unit, page.iter().flat_map(|g| [g.first, g.last]))?;
            let mut lines = vec![format!("{} of {what}", page.len())];
            let fields: Vec<usize> = a.group_by.iter().filter_map(|n| s.schema.iter().position(|f| f.name == *n)).collect();
            for g in page {
                let values: Vec<String> = fields.iter().zip(&g.values).map(|(&k, v)| format!("{}={}", s.schema[k].name, field_text(&app, &s.schema[k], v, u, radixes[k]))).collect();
                lines.push(format!("{} × {} ({} .. {})", g.count, values.join(" "), show(&app, g.first, u), show(&app, g.last, u)));
            }
            return Ok(budget(lines.join("\n")));
        }
        if a.count_only {
            let u = unit(&app, &a.unit, [first.start, last.start])?;
            return Ok(format!("{}{matching} records{of}, first {}, last {}", picked.len(), show(&app, first.start, u), show(&app, last.start, u)));
        }
        let page = &picked[..picked.len().min(count)];
        let u = unit(&app, &a.unit, page.iter().flat_map(|r| [r.start, r.end]))?;
        // several records: long values shortened, as the rest of the row is what's compared
        let whole = page.len() == 1;
        let texts: Vec<Vec<String>> = page.iter().map(|r| s.schema.iter().zip(&r.fields).zip(&radixes).map(|((f, v), &rf)| field_text(&app, f, v, u, rf)).map(|t| if whole { t } else { shorten(t) }).collect()).collect();
        let cut = texts.iter().flatten().any(|t| t.ends_with(" bytes)") || t.ends_with(" chars)"));
        let mut lines = vec![format!("{} of {}{matching}{of}{}", page.len(), picked.len(), if cut { " (long values shortened; one record, `count: 1` with `from`, shows them whole)" } else { "" })];
        lines.push(["n", "start", "end"].into_iter().chain(s.schema.iter().map(|f| f.name.as_str())).collect::<Vec<_>>().join("\t"));
        for ((r, n), texts) in page.iter().zip(&numbers).zip(texts) {
            lines.push(std::iter::once(format!("{n}\t{}\t{}", show(&app, r.start, u), show(&app, r.end, u))).chain(texts).collect::<Vec<_>>().join("\t"));
        }
        Ok(budget(lines.join("\n")))
    }

    #[tool(description = "Values of signals at one time.")]
    fn value_at(&self, Parameters(a): Parameters<ValueAtArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        // after everything that prints as the time, and a change among it said
        let (lo, t) = at(&app, &a.time)?;
        let lines: Vec<String> = probes(&mut app, &a.signals, &a.radix)?
            .iter()
            .map(|(n, p, s)| match (value(&app, *p, s, lo), value(&app, *p, s, t)) {
                (early, v) if early != v => format!("{n} = {v} ({early} earlier in {})", a.time.trim()),
                (_, v) => format!("{n} = {v}"),
            })
            .collect();
        // before there is a unit, in the asked time's own: `20ns` is the span's end, 20.000499ns
        let own = (a.unit.is_none() && app.time_unit().is_none() && time::has_unit(&a.time))
            .then(|| time::unit_named(a.time.trim().trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')))
            .transpose()?;
        let u = match own {
            Some(index) => Unit { index, decimals: Some(3) },
            None => unit(&app, &a.unit, [t])?,
        };
        Ok(budget(format!("at {}:\n{}", show(&app, t, u), lines.join("\n"))))
    }

    #[tool(description = "The times a search expression matches, as a table. With `@clk ` first, a match is a clock cycle whose ending edge samples it true, reported at the cycle's starting edge; without, where the expression becomes true, so back-to-back handshakes are one match. `show` adds values (and the `protocol` line making them records), `count_only` counts (`help expressions`). For a bus, decode it instead (`help decode`).")]
    fn find(&self, Parameters(a): Parameters<FindArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let q = app.query(&a.expr)?;
        let shown = probes(&mut app, &a.show, &None)?;
        let forward = !a.backward;
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
            return Ok("no match".into());
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
            return Ok(format!("{total} matches{cycles}, first {}, last {}{more}", show(&app, first, u), show(&app, last, u)));
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
        if let (Some(_), Some((clock, when))) = (q.clock, a.expr.trim().strip_prefix('@').and_then(|e| e.split_once(char::is_whitespace)))
            && !shown.is_empty()
        {
            // a field is its signal's last name; two alike take their scope's (`cpu_state`), then a number
            let word = |s: &str| -> String { s.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect() };
            let mut fields: Vec<String> = shown.iter().map(|(n, _, _)| word(n.rsplit('.').next().unwrap_or(n))).collect();
            let alike = |fields: &[String]| -> Vec<usize> { (0..fields.len()).filter(|&k| fields.iter().filter(|f| **f == fields[k]).count() > 1).collect() };
            for k in alike(&fields) {
                let mut parts = shown[k].0.rsplit('.');
                let (last, scope) = (parts.next().unwrap_or(""), parts.next().map(word).unwrap_or_default());
                fields[k] = if scope.is_empty() { word(last) } else { format!("{scope}_{}", word(last)) };
            }
            for k in alike(&fields) {
                fields[k] = format!("{}_{k}", fields[k]);
            }
            let fields: Vec<String> = fields.iter().zip(&shown).map(|(f, (n, _, _))| format!("{f}={n}")).collect();
            lines.push(format!("as records (`help decode`): `protocol s clock={clock} when={} {}`", when.trim(), fields.join(" ")));
        }
        Ok(budget(lines.join("\n")))
    }

    #[tool(description = "Draw signals over a time range as text, as wtf's screen does; without signals, the workspace.")]
    fn render(&self, Parameters(a): Parameters<RenderArgs>) -> Result<String, String> {
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
            if let Some((from, to)) = range(&app)? {
                app.view.start = from;
                app.view.show_until(to);
            }
            return Ok(budget(screen(&mut app, w, &a.unit)?));
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
        Ok(budget(text?))
    }

    #[tool(description = "Run one of wtf's `:` commands on the workspace: `decode`, `protocol`, rows, cursors (`help commands`).")]
    fn command(&self, Parameters(a): Parameters<CommandArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let line = a.line.trim().trim_start_matches(':');
        let cmd = line.split_whitespace().next().unwrap_or("");
        if matches!(cmd, "w" | "wq" | "q" | "q!" | "e" | "help") {
            return Err(format!(":{cmd} belongs to the user's window; use `present` to show the workspace, `reload` to reopen the trace"));
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
            out.push(format!("`records <stream>` lists one; `protocols {protocol}` says what each stream and field is{pair}"));
        }
        Ok(if out.is_empty() { "done".into() } else { out.join("\n") })
    }

    #[tool(description = "Describe the workspace: rows, cursors with the time between them, visible range.")]
    fn view(&self, Parameters(a): Parameters<ViewArgs>) -> Result<String, String> {
        let w = current(self)?;
        let app = lock(&w);
        let u = a.unit.as_deref().map(time::unit_named).transpose()?;
        Ok(budget(describe(&app, u)))
    }

    #[tool(description = "Set up the whole workspace in one call: rows, cursors, range, decodes (`help workspace`). Answers as `view` does.")]
    fn workspace(&self, Parameters(a): Parameters<WorkspaceArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let text = declared(&mut app, &a)?;
        let (msg, warned) = app.present(&text)?;
        let warn = if warned { format!("({msg})\n") } else { String::new() };
        Ok(budget(format!("{}{warn}", describe(&app, None))))
    }

    #[tool(description = "Show the workspace to the user, in their wtf window on this trace or as a session file.")]
    fn present(&self) -> Result<String, String> {
        let w = current(self)?;
        let app = lock(&w);
        if app.rows.is_empty() {
            return Err("the workspace has no signals yet: set it up with `workspace` or `command`".into());
        }
        let text = shared(&app);
        match live::window_on(&app.trace_path) {
            Some(id) => live::request(&id, live::Request::Put(text)).map(|m| format!("shown in the user's window: {m}")),
            None => {
                let mut path = app.trace_path.as_os_str().to_owned();
                path.push(".agent.session.toml");
                let path = std::path::PathBuf::from(path);
                session::write(&path, &text)?;
                Ok(format!("no wtf window is open on this trace; written to {0} (open it with: wtf -S {0} {1})", path.display(), app.trace_path.display()))
            }
        }
    }

    #[tool(description = "What the user is looking at in their wtf window on this trace; `adopt` copies it into the workspace.")]
    fn user_view(&self, Parameters(a): Parameters<UserViewArgs>) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        let id = live::window_on(&app.trace_path).ok_or("no wtf window is open on this trace")?;
        let answer = live::request(&id, live::Request::Get)?;
        let (described, toml) = answer.split_once("\n---\n").unwrap_or((&answer, ""));
        if a.adopt {
            app.marks = [None; 9];
            app.rows.clear();
            let warn = session::apply(&mut app, toml)?;
            if !warn.is_empty() {
                return Ok(format!("{described}\n(adopted into the workspace; {})", warn.join("; ")));
            }
            return Ok(format!("{described}\n(adopted into the workspace)"));
        }
        Ok(budget(described.to_string()))
    }

    #[tool(description = "Reopen the trace file, after the simulation was run again.")]
    fn reload(&self) -> Result<String, String> {
        let w = current(self)?;
        let mut app = lock(&w);
        app.reload()?;
        let end = time::format_exact(app.trace.end(), app.trace.fs_per_tick);
        Ok(format!("reloaded {}: now 0 - {end}", app.file_name))
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

/// window of another width shows the same stretch.
pub fn shared(app: &Workspace) -> String {
    let to = format!("to = \"{}\"\n", time::format_exact(app.view.last_visible().min(app.trace.end()), app.trace.fs_per_tick));
    session::to_toml(app).replacen("\n[cursors]", &format!("{to}\n[cursors]"), 1)
}

/// The answer to `GET` on the link: the session in words, `---`, then as `shared`.
pub fn link_answer(app: &Workspace) -> String {
    format!("{}\n---\n{}", describe(app, None), shared(app))
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
                let radix = if w > 1 { format!("  {}", radix::name(s.radix, s.frac, w)) } else { String::new() };
                let color = s.color.map(|c| format!("  {}", rows::COLORS[c])).unwrap_or_default();
                let bits = if s.bits > 0 { "  (expanded)" } else { "" };
                format!("  {}{radix}{color}{bits}", var.path)
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
            format!("  {}: {}{dt}", i + 1, show(app, t, u))
        })
        .collect();
    // the end is where a zoom step happens to stop: rounded, unlike the times the agent set
    let rounded = Unit { decimals: Some(3), ..u };
    format!("{out}cursors, in time:\n{}\nvisible: {} - {}\n", list.join("\n"), show(app, app.view.start, u), show(app, end, rounded))
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Wtf {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("wtf", env!("CARGO_PKG_VERSION")))
            .with_instructions(
            "wtf reads VCD and FST waveforms from digital simulations. The usual path: `open` a trace \
             (`sessions` lists the user's windows and their traces); it lists the interfaces a built-in \
             protocol decodes, each with its line. Decode first, with `command` (`decode bus axilite \
             clock=aclk prefix=s_axi_`): records are exact and the handshake checks built in. Read them \
             with `records` (`where`, `group_by`, `count_only`); `find` for what no protocol covers, \
             `values` / `value_at` for values. To show evidence, declare the `workspace`, check it with \
             `view` / `render`, then `present` it. `help` has the reference: names, times, radix, \
             expressions, decode, records, workspace, commands.",
        )
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
        let opened = agent.open(Parameters(OpenArgs { trace: trace.display().to_string() })).unwrap();
        let id = format!("{}-1", std::process::id());
        assert!(opened.starts_with(&format!("workspace {id} (wtf --attach {id})\nsimple.vcd\n")), "{opened}");
        assert!(agent.sessions().contains(&format!("{id}  agent  {}  (yours, current)", trace.display())), "{}", agent.sessions());
        assert!(agent.use_session(Parameters(UseArgs { session: "nope".into() })).is_err());

        // headless: no window, so present writes a session file
        cmd("add top.cpu.data").unwrap();
        assert_eq!(cmd("q").unwrap_err(), ":q belongs to the user's window; use `present` to show the workspace, `reload` to reopen the trace");
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
        // what `wtf --attach` gets from the workspace
        let attached = live::request(&id, live::Request::Get).unwrap();
        assert!(attached.starts_with(&view) && attached.contains("\nto = \"79ns\"\n"), "{attached}");
        assert!(agent.render(Parameters(RenderArgs { signals: vec![], from: None, to: None, width: Some(80), unit: None })).unwrap().contains("▾ evidence"));
        assert_eq!(agent.present().unwrap(), "shown in the user's window: agent: presented 2 signals, 2 cursors");
        let seen = agent.user_view(Parameters(UserViewArgs { adopt: true })).unwrap();
        assert!(seen.contains("▾ evidence") && seen.contains("2: 20ns") && seen.contains("adopted"), "{seen}");
        // the user's own view comes back with u; the session file wasn't touched
        let mut window = window.join().unwrap();
        assert!(window.ws.modified());
        window.ws.undo().unwrap();
        assert_eq!(wtf_core::rows::display_names(&window.ws.trace, &window.ws.rows), ["clk"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
