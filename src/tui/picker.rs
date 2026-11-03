//! The picker (spec §5 "Picker"): one floating fuzzy list with a preview, over the signals, the
//! marks, a stream's records, the `:` commands, the agents' sessions and the agent's trail.

use crate::draw::{self, gl, pal, put};
use crate::tui::app::{App, Target};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::time::Instant;
use wtf_core::rows::{self, Look, Row, Sig};
use wtf_core::time::{self, Tick};

/// The sources, in the order `Ctrl-s` goes through them.
pub const SOURCES: [&str; 7] = ["signals", "marks", "records", "commands", "sessions", "trail", "interfaces"];

/// What `Enter` does with an entry.
#[derive(Clone)]
pub enum Act {
    /// Add the signal as a row.
    Var(usize),
    /// Jump to cursor n (0 is the cursor itself).
    Mark(usize),
    /// Jump to a record's start and select its stream's row.
    Record(Tick),
    /// Put the command on the command line.
    Command(String),
    /// Follow the agent, if it works on this trace.
    Agent(String, bool),
    /// Jump to the step's time, if it has one.
    Step(Option<Tick>),
    /// Run the decode line and add the instance's streams as a group; with the clock still to
    /// name, put the line on the command line. A decode already made opens its records.
    Decode { line: String, name: String, ready: bool, done: Option<String> },
}

pub struct Entry {
    /// What the query matches; its first `head` characters are drawn dim (a scope, a number).
    pub text: String,
    pub head: usize,
    /// Drawn right-aligned beside it: a width, a time.
    pub right: String,
    pub act: Act,
    /// The preview's lines, the first in bold; a signal's is drawn from the trace instead.
    pub preview: Vec<String>,
}

pub struct Picker {
    /// Index into `SOURCES`.
    pub source: usize,
    /// The stream of the records source.
    pub stream: Option<String>,
    pub query: String,
    pub entries: Vec<Entry>,
    /// The entries the query matches, best first.
    pub shown: Vec<usize>,
    /// Why the source is empty.
    pub empty: String,
    /// Highlight among `shown`, and the first shown on screen and how many fit, as last drawn.
    pub hl: usize,
    pub top: usize,
    pub page: usize,
    /// Signals marked with `Tab`, in the order marked.
    pub marked: Vec<usize>,
    /// The entry highlighted, since when, and whether its preview was drawn whole: a signal not
    /// read yet is read only once the highlight rests on it (`PREVIEW_WAIT`), so holding `j`
    /// over a big trace doesn't read every signal it passes.
    pub rest: Option<(usize, std::time::Instant, bool)>,
}

/// How long the highlight rests on a signal not read yet before its preview reads it.
pub const PREVIEW_WAIT: std::time::Duration = std::time::Duration::from_millis(120);

impl Picker {
    /// Rank the entries by the query, best first; on a tie the shorter, then the source's order.
    fn filter(&mut self) {
        let pat = Pattern::parse(&self.query, CaseMatching::Ignore, Normalization::Smart);
        let (mut m, mut buf) = (Matcher::new(Config::DEFAULT.match_paths()), Vec::new());
        let mut scored: Vec<(u32, usize)> = self.entries.iter().enumerate().filter_map(|(i, e)| pat.score(Utf32Str::new(&e.text, &mut buf), &mut m).map(|s| (s, i))).collect();
        if !self.query.is_empty() {
            scored.sort_by_key(|&(s, i)| (std::cmp::Reverse(s), self.entries[i].text.len()));
        }
        self.shown = scored.into_iter().map(|(_, i)| i).collect();
        (self.hl, self.top) = (0, 0);
    }

    fn current(&self) -> Option<&Entry> {
        self.shown.get(self.hl).map(|&i| &self.entries[i])
    }

    fn step(&mut self, d: isize) {
        self.hl = self.hl.saturating_add_signed(d).min(self.shown.len().saturating_sub(1));
    }
}

/// The characters of `text` the query matches, for the highlight.
fn matched(query: &str, text: &str) -> Vec<u32> {
    let pat = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let (mut m, mut buf, mut idx) = (Matcher::new(Config::DEFAULT.match_paths()), Vec::new(), Vec::new());
    pat.indices(Utf32Str::new(text, &mut buf), &mut m, &mut idx);
    idx.sort_unstable();
    idx.dedup();
    idx
}

