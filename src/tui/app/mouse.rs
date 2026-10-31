//! The mouse (spec §2 "Mouse" and "Menu", §5 "Signal panel"): what is on top takes it, the rows
//! last. Where things are comes from the last screen drawn (`Hit`), so the geometry has one home.

use super::{App, Mode};
use crate::draw::{gl, Marked, RowLine};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use wtf_core::radix;
use wtf_core::rows::{self, Row};
use wtf_core::time::{self, Tick};

/// A second click on the same thing within this is a double click.
const DOUBLE: Duration = Duration::from_millis(500);
/// A drag resting at an edge scrolls once per this.
const SCROLL: Duration = Duration::from_millis(50);

/// Where the last screen drew what the mouse can hit; `render` fills it.
#[derive(Default)]
pub struct Hit {
    /// The screen's lines above the status line.
    pub body: usize,
    /// Where the rows start: the panel's width, 0 without it.
    pub x0: usize,
    /// The first screen line of rows, under the time axis (and the ruler).
    pub head: usize,
    pub name_w: usize,
    /// Each row line on screen: its first screen line, the line it shows and its marker's column.
    pub lines: Vec<RowLine>,
    /// The panel's lines on screen: screen line and tree node.
    pub panel: Vec<(usize, usize)>,
    /// The `e`, `?` or `:records` box.
    pub boxed: Option<Rect>,
    /// The records box's items on screen: screen line and item.
    pub records: Vec<(usize, usize)>,
    /// The command completions on screen: screen line and item.
    pub completions: Vec<(usize, usize)>,
    /// The menu, then its second menu: where each was drawn, its items from the line under the
    /// top.
    pub menus: Vec<Rect>,
}

impl Hit {
    /// The row line drawn at screen line `y`.
    fn line(&self, y: usize) -> Option<Marked> {
        self.lines.iter().find(|(ly, ..)| (*ly..ly + crate::draw::ROW_LINES).contains(&y)).map(|&(_, l, m)| (l, m))
    }

    fn node(&self, y: usize) -> Option<usize> {
        self.panel.iter().find(|(ly, _)| *ly == y).map(|&(_, n)| n)
    }
}

/// What a click was on, to tell a double click.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Node(usize),
    Record(usize),
    /// A completion's screen line: its item changes as the first click completes it.
    Completion(usize),
}

/// The left button held down in the panel (spec §5): where it was pressed and what the drag
/// has done so far, to undo on a cancel.
pub struct Drag {
    /// The node pressed on, and the highlight and the rows' selection before.
    from: usize,
    sel: usize,
    rows_sel: (usize, Option<u32>),
    mods: KeyModifiers,
    /// The pointer left the line it was pressed on.
    moved: bool,
    /// The pointer, and the last time a resting drag scrolled.
    at: (usize, usize),
    scrolled: Instant,
}

/// A right click's menu (spec §2 "Menu"): drawn with its corner at `at`, moved to fit.
pub struct Menu {
    pub at: (usize, usize),
    pub items: Vec<Item>,
    pub hl: Option<usize>,
    /// The second menu, opened from item `.0`.
    pub sub: Option<(usize, Box<Menu>)>,
}

#[derive(Clone)]
pub struct Item {
    pub label: String,
    /// The key or command that does the same, `▸` for a second menu, `●` for the current choice.
    pub key: String,
    /// A signal colour to draw the label in.
    pub hue: Option<usize>,
    act: Act,
}

#[derive(Clone)]
enum Act {
    Toggle,
    Examine,
    Radix(usize),
    /// Fixed point needs its bits typed: the command line with `:radix q`.
    RadixLine,
    Color(Option<usize>),
    Copy,
    Remove,
    MarkAt(Tick),
    CopyTime(Tick),
    Line(String),
    Ungroup,
    AddSignals,
    Undo,
    Redo,
    Key(KeyEvent),
    Expand(bool),
    Sub(Vec<Item>),
}

