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
pub const SOURCES: [&str; 6] = ["signals", "marks", "records", "commands", "sessions", "trail"];

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
}

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
        let mut p = Picker { source, stream, query: String::new(), entries: Vec::new(), shown: Vec::new(), empty: String::new(), hl: 0, top: 0, page: 1, marked: Vec::new() };
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
                let name = p.stream.get_or_insert_with(|| names.first().map_or(String::new(), |s| s.to_string()));
                let Some(st) = self.ws.streams.iter().find(|s| s.name == *name) else {
                    return Err(match names.is_empty() {
                        true => "no streams yet: make one with :decode or :protocol".into(),
                        false => format!("unknown stream: {name} (streams: {})", names.join(", ")),
                    });
                };
                (0..st.records.len())
                    .map(|i| {
                        let r = &st.records[i];
                        let fields = st.fields_text(i, fpt);
                        let n = format!("#{i} ");
                        let text = format!("{n}{}", fields.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join(" "));
                        let mut preview = vec![format!("{} #{i}", st.name), format!("{}{}{}", fmt(r.start), gl().dash, fmt(r.end)), String::new()];
                        preview.extend(fields.iter().map(|(n, v)| format!("{n} = {v}")));
                        Entry { head: n.chars().count(), text, right: fmt(r.start), act: Act::Record(r.start), preview }
                    })
                    .collect()
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
                _ => "no trail yet: the steps of an agent working on this trace show here",
            }
            .into());
        }
        Ok(out)
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
    let (bw, bh) = (w.saturating_sub(8).min(120), body.saturating_sub(2).min(30));
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
    let b = gl().border;
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
        let s = if i == 2 && p.source == 2 { format!(" records {} ", p.stream.as_deref().unwrap_or("")) } else { format!(" {s} ") };
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
        _ => " Enter jump  C-s source  Esc close ".into(),
    };
    if keys.chars().count() + 4 < bw {
        put(buf, x0 + bw - 2 - keys.chars().count(), y0 + bh - 1, &keys, dim);
    }
    // the query, how many match, and a rule under them
    let iw = lw - 2;
    let count = format!("{}/{}", p.shown.len(), p.entries.len());
    put(buf, x0 + 2, y0 + 1, &format!("{} ", gl().insert), accent);
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
        let rw = if e.right.is_empty() { 0 } else { e.right.chars().count() + 1 };
        let tw = iw.saturating_sub(2 + rw);
        let hits = matched(&p.query, &e.text);
        for (c, ch) in e.text.chars().enumerate().take(tw) {
            let s = if hits.contains(&(c as u32)) {
                accent.bg(row.bg.unwrap_or(pal().panel))
            } else if c < e.head {
                row.fg(pal().dim)
            } else {
                row
            };
            put(buf, x0 + 4 + c, y, &ch.to_string(), if on { s.add_modifier(Modifier::BOLD) } else { s });
        }
        if rw > 0 && e.text.chars().count() + rw + 1 < iw {
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
    let Some(e) = p.shown.get(p.hl).map(|&i| &p.entries[i]) else { return };
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
    // a signal: its value at the cursor and its waveform around it, drawn as the rows are
    let ws = &mut app.ws;
    ws.trace.load(&[v]);
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
    y += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks() {
        let paths = ["top.s_axi_rdata", "top.s_axi_rready", "top.bus.rd", "top.cpu.state"];
        let mut p = Picker { source: 0, stream: None, query: "rd".into(), entries: Vec::new(), shown: Vec::new(), empty: String::new(), hl: 0, top: 0, page: 1, marked: Vec::new() };
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