/// A long line in lines of at most `w` characters, broken at spaces.
fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    for word in s.split(' ') {
        let last = out.last_mut().expect("never empty");
        if !last.is_empty() && last.chars().count() + 1 + word.chars().count() > w {
            out.push(word.into());
        } else {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }
    out
}

impl App {
    /// `:pick <source>`: open the picker on a source (spec §7).
    pub fn open_picker(&mut self, arg: &str) -> Result<(), String> {
        let (name, rest) = arg.split_once(char::is_whitespace).unwrap_or((arg, ""));
        let name = if name.is_empty() { "signals" } else { name };
        let source = SOURCES.iter().position(|s| *s == name).ok_or_else(|| format!("no picker source {name} ({})", SOURCES.join(", ")))?;
        let stream = Some(rest.trim().to_string()).filter(|s| !s.is_empty());
        let mut p = Picker { source, stream, query: String::new(), entries: Vec::new(), shown: Vec::new(), empty: String::new(), hl: 0, top: 0, page: 1, marked: Vec::new(), rest: None };
        p.entries = self.entries(&mut p)?;
        p.filter();
        // the trail opens on its newest step, as `:trail`
        if source == 5 {
            p.hl = p.shown.len().saturating_sub(1);
        }
        self.picker = Some(p);
        Ok(())
    }

