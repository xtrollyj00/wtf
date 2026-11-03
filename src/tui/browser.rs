//! The records browser (spec §5 "Records"): a stream's records as a table, a column a field,
//! filtered by a `where` expression as it is typed, sorted by any column, grouped by a field's
//! values; `Enter` jumps to a record, or keeps a group's records only.

use crate::draw::{gl, pal, put};
use crate::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::time::Instant;
use wtf_core::rows::Row;
use wtf_core::stream::{Group, Stream};
use wtf_core::time;
use wtf_core::trace::Val;

/// The columns before the fields: the record's number, its start and its length.
const FIXED: [&str; 3] = ["n", "start", "length"];

/// A column's widest, so a frame's bytes don't take the table.
const CELL: usize = 24;

pub struct Browser {
    pub stream: String,
    /// The `where` expression, as typed; empty for every record.
    pub filter: String,
    /// `/` typed: keys go to the filter until `Enter` or `Esc`.
    pub typing: bool,
    /// Why the filter keeps none (it doesn't parse, a field it names isn't there).
    pub error: Option<String>,
    /// The column sorted by (`FIXED`, then the fields) and whether descending.
    pub sort: (usize, bool),
    /// Grouped by the sort column, a field: a line per value, with how many and when.
    pub group: bool,
    /// The records shown, by number, in order; the groups when grouped.
    pub rows: Vec<usize>,
    pub groups: Vec<Group>,
    /// The stream's records in all.
    pub total: usize,
    /// Each column's width, worked out over the records shown.
    pub widths: Vec<usize>,
    pub hl: usize,
    pub top: usize,
    pub page: usize,
}

impl Browser {
    pub fn new(stream: &str, filter: &str) -> Browser {
        Browser { stream: stream.into(), filter: filter.into(), typing: false, error: None, sort: (0, false), group: false, rows: Vec::new(), groups: Vec::new(), total: 0, widths: Vec::new(), hl: 0, top: 0, page: 1 }
    }

    fn len(&self) -> usize {
        if self.group { self.groups.len() } else { self.rows.len() }
    }

    fn step(&mut self, d: isize) {
        self.hl = self.hl.saturating_add_signed(d).min(self.len().saturating_sub(1));
    }

    /// Filters, sorts and groups the stream's records again. A filter that fails keeps the rows
    /// it had and says why.
    pub fn refresh(&mut self, st: &Stream, end: time::Tick, fpt: Option<u128>) {
        let filter = Some(self.filter.trim()).filter(|f| !f.is_empty());
        match st.select(0, end, filter, fpt) {
            Ok((rows, total)) => (self.rows, self.total, self.error) = (rows, total, None),
            Err(e) => self.error = Some(e),
        }
        let col = self.sort.0;
        match col {
            0 => {}
            1 => self.rows.sort_by_key(|&i| st.records[i].start),
            2 => self.rows.sort_by_key(|&i| st.records[i].end - st.records[i].start),
            _ => self.rows.sort_by_cached_key(|&i| key(&st.records[i].fields[col - 3], st.schema[col - 3].signed)),
        }
        if self.sort.1 {
            self.rows.reverse();
        }
        self.groups = match (self.group, col >= 3) {
            (true, true) => {
                let records: Vec<_> = self.rows.iter().map(|&i| &st.records[i]).collect();
                let mut g = st.group_by(&records, &[st.schema[col - 3].name.clone()]).unwrap_or_default();
                // the commonest first
                g.sort_by_key(|g| std::cmp::Reverse(g.count));
                g
            }
            _ => Vec::new(),
        };
        self.group &= col >= 3;
        let mut widths: Vec<usize> = self.columns(st).iter().map(|c| c.chars().count() + 2).collect();
        for line in (0..self.len()).map(|k| self.cells(st, k, fpt)) {
            for (w, c) in widths.iter_mut().zip(line) {
                *w = (*w).max(c.chars().count()).min(CELL);
            }
        }
        self.widths = widths;
        self.hl = self.hl.min(self.len().saturating_sub(1));
    }

    /// The column names: `n start length` and the fields; grouped, the field and what a group
    /// has.
    fn columns(&self, st: &Stream) -> Vec<String> {
        match self.group {
            true => vec![st.schema[self.sort.0 - 3].name.clone(), "count".into(), "first".into(), "last".into()],
            false => FIXED.iter().map(|s| s.to_string()).chain(st.schema.iter().map(|f| f.name.clone())).collect(),
        }
    }

