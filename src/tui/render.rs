//! The terminal UI's screen (spec §4, §14): the workspace's rows (`draw`), with the signal panel,
//! the `?` box, the launch guide, the status line and the command line around them.

use crate::tui::app::{App, Mode};
use crate::draw::{self, pal, put, red, ROW_LINES};
use wtf_core::rows::Row;
use wtf_core::time;
use wtf_core::view::View;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// Draws the whole screen. Returns where the terminal cursor goes, if anywhere.
pub fn draw(app: &mut App, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let body = h - 1;
    // computed signals made since (`:signal`, a slice named in a search)
    if app.panel.vars != app.ws.trace.vars.len() {
        app.panel = crate::tui::panel::Panel::new(&app.ws.trace);
    }
    // the signal panel takes the left of the screen; the rows are drawn beside it
    // (not on screens too small to hold it)
    let pw = if app.panel_shown() && w >= 30 && body >= 4 { (w / 3).clamp(24, 48).min(w / 2) } else { 0 };
    let mut right = Buffer::empty(Rect::new(0, 0, (w - pw) as u16, h as u16));
    if app.ws.rows.is_empty() {
        app.ws.view.set_width((w - pw) as u64);
        guide(app, &mut right, w - pw, body);
    } else if body > ROW_LINES {
        // the time axis and at least one row
        let adding = app.panel_shown();
        draw::rows(&mut app.ws, &mut app.top, adding, &mut right, w - pw, body);
    }
    right.area.x = area.x + pw as u16;
    buf.merge(&right);
    let search = if pw > 0 { panel(app, buf, pw, body) } else { None };
    if app.help {
        help(app, buf, w, body);
    }
    if let Some((title, lines)) = &app.examine {
        boxed(buf, w, body, title, lines, " Esc e close ");
    }
    if matches!(app.mode, Mode::Command | Mode::Search) {
        command_line(app, buf, w, h)
    } else {
        status(app, buf, w, h - 1);
        search
    }
}

/// The signal panel (spec §4 "Edit mode", §5). Returns the search cursor while typing a search.
fn panel(app: &mut App, buf: &mut Buffer, pw: usize, body: usize) -> Option<(u16, u16)> {
    let dim = Style::new().fg(pal().dim);
    let inner = pw - 2;
    put(buf, 0, 0, &format!("┌ signals {}┐", "─".repeat(pw.saturating_sub(11))), dim);
    for y in 1..body.saturating_sub(1) {
        put(buf, 0, y, &format!("│{}│", " ".repeat(inner)), dim);
    }
    put(buf, 0, body.saturating_sub(1), &format!("└{}┘", "─".repeat(inner)), dim);
    if !app.panel.marked.is_empty() {
        put(buf, 2, body.saturating_sub(1), &format!(" {} marked ", app.panel.marked.len()), Style::new().fg(pal().cursor));
    }

    let p = &mut app.panel;
    match &p.query {
        Some(q) => put(buf, 2, 1, &format!("/{q}"), Style::new()),
        None if p.pattern.is_empty() => put(buf, 2, 1, "/ to search", dim),
        None => {
            put(buf, 2, 1, &format!("/{}", p.pattern), dim);
            let pos = p.match_pos().map_or(String::new(), |k| format!("{k}/{}", p.matches.len()));
            let count = if pos.is_empty() { format!("{}", p.matches.len()) } else { pos };
            put(buf, (pw - 2).saturating_sub(count.chars().count() + 1), 1, &format!(" {count}"), dim);
        }
    }
    let range = p.range();
    let items = p.visible();
    let cur = Some(p.sel);
    let at = items.iter().position(|&i| Some(i) == cur).unwrap_or(0);
    let fit = body.saturating_sub(4).max(1);
    if at < p.top {
        p.top = at;
    } else if at >= p.top + fit {
        p.top = at + 1 - fit;
    }
    p.top = p.top.min(items.len().saturating_sub(1));
    for (k, &i) in items[p.top.min(items.len())..].iter().take(fit).enumerate() {
        let n = &app.panel.nodes[i];
        let y = 2 + k;
        let mark = match n.var {
            None if n.open => "▾",
            None => "▸",
            Some(v) if app.ws.shown(v) => "●",
            Some(v) if app.panel.marked.contains(&v) => "+",
            Some(_) => " ",
        };
        let text = format!("{}{mark} {}", " ".repeat(2 * n.depth), n.name);
        // too long: keep the end, where the leaf name is
        let len = text.chars().count();
        let text = if len > inner - 1 { format!("…{}", text.chars().skip(len + 2 - inner).collect::<String>()) } else { text };
        let marked = n.var.is_some_and(|v| app.panel.marked.contains(&v));
        let st = match (Some(i) == cur, marked) {
            (true, _) => Style::new().bg(pal().selected).fg(pal().cursor).add_modifier(Modifier::BOLD),
            (false, true) => Style::new().fg(pal().cursor),
            (false, false) => Style::new(),
        };
        let st = if range.contains(&i) { st.bg(pal().selected) } else { st };
        put(buf, 1, y, &format!(" {text:<w$}", w = inner - 1), st);
    }
    let q = app.panel.query.as_ref()?;
    Some(((3 + q.chars().count()).min(pw - 2) as u16, 1))
}

