//! The records browser (spec §5 "Records"): the records of a set of streams as one table in
//! order of start, a column a field, filtered by a `where` expression as it is typed, sorted by
//! any column, grouped by a field's values; `Enter` jumps to a record, or keeps a group's records
//! only. `Tab` goes through the sets of the decode: all its results, then each stream.

use crate::draw::{gl, pal, put};
use crate::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::time::Instant;
use wtf_core::rows::Row;
use wtf_core::stream::{At, Group, Set};
use wtf_core::time;
use wtf_core::trace::Val;
use wtf_core::workspace::Workspace;

/// A column's widest, so a frame's bytes don't take the table.
const CELL: usize = 24;

pub struct Browser {
    /// The sets `Tab` goes through (a decode's name, then its streams), and the one shown.
    pub tabs: Vec<String>,
    pub tab: usize,
    /// The decode's `all` set: its name, or its results one by one when a stream has its name.
    pub all: Option<String>,
    /// The decode the tabs are of, its name cut from the streams' (`wr` for `s_axi.wr`).
    pub decode: Option<String>,
    /// The `where` expression, as typed; empty for every record.
    pub filter: String,
    /// `/` typed: keys go to the filter until `Enter` or `Esc`.
    pub typing: bool,
    /// Why the filter keeps none (it doesn't parse, a field it names isn't there).
    pub error: Option<String>,
    /// The column sorted by (`heads`) and whether descending.
    pub sort: (usize, bool),
    /// Grouped by the sort column, a field: a line per value, with how many and when.
    pub group: bool,
    /// The set's streams (indices in the workspace's), the records shown in order, the groups
    /// when grouped.
    pub streams: Vec<usize>,
    pub rows: Vec<At>,
    pub groups: Vec<Group>,
    /// The set's records in all.
    pub total: usize,
    /// Each column's width, worked out over the lines shown.
    pub widths: Vec<usize>,
    /// The first column shown after the pinned ones (`h` / `l`).
    pub left: usize,
    pub hl: usize,
    pub top: usize,
    pub page: usize,
}

impl Browser {
    /// On `names` (a stream, a decode, a glob, several), the tabs of its decode if it has one.
    pub fn new(ws: &Workspace, names: &str, filter: &str) -> Browser {
        let streams = ws.stream_set(names).unwrap_or_default();
        // the decode every stream named is of
        let of = |i: usize| ws.decodes.iter().find(|d| ws.streams_of_decode(&d.name).contains(&i)).map(|d| d.name.clone());
        let decode = of(streams[0]).filter(|d| streams.iter().all(|&i| of(i).as_deref() == Some(d)));
        let mut all = None;
        let tabs = match &decode {
            Some(d) => {
                // all, its results, its violations, then the streams they are made from
                let mut each = ws.results(d);
                let rest: Vec<usize> = ws.streams_of_decode(d).into_iter().filter(|i| !each.contains(i)).collect();
                each.extend(rest);
                // a stream named as its decode (`pair`, `check`, `frame`) is that stream alone, so
                // the results are then named one by one
                let a = match ws.streams.iter().any(|s| &s.name == d) {
                    true => ws.results(d).iter().map(|&i| ws.streams[i].name.as_str()).collect::<Vec<_>>().join(" "),
                    false => d.clone(),
                };
                all = Some(a.clone());
                std::iter::once(a).chain(each.into_iter().map(|i| ws.streams[i].name.clone())).collect()
            }
            None => vec![names.to_string()],
        };
        let tab = tabs.iter().position(|t| t == names).unwrap_or(0);
        let mut tabs = tabs;
        if tabs[tab] != names {
            tabs.insert(0, names.to_string());
        }
        // by start, as `select` gives them
        let sort = (1 + (streams.len() > 1) as usize, false);
        Browser { tabs, tab, all, decode, filter: filter.into(), typing: false, error: None, sort, group: false, streams, rows: Vec::new(), groups: Vec::new(), total: 0, widths: Vec::new(), left: 0, hl: 0, top: 0, page: 1 }
    }

    fn len(&self) -> usize {
        if self.group { self.groups.len() } else { self.rows.len() }
    }

    fn step(&mut self, d: isize) {
        self.hl = self.hl.saturating_add_signed(d).min(self.len().saturating_sub(1));
    }