    /// A line's cells as text: a record's, or a group's.
    fn cells(&self, st: &Stream, k: usize, fpt: Option<u128>) -> Vec<String> {
        if self.group {
            let g = &self.groups[k];
            let f = &st.schema[self.sort.0 - 3];
            return vec![wtf_core::stream::value_text(f, &g.values[0], fpt, None, None), g.count.to_string(), time::format(g.first, fpt), time::format(g.last, fpt)];
        }
        let i = self.rows[k];
        let r = &st.records[i];
        [format!("#{i}"), time::format(r.start, fpt), time::format(r.end - r.start, fpt)].into_iter().chain(st.fields_text(i, fpt).into_iter().map(|(_, v)| v)).collect()
    }
}

/// A field's value as it sorts: numbers by value (a signed field's as signed), then values with
/// X or Z bits and text by their text, then none.
fn key(v: &Option<Val>, signed: bool) -> (u8, i128, String) {
    match v {
        Some(Val::Bits(b)) if b.len() < 127 && b.chars().all(|c| c == '0' || c == '1') => {
            let n = i128::from_str_radix(b, 2).unwrap_or(0);
            let n = if signed && b.starts_with('1') { n - (1i128 << b.len()) } else { n };
            (0, n, String::new())
        }
        Some(Val::Bits(b)) => (1, 0, b.clone()),
        Some(Val::Text(t)) => (1, 0, t.clone()),
        None => (2, 0, String::new()),
    }
}

impl App {
    /// `:records <stream> [where <expr>]`: the browser on a stream's records (spec §5 "Records").
    pub fn open_browser(&mut self, arg: &str) -> Result<(), String> {
        let (name, filter) = match arg.split_once(" where ") {
            Some((n, f)) => (n.trim(), f.trim()),
            None => (arg.trim(), ""),
        };
        let Some(st) = self.ws.streams.iter().find(|s| s.name == name) else {
            let names: Vec<&str> = self.ws.streams.iter().map(|s| s.name.as_str()).collect();
            return Err(match names.len() {
                0 => "no streams yet: make one with :decode or :protocol".into(),
                _ if name.is_empty() => format!("records of which stream? ({})", names.join(", ")),
                _ => format!("unknown stream: {name} (streams: {})", names.join(", ")),
            });
        };
        let mut b = Browser::new(name, filter);
        b.refresh(st, self.ws.trace.end(), self.ws.trace.fs_per_tick);
        // a filter typed with the command that fails is an error, as the command's
        if let Some(e) = b.error.take() {
            return Err(e);
        }
        self.browser = Some(b);
        Ok(())
    }

    fn browser_refresh(&mut self) {
        let Some(b) = &mut self.browser else { return };
        if let Some(st) = self.ws.streams.iter().find(|s| s.name == b.stream) {
            b.refresh(st, self.ws.trace.end(), self.ws.trace.fs_per_tick);
        }
    }