fn item(label: &str, key: &str, act: Act) -> Item {
    Item { label: label.into(), key: key.into(), hue: None, act }
}

fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

impl Menu {
    fn new(at: (usize, usize), items: Vec<Item>) -> Self {
        Menu { at, items, hl: None, sub: None }
    }

    /// The menu the keys move in: the second one when open.
    fn inner(&mut self) -> &mut Menu {
        if self.sub.is_none() {
            return self;
        }
        &mut self.sub.as_mut().expect("checked above").1
    }
}

impl App {
    pub fn mouse(&mut self, m: MouseEvent) {
        self.mouse_at(m, Instant::now());
    }

    /// The mouse at time `now` (double clicks): whatever is on top takes it.
    pub fn mouse_at(&mut self, m: MouseEvent, now: Instant) {
        let at = (m.column as usize, m.row as usize);
        if self.menu.is_some() {
            return self.menu_mouse(m, at, now);
        }
        if matches!(self.mode, Mode::Command | Mode::Search) {
            return self.line_mouse(m, at, now);
        }
        if self.help || self.examine.is_some() {
            return self.box_mouse(m, at);
        }
        if self.records.is_some() {
            return self.records_mouse(m, at, now);
        }
        if self.panel_shown() {
            let mark = self.ws.mark();
            self.panel_mouse(m, at, now);
            return self.ws.record(mark);
        }
        if matches!(self.mode, Mode::View | Mode::Edit) {
            let mark = self.ws.mark();
            self.rows_mouse(m, at);
            self.ws.record(mark);
        }
    }

    /// A double click: the same target clicked again soon. Remembers this click otherwise.
    fn double(&mut self, t: Target, now: Instant) -> bool {
        let double = self.click.is_some_and(|(c, when)| c == t && now.duration_since(when) < DOUBLE);
        self.click = if double { None } else { Some((t, now)) };
        double
    }