/// The keys of each mode for the `?` box: (title, (keys, action) pairs, notes under them).
fn help_text(app: &App) -> (&'static str, &'static [(&'static str, &'static str)], &'static [&'static str]) {
    if app.panel_shown() {
        let keys: &[(&str, &str)] = &[
            ("j k", "move"),
            ("l h", "open / close a scope"),
            ("gg G", "first / last line"),
            ("/", "search paths"),
            ("n N", "next / previous match"),
            ("Space", "mark a signal or scope"),
            ("V", "mark a range"),
            ("Ctrl-a", "mark all matches"),
            ("Enter", "add as rows"),
            ("Ctrl-g", "add as a group"),
            ("u Ctrl-r", "undo / redo"),
            ("Esc", "clear marks / close"),
        ];
        return ("signal panel", keys, &["New signals go below the selected row (the yellow line)."]);
    }
    if app.editing_rows() {
        let keys: &[(&str, &str)] = &[
            ("j k", "select a row"),
            ("a", "add signals"),
            ("/", "add signals, searching"),
            ("dd", "remove"),
            ("J K", "move down / up"),
            ("V", "select a range"),
            ("r", "next radix"),
            ("Space", "fold a group, bus or stream"),
            ("e", "examine: the whole value"),
            ("u Ctrl-r", "undo / redo"),
            (":", "command"),
            ("Esc", "back to View"),
        ];
        return ("EDIT", keys, &[":group <name>  :rename <name>  :radix q4.12  :rm  :w"]);
    }
    let keys: &[(&str, &str)] = &[
        ("h l", "cursor left / right"),
        ("w b", "next / previous change"),
        ("gg G", "start / end"),
        ("z x", "zoom in / out"),
        ("j k", "select a row"),
        ("Space", "fold a group, bus or stream"),
        ("e", "examine: the whole value"),
        ("/", "search"),
        ("n N", "next / previous match"),
        ("m M", "put / remove a mark (3m: mark 3)"),
        ("Tab", "jump to the next mark"),
        ("'2..'9", "jump to a mark"),
        ("''", "back to before the jump"),
        ("i", "edit rows"),
        (":", "command"),
        ("q", "quit"),
    ];
    let notes: &[&str] = &[
        "A count repeats a key: 5l 3w 2z.",
        "/0x40  /-12  /x  /z  /valid && ready && data == 0x40  /@clk valid && ready",
        "/@clk fell(valid) && !past(ready)  rose( ) stable( ) past( )  data[7:4]  {a, b}",
    ];
    ("VIEW", keys, notes)
}