    /// The entries of the picker's source; an error says why there are none.
    fn entries(&self, p: &mut Picker) -> Result<Vec<Entry>, String> {
        let fpt = self.ws.trace.fs_per_tick;
        let fmt = |t: Tick| time::format(t, fpt);
        let out: Vec<Entry> = match p.source {
            0 => self
                .ws
                .trace
                .vars
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let head = v.path.rfind('.').map_or(0, |k| v.path[..=k].chars().count());
                    let right = match v.width {
                        Some(w) if w > 1 => format!("[{}:0]", w - 1),
                        _ => String::new(),
                    };
                    Entry { text: v.path.clone(), head, right, act: Act::Var(i), preview: Vec::new() }
                })
                .collect(),
            1 => {
                let mut out = vec![Entry { text: "1 cursor".into(), head: 2, right: fmt(self.ws.view.cursor), act: Act::Mark(0), preview: vec!["cursor 1".into(), format!("at {}", fmt(self.ws.view.cursor))] }];
                for (n, t) in self.ws.marks.iter().enumerate().filter_map(|(n, t)| t.map(|t| (n, t))) {
                    let l = &self.ws.labels[n];
                    let mut preview = vec![format!("mark {}{}", n + 1, if l.name.is_empty() { String::new() } else { format!(": {}", l.name) }), format!("at {}", fmt(t))];
                    if !l.note.is_empty() {
                        preview.push(String::new());
                        preview.extend(wrap(&l.note, 40));
                    }
                    out.push(Entry { text: format!("{} {}", n + 1, l.name), head: 2, right: fmt(t), act: Act::Mark(n), preview });
                }
                out
            }
            2 => {
                let names: Vec<&str> = self.ws.streams.iter().map(|s| s.name.as_str()).collect();
                if names.is_empty() {
                    return Err("no streams yet: make one with :decode or :protocol".into());
                }
                // the stream asked for, else the selected row's (or its field's), else them all
                if p.stream.is_none()
                    && let Some(rows::Row::Signal(r)) = self.ws.rows.get(self.ws.sel)
                {
                    let path = &self.ws.trace.vars[r.var].path;
                    p.stream = names.iter().filter(|n| path == *n || path.strip_prefix(**n).is_some_and(|f| f.starts_with('.'))).max_by_key(|n| n.len()).map(|n| n.to_string());
                }
                let streams: Vec<&wtf_core::stream::Stream> = match &p.stream {
                    Some(name) => vec![self.ws.streams.iter().find(|s| s.name == *name).ok_or_else(|| format!("unknown stream: {name} (streams: {})", names.join(", ")))?],
                    None => self.ws.streams.iter().collect(),
                };
                let all = streams.len() > 1;
                let mut out: Vec<(u64, Entry)> = Vec::new();
                for st in streams {
                    out.extend((0..st.records.len()).map(|i| {
                        let r = &st.records[i];
                        let fields = st.fields_text(i, fpt);
                        // with every stream listed, each record says whose it is
                        let n = if all { format!("{} #{i} ", st.name) } else { format!("#{i} ") };
                        let text = format!("{n}{}", fields.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join(" "));
                        let mut preview = vec![format!("{} #{i}", st.name), format!("{}{}{}", fmt(r.start), gl().dash, fmt(r.end)), String::new()];
                        preview.extend(fields.iter().map(|(n, v)| format!("{n} = {v}")));
                        (r.start, Entry { head: n.chars().count(), text, right: fmt(r.start), act: Act::Record(r.start), preview })
                    }));
                }
                // in time order, as they happened across the streams
                out.sort_by_key(|e| e.0);
                out.into_iter().map(|e| e.1).collect()
            }
            3 => crate::tui::commands::COMMANDS
                .iter()
                .map(|c| {
                    let line = if c.name.starts_with('<') { String::new() } else if c.args { format!("{} ", c.name) } else { c.name.to_string() };
                    let mut preview = vec![c.usage.to_string(), String::new()];
                    preview.extend(wrap(c.desc, 40));
                    Entry { text: c.name.to_string(), head: 0, right: c.usage.to_string(), act: Act::Command(line), preview }
                })
                .collect(),
            4 => {
                let here = crate::live::on_trace("agent", &self.ws.trace_path);
                crate::live::sessions()?
                    .into_iter()
                    .filter(|s| s.kind == "agent")
                    .map(|s| {
                        let on = here.contains(&s.id);
                        let file = s.trace.file_name().map_or(String::new(), |f| f.to_string_lossy().into_owned());
                        let what = if on { "on this trace: Enter follows it".to_string() } else { format!("on another trace: open it to follow ({})", s.trace.display()) };
                        let preview = vec![format!("agent {}", s.id), s.trace.display().to_string(), String::new(), what];
                        Entry { text: format!("{} {file}", s.id), head: 0, right: if on { "this trace".into() } else { String::new() }, act: Act::Agent(s.id, on), preview }
                    })
                    .collect()
            }
            6 => self.interfaces(),
            _ => self
                .ws
                .trail
                .iter()
                .map(|s| {
                    let mut preview = vec![format!("{} {}", s.tool, s.ask), String::new()];
                    preview.extend(wrap(&s.answer, 40));
                    if let Some(t) = s.time {
                        preview.extend([String::new(), format!("Enter jumps to {}", fmt(t))]);
                    }
                    Entry { text: format!("{} {}", s.tool, s.ask), head: 0, right: s.time.map_or(String::new(), fmt), act: Act::Step(s.time), preview }
                })
                .collect(),
        };
        if out.is_empty() {
            return Err(match p.source {
                0 => "no signals in this trace",
                1 => "no marks",
                2 => "no records",
                4 => "no agent is working on this machine",
                6 => "no interface a built-in protocol decodes: :decode names the signals (:help decode)",
                _ => "no trail yet: the steps of an agent working on this trace show here",
            }
            .into());
        }
        Ok(out)
    }

    /// The interfaces built-in protocols decode (`protocol::detect`), each with its decode line,
    /// the signals its roles take and its clock in the preview (spec §5 "Picker").
    fn interfaces(&self) -> Vec<Entry> {
        let t = &self.ws.trace;
        wtf_core::protocol::detect(t)
            .into_iter()
            .map(|f| {
                let done = self.ws.decodes.iter().find(|d| d.protocol == f.protocol && d.params.iter().any(|p| p.0 == "prefix" && p.1 == f.prefix)).map(|d| d.name.clone());
                // a name not taken by another decode
                let base = f.name().to_string();
                let name = (1..).map(|k| if k == 1 { base.clone() } else { format!("{base}{k}") }).find(|n| !self.ws.decodes.iter().any(|d| d.name == *n)).expect("a free name");
                let line = f.decode_line().replacen(&format!("decode {base} "), &format!("decode {name} "), 1);
                let b = wtf_core::protocol::builtin(f.protocol).expect("detect finds built-ins");
                let ready = f.clock.is_some() || !b.clocked;
                let mut preview = vec![format!("{} at {}", f.protocol, f.prefix), line.clone(), String::new()];
                let roles: Vec<&&str> = b.required.iter().chain(b.optional).filter(|r| t.by_path.contains_key(&format!("{}{r}", f.prefix))).collect();
                let rw = roles.iter().map(|r| r.len()).max().unwrap_or(0).max(5);
                preview.extend(roles.iter().map(|r| format!("{r:<rw$}  {}{r}", f.prefix)));
                if b.clocked {
                    preview.push(format!("{:<rw$}  {}", "clock", f.clock.clone().unwrap_or_else(|| "none found: Enter puts the line on the command line to name it".into())));
                }
                preview.push(String::new());
                preview.push(match &done {
                    Some(d) => format!("decoded already as {d}: Enter shows its records"),
                    None => "Enter decodes it and adds its streams as a group".into(),
                });
                let right = done.as_ref().map_or_else(|| f.protocol.to_string(), |d| format!("{} {} {d}", f.protocol, gl().arrow));
                Entry { text: format!("{name} {}", f.prefix), head: 0, right, act: Act::Decode { line, name, ready, done }, preview }
            })
            .collect()
    }

    /// Runs a decode line from the interfaces, then adds its results and violations (spec §16
    /// "Results") below the selected row as a group named after it, the group selected.
    fn decode_and_show(&mut self, line: &str, name: &str) -> Result<(), String> {
        self.run(line)?;
        let vars: Vec<usize> = self.ws.results(name).into_iter().filter_map(|i| self.ws.trace.by_path.get(&self.ws.streams[i].name).copied()).filter(|&v| !self.ws.shown(v)).collect();
        let (Some(&first), Some(&last)) = (vars.first(), vars.last()) else { return Ok(()) };
        self.ws.add_rows(&vars);
        let (a, b) = (self.ws.trace.vars[first].path.clone(), self.ws.trace.vars[last].path.clone());
        wtf_core::command::run(&mut self.ws, &format!("select {a} {b}"))?;
        wtf_core::command::run(&mut self.ws, &format!("group {name}"))?;
        Ok(())
    }

    /// The picker has the keys (spec §5 "Picker"); they are fixed.
    pub fn picker_key(&mut self, k: KeyEvent) {
        let Some(p) = &mut self.picker else { return };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Enter => self.pick(),
            KeyCode::Down => p.step(1),
            KeyCode::Up => p.step(-1),
            KeyCode::Char('n') if ctrl => p.step(1),
            KeyCode::Char('p') if ctrl => p.step(-1),
            KeyCode::PageDown => p.step(p.page as isize),
            KeyCode::PageUp => p.step(-(p.page as isize)),
            KeyCode::Char('s') if ctrl => {
                // the next source; an empty one shows why
                let mut p = self.picker.take().expect("checked above");
                (p.source, p.marked) = ((p.source + 1) % SOURCES.len(), Vec::new());
                match self.entries(&mut p) {
                    Ok(e) => (p.entries, p.empty) = (e, String::new()),
                    Err(e) => (p.entries, p.empty) = (Vec::new(), e),
                }
                p.filter();
                self.picker = Some(p);
            }
            KeyCode::Tab => {
                // signals only: mark or unmark the highlighted one, then the next
                let Some(&Entry { act: Act::Var(v), .. }) = p.current() else { return };
                if self.ws.shown(v) {
                    self.msg = Some(("nothing to mark: already shown".into(), true));
                } else if let Some(i) = p.marked.iter().position(|&m| m == v) {
                    p.marked.remove(i);
                } else {
                    p.marked.push(v);
                }
                p.step(1);
            }
            KeyCode::Backspace => {
                p.query.pop();
                p.filter();
            }
            KeyCode::Char(c) if !ctrl => {
                p.query.push(c);
                p.filter();
            }
            _ => {}
        }
    }

    /// `Enter`: do what the highlighted entry is for, and close the picker.
    fn pick(&mut self) {
        let Some(p) = self.picker.take() else { return };
        let Some(e) = p.current() else { return self.picker = Some(p) };
        let r = match e.act.clone() {
            Act::Var(v) => {
                // the marked, or the highlighted; below the selected row, as the signal panel adds
                let mut vars = if p.marked.is_empty() { vec![v] } else { p.marked.clone() };
                vars.retain(|&v| !self.ws.shown(v));
                if vars.is_empty() {
                    Err("nothing to add: already shown".into())
                } else {
                    self.ws.add_rows(&vars);
                    Ok(())
                }
            }
            Act::Mark(n) => self.ws.goto_mark(n),
            Act::Record(t) => {
                let name = p.stream.clone().unwrap_or_default();
                if let Some(i) = self.ws.rows.iter().position(|row| matches!(row, Row::Signal(s) if self.ws.trace.vars[s.var].path == name)) {
                    (self.ws.sel, self.ws.bit) = (i, None);
                }
                self.ws.jump(t, false);
                Ok(())
            }
            Act::Command(line) => {
                self.prefill(&line);
                Ok(())
            }
            Act::Agent(id, true) => self.run(&format!("follow {id}")),
            Act::Agent(id, false) => Err(format!("agent {id} works on another trace: open that trace to follow it")),
            Act::Step(Some(t)) => {
                self.ws.jump(t, false);
                Ok(())
            }
            Act::Step(None) => Err("this step is about no time".into()),
            Act::Decode { done: Some(d), .. } => self.open_browser(&d),
            Act::Decode { line, ready: false, .. } => {
                self.prefill(&line);
                Ok(())
            }
            Act::Decode { line, name, .. } => self.decode_and_show(&line, &name),
        };
        if let Err(e) = r {
            self.msg = Some((e, true));
        }
    }

    /// The mouse over the picker: a click highlights, a double click picks, the wheel moves, a
    /// click outside closes it (spec §2 "Over the rows").
    pub fn picker_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize), now: Instant) {
        let inside = self.hit.boxed.is_some_and(|r| r.contains((x as u16, y as u16).into()));
        let Some(p) = &mut self.picker else { return };
        match m.kind {
            MouseEventKind::ScrollDown => p.step(1),
            MouseEventKind::ScrollUp => p.step(-1),
            MouseEventKind::Down(_) if !inside => self.picker = None,
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(&(_, k)) = self.hit.records.iter().find(|(ly, _)| *ly == y) else { return };
                p.hl = k;
                if self.double(Target::Record(k), now) {
                    self.pick();
                }
            }
            _ => {}
        }
    }
}

