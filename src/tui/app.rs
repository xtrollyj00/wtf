//! App state and the keys of each mode (spec §2, §3, §4 "Command line", §5).

use wtf_core::{command, query};
use crate::draw::gl;
use crate::tui::panel::Panel;
use wtf_core::rows::{self, Row};
use wtf_core::time;
use wtf_core::workspace::{Msg, Workspace};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    View,
    Edit,
    Command,
    /// Typing a value search (`/` in View mode); shares the line with Command.
    Search,
}

pub struct App {
    /// What is shown, and everything that changes it; the rest is how the terminal UI edits it.
    pub ws: Workspace,
    /// First row on screen, as a position among the visible rows.
    pub top: usize,
    /// Lines of rows on screen, as last drawn: what `Ctrl-f` / `Ctrl-b` move.
    pub page: usize,
    pub mode: Mode,
    /// The mode the command line returns to.
    ret: Mode,
    pub panel: Panel,
    /// The signal panel is open in Edit mode (`a`, `/`) and has the keys.
    pub adding: bool,
    pub line: String,
    /// Cursor in `line`, in characters; past the end means at the end.
    col: usize,
    /// Highlighted completion.
    pub hl: usize,
    history: Vec<String>,
    searches: Vec<String>,
    hist_pos: Option<usize>,
    /// The search `n` / `N` repeat.
    last_search: Option<String>,
    /// Its matches counted in the whole trace, for the query it was (as `Debug` shows it).
    matches: Option<(String, u64, bool)>,
    /// Status line message; true for errors.
    pub msg: Option<Msg>,
    count: Option<u64>,
    pending: Option<char>,
    pub quit: bool,
    /// The `?` box with the keys of the current mode is open.
    pub help: bool,
    /// The `e` box: a title and the full value of the selected signal at the cursor.
    pub examine: Option<(String, Vec<String>)>,
}

impl App {
    pub fn new(ws: Workspace) -> Self {
        let panel = Panel::new(&ws.trace);
        App {
            ws,
            top: 0,
            page: 1,
            mode: Mode::View,
            ret: Mode::View,
            panel,
            adding: false,
            line: String::new(),
            col: 0,
            hl: 0,
            history: Vec::new(),
            searches: Vec::new(),
            hist_pos: None,
            last_search: None,
            matches: None,
            msg: None,
            count: None,
            pending: None,
            quit: false,
            help: false,
            examine: None,
        }
    }

    /// Show a session the agent prepared (`Workspace::present`) and say so.
    pub fn present(&mut self, text: &str) -> Result<String, String> {
        let msg = self.ws.present(text)?;
        self.mode = Mode::View;
        self.msg = Some(msg.clone());
        Ok(msg.0)
    }

    /// A `:` command: the window's own (`:w`, `:wq`, `:q`, `:q!`, `:help`, and `:e`, which also
    /// rebuilds the signal panel), or one on the workspace. Its message goes to the status line.
    pub fn run(&mut self, line: &str) -> Result<(), String> {
        let line = line.trim();
        let (cmd, arg) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let msg = match cmd {
            "help" => {
                self.help = true;
                None
            }
            "w" => Some(command::save(&mut self.ws, arg.trim())?),
            "wq" => {
                command::save(&mut self.ws, arg.trim())?;
                self.quit = true;
                None
            }
            "q" if self.ws.modified() => return Err("unsaved session (:w to save, :q! to quit)".into()),
            "q" | "q!" => {
                self.quit = true;
                None
            }
            "e" => return self.reload(),
            // the window picks the unit that fits; `unit` is for agents' answers (spec §15)
            "unit" => return Err("unit sets the time unit of an agent's answers; the window picks what fits".into()),
            _ => command::run(&mut self.ws, line)?,
        };
        if msg.is_some() {
            self.msg = msg;
        }
        Ok(())
    }

    /// `:e`, with the signal panel rebuilt for the new file.
    pub fn reload(&mut self) -> Result<(), String> {
        self.msg = Some(self.ws.reload()?);
        self.matches = None;
        self.panel = Panel::new(&self.ws.trace);
        Ok(())
    }