/// The `?` box: the current mode's keys in two columns, over the middle of the screen.
fn help(app: &App, buf: &mut Buffer, w: usize, body: usize) {
    let (title, keys, notes) = help_text(app);
    let kw = keys.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let entry = |&(k, d): &(&str, &str)| format!("{k:<kw$}  {d}");
    let ew = keys.iter().map(|e| entry(e).chars().count()).max().unwrap_or(0);
    let half = keys.len().div_ceil(2);
    let mut lines: Vec<String> = (0..half)
        .map(|i| format!("{:<ew$}    {}", entry(&keys[i]), keys.get(i + half).map(entry).unwrap_or_default()))
        .collect();
    if !notes.is_empty() {
        lines.push(String::new());
        lines.extend(notes.iter().map(|n| n.to_string()));
    }
    boxed(buf, w, body, &format!("help · {title}"), &lines, " Esc ? close ");
}

/// A box over the middle of the screen: a title, lines, and how to close it. Lines it can't hold
/// end in `… n more lines`.
fn boxed(buf: &mut Buffer, w: usize, body: usize, title: &str, lines: &[String], close: &str) {
    let mut lines = lines.to_vec();
    let room = body.saturating_sub(2);
    if lines.len() > room && room > 0 {
        // shortcut: a box can't scroll; add scrolling when values longer than a screen matter
        let more = lines.len() - room + 1;
        lines.truncate(room - 1);
        lines.push(format!("… {more} more lines"));
    }
    let top = format!(" {title} ");
    let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(top.chars().count()).max(close.chars().count());
    let inner = widest.min(w.saturating_sub(4));
    let (bw, bh) = (inner + 4, lines.len() + 2);
    if bw > w || bh > body || bw < close.chars().count() + 3 {
        return;
    }
    let (x0, y0) = ((w - bw) / 2, (body - bh) / 2);
    // opaque: the waveforms and cursor columns must not show through
    let st = Style::new().bg(Color::Reset).fg(Color::Reset);
    let dim = st.fg(pal().dim);
    let top: String = top.chars().take(bw - 2).collect();
    put(buf, x0, y0, &format!("┌{top}{}┐", "─".repeat(bw - 2 - top.chars().count())), dim);
    for (i, l) in lines.iter().enumerate() {
        let l: String = l.chars().take(inner).collect();
        put(buf, x0, y0 + 1 + i, "│ ", dim);
        put(buf, x0 + 2, y0 + 1 + i, &format!("{l:<inner$}"), st);
        put(buf, x0 + bw - 2, y0 + 1 + i, " │", dim);
    }
    put(buf, x0, y0 + bh - 1, &format!("└{}{close}─┘", "─".repeat(bw - 3 - close.chars().count())), dim);
}

fn guide(app: &App, buf: &mut Buffer, w: usize, body: usize) {
    let lines: &[&str] = if app.panel_shown() {
        // the panel is open: how to add from it
        &[&app.ws.file_name, "", "/       search", "Space   mark", "Enter   add", "Ctrl-g  add as a group", "Esc     close"]
    } else {
        &[&app.ws.file_name, "", "i       add signals", ":       commands", "?       help", "q       quit"]
    };
    let bw = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let y0 = body.saturating_sub(lines.len()) / 2;
    for (i, l) in lines.iter().enumerate().filter(|(i, _)| y0 + i < body) {
        // the file name is centred on its own, the key list as a block
        let x = if i == 0 { (w.saturating_sub(l.chars().count())) / 2 } else { w.saturating_sub(bw) / 2 };
        put(buf, x, y0 + i, l, Style::new());
    }
}

fn zoom_text(v: &View, fpt: Option<u128>) -> String {
    if v.cols == 1 {
        format!("{}/col", time::format(v.ticks, fpt))
    } else {
        format!("{}/{}col", time::format(1, fpt), v.cols)
    }
}