/// Draw the picker over the rows; its box and lines go to `hit` for the mouse.
pub fn draw(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    let Some(p) = &mut app.picker else { return };
    let (bw, bh) = (w.saturating_sub(8).min(120), body.saturating_sub(2).min(26));
    if bw < 30 || bh < 6 {
        return;
    }
    let (x0, y0) = ((w - bw) / 2, (body - bh) / 2);
    let mono = pal().mono;
    // what is behind fades, so the box stands out
    if !mono {
        for y in 0..body {
            for x in 0..w {
                buf[(x as u16, y as u16)].set_fg(pal().dim);
            }
        }
    }
    let st = Style::new().bg(pal().panel).fg(pal().text);
    let dim = st.fg(pal().dim);
    let accent = if mono { Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED) } else { st.fg(pal().cursor).add_modifier(Modifier::BOLD) };
    let border = if p.source == 5 && !mono { st.fg(pal().hues[4]) } else { dim };
    // the list and the preview side by side; on a narrow screen, the list alone
    let lw = if bw >= 70 { bw * 9 / 20 } else { bw - 1 };
    let b = gl().card;
    let (tj, bj) = if b[4] == '-' { ('+', '+') } else { ('┬', '┴') };
    for y in 0..bh {
        let (l, r, fill) = match y {
            0 => (b[0], b[1], b[4]),
            _ if y == bh - 1 => (b[2], b[3], b[4]),
            _ => (b[5], b[5], ' '),
        };
        put(buf, x0, y0 + y, &format!("{l}{}{r}", fill.to_string().repeat(bw - 2)), st);
        put(buf, x0, y0 + y, &l.to_string(), border);
        put(buf, x0 + bw - 1, y0 + y, &r.to_string(), border);
        if fill != ' ' {
            put(buf, x0 + 1, y0 + y, &fill.to_string().repeat(bw - 2), border);
        }
        if lw < bw - 1 {
            let c = match y {
                0 => tj,
                _ if y == bh - 1 => bj,
                _ => b[5],
            };
            put(buf, x0 + lw, y0 + y, &c.to_string(), border);
        }
    }
    // the sources in the top border, the current one in the accent
    let mut x = x0 + 2;
    for (i, s) in SOURCES.iter().enumerate() {
        let s = if i == 2 && p.source == 2 { format!("records {}", p.stream.as_deref().unwrap_or("")).trim_end().to_string() } else { s.to_string() };
        // the current one with its icon (spec §14 `nerd`)
        let ic = if i == p.source { draw::icon(source_icon(i)).map_or(String::new(), |c| format!("{c} ")) } else { String::new() };
        let s = format!(" {ic}{s} ");
        if x + s.chars().count() < x0 + bw - 1 {
            put(buf, x, y0, &s, if i == p.source { accent } else { dim });
        }
        x += s.chars().count();
    }
    let keys = match p.source {
        0 if !p.marked.is_empty() => format!(" {} marked  Enter add  Tab mark  C-s source  Esc close ", p.marked.len()),
        0 => " Enter add  Tab mark  C-s source  Esc close ".into(),
        3 => " Enter type it  C-s source  Esc close ".into(),
        4 => " Enter follow  C-s source  Esc close ".into(),
        6 => " Enter decode  C-s source  Esc close ".into(),
        _ => " Enter jump  C-s source  Esc close ".into(),
    };
    if keys.chars().count() + 4 < bw {
        put(buf, x0 + bw - 2 - keys.chars().count(), y0 + bh - 1, &keys, dim);
    }
    // the query, how many match, and a rule under them
    let iw = lw - 2;
    let count = format!("{}/{}", p.shown.len(), p.entries.len());
    put(buf, x0 + 2, y0 + 1, &format!("{} ", draw::icon(draw::icon::SEARCH).unwrap_or(gl().insert)), accent);
    let q: String = p.query.chars().rev().take(iw.saturating_sub(count.len() + 4)).collect::<Vec<_>>().into_iter().rev().collect();
    put(buf, x0 + 4, y0 + 1, &q, st.add_modifier(Modifier::BOLD));
    put(buf, x0 + lw - 1 - count.len(), y0 + 1, &count, dim);
    put(buf, x0 + 1, y0 + 2, &b[4].to_string().repeat(lw - 1), border);
    // the list, scrolled to show the highlight
    p.page = bh - 4;
    if p.hl < p.top {
        p.top = p.hl;
    } else if p.hl >= p.top + p.page {
        p.top = p.hl + 1 - p.page;
    }
    if p.shown.is_empty() {
        let why = if p.empty.is_empty() { "nothing matches" } else { &p.empty };
        put(buf, x0 + 3, y0 + 3, &why.chars().take(iw - 2).collect::<String>(), dim);
    }
    let mut lines = Vec::new();
    for (k, &i) in p.shown.iter().enumerate().skip(p.top).take(p.page) {
        let (e, y, on) = (&p.entries[i], y0 + 3 + k - p.top, k == p.hl);
        lines.push((y, k));
        let row = if on { st.bg(pal().selected) } else { st };
        put(buf, x0 + 1, y, &" ".repeat(lw - 1), row);
        if on {
            put(buf, x0 + 1, y, &gl().hl.to_string(), if mono { accent } else { row.fg(pal().cursor) });
        }
        // shown already, or marked
        let tag = match e.act {
            Act::Var(v) if p.marked.contains(&v) => "+",
            Act::Var(v) if app.ws.shown(v) => &gl().shown.to_string(),
            _ => " ",
        };
        put(buf, x0 + 2, y, tag, row.fg(pal().dim));
        // its kind's icon, with the `nerd` glyphs
        let ix = if gl().icons { 2 } else { 0 };
        if let Some((c, col)) = kind_icon(&app.ws, &e.act) {
            put(buf, x0 + 4, y, &c.to_string(), row.fg(col));
        }
        let rw = if e.right.is_empty() { 0 } else { e.right.chars().count() + 1 };
        let tw = iw.saturating_sub(2 + ix + rw);
        let hits = matched(&p.query, &e.text);
        for (c, ch) in e.text.chars().enumerate().take(tw) {
            let s = if hits.contains(&(c as u32)) {
                accent.bg(row.bg.unwrap_or(pal().panel))
            } else if c < e.head {
                row.fg(pal().dim)
            } else {
                row
            };
            put(buf, x0 + 4 + ix + c, y, &ch.to_string(), if on { s.add_modifier(Modifier::BOLD) } else { s });
        }
        if rw > 0 && e.text.chars().count() + ix + rw + 1 < iw {
            put(buf, x0 + lw - rw, y, &e.right, row.fg(pal().dim));
        }
    }
    app.hit.boxed = Some(Rect::new(x0 as u16, y0 as u16, bw as u16, bh as u16));
    app.hit.records = lines;
    if lw >= bw - 1 {
        return;
    }
    // the preview, right of the list
    let (px, pw) = (x0 + lw + 2, bw - lw - 4);
    let Some(&ei) = p.shown.get(p.hl) else { return };
    let e = &p.entries[ei];
    let now = std::time::Instant::now();
    if p.rest.is_none_or(|r| r.0 != ei) {
        p.rest = Some((ei, now, false));
    }
    let since = p.rest.map_or(now, |r| r.1);
    let mut y = y0 + 1;
    let line = |buf: &mut Buffer, y: &mut usize, s: &str, style: Style| {
        if *y < y0 + bh - 1 {
            put(buf, px, *y, &s.chars().take(pw).collect::<String>(), style);
        }
        *y += 1;
    };
    let Act::Var(v) = e.act else {
        for (i, l) in e.preview.iter().enumerate() {
            line(buf, &mut y, l, if i == 0 { st.add_modifier(Modifier::BOLD) } else { st });
        }
        return;
    };
    // a signal: its value at the cursor and its waveform around it, drawn as the rows are, once
    // it is read; what it is and its decodes need no reading
    let whole = app.ws.trace.loaded(v) || now.duration_since(since) >= app.preview_wait;
    if let Some(r) = app.picker.as_mut().and_then(|p| p.rest.as_mut()) {
        r.2 |= whole;
    }
    let ws = &mut app.ws;
    if whole {
        ws.trace.load(&[v]);
    }
    let var = &ws.trace.vars[v];
    let t = ws.view.cursor;
    let fpt = ws.trace.fs_per_tick;
    let sig = Sig::of(v, var);
    line(buf, &mut y, &var.path, st.add_modifier(Modifier::BOLD));
    let what = match (ws.stream_of(v), var.width) {
        (Some(s), _) => format!("stream, {} records", s.records.len()),
        (_, Some(1)) => "1 bit".into(),
        (_, Some(w)) => format!("{w} bits"),
        _ => "text".into(),
    };
    line(buf, &mut y, &what, dim);
    // already decoded: its records may be what is wanted rather than the signal (spec §5); a
    // table of decode, protocol and role under a dim heading
    let decodes: Vec<(&str, &str, String)> = ws.decodes_of(v).into_iter().map(|(d, role)| (d, ws.decodes.iter().find(|x| x.name == d).map_or("", |x| x.protocol.as_str()), role)).collect();
    if !decodes.is_empty() {
        y += 1;
        line(buf, &mut y, "decodes", dim);
        let (dw, pw2) = (decodes.iter().map(|d| d.0.chars().count()).max().unwrap_or(0), decodes.iter().map(|d| d.1.chars().count()).max().unwrap_or(0));
        for (d, proto, role) in &decodes {
            if y >= y0 + bh - 1 {
                break;
            }
            let cells = [(format!("  {d:<dw$}  "), st.fg(pal().hues[5])), (format!("{proto:<pw2$}  "), dim), (role.clone(), st)];
            let mut x = px;
            for (t, style) in cells {
                let t: String = t.chars().take((px + pw).saturating_sub(x)).collect();
                put(buf, x, y, &t, style);
                x += t.chars().count();
            }
            y += 1;
        }
    }
    y += 1;
    if !whole {
        line(buf, &mut y, gl().cut.to_string().as_str(), dim);
        return;
    }
    let value = ws.trace.value_at(v, t).map_or("none".to_string(), |val| rows::label(var, &Look::of(var), &val));
    line(buf, &mut y, &format!("at {}  {value}", time::format(t, fpt)), st);
    y += 1;
    let mut view = ws.view.clone();
    view.set_width(pw as u64);
    view.center(t);
    let cells = draw::wave(&ws.trace, &view, &sig, None, None, ws.stream_of(v), None, pw);
    let cur = view.col_of(t);
    for half in 0..2 {
        if y + half >= y0 + bh - 1 {
            break;
        }
        for (c, cell) in cells.iter().enumerate() {
            let mut s = st.patch(cell[half].1);
            if Some(c as u64) == cur {
                s = if mono { s.add_modifier(Modifier::REVERSED) } else { s.bg(pal().column) };
            }
            put(buf, px + c, y + half, &cell[half].0.to_string(), s);
        }
    }
    y += 2;
    let (from, to) = (time::format(view.start, fpt), time::format(view.last_visible(), fpt));
    line(buf, &mut y, &format!("{from}{}{to:>w$}", "", w = pw.saturating_sub(from.chars().count())), dim);
    y += 1;
    let r#where = match ws.rows.get(ws.sel) {
        _ if ws.shown(v) => "shown already".to_string(),
        Some(Row::Signal(s)) => format!("Enter adds it below {}", ws.trace.vars[s.var].path),
        Some(Row::Group { name, .. }) => format!("Enter adds it below the group {name}"),
        None => "Enter adds it".into(),
    };
    for l in wrap(&r#where, pw) {
        line(buf, &mut y, &l, dim);
    }
}