    fn set<'a>(&self, ws: &'a Workspace) -> Set<'a> {
        Set::new(self.streams.iter().map(|&i| &ws.streams[i]).collect())
    }

    /// The columns before the fields: the stream (when several), the number, start and length.
    fn fixed(&self) -> usize {
        3 + (self.streams.len() > 1) as usize
    }

    /// The columns pinned at the left: the stream and the number.
    fn pinned(&self) -> usize {
        self.fixed() - 2
    }

    /// A stream's name as its column shows it: without the decode's.
    fn short<'a>(&self, name: &'a str) -> &'a str {
        self.decode.as_ref().and_then(|d| name.strip_prefix(d.as_str())).and_then(|n| n.strip_prefix('.')).unwrap_or(name)
    }

    /// Filters, sorts and groups the records again. A filter that fails keeps the rows it had and
    /// says why.
    pub fn refresh(&mut self, ws: &Workspace) {
        let fpt = ws.trace.fs_per_tick;
        let set = self.set(ws);
        let filter = Some(self.filter.trim()).filter(|f| !f.is_empty());
        match set.select(0, ws.trace.end(), filter, fpt) {
            Ok((rows, total)) => (self.rows, self.total, self.error) = (rows, total, None),
            Err(e) => self.error = Some(e),
        }
        let (col, fixed) = (self.sort.0, self.fixed());
        match fixed - col.min(fixed) {
            // the stream, then time
            4 => self.rows.sort_by_key(|&(s, i)| (s, set.record((s, i)).start)),
            3 => self.rows.sort_by_key(|&(s, i)| (i, s)),
            2 => self.rows.sort_by_key(|&at| set.record(at).start),
            1 => self.rows.sort_by_key(|&at| set.record(at).end - set.record(at).start),
            _ => {
                let c = col - fixed;
                self.rows.sort_by_cached_key(|&at| key(set.value(at, c), set.columns[c].field.signed));
            }
        }
        if self.sort.1 {
            self.rows.reverse();
        }
        self.group &= col >= fixed;
        self.groups = match self.group {
            true => {
                let mut g = set.group_by(&self.rows, &[set.columns[col - fixed].name.clone()]).unwrap_or_default();
                // the commonest first
                g.sort_by_key(|g| std::cmp::Reverse(g.count));
                g
            }
            false => Vec::new(),
        };
        let mut widths: Vec<usize> = self.heads(&set).iter().map(|c| c.chars().count() + 2).collect();
        for line in (0..self.len()).map(|k| self.cells(&set, k, fpt)) {
            for (w, c) in widths.iter_mut().zip(line) {
                *w = (*w).max(c.chars().count()).min(CELL);
            }
        }
        self.widths = widths;
        self.hl = self.hl.min(self.len().saturating_sub(1));
    }

    /// The column names: `stream n start length` and the fields; grouped, the field and what a
    /// group has.
    fn heads(&self, set: &Set) -> Vec<String> {
        let fixed = ["stream", "n", "start", "length"];
        match self.group {
            true => vec![set.columns[self.sort.0 - self.fixed()].name.clone(), "count".into(), "first".into(), "last".into()],
            false => fixed[4 - self.fixed()..].iter().map(|s| s.to_string()).chain(set.columns.iter().map(|c| c.name.clone())).collect(),
        }
    }

    /// A line's cells as text: a record's (blank where its stream has no such field), or a
    /// group's.
    fn cells(&self, set: &Set, k: usize, fpt: Option<u128>) -> Vec<String> {
        if self.group {
            let g = &self.groups[k];
            let f = &set.columns[self.sort.0 - self.fixed()].field;
            return vec![wtf_core::stream::value_text(f, &g.values[0], fpt, None, None), g.count.to_string(), time::format(g.first, fpt), time::format(g.last, fpt)];
        }
        let (s, i) = self.rows[k];
        let r = set.record((s, i));
        let stream = (self.streams.len() > 1).then(|| self.short(&set.streams[s].name).to_string());
        let fields = set.columns.iter().map(|c| match c.at[s] {
            Some(f) => wtf_core::stream::value_text(&c.field, &r.fields[f], fpt, None, Some(3)),
            None => String::new(),
        });
        stream.into_iter().chain([format!("#{i}"), time::format(r.start, fpt), time::format(r.end - r.start, fpt)]).chain(fields).collect()
    }

    /// The columns drawn, in order: the pinned ones, then from `left` on.
    fn shown(&self) -> Vec<usize> {
        let pinned = if self.group { 0 } else { self.pinned() };
        (0..pinned).chain(pinned + self.left..self.widths.len()).collect()
    }
}