    /// The browser has the keys; they are fixed (spec §5 "Records").
    pub fn browser_key(&mut self, k: KeyEvent) {
        let Some(b) = &mut self.browser else { return };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if b.typing {
            match k.code {
                KeyCode::Enter | KeyCode::Esc => b.typing = false,
                KeyCode::Backspace => _ = b.filter.pop(),
                KeyCode::Char('u') if ctrl => b.filter.clear(),
                KeyCode::Char(c) if !ctrl => b.filter.push(c),
                _ => return,
            }
            return self.browser_refresh();
        }
        let ncols = self.ws.streams.iter().find(|s| s.name == b.stream).map_or(3, |s| 3 + s.schema.len());
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.browser = None,
            KeyCode::Char('j') | KeyCode::Down => b.step(1),
            KeyCode::Char('k') | KeyCode::Up => b.step(-1),
            KeyCode::Char('f') if ctrl => b.step(b.page as isize),
            KeyCode::Char('b') if ctrl => b.step(-(b.page as isize)),
            KeyCode::Char('g') => b.hl = 0,
            KeyCode::Char('G') => b.hl = usize::MAX,
            KeyCode::Char('/') => b.typing = true,
            KeyCode::Char('s') => {
                b.sort = ((b.sort.0 + 1) % ncols, false);
                self.browser_refresh();
            }
            KeyCode::Char('S') => {
                b.sort.1 = !b.sort.1;
                self.browser_refresh();
            }
            KeyCode::Char('=') if b.sort.0 < 3 => self.msg = Some(("= groups by the sorted column, a field: s sorts by one".into(), true)),
            KeyCode::Char('=') => {
                (b.group, b.hl) = (!b.group, 0);
                self.browser_refresh();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // the next stream, its filter and sort its own
                let names: Vec<String> = self.ws.streams.iter().map(|s| s.name.clone()).collect();
                let at = names.iter().position(|n| *n == b.stream).unwrap_or(0);
                let d = if k.code == KeyCode::Tab { 1 } else { names.len() - 1 };
                *b = Browser::new(&names[(at + d) % names.len()], "");
                self.browser_refresh();
            }
            KeyCode::Enter => self.browser_pick(),
            _ => {}
        }
        if let Some(b) = &mut self.browser {
            b.hl = b.hl.min(b.len().saturating_sub(1));
        }
    }

    /// `Enter`: on a group, keep its records only (its field equal to its value); on a record,
    /// jump to its start, select its stream's row and close.
    fn browser_pick(&mut self) {
        let Some(b) = &mut self.browser else { return };
        let Some(st) = self.ws.streams.iter().find(|s| s.name == b.stream) else { return };
        let fpt = self.ws.trace.fs_per_tick;
        if b.group {
            let Some(g) = b.groups.get(b.hl) else { return };
            let f = &st.schema[b.sort.0 - 3];
            if !matches!(&g.values[0], Some(Val::Bits(v)) if v.chars().all(|c| c == '0' || c == '1')) {
                return self.msg = Some((format!("{}: only a number can be kept by its value", f.name), true));
            }
            let value = wtf_core::stream::value_text(f, &g.values[0], fpt, None, None);
            let this = format!("{} == {value}", f.name);
            b.filter = match b.filter.trim() {
                "" => this,
                old => format!("({old}) && {this}"),
            };
            (b.group, b.hl) = (false, 0);
            return self.browser_refresh();
        }
        let Some(&i) = b.rows.get(b.hl) else { return };
        let (t, name) = (st.records[i].start, b.stream.clone());
        self.browser = None;
        if let Some(r) = self.ws.rows.iter().position(|row| matches!(row, Row::Signal(s) if self.ws.trace.vars[s.var].path == name)) {
            (self.ws.sel, self.ws.bit) = (r, None);
        }
        self.ws.jump(t, false);
    }

    /// The mouse over the browser: the wheel moves, a click highlights, a double click picks, a
    /// click outside closes it.
    pub fn browser_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize), now: Instant) {
        let inside = self.hit.boxed.is_some_and(|r| r.contains((x as u16, y as u16).into()));
        let Some(b) = &mut self.browser else { return };
        match m.kind {
            MouseEventKind::ScrollDown => b.step(1),
            MouseEventKind::ScrollUp => b.step(-1),
            MouseEventKind::Down(_) if !inside => self.browser = None,
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(&(_, k)) = self.hit.records.iter().find(|(ly, _)| *ly == y) else { return };
                b.hl = k;
                if self.double(crate::tui::app::Target::Record(k), now) {
                    self.browser_pick();
                }
            }
            _ => {}
        }
    }
}