    /// The rows, in View and Edit mode with nothing over them.
    fn rows_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize)) {
        let v = &mut self.ws.view;
        let eighth = (v.width / 8).max(1) as i64;
        let ctrl = m.modifiers.contains(KeyModifiers::CONTROL);
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        let hit = self.hit.line(y).filter(|_| y < self.hit.body);
        match m.kind {
            MouseEventKind::ScrollUp if ctrl => v.zoom_in(),
            MouseEventKind::ScrollDown if ctrl => v.zoom_out(),
            MouseEventKind::ScrollLeft => v.scroll(-eighth),
            MouseEventKind::ScrollRight => v.scroll(eighth),
            MouseEventKind::ScrollUp if shift => v.scroll(-eighth),
            MouseEventKind::ScrollDown if shift => v.scroll(eighth),
            MouseEventKind::ScrollUp => self.move_sel(-1),
            MouseEventKind::ScrollDown => self.move_sel(1),
            // with no rows yet, the way in: the signal panel
            MouseEventKind::Down(MouseButton::Left) if self.ws.rows.is_empty() => {
                if y < self.hit.body {
                    self.open_panel();
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((line, marker)) = hit {
                    let was = (self.ws.sel, self.ws.bit) == line;
                    (self.ws.sel, self.ws.bit) = line;
                    let name = x >= self.hit.x0 && x < self.hit.x0 + self.hit.name_w;
                    if marker.is_some_and(|c| x == self.hit.x0 + c) || (name && was) {
                        self.ws.toggle_group();
                    }
                }
                // on a row's waveform or on the time axis, not below the rows
                if x >= self.wave_x && (hit.is_some() || y < self.hit.head) {
                    let v = &mut self.ws.view;
                    v.set_cursor(v.col_start((x - self.wave_x) as u64).min(v.end));
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some((line, _)) = hit {
                    // inside a `V` range, the menu is for the range
                    if !(self.ws.anchor.is_some() && self.ws.selection().contains(&line.0)) {
                        self.ws.anchor = None;
                        (self.ws.sel, self.ws.bit) = line;
                    }
                    let t = (x >= self.wave_x).then(|| self.ws.view.col_start((x - self.wave_x) as u64).min(self.ws.view.end));
                    let items = self.row_items(t);
                    self.menu = Some(Menu::new((x, y), items));
                } else if y >= self.hit.head && y < self.hit.body {
                    let (undo, redo) = self.ws.can_undo();
                    let mut items = vec![item("Add signals…", "a", Act::AddSignals)];
                    if undo {
                        items.push(item("Undo", "u", Act::Undo));
                    }
                    if redo {
                        items.push(item("Redo", "Ctrl-r", Act::Redo));
                    }
                    self.menu = Some(Menu::new((x, y), items));
                }
            }
            _ => {}
        }
    }

    /// Edit mode with the signal panel open, as `a` opens it.
    fn open_panel(&mut self) {
        self.mode = Mode::Edit;
        self.adding = true;
        self.ws.anchor = None;
    }

    /// The `e` and `?` boxes: the wheel scrolls, a click outside closes.
    fn box_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize)) {
        match m.kind {
            MouseEventKind::ScrollDown => self.scroll += 1,
            MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(1),
            MouseEventKind::Down(_) if !inside(self.hit.boxed, x, y) => (self.help, self.examine, self.scroll) = (false, None, 0),
            _ => {}
        }
    }

    /// The `:records` box: a click highlights, a double click jumps, the wheel moves.
    fn records_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize), now: Instant) {
        match m.kind {
            MouseEventKind::ScrollDown => self.records_key(key(KeyCode::Char('j'), KeyModifiers::NONE)),
            MouseEventKind::ScrollUp => self.records_key(key(KeyCode::Char('k'), KeyModifiers::NONE)),
            MouseEventKind::Down(_) if !inside(self.hit.boxed, x, y) => self.records = None,
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(&(_, k)) = self.hit.records.iter().find(|(ly, _)| *ly == y) else { return };
                if let Some(r) = &mut self.records {
                    r.hl = k;
                }
                if self.double(Target::Record(k), now) {
                    self.records_key(key(KeyCode::Enter, KeyModifiers::NONE));
                }
            }
            _ => {}
        }
    }

    /// The command line: a click on a completion completes it as `Tab`, a double click runs it;
    /// the wheel moves the highlight; a click elsewhere closes the line.
    fn line_mouse(&mut self, m: MouseEvent, (_, y): (usize, usize), now: Instant) {
        let n = crate::tui::commands::completions(&self.line).len();
        match m.kind {
            // the list grows upward: up is the next item
            MouseEventKind::ScrollUp if n > 0 && self.mode == Mode::Command => self.hl = (self.hl + 1) % n,
            MouseEventKind::ScrollDown if n > 0 && self.mode == Mode::Command => self.hl = (self.hl + n - 1) % n,
            MouseEventKind::Down(MouseButton::Left) => match self.hit.completions.iter().find(|(ly, _)| *ly == y) {
                Some(&(_, k)) => {
                    if self.double(Target::Completion(y), now) {
                        return self.command_key(key(KeyCode::Enter, KeyModifiers::NONE));
                    }
                    self.hl = k;
                    self.command_key(key(KeyCode::Tab, KeyModifiers::NONE));
                }
                None if y != self.hit.body => self.mode = self.ret,
                None => {}
            },
            _ => {}
        }
    }

    /// The signal panel (spec §5): clicks act on release on the line pressed; a drag marks the
    /// lines it covers, or drops what it carries on the rows.
    fn panel_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize), now: Instant) {
        let in_panel = x < self.hit.x0;
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let d = if m.kind == MouseEventKind::ScrollUp { -1 } else { 1 };
                match self.drag.as_ref().is_some_and(|_| !in_panel) {
                    true => self.move_sel(d),
                    false if in_panel || self.drag.is_some() => self.panel.step(d),
                    false => {}
                }
            }
            MouseEventKind::Down(MouseButton::Left) if in_panel => {
                if let Some(n) = self.hit.node(y) {
                    self.drag = Some(Drag { from: n, sel: self.panel.sel, rows_sel: (self.ws.sel, self.ws.bit), mods: m.modifiers, moved: false, at: (x, y), scrolled: now });
                }
            }
            // a click on the rows side closes the panel, as Esc
            MouseEventKind::Down(MouseButton::Left) => {
                self.adding = false;
                self.panel.anchor = None;
            }
            MouseEventKind::Down(MouseButton::Right) if in_panel => {
                if let Some(n) = self.hit.node(y) {
                    self.panel.sel = n;
                    let items = self.panel_items();
                    self.menu = Some(Menu::new((x, y), items));
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => self.drag_to(x, y),
            MouseEventKind::Up(MouseButton::Left) => self.drop_at(x, y, now),
            _ => {}
        }
    }

    /// The pointer moved with the button held: the range in the panel grows, over the rows the
    /// insertion line follows.
    fn drag_to(&mut self, x: usize, y: usize) {
        let Some(d) = &mut self.drag else { return };
        d.at = (x, y);
        let node = if x < self.hit.x0 { self.hit.node(y) } else { None };
        // off the line pressed, it is a drag from then on
        d.moved |= node != Some(d.from);
        if x < self.hit.x0 {
            (self.ws.sel, self.ws.bit) = d.rows_sel;
            if let Some(n) = node.filter(|_| d.moved) {
                self.panel.anchor = Some(d.from);
                self.panel.sel = n;
            }
        } else if y < self.hit.body {
            // a row's line: below it; below the rows: below the last
            let line = self.hit.line(y).map(|(l, _)| l).or_else(|| self.hit.lines.last().filter(|(ly, ..)| y > *ly).map(|&(_, l, _)| l));
            if let Some((row, _)) = line {
                (self.ws.sel, self.ws.bit) = (row, None);
            }
        }
        self.drag_msg();
    }

    /// What a drop would add: the marks when pressed on a marked line, the lines dragged over,
    /// or the line pressed. As `(vars, the line's node)`; a node alone adds as `Enter` does.
    fn carried(&mut self) -> (Vec<usize>, Option<usize>) {
        let Some(d) = &self.drag else { return (Vec::new(), None) };
        let from = d.from;
        let marked = self.panel.vars_of(from).iter().any(|v| self.panel.marked.contains(v));
        if marked {
            return (self.panel.marked.iter().copied().filter(|&v| !self.ws.shown(v)).collect(), None);
        }
        if self.panel.anchor.is_some() && self.panel.range().len() > 1 {
            return (self.panel.range_vars().into_iter().filter(|&v| !self.ws.shown(v)).collect(), None);
        }
        (Vec::new(), Some(from))
    }

    /// The status line says what a drop does; the label at the pointer what it adds.
    fn drag_msg(&mut self) {
        let Some(&Drag { at: (x, y), moved, .. }) = self.drag.as_ref() else { return };
        let dot = gl().dot;
        let text = if !moved {
            String::new()
        } else if x < self.hit.x0 && self.hit.node(y).is_some() {
            format!("mark {} {dot} drag out to add {dot} Esc cancels", self.panel.range().len())
        } else if x >= self.hit.x0 && y < self.hit.body && y >= self.hit.head {
            let (what, _) = self.drag_label();
            // the row by the name it shows
            let below = match rows::display_names(&self.ws.trace, &self.ws.rows).get(self.ws.sel) {
                Some(n) if matches!(self.ws.rows[self.ws.sel], Row::Group { .. }) => format!(" below {}", n.chars().skip(2).collect::<String>()),
                Some(n) => format!(" below {}", n.trim()),
                None => String::new(),
            };
            format!("drop to add {what}{below} {dot} Esc cancels")
        } else {
            "release to cancel".into()
        };
        self.msg = (!text.is_empty()).then_some((text, false));
    }

    /// What the label at the pointer says (`state`, `3`), and whether to draw it: over the rows.
    pub fn drag_label(&mut self) -> (String, Option<(usize, usize)>) {
        let Some(&Drag { at: (x, y), moved, .. }) = self.drag.as_ref() else { return (String::new(), None) };
        let (vars, node) = self.carried();
        let what = match node {
            Some(n) => self.panel.nodes[n].name.clone(),
            None => vars.len().to_string(),
        };
        let over = moved && x >= self.hit.x0 && y >= self.hit.head && y < self.hit.body;
        (what, over.then_some((x, y)))
    }

    /// The button released: a click on the line pressed, a range marked in the panel, signals
    /// dropped on the rows, or nothing (a drag let go elsewhere).
    fn drop_at(&mut self, x: usize, y: usize, now: Instant) {
        let Some(d) = self.drag.as_ref() else { return };
        let (from, moved, mods) = (d.from, d.moved, d.mods);
        if x < self.hit.x0 && !moved {
            self.drag = None;
            if self.hit.node(y) == Some(from) {
                self.panel_click(from, mods, now);
            }
            return;
        }
        let over_rows = x >= self.hit.x0 && y >= self.hit.head && y < self.hit.body && !self.ws.rows.is_empty() || (x >= self.hit.x0 && self.ws.rows.is_empty() && y < self.hit.body);
        if x < self.hit.x0 && self.hit.node(y).is_some() {
            self.drag = None;
            self.msg = None;
            self.toggle_mark();
        } else if over_rows {
            let (vars, node) = self.carried();
            self.drag = None;
            self.msg = None;
            match node {
                Some(n) => {
                    self.panel.sel = n;
                    self.panel.anchor = None;
                    self.add_current();
                }
                None => {
                    if vars.iter().any(|v| self.panel.marked.contains(v)) {
                        self.panel.marked.clear();
                    }
                    self.panel.anchor = None;
                    self.ws.add_rows(&vars);
                }
            }
        } else {
            self.cancel_drag();
        }
    }

    /// Esc during a drag, or a drop where nothing can go: as it was before the press.
    pub fn cancel_drag(&mut self) {
        let Some(d) = self.drag.take() else { return };
        self.panel.sel = d.sel;
        self.panel.anchor = None;
        (self.ws.sel, self.ws.bit) = d.rows_sel;
        self.msg = None;
    }

    /// A click on a panel line, released where it was pressed.
    fn panel_click(&mut self, n: usize, mods: KeyModifiers, now: Instant) {
        let node = &self.panel.nodes[n];
        let signal = node.var.or(node.whole).is_some();
        let scope = node.var.is_none();
        if self.double(Target::Node(n), now) {
            // the first click opened or closed a scope: the second only adds
            if signal {
                self.panel.sel = n;
                self.add_current();
            }
            return;
        }
        if mods.contains(KeyModifiers::CONTROL) {
            self.panel.sel = n;
            return self.toggle_mark();
        }
        if mods.contains(KeyModifiers::SHIFT) {
            self.panel.anchor = Some(self.panel.sel);
            self.panel.sel = n;
            return self.toggle_mark();
        }
        self.panel.sel = n;
        if scope {
            self.panel.nodes[n].open = !self.panel.nodes[n].open;
        }
    }

    /// A drag resting at an edge scrolls what it is over (spec §5); called on the event loop's
    /// tick. Returns true when it scrolled.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(d) = &self.drag else { return false };
        if !d.moved || now.duration_since(d.scrolled) < SCROLL {
            return false;
        }
        let (x, y) = d.at;
        // lines past the edge: one at the edge, four from two past it
        let speed = |past: usize| if past >= 2 { 4 } else { 1 };
        let step = if x < self.hit.x0 {
            let (Some(&(first, _)), Some(&(last, _))) = (self.hit.panel.first(), self.hit.panel.last()) else { return false };
            match y {
                _ if y <= first => -(speed(first - y) as isize),
                _ if y >= last => speed(y - last) as isize,
                _ => return false,
            }
        } else {
            let (Some(&(first, ..)), Some(&(last, ..))) = (self.hit.lines.first(), self.hit.lines.last()) else { return false };
            let last_end = last + crate::draw::ROW_LINES - 1;
            match y {
                _ if y <= first + 1 => -(speed(first.saturating_sub(y)) as isize),
                _ if y >= last => speed(y.saturating_sub(last_end)) as isize,
                _ => return false,
            }
        };
        let d = self.drag.as_mut().expect("checked above");
        d.scrolled = now;
        if x < self.hit.x0 {
            self.panel.anchor.get_or_insert(d.from);
            self.panel.step(step);
        } else {
            self.move_sel(step);
            self.ws.bit = None;
        }
        self.drag_msg();
        true
    }

    /// The menu has the mouse: the pointer highlights, a click runs an item or opens its second
    /// menu, a click outside closes it (a right click opens one there instead).
    fn menu_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize), now: Instant) {
        // the second menu first: it is drawn over the first
        let found = self.hit.menus.iter().enumerate().rev().find(|(_, r)| inside(Some(**r), x, y)).map(|(k, r)| (k, y as isize - r.y as isize - 1));
        let Some(menu) = &mut self.menu else { return };
        match (m.kind, found) {
            (MouseEventKind::Moved | MouseEventKind::Drag(_), Some((k, i))) => {
                let target = if k == 1 { menu.inner() } else { &mut *menu };
                if i >= 0 && (i as usize) < target.items.len() {
                    target.hl = Some(i as usize);
                }
            }
            (MouseEventKind::Down(MouseButton::Left), Some((k, i))) => {
                let target = if k == 1 { menu.inner() } else { &mut *menu };
                if i >= 0 && (i as usize) < target.items.len() {
                    target.hl = Some(i as usize);
                    if k == 0 {
                        menu.sub = None;
                    }
                    self.menu_run();
                }
            }
            (MouseEventKind::Down(b), None) => {
                self.menu = None;
                if b == MouseButton::Right {
                    self.mouse_at(m, now);
                }
            }
            _ => {}
        }
    }

    /// The keys in a menu: `j` / `k` move, `Enter` runs, `l` opens a second menu, `h` closes it,
    /// `Esc` closes the second menu, then the menu; `q` closes it. Other keys do nothing.
    pub fn menu_key(&mut self, k: KeyEvent) {
        let Some(menu) = &mut self.menu else { return };
        let inner = menu.inner();
        let n = inner.items.len();
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => inner.hl = Some(inner.hl.map_or(0, |h| (h + 1).min(n - 1))),
            KeyCode::Char('k') | KeyCode::Up => inner.hl = Some(inner.hl.map_or(0, |h| h.saturating_sub(1))),
            KeyCode::Enter => self.menu_run(),
            KeyCode::Char('l') if inner.hl.is_some_and(|h| matches!(inner.items[h].act, Act::Sub(_))) => self.menu_run(),
            KeyCode::Char('h') | KeyCode::Esc if menu.sub.is_some() => menu.sub = None,
            KeyCode::Esc | KeyCode::Char('q') => self.menu = None,
            _ => {}
        }
    }

    /// Run the highlighted item of the innermost menu: open its second menu, or do it and close.
    fn menu_run(&mut self) {
        let Some(menu) = &mut self.menu else { return };
        let (pos, inner) = match &mut menu.sub {
            Some((p, m)) => (Some(*p), &mut **m),
            None => (None, &mut *menu),
        };
        let Some(h) = inner.hl else { return };
        let act = inner.items[h].act.clone();
        if let Act::Sub(items) = act {
            if pos.is_none() {
                menu.sub = Some((h, Box::new(Menu::new((0, 0), items))));
            }
            return;
        }
        self.menu = None;
        let mark = self.ws.mark();
        let r = self.act(act);
        self.report(r);
        self.ws.record(mark);
    }

    fn act(&mut self, act: Act) -> Result<(), String> {
        match act {
            Act::Toggle => self.ws.toggle_group(),
            Act::Examine => self.examine(),
            Act::Radix(r) => return self.each_row(|ws| ws.set_radix(Some((r, 0, None)))),
            Act::Color(c) => return self.each_row(|ws| ws.set_color(c)),
            Act::RadixLine => self.prefill("radix q"),
            Act::Copy => return self.copy('y'),
            Act::Remove => return self.ws.delete(),
            Act::MarkAt(t) => {
                let cursor = std::mem::replace(&mut self.ws.view.cursor, t);
                let r = self.ws.set_mark(None);
                self.ws.view.cursor = cursor;
                return r;
            }
            Act::CopyTime(t) => {
                let text = time::format(t, self.ws.trace.fs_per_tick);
                self.msg = Some((format!("copied: {text}"), false));
                self.yank = Some(text);
            }
            Act::Line(l) => self.prefill(&l),
            Act::Ungroup => return self.run("ungroup"),
            Act::AddSignals => self.open_panel(),
            Act::Undo => return self.ws.undo(),
            Act::Redo => return self.ws.redo(),
            Act::Key(k) => self.panel_key(k),
            Act::Expand(open) => self.panel.open(open),
            Act::Sub(_) => {}
        }
        Ok(())
    }

    /// The command line opened with `text` typed, to finish and run with `Enter`.
    fn prefill(&mut self, text: &str) {
        self.open_command();
        self.line = text.into();
        self.col = usize::MAX;
    }

    /// `f` on each row of the `V` range (a header brings its group), or on the selected row.
    fn each_row(&mut self, f: impl Fn(&mut wtf_core::workspace::Workspace) -> Result<(), String>) -> Result<(), String> {
        if self.ws.anchor.is_none() {
            return f(&mut self.ws);
        }
        let (sel, anchor) = (self.ws.sel, self.ws.anchor);
        let mut r = Err("nothing here takes it".into());
        for i in self.ws.selection() {
            self.ws.sel = i;
            if f(&mut self.ws).is_ok() {
                r = Ok(());
            }
        }
        (self.ws.sel, self.ws.anchor) = (sel, anchor);
        r
    }

    /// The menu of the selected row, the `V` range, or a header; `t` the time clicked on a
    /// waveform.
    fn row_items(&self, t: Option<Tick>) -> Vec<Item> {
        let ws = &self.ws;
        let range = ws.anchor.is_some() && ws.selection().len() > 1;
        let mut items = Vec::new();
        let radix = |items: &mut Vec<Item>| {
            if let Some(sub) = self.radix_items() {
                items.push(item("Radix", gl().group_closed.to_string().as_str(), Act::Sub(sub)));
            }
        };
        let colour = |items: &mut Vec<Item>| items.push(item("Colour", gl().group_closed.to_string().as_str(), Act::Sub(self.color_items())));
        match ws.rows.get(ws.sel) {
            _ if range => {
                items.push(item("Group…", ":group", Act::Line("group ".into())));
                radix(&mut items);
                colour(&mut items);
                items.push(item("Remove", "dd", Act::Remove));
            }
            Some(Row::Group { collapsed, name, .. }) => {
                items.push(item(if *collapsed { "Open" } else { "Fold" }, "Space", Act::Toggle));
                radix(&mut items);
                colour(&mut items);
                items.push(item("Rename…", ":rename", Act::Line(format!("rename {name}"))));
                items.push(item("Ungroup", ":ungroup", Act::Ungroup));
                items.push(item("Remove", "dd", Act::Remove));
            }
            Some(Row::Signal(s)) => {
                let (var, bit) = ws.probe(s, ws.bit);
                if bit.is_none() && !ws.children(var).is_empty() {
                    items.push(item(if s.open.contains(&var) { "Fold" } else { "Open" }, "Space", Act::Toggle));
                }
                items.push(item("Examine", "e", Act::Examine));
                if ws.bit.is_none() {
                    radix(&mut items);
                    colour(&mut items);
                }
                items.push(item("Copy value", "yy", Act::Copy));
                items.push(item("Remove", "dd", Act::Remove));
            }
            None => {}
        }
        if let Some(t) = t.filter(|_| !range) {
            items.push(item("Mark here", "m", Act::MarkAt(t)));
            items.push(item("Copy time", "yt", Act::CopyTime(t)));
        }
        items
    }

    /// The radixes the first multi-bit signal of the selection can take (spec §10), the current
    /// one marked; fixed point opens the command line. None when there is no such signal.
    fn radix_items(&self) -> Option<Vec<Item>> {
        let ws = &self.ws;
        let r = ws.selection();
        let s = ws.rows[r.start..r.end.max(ws.sel + 1)].iter().filter_map(Row::sig).find(|s| ws.trace.vars[s.var].width != Some(1))?;
        let v = &ws.trace.vars[s.var];
        let (width, named) = (v.width.unwrap_or(0), v.names.is_some());
        let shown = gl().shown.to_string();
        let mut items: Vec<Item> = (0..radix::RADIXES.len())
            .filter(|&k| !radix::RADIXES[k].fixed && radix::fits(k, None, width, named))
            .map(|k| item(radix::RADIXES[k].name, if k == s.radix { &shown } else { "" }, Act::Radix(k)))
            .collect();
        if width <= 128 {
            items.push(item("q<m>.<n>…", ":radix", Act::RadixLine));
        }
        Some(items)
    }

    /// The eight colours, each drawn in itself, and `off`; the current one marked.
    fn color_items(&self) -> Vec<Item> {
        let now = self.ws.rows.get(self.ws.sel).and_then(Row::sig).and_then(|s| s.color);
        let shown = gl().shown.to_string();
        let mut items: Vec<Item> = rows::COLORS
            .iter()
            .enumerate()
            .map(|(k, c)| Item { hue: Some(k), ..item(c, if now == Some(k) { &shown } else { "" }, Act::Color(Some(k))) })
            .collect();
        items.push(item("off", if now.is_none() { &shown } else { "" }, Act::Color(None)));
        items
    }

    /// The menu of the highlighted panel line: what its keys do there.
    fn panel_items(&self) -> Vec<Item> {
        let p = &self.panel;
        let n = &p.nodes[p.sel];
        let vars: Vec<usize> = p.vars_of(p.sel).into_iter().filter(|&v| !self.ws.shown(v)).collect();
        let none = KeyModifiers::NONE;
        let mut items = Vec::new();
        if !vars.is_empty() || !p.marked.is_empty() {
            items.push(item("Add", "Enter", Act::Key(key(KeyCode::Enter, none))));
            items.push(item("Add as group", "Ctrl-g", Act::Key(key(KeyCode::Char('g'), KeyModifiers::CONTROL))));
        }
        if !vars.is_empty() {
            let marked = vars.iter().all(|v| p.marked.contains(v));
            items.push(item(if marked { "Unmark" } else { "Mark" }, "Space", Act::Key(key(KeyCode::Char(' '), none))));
        }
        if n.var.is_none() {
            items.push(match n.open {
                true => item("Collapse", "h", Act::Expand(false)),
                false => item("Expand", "l", Act::Expand(true)),
            });
        }
        items
    }
}

fn inside(r: Option<Rect>, x: usize, y: usize) -> bool {
    r.is_some_and(|r| r.contains(ratatui::layout::Position::new(x as u16, y as u16)))
}