    /// Every key that changes the rows records an undo step, except collapsing and expanding
    /// groups (a View-mode action) and undo / redo themselves.
    pub fn key(&mut self, k: KeyEvent) {
        if self.help {
            // the help box takes no other keys
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('q' | '?')) {
                self.help = false;
            }
            return;
        }
        if self.examine.is_some() {
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('q' | 'e')) {
                self.examine = None;
            }
            return;
        }
        let mark = self.ws.mark();
        match self.mode {
            Mode::View => self.view_key(k),
            Mode::Edit if self.adding => self.panel_key(k),
            Mode::Edit => self.edit_key(k),
            Mode::Command | Mode::Search => self.command_key(k),
        }
        self.ws.record(mark);
        // a bit line stays selected only while its bus shows it
        if let Some(b) = self.ws.bit
            && !matches!(self.ws.rows.get(self.ws.sel), Some(Row::Signal(s)) if b < s.bits)
        {
            self.ws.bit = None;
        }
    }


    /// `e`: open a box with the selected signal's whole value at the cursor, or on a stream its
    /// record's: the times and every field.
    fn examine(&mut self) {
        let fpt = self.ws.trace.fs_per_tick;
        let Some(Row::Signal(s)) = self.ws.rows.get(self.ws.sel) else {
            return self.msg = Some(("select a signal to examine".into(), true));
        };
        if let Some((st, i)) = self.ws.record_at(s.var) {
            let r = &st.records[i];
            let mut lines = vec![format!("{} {} {} ({})", time::format(r.start, fpt), gl().dash, time::format(r.end, fpt), time::format(r.end - r.start, fpt)), String::new()];
            let fields = st.fields_text(i, fpt);
            let nw = fields.iter().map(|f| f.0.chars().count()).max().unwrap_or(0);
            for (name, v) in fields {
                for (k, part) in wrap(&v).into_iter().enumerate() {
                    lines.push(format!("{:<nw$}  {part}", if k == 0 { name } else { "" }));
                }
            }
            return self.examine = Some((format!("{} {}", st.name, st.label(i, fpt)), lines));
        }
        let (var, bit) = self.ws.probe(s.var, self.ws.bit);
        let path = &self.ws.trace.vars[var].path;
        let name = bit.map_or(path.clone(), |b| format!("{path}[{b}]"));
        let value = match self.ws.trace.value_of(var, bit, self.ws.view.cursor) {
            Some(_) if self.ws.stream_of(var).is_some() => vec!["no record here".into()],
            Some(v) => wrap(&rows::label(&self.ws.trace.vars[s.var], s, &v)),
            None => vec!["no value here".into()],
        };
        self.examine = Some((format!("{name} @ {}", time::format(self.ws.view.cursor, fpt)), value));
    }

    /// A value search (spec §8) that says where the match is: `match 3 of 157`.
    fn search(&mut self, text: &str, forward: bool, n: u64) -> Result<(), String> {
        // shortcut: counting stops at MAX matches, so `n` stays quick on huge traces
        const MAX: u64 = 100_000;
        let q = self.ws.search(text, forward, n)?;
        let count = |to| query::find(&self.ws.trace, &q, &query::Search { from: 0, to, forward: true, merge: false, runs: usize::MAX, matches: MAX });
        let key = format!("{q:?}");
        if self.matches.as_ref().is_none_or(|m| m.0 != key) {
            let all = count(None);
            self.matches = Some((key, all.total, all.more));
        }
        let at = count(Some(self.ws.view.cursor));
        let (_, total, more) = self.matches.as_ref().unwrap();
        let i = if at.more { format!("{MAX}+") } else { at.total.to_string() };
        self.msg = Some((format!("match {i} of {total}{}", if *more { "+" } else { "" }), false));
        Ok(())
    }

    /// Move the selection by `d` screen lines (bit lines of expanded buses included).
    fn move_sel(&mut self, d: isize) {
        let lines = rows::lines(&self.ws.rows);
        let Some(p) = lines.iter().position(|&l| l == (self.ws.sel, self.ws.bit)) else { return };
        (self.ws.sel, self.ws.bit) = lines[p.saturating_add_signed(d).min(lines.len() - 1)];
    }


    /// Edit mode on the rows side, or the command line opened from it.
    pub fn editing_rows(&self) -> bool {
        !self.adding && (self.mode == Mode::Edit || (self.mode == Mode::Command && self.ret == Mode::Edit))
    }


    /// The signal panel is shown: open in Edit mode, or the command line was opened from it.
    pub fn panel_shown(&self) -> bool {
        self.adding && (self.mode == Mode::Edit || (self.mode == Mode::Command && self.ret == Mode::Edit))
    }


    /// Counts and two-key sequences. Returns the first key of a sequence (if one is pending)
    /// and the count, or None while a count is being typed.
    fn counted(&mut self, c: char) -> Option<(Option<char>, u64)> {
        let pending = self.pending.take();
        if pending.is_none()
            && let Some(d) = c.to_digit(10)
            && (d > 0 || self.count.is_some())
        {
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(d as u64));
            return None;
        }
        self.msg = None;
        Some((pending, self.count.take().unwrap_or(1)))
    }


    /// Esc and other non-character keys cancel a count or a pending sequence.
    fn cancel(&mut self) {
        self.count = None;
        self.pending = None;
    }


    fn open_command(&mut self) {
        self.open_line(Mode::Command);
    }


    fn open_line(&mut self, mode: Mode) {
        self.ret = self.mode;
        self.mode = mode;
        self.line.clear();
        self.hl = 0;
        self.hist_pos = None;
    }


    fn report(&mut self, r: Result<(), String>) {
        if let Err(e) = r {
            self.msg = Some((e, true));
        }
    }


    fn view_key(&mut self, k: KeyEvent) {
        if matches!(k.code, KeyCode::Tab | KeyCode::BackTab) {
            self.cancel();
            self.msg = None;
            let r = self.ws.next_mark(k.code == KeyCode::Tab);
            return self.report(r);
        }
        let KeyCode::Char(c) = k.code else { return self.cancel() };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // `3m`: the count names the mark; `m` alone takes a free one
        let given = self.count.map(|n| n.saturating_sub(1) as usize);
        let Some((pending, n)) = self.counted(c) else { return };
        if let Some(p) = pending {
            if (p, c) == ('g', 'g') {
                self.ws.view.set_cursor(self.ws.trace.start());
            }
            if p == '\'' {
                let r = match c.to_digit(10) {
                    _ if c == '\'' => self.ws.jump_back(),
                    Some(d) if d > 0 => self.ws.goto_mark(d as usize - 1),
                    _ => Err(format!("no cursor {c}")),
                };
                self.report(r);
            }
            return;
        }
        let v = &mut self.ws.view;
        let half = (v.width / 2).max(1) as i64;
        let n_i = n.min(u32::MAX as u64) as i64;
        match c {
            // half a screen, the cursor along; at the start or end it moves on alone
            'd' | 'u' if ctrl => {
                let cols = if c == 'd' { half * n_i } else { -half * n_i };
                v.scroll(cols);
                let d = cols.unsigned_abs().saturating_mul(v.ticks) / v.cols;
                v.set_cursor(if c == 'd' { v.cursor.saturating_add(d) } else { v.cursor.saturating_sub(d) });
            }
            'f' if ctrl => self.move_sel(n_i.saturating_mul(self.page as i64) as isize),
            'b' if ctrl => self.move_sel(-(n_i.saturating_mul(self.page as i64) as isize)),
            _ if ctrl => {}
            // the screen alone; the cursor keeps its time
            'H' => v.scroll(-half * n_i),
            'L' => v.scroll(half * n_i),
            'c' => v.center(v.cursor),
            'h' => v.set_cursor(v.cursor.saturating_sub(n.saturating_mul(v.step()))),
            'l' => v.set_cursor(v.cursor.saturating_add(n.saturating_mul(v.step()))),
            'w' | 'b' => {
                // on a group: the nearest change of any of its signals
                let vars = self.ws.sel_vars();
                let mut t = self.ws.view.cursor;
                for _ in 0..n {
                    let next = vars.iter().filter_map(|&(v, b)| {
                        if c == 'w' { self.ws.trace.next_change_of(v, b, t) } else { self.ws.trace.prev_change_of(v, b, t) }
                    });
                    match if c == 'w' { next.min() } else { next.max() } {
                        Some(x) => t = x,
                        None => break,
                    }
                }
                self.ws.view.set_cursor(t);
            }
            'j' => self.move_sel(n.min(u32::MAX as u64) as isize),
            'k' => self.move_sel(-(n.min(u32::MAX as u64) as isize)),
            ' ' => self.ws.toggle_group(),
            'g' => self.pending = Some('g'),
            'G' => v.set_cursor(v.end),
            'z' => (0..n).for_each(|_| v.zoom_in()),
            'x' => (0..n).for_each(|_| v.zoom_out()),
            '/' => self.open_line(Mode::Search),
            '\'' => self.pending = Some('\''),
            'm' => {
                let r = self.ws.set_mark(given);
                self.report(r);
            }
            'M' => {
                let r = self.ws.remove_mark(given);
                self.report(r);
            }
            'n' | 'N' => {
                let r = match self.last_search.clone() {
                    Some(text) => self.search(&text, c == 'n', n),
                    None => Err("no search yet".into()),
                };
                self.report(r);
            }
            'i' => {
                // with nothing to edit yet, straight to adding
                self.mode = Mode::Edit;
                self.adding = self.ws.rows.is_empty();
            }
            ':' => self.open_command(),
            '?' => self.help = true,
            'e' => self.examine(),
            'q' => {
                let r = self.run("q");
                self.report(r);
            }
            _ => {}
        }
    }


    /// Edit mode, rows side (spec §5 "Rows", §6).
    fn edit_key(&mut self, k: KeyEvent) {
        let c = match k.code {
            KeyCode::Esc => {
                self.cancel();
                if self.ws.anchor.take().is_none() {
                    self.mode = Mode::View;
                }
                return;
            }
            KeyCode::Char('r') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cancel();
                let r = self.ws.redo();
                return self.report(r);
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => c,
            _ => return self.cancel(),
        };
        let Some((pending, n)) = self.counted(c) else { return };
        let n = n.min(u32::MAX as u64) as isize;
        if let Some(p) = pending {
            if (p, c) == ('d', 'd') {
                let r = self.ws.delete();
                self.report(r);
            }
            return;
        }
        if self.ws.rows.is_empty() && !matches!(c, 'u' | ':' | 'a' | '/') {
            return;
        }
        match c {
            'j' => self.move_sel(n),
            'k' => self.move_sel(-n),
            'J' | 'K' => {
                self.ws.anchor = None;
                for _ in 0..n {
                    self.ws.sel = rows::move_row(&mut self.ws.rows, self.ws.sel, c == 'J');
                }
            }
            'd' => self.pending = Some('d'),
            'a' => {
                self.ws.anchor = None;
                self.adding = true;
            }
            '/' => {
                self.ws.anchor = None;
                self.adding = true;
                self.panel.start_search();
            }
            'V' => self.ws.anchor = if self.ws.anchor.is_some() { None } else { Some(self.ws.sel) },
            'r' => {
                let r = self.ws.set_radix(None);
                self.report(r);
            }
            'u' => {
                let r = self.ws.undo();
                self.report(r);
            }
            ' ' => self.ws.toggle_group(),
            ':' => self.open_command(),
            '?' => self.help = true,
            'e' => self.examine(),
            _ => {}
        }
    }


    /// Edit mode, signal panel side (spec §5 "Signal panel").
    fn panel_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(q) = &mut self.panel.query {
            // typing a search, as in vim: everything goes into it until Enter or Esc
            self.msg = None;
            match k.code {
                KeyCode::Esc => self.panel.cancel_search(),
                KeyCode::Enter => {
                    let q = q.clone();
                    if !self.panel.confirm() && !q.is_empty() {
                        self.msg = Some((format!("not found: {q}"), true));
                    }
                }
                KeyCode::Backspace if q.is_empty() => self.panel.cancel_search(),
                KeyCode::Backspace => {
                    q.pop();
                    self.panel.search();
                }
                KeyCode::Char('a') if ctrl => self.mark_matches(),
                KeyCode::Char(c) if !ctrl => {
                    q.push(c);
                    self.panel.search();
                }
                _ => {}
            }
            return;
        }
        let c = match k.code {
            KeyCode::Esc => {
                // the range first, then the marks, then the panel
                self.cancel();
                self.msg = None;
                if self.panel.anchor.take().is_none() && std::mem::take(&mut self.panel.marked).is_empty() {
                    self.adding = false;
                }
                return;
            }
            KeyCode::Enter => {
                self.cancel();
                self.msg = None;
                if self.panel.marked.is_empty() && self.panel.anchor.is_none() {
                    return self.add_current();
                }
                let vars = self.picked();
                if vars.is_empty() {
                    self.msg = Some(("nothing to add: already shown".into(), true));
                }
                return self.ws.add_rows(&vars);
            }
            KeyCode::Char(' ') => {
                self.cancel();
                self.msg = None;
                return self.toggle_mark();
            }
            KeyCode::Char('r') if ctrl => {
                self.cancel();
                let r = self.ws.redo();
                return self.report(r);
            }
            KeyCode::Char('a') if ctrl => {
                self.cancel();
                return self.mark_matches();
            }
            KeyCode::Char('g') if ctrl => {
                self.cancel();
                return self.add_as_group();
            }
            KeyCode::Char(c) if !ctrl => c,
            _ => return self.cancel(),
        };
        let Some((pending, n)) = self.counted(c) else { return };
        if let Some(p) = pending {
            if (p, c) == ('g', 'g') {
                self.panel.step(isize::MIN / 2);
            }
            return;
        }
        let n = n.min(u32::MAX as u64) as isize;
        match c {
            'j' => self.panel.step(n),
            'k' => self.panel.step(-n),
            'l' => self.panel.open(true),
            'h' => self.panel.open(false),
            'V' => self.panel.anchor = if self.panel.anchor.is_some() { None } else { Some(self.panel.sel) },
            'n' | 'N' if self.panel.matches.is_empty() => {
                let msg = if self.panel.pattern.is_empty() { "no search yet".into() } else { format!("not found: {}", self.panel.pattern) };
                self.msg = Some((msg, true));
            }
            'g' => self.pending = Some('g'),
            'G' => self.panel.step(isize::MAX / 2),
            'n' => self.panel.next(true, n as usize),
            'N' => self.panel.next(false, n as usize),
            '/' => self.panel.start_search(),
            'u' => {
                let r = self.ws.undo();
                self.report(r);
            }
            ':' => self.open_command(),
            '?' => self.help = true,
            _ => {}
        }
    }


    /// Space: mark or unmark the highlighted signal, or the signals directly in a scope.
    /// With a `V` range, its signals; the range ends.
    fn toggle_mark(&mut self) {
        let vars = if self.panel.anchor.is_some() { self.panel.range_vars() } else { self.panel.vars_here() };
        self.panel.anchor = None;
        let vars: Vec<usize> = vars.into_iter().filter(|&v| !self.ws.shown(v)).collect();
        let marked = &mut self.panel.marked;
        if vars.is_empty() {
            self.msg = Some(("nothing to mark: already shown".into(), true));
        } else if vars.iter().all(|v| marked.contains(v)) {
            marked.retain(|v| !vars.contains(v));
        } else {
            for v in vars {
                if !marked.contains(&v) {
                    marked.push(v);
                }
            }
        }
    }


    /// The marked signals and those in the `V` range, not shown yet; marks and range are cleared.
    fn picked(&mut self) -> Vec<usize> {
        let mut vars = std::mem::take(&mut self.panel.marked);
        for v in self.panel.range_vars() {
            if !vars.contains(&v) {
                vars.push(v);
            }
        }
        self.panel.anchor = None;
        vars.retain(|&v| !self.ws.shown(v));
        vars
    }


    /// `Ctrl-a`: mark every signal the search matches.
    fn mark_matches(&mut self) {
        let vars: Vec<usize> = self.panel.matches.iter().filter_map(|&i| self.panel.nodes[i].var).collect();
        let new: Vec<usize> = vars.into_iter().filter(|&v| !self.ws.shown(v) && !self.panel.marked.contains(&v)).collect();
        if new.is_empty() {
            self.msg = Some(("nothing to mark".into(), true));
        }
        self.panel.marked.extend(new);
    }


    /// `Ctrl-g`: the marked signals and the `V` range (or the highlighted signal or scope) as a new group, named after
    /// the scope they share. The command line then offers `:rename` with that name: Enter keeps
    /// it, typing changes it, Esc keeps it too.
    fn add_as_group(&mut self) {
        let vars = match self.panel.marked.is_empty() && self.panel.anchor.is_none() {
            true => self.panel.vars_here(),
            false => self.picked(),
        };
        let vars: Vec<usize> = vars.into_iter().filter(|&v| !self.ws.shown(v)).collect();
        if vars.is_empty() {
            self.msg = Some(("nothing to add: already shown".into(), true));
            return;
        }
        let paths: Vec<&str> = vars.iter().map(|&v| self.ws.trace.vars[v].path.as_str()).collect();
        let name = common_scope(&paths).rsplit('.').next().filter(|n| !n.is_empty()).unwrap_or("group").to_string();
        self.ws.add_group(&name, &vars);
        self.open_command();
        self.line = format!("rename {name}");
        self.col = usize::MAX;
    }


    /// Enter in the panel with nothing marked: a signal becomes a row, a scope a group.
    fn add_current(&mut self) {
        let i = self.panel.sel;
        if i >= self.panel.nodes.len() {
            return;
        }
        let path = self.panel.nodes[i].path.clone();
        if self.panel.nodes[i].var.is_some_and(|v| self.ws.shown(v)) {
            return;
        }
        match command::add(&mut self.ws, &path) {
            Ok(m) => self.msg = m,
            Err(e) => self.report(Err(e)),
        }
    }


    /// The command line, and the search line, which works the same without completions.
    fn command_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let search = self.mode == Mode::Search;
        match k.code {
            KeyCode::Esc => self.mode = self.ret,
            KeyCode::Enter => {
                self.mode = self.ret;
                let line = std::mem::take(&mut self.line);
                let hist = if search { &mut self.searches } else { &mut self.history };
                if !line.trim().is_empty() {
                    hist.retain(|h| *h != line);
                    hist.push(line.clone());
                }
                let r = match search {
                    true => {
                        self.last_search = Some(line.clone());
                        self.search(&line, true, 1)
                    }
                    false => self.run(&line),
                };
                self.report(r);
            }
            KeyCode::Backspace if self.line.is_empty() => self.mode = self.ret,
            // editing at the cursor, with the keys of vim's command line
            KeyCode::Left => self.col = self.cursor().saturating_sub(1),
            KeyCode::Right => self.col = self.cursor() + 1,
            KeyCode::Home => self.col = 0,
            KeyCode::Char('b') if ctrl => self.col = 0,
            KeyCode::End => self.col = usize::MAX,
            KeyCode::Char('e') if ctrl => self.col = usize::MAX,
            KeyCode::Backspace => {
                let c = self.cursor();
                self.edit(c.saturating_sub(1)..c, "");
            }
            KeyCode::Delete => {
                let c = self.cursor();
                self.edit(c..c + 1, "");
            }
            KeyCode::Char('w') if ctrl => {
                // the word before the cursor, and the spaces after it
                let c = self.cursor();
                let before: Vec<char> = self.line.chars().take(c).collect();
                let spaces = before.iter().rev().take_while(|ch| ch.is_whitespace()).count();
                let word = before.iter().rev().skip(spaces).take_while(|ch| !ch.is_whitespace()).count();
                self.edit(c - spaces - word..c, "");
            }
            KeyCode::Char('u') if ctrl => {
                let c = self.cursor();
                self.edit(0..c, "");
            }
            KeyCode::Up => self.move_hl(true),
            KeyCode::Down => self.move_hl(false),
            KeyCode::Char('p') if ctrl => self.move_hl(true),
            KeyCode::Char('n') if ctrl => self.move_hl(false),
            KeyCode::Tab if search => {}
            KeyCode::Tab => {
                if let Some(c) = crate::tui::commands::completions(&self.line).get(self.hl)
                    && !c.name.starts_with('<')
                {
                    self.line = if c.args { format!("{} ", c.name) } else { c.name.to_string() };
                    self.col = usize::MAX;
                }
            }
            KeyCode::Char(ch) if !ctrl => {
                let c = self.cursor();
                self.edit(c..c, ch.encode_utf8(&mut [0; 4]));
            }
            _ => {}
        }
    }


    /// The cursor in the command or search line, in characters.
    pub fn cursor(&self) -> usize {
        self.col.min(self.line.chars().count())
    }


    /// Replace the characters in `r` (clamped to the line) with `text`; the cursor goes after it.
    fn edit(&mut self, r: std::ops::Range<usize>, text: &str) {
        let byte = |c: usize| self.line.char_indices().nth(c).map_or(self.line.len(), |(i, _)| i);
        let (a, b) = (byte(r.start), byte(r.end));
        self.line.replace_range(a..b, text);
        self.col = r.start + text.chars().count();
        self.hl = 0;
        self.hist_pos = None;
    }


    /// Up / down: walk earlier commands on an empty (or recalled) line, otherwise the completion list.
    fn move_hl(&mut self, up: bool) {
        let search = self.mode == Mode::Search;
        if self.line.is_empty() || self.hist_pos.is_some() || search {
            let hist = if search { &self.searches } else { &self.history };
            let len = hist.len();
            if len == 0 {
                return;
            }
            let pos = match (self.hist_pos, up) {
                (None, true) => Some(len - 1),
                (None, false) => None,
                (Some(p), true) => Some(p.saturating_sub(1)),
                (Some(p), false) => (p + 1 < len).then_some(p + 1),
            };
            self.hist_pos = pos;
            self.line = pos.map(|p| hist[p].clone()).unwrap_or_default();
            self.col = usize::MAX;
            return;
        }
        let n = crate::tui::commands::completions(&self.line).len();
        if n > 0 {
            // the list grows upward, so "up" moves to the next item
            self.hl = if up { (self.hl + 1) % n } else { (self.hl + n - 1) % n };
        }
    }
}