/// Draws the browser: a card over the waveforms, the filter line under its title, the column
/// names, then the records or groups scrolled to the highlight (spec §5 "Records").
pub fn draw(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    let Some(b) = &mut app.browser else { return };
    let Some(st) = app.ws.streams.iter().find(|s| s.name == b.stream) else { return };
    let fpt = app.ws.trace.fs_per_tick;
    let (bw, bh) = (w.saturating_sub(4).min(140), body.saturating_sub(2));
    if bw < 30 || bh < 7 {
        return;
    }
    let (x0, y0) = ((w - bw) / 2, 1);
    let r = Rect::new(x0 as u16, y0 as u16, bw as u16, bh as u16);
    let acc = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    let title = vec![(format!("{}records {} {}", crate::tui::render::icon_text(crate::draw::icon::RECORD), gl().dot, b.stream), acc)];
    let count = match (b.group, b.filter.trim().is_empty()) {
        (true, _) => format!("{} values, {} of {} records", b.groups.len(), b.rows.len(), b.total),
        (false, true) => format!("{} records", b.total),
        (false, false) => format!("{} of {}", b.rows.len(), b.total),
    };
    let keys = match (b.typing, b.group) {
        (true, _) => "Enter Esc done  Ctrl-u clear",
        (false, true) => "Enter keep these  = ungroup  s sort  Esc close",
        (false, false) => "Enter jump  / filter  s S sort  = group  Tab stream  Esc close",
    };
    crate::tui::render::card_frame(buf, r, pal().dim, &title, &count, keys);
    let st_card = crate::tui::render::card_style();
    let dim = st_card.fg(pal().dim);
    let (ix, iw) = (x0 + 2, bw - 4);
    // the filter
    let prompt = format!("{} where ", crate::draw::icon(crate::draw::icon::SEARCH).unwrap_or(gl().insert));
    put(buf, ix, y0 + 1, &prompt, if b.typing { st_card.patch(acc) } else { dim });
    let px = ix + prompt.chars().count();
    match (b.filter.is_empty(), b.typing) {
        (true, false) => put(buf, px, y0 + 1, "/ filters: resp != 0 && addr >= 0x40", dim),
        _ => put(buf, px, y0 + 1, &b.filter.chars().take(iw.saturating_sub(prompt.len())).collect::<String>(), st_card),
    }
    if let Some(e) = &b.error {
        let e: String = e.chars().take(iw / 2).collect();
        put(buf, (ix + iw).saturating_sub(e.chars().count()), y0 + 1, &e, st_card.fg(pal().unknown));
    }
    put(buf, x0 + 1, y0 + 2, &gl().card[4].to_string().repeat(bw - 2), dim);
    // the column names, the sorted one in the accent
    let cols = b.columns(st);
    let sorted = if b.group { 0 } else { b.sort.0 };
    let mut x = ix + 2;
    for (k, (c, &cw)) in cols.iter().zip(&b.widths).enumerate() {
        if x >= ix + iw {
            break;
        }
        let name = if k == sorted && b.sort.1 && !b.group { format!("{c} {}", gl().group_open) } else { c.clone() };
        let s = if k == sorted { st_card.patch(acc) } else { dim };
        put(buf, x, y0 + 3, &name.chars().take(ix + iw - x).collect::<String>(), s);
        x += cw + 2;
    }
    // the lines, scrolled to the highlight
    b.page = bh.saturating_sub(6).max(1);
    let n = b.len();
    if b.hl < b.top {
        b.top = b.hl;
    } else if b.hl >= b.top + b.page {
        b.top = b.hl + 1 - b.page;
    }
    app.hit.records.clear();
    let red = st.violations;
    for (row, k) in (b.top..n).take(b.page).enumerate() {
        let y = y0 + 4 + row;
        let on = k == b.hl;
        let line = if on { st_card.bg(pal().selected) } else { st_card };
        put(buf, x0 + 1, y, &" ".repeat(bw - 2), line);
        if on {
            put(buf, x0 + 1, y, &gl().hl.to_string(), line.fg(pal().cursor));
        }
        let mut x = ix + 2;
        for (c, (cell, &cw)) in b.cells(st, k, fpt).iter().zip(&b.widths).enumerate() {
            if x >= ix + iw {
                break;
            }
            let room = cw.min(ix + iw - x);
            let text: String = match cell.chars().count() > room {
                true => cell.chars().take(room.saturating_sub(1)).chain([gl().cut]).collect(),
                false => cell.clone(),
            };
            // the number and times dim, a record of violations red
            let s = match (c, red) {
                (_, true) => line.fg(pal().unknown),
                (0..3, _) if !b.group => line.fg(pal().dim),
                _ => line,
            };
            put(buf, x, y, &text, if on { s.add_modifier(Modifier::BOLD) } else { s });
            x += cw + 2;
        }
        app.hit.records.push((y, k));
    }
    if n == 0 {
        put(buf, ix + 2, y0 + 4, if b.error.is_some() { "the filter keeps none" } else { "no records match" }, dim);
    }
    app.hit.boxed = Some(r);
}