/// Mode and the selected signal (or a message) on the left; cursor time, zoom and file on the right.
fn status(app: &App, buf: &mut Buffer, w: usize, y: usize) {
    let fpt = app.ws.trace.fs_per_tick;
    let mode = if app.mode == Mode::Edit { " EDIT " } else { " VIEW " };
    put(buf, 0, y, mode, Style::new().add_modifier(Modifier::REVERSED));
    let plus = if app.ws.modified() { " [+]" } else { "" };
    let cursor = time::format(app.ws.view.cursor, fpt);
    let right = format!("{cursor}   {}   {}{plus} ", zoom_text(&app.ws.view, fpt), app.ws.file_name);
    let (left, st) = match &app.msg {
        Some((m, err)) => (m.clone(), if *err { red() } else { Style::new() }),
        None => match app.ws.rows.get(app.ws.sel) {
            // a stream's record at the cursor, as much of it as fits (`e` shows all of it)
            Some(Row::Signal(r)) if app.ws.record_at(r.var).is_some() => {
                let (s, i) = app.ws.record_at(r.var).unwrap();
                let rec = &s.records[i];
                let fields: Vec<String> = s.fields_text(i, fpt).into_iter().map(|(n, v)| format!("{n}={v}")).collect();
                let times = format!("{}–{}", time::format(rec.start, fpt), time::format(rec.end, fpt));
                let text = format!("{} #{i}  {times}  {}", s.name, fields.join(" "));
                let room = w.saturating_sub(mode.len() + 1 + right.chars().count() + 2);
                let text = if text.chars().count() > room { text.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { text };
                (text, Style::new())
            }
            Some(Row::Signal(r)) => {
                // a bit as `data[3]`, a stream's field by its own path
                let (var, bit) = app.ws.probe(r.var, app.ws.bit);
                let bit = bit.map_or(String::new(), |b| format!("[{b}]"));
                (format!("{}{bit}", app.ws.trace.vars[var].path), Style::new())
            }
            Some(Row::Group { name, .. }) => (name.clone(), Style::new()),
            None => (String::new(), Style::new()),
        },
    };
    let x = mode.len() + 1;
    let room = w.saturating_sub(x + right.chars().count() + 2);
    let len = left.chars().count();
    if app.msg.is_some() && len > room {
        // a long message wins over the times
        put(buf, x, y, &left, st);
        return;
    }
    // a long path keeps its end, where the signal name is
    let left = if len > room { format!("…{}", left.chars().skip(len + 1 - room).collect::<String>()) } else { left };
    put(buf, x, y, &left, st);
    put(buf, w.saturating_sub(right.chars().count()), y, &right, Style::new());
}

fn command_line(app: &App, buf: &mut Buffer, w: usize, h: usize) -> Option<(u16, u16)> {
    let y = h - 1;
    let search = app.mode == Mode::Search;
    let line = format!("{}{}", if search { '/' } else { ':' }, app.line);
    put(buf, 0, y, &format!("{line:<w$}"), Style::new());
    let items = if search { Vec::new() } else { crate::tui::commands::completions(&app.line) };
    let uw = items.iter().map(|c| c.usage.len()).max().unwrap_or(0);
    for (k, c) in items.iter().enumerate().take(y) {
        let mark = if k == app.hl { " ▌ " } else { "   " };
        let text = format!("{mark}{:<uw$}  {}", c.usage, c.desc);
        // opaque: the cursor column, selection and grid lines must not show through
        let st = Style::new().bg(Color::Reset).fg(Color::Reset);
        let st = if k == app.hl { st.add_modifier(Modifier::BOLD) } else { st };
        put(buf, 0, y - 1 - k, &format!("{text:<w$}"), st);
    }
    // a rule between the list and the waveforms above it
    if let Some(ry) = y.checked_sub(items.len() + 1).filter(|_| !items.is_empty()) {
        put(buf, 0, ry, &"─".repeat(w), Style::new().fg(pal().dim).bg(Color::Reset));
    }
    Some(((1 + app.cursor()).min(w.saturating_sub(1)) as u16, y as u16))
}