/// A source's icon, in the top border beside its name.
fn source_icon(i: usize) -> char {
    use crate::draw::icon;
    [icon::WIRE, icon::MARK, icon::RECORD, icon::CMD, icon::EYE, icon::TRAIL, icon::PLUG].get(i).copied().unwrap_or(icon::SEARCH)
}

/// An entry's icon and its colour by what it is: a signal's kind (`draw::kind`), a mark, a record, a
/// command, an agent, a step. None without the `nerd` glyphs.
fn kind_icon(ws: &wtf_core::workspace::Workspace, act: &Act) -> Option<(char, ratatui::style::Color)> {
    use crate::draw::icon;
    let p = pal();
    let (c, col) = match act {
        Act::Var(v) => return draw::kind(ws, *v, None),
        Act::Mark(0) => (icon::CLOCK, p.hues[4]),
        Act::Mark(_) => (icon::MARK, p.bus),
        Act::Record(_) => (icon::RECORD, p.hues[1]),
        Act::Command(_) => (icon::CMD, p.hues[5]),
        Act::Decode { done: Some(_), .. } => (icon::OK, p.hues[3]),
        Act::Decode { .. } => (icon::PLUG, p.hues[5]),
        Act::Agent(..) => (icon::AGENT, p.hues[4]),
        Act::Step(_) => (icon::TRAIL, p.hues[4]),
    };
    draw::icon(c).map(|c| (c, col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks() {
        let paths = ["top.s_axi_rdata", "top.s_axi_rready", "top.bus.rd", "top.cpu.state"];
        let mut p = Picker { source: 0, stream: None, query: "rd".into(), entries: Vec::new(), shown: Vec::new(), empty: String::new(), hl: 0, top: 0, page: 1, marked: Vec::new(), rest: None };
        p.entries = paths.iter().enumerate().map(|(i, s)| Entry { text: s.to_string(), head: 0, right: String::new(), act: Act::Var(i), preview: Vec::new() }).collect();
        p.filter();
        // a word boundary first; state has no r then d
        assert_eq!(p.shown.first().map(|&i| paths[i]), Some("top.bus.rd"));
        assert!(!p.shown.iter().any(|&i| paths[i] == "top.cpu.state"));
        assert_eq!(matched("rd", "top.bus.rd"), vec![8, 9]);
        p.query.clear();
        p.filter();
        assert_eq!(p.shown, vec![0, 1, 2, 3]);
    }
}