/// The scope all these paths are in: `top.cpu.a`, `top.cpu.sub.b` -> `top.cpu`.
/// A long value in lines: text by 16 words (a frame's bytes), digits by 64.
fn wrap(v: &str) -> Vec<String> {
    let lines: Vec<String> = match v.contains(' ') {
        true => v.split(' ').collect::<Vec<_>>().chunks(16).map(|c| c.join(" ")).collect(),
        false => v.chars().collect::<Vec<_>>().chunks(64).map(|c| c.iter().collect()).collect(),
    };
    if lines.is_empty() { vec![String::new()] } else { lines }
}

fn common_scope(paths: &[&str]) -> String {
    let scopes: Vec<Vec<&str>> = paths.iter().map(|p| p.split('.').collect::<Vec<_>>()).map(|mut v| {
        v.pop();
        v
    }).collect();
    let first = scopes.first().cloned().unwrap_or_default();
    let n = (0..first.len()).take_while(|&i| scopes.iter().all(|s| s.get(i) == Some(&first[i]))).count();
    first[..n].join(".")
}

#[cfg(test)]
mod tests {
    #[test]
    fn common_scope() {
        assert_eq!(super::common_scope(&["top.cpu.a", "top.cpu.sub.b"]), "top.cpu");
        assert_eq!(super::common_scope(&["top.cpu.a", "top.dma.b"]), "top");
        assert_eq!(super::common_scope(&["a", "top.b"]), "");
    }
}