/// A field's value as it sorts: numbers by value (a signed field's as signed), then values with
/// X or Z bits and text by their text, then none.
fn key(v: Option<&Val>, signed: bool) -> (u8, i128, String) {
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
    /// `:records <streams> [where <expr>]`: the browser on a set of streams (spec §5 "Records").
    pub fn open_browser(&mut self, arg: &str) -> Result<(), String> {
        let (names, filter) = match arg.split_once(" where ") {
            Some((n, f)) => (n.trim(), f.trim()),
            None => (arg.trim(), ""),
        };
        if self.ws.streams.is_empty() {
            return Err("no streams yet: make one with :decode or :protocol".into());
        }
        self.ws.stream_set(names)?;
        let mut b = Browser::new(&self.ws, names, filter);
        b.refresh(&self.ws);
        // a filter typed with the command that fails is an error, as the command's
        if let Some(e) = b.error.take() {
            return Err(e);
        }
        self.browser = Some(b);
        Ok(())
    }

    fn browser_refresh(&mut self) {
        if let Some(b) = &mut self.browser {
            b.refresh(&self.ws);
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
        let ncols = b.fixed() + b.set(&self.ws).columns.len();
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.browser = None,
            KeyCode::Char('j') | KeyCode::Down => b.step(1),
            KeyCode::Char('k') | KeyCode::Up => b.step(-1),
            KeyCode::Char('f') if ctrl => b.step(b.page as isize),
            KeyCode::Char('b') if ctrl => b.step(-(b.page as isize)),
            KeyCode::Char('g') => b.hl = 0,
            KeyCode::Char('G') => b.hl = usize::MAX,
            KeyCode::Char('h') | KeyCode::Left => b.left = b.left.saturating_sub(1),
            KeyCode::Char('l') | KeyCode::Right => b.left = (b.left + 1).min(b.widths.len().saturating_sub(b.pinned() + 1)),
            KeyCode::Char('/') => b.typing = true,
            KeyCode::Char('s') => {
                b.sort = ((b.sort.0 + 1) % ncols, false);
                self.browser_refresh();
            }
            KeyCode::Char('S') => {
                b.sort.1 = !b.sort.1;
                self.browser_refresh();
            }
            KeyCode::Char('=') if b.sort.0 < b.fixed() => self.msg = Some(("= groups by the sorted column, a field: s sorts by one".into(), true)),
            KeyCode::Char('=') => {
                (b.group, b.hl, b.left) = (!b.group, 0, 0);
                self.browser_refresh();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // the next set, its filter and sort fresh
                let d = if k.code == KeyCode::Tab { 1 } else { b.tabs.len() - 1 };
                let (tabs, tab) = (b.tabs.clone(), (b.tab + d) % b.tabs.len());
                let mut next = Browser::new(&self.ws, &tabs[tab], "");
                (next.tabs, next.tab) = (tabs, tab);
                *b = next;
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
        let set = b.set(&self.ws);
        let fpt = self.ws.trace.fs_per_tick;
        if b.group {
            let Some(g) = b.groups.get(b.hl) else { return };
            let c = &set.columns[b.sort.0 - b.fixed()];
            // a value the filter can write back: a number, or a word
            if !matches!(&g.values[0], Some(Val::Bits(v)) if v.chars().all(|c| c == '0' || c == '1')) && !matches!(&g.values[0], Some(Val::Text(t)) if !t.is_empty() && !t.contains(char::is_whitespace)) {
                return self.msg = Some((format!("{}: this value can't be written in a filter", c.name), true));
            }
            let value = wtf_core::stream::value_text(&c.field, &g.values[0], fpt, None, None);
            let this = format!("{} == {value}", c.name);
            b.filter = match b.filter.trim() {
                "" => this,
                old => format!("({old}) && {this}"),
            };
            (b.group, b.hl) = (false, 0);
            return self.browser_refresh();
        }
        let Some(&at) = b.rows.get(b.hl) else { return };
        let (t, name) = (set.record(at).start, set.streams[at.0].name.clone());
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

/// Draws the browser: a card over the waveforms, the tabs and the filter under its title, the
/// column names, then the records or groups scrolled to the highlight (spec §5 "Records").
pub fn draw(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    let Some(b) = &mut app.browser else { return };
    let set = b.set(&app.ws);
    let fpt = app.ws.trace.fs_per_tick;
    let (bw, bh) = (w.saturating_sub(4).min(140), body.saturating_sub(2));
    if bw < 30 || bh < 8 {
        return;
    }
    let (x0, y0) = ((w - bw) / 2, 1);
    let r = Rect::new(x0 as u16, y0 as u16, bw as u16, bh as u16);
    let acc = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    let title = vec![(format!("{}records {} {}", crate::tui::render::icon_text(crate::draw::icon::RECORD), gl().dot, b.tabs[b.tab]), acc)];
    let count = match (b.group, b.filter.trim().is_empty()) {
        (true, _) => format!("{} values, {} of {} records", b.groups.len(), b.rows.len(), b.total),
        (false, true) => format!("{} records", b.total),
        (false, false) => format!("{} of {}", b.rows.len(), b.total),
    };
    let keys = match (b.typing, b.group) {
        (true, _) => "Enter Esc done  Ctrl-u clear",
        (false, true) => "Enter keep these  = ungroup  s sort  Esc close",
        (false, false) => "Enter jump  / filter  s S sort  = group  h l scroll  Tab set  Esc close",
    };
    crate::tui::render::card_frame(buf, r, pal().dim, &title, &count, keys);
    let st_card = crate::tui::render::card_style();
    let dim = st_card.fg(pal().dim);
    let (ix, iw) = (x0 + 2, bw - 4);
    let mut y = y0 + 1;
    // the tabs, when there are several sets
    if b.tabs.len() > 1 {
        let mut x = ix;
        for (k, t) in b.tabs.iter().enumerate() {
            let label = if b.all.as_ref() == Some(t) { "all".to_string() } else { b.short(t).to_string() };
            if x + label.chars().count() + 1 >= ix + iw {
                put(buf, x, y, &gl().cut.to_string(), dim);
                break;
            }
            put(buf, x, y, &label, if k == b.tab { st_card.patch(acc) } else { dim });
            x += label.chars().count() + 2;
        }
        y += 1;
    }
    // the filter
    let prompt = format!("{} where ", crate::draw::icon(crate::draw::icon::SEARCH).unwrap_or(gl().insert));
    put(buf, ix, y, &prompt, if b.typing { st_card.patch(acc) } else { dim });
    let px = ix + prompt.chars().count();
    match (b.filter.is_empty(), b.typing) {
        (true, false) => {
            // the set's own fields, so the hint never names one it hasn't
            let fields: Vec<&str> = set.columns.iter().map(|c| c.name.as_str()).collect();
            let hint = format!("/ filters on {}", if fields.is_empty() { "n, start".into() } else { fields.join(", ") });
            put(buf, px, y, &hint.chars().take(iw.saturating_sub(prompt.chars().count())).collect::<String>(), dim)
        }
        _ => put(buf, px, y, &b.filter.chars().take(iw.saturating_sub(prompt.len())).collect::<String>(), st_card),
    }
    if let Some(e) = &b.error {
        let e: String = e.chars().take(iw / 2).collect();
        put(buf, (ix + iw).saturating_sub(e.chars().count()), y, &e, st_card.fg(pal().unknown));
    }
    put(buf, x0 + 1, y + 1, &gl().card[4].to_string().repeat(bw - 2), dim);
    let (head_y, first_y) = (y + 2, y + 3);
    // the column names, the sorted one in the accent; the stream and number stay at the left
    let heads = b.heads(&set);
    let shown = b.shown();
    let sorted = if b.group { 0 } else { b.sort.0 };
    let mut x = ix + 2;
    for &k in &shown {
        if x >= ix + iw {
            break;
        }
        let name = if k == sorted && b.sort.1 && !b.group { format!("{} {}", heads[k], gl().group_open) } else { heads[k].clone() };
        let s = if k == sorted { st_card.patch(acc) } else { dim };
        put(buf, x, head_y, &name.chars().take(ix + iw - x).collect::<String>(), s);
        x += b.widths[k] + 2;
    }
    // the lines, scrolled to the highlight
    b.page = (y0 + bh - 1).saturating_sub(first_y).max(1);
    let n = b.len();
    if b.hl < b.top {
        b.top = b.hl;
    } else if b.hl >= b.top + b.page {
        b.top = b.hl + 1 - b.page;
    }
    app.hit.records.clear();
    for (row, k) in (b.top..n).take(b.page).enumerate() {
        let y = first_y + row;
        let on = k == b.hl;
        let line = if on { st_card.bg(pal().selected) } else { st_card };
        put(buf, x0 + 1, y, &" ".repeat(bw - 2), line);
        if on {
            put(buf, x0 + 1, y, &gl().hl.to_string(), line.fg(pal().cursor));
        }
        // a record of violations red
        let red = !b.group && set.streams[b.rows[k].0].violations;
        let cells = b.cells(&set, k, fpt);
        let mut x = ix + 2;
        for &c in &shown {
            if x >= ix + iw {
                break;
            }
            let room = b.widths[c].min(ix + iw - x);
            let cell = &cells[c];
            let text: String = match cell.chars().count() > room {
                true => cell.chars().take(room.saturating_sub(1)).chain([gl().cut]).collect(),
                false => cell.clone(),
            };
            // the stream, number and times dim
            let s = match red {
                true => line.fg(pal().unknown),
                false if !b.group && c < b.fixed() => line.fg(pal().dim),
                false => line,
            };
            put(buf, x, y, &text, if on { s.add_modifier(Modifier::BOLD) } else { s });
            x += b.widths[c] + 2;
        }
        app.hit.records.push((y, k));
    }
    if n == 0 {
        put(buf, ix + 2, first_y, if b.error.is_some() { "the filter keeps none" } else { "no records match" }, dim);
    }
    app.hit.boxed = Some(r);
}
