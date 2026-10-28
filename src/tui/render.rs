//! The terminal UI's screen (spec §4, §14): the workspace's rows (`draw`), with the signal panel,
//! the `?` box, the launch guide, the status line and the command line around them.

use crate::tui::app::{App, Mode};
use crate::draw::{self, gl, pal, put, red, ROW_LINES};
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
        app.page = draw::rows(&mut app.ws, &mut app.top, adding, &mut right, w - pw, body);
    }
    right.area.x = area.x + pw as u16;
    buf.merge(&right);
    let search = if pw > 0 { panel(app, buf, pw, body) } else { None };
    if app.help {
        help(app, buf, w, body);
    }
    if let Some((title, lines)) = &app.examine {
        boxed(buf, w, body, title, &plain(lines), " Esc e close ");
    }
    if let Some(r) = &mut app.records {
        // the title line and a blank, then as many lines as fit; records scroll to show the
        // highlight, a text by the line (`hl` is its first line shown)
        let text = r.title.is_empty();
        r.page = body.saturating_sub(if text { 2 } else { 4 }).max(1);
        if text {
            r.hl = r.hl.min(r.items.len().saturating_sub(r.page));
            r.top = r.hl;
        } else if r.hl < r.top {
            r.top = r.hl;
        } else if r.hl >= r.top + r.page {
            r.top = r.hl + 1 - r.page;
        }
        // as wide as the widest line, so the box keeps its size while scrolling
        let ww = r.items.iter().map(|i| i.1.chars().count() + 2).max().unwrap_or(0);
        let mut lines = if text { Vec::new() } else { vec![format!("{:<ww$}", r.title), String::new()] };
        for (k, (_, l)) in r.items.iter().enumerate().skip(r.top).take(r.page) {
            lines.push(match text {
                true => format!("{l:<ww$}"),
                false => format!("{}{l}", if k == r.hl { format!("{} ", gl().hl) } else { "  ".into() }),
            });
        }
        boxed(buf, w, body, &r.name, &plain(&lines), if text { " j k scroll  Esc close " } else { " Enter jump  Esc close " });
    }
    let at = if matches!(app.mode, Mode::Command | Mode::Search) {
        command_line(app, buf, w, h)
    } else {
        status(app, buf, w, h - 1);
        search
    };
    draw::fill_screen(buf);
    at
}

/// The signal panel (spec §4 "Edit mode", §5). Returns the search cursor while typing a search.
fn panel(app: &mut App, buf: &mut Buffer, pw: usize, body: usize) -> Option<(u16, u16)> {
    let dim = Style::new().fg(pal().dim);
    let inner = pw - 2;
    if pal().mono {
        // no background to tell the panel by: a border
        put(buf, 0, 0, &format!("{} signals {}{}", gl().border[0], gl().border[4].to_string().repeat(pw.saturating_sub(11)), gl().border[1]), dim);
        for y in 1..body.saturating_sub(1) {
            put(buf, 0, y, &format!("{v}{}{v}", " ".repeat(inner), v = gl().border[5]), dim);
        }
        put(buf, 0, body.saturating_sub(1), &format!("{}{}{}", gl().border[2], gl().border[4].to_string().repeat(inner), gl().border[3]), dim);
    } else {
        let st = Style::new().bg(pal().panel).fg(Color::Reset);
        for y in 0..body {
            put(buf, 0, y, &" ".repeat(pw), st);
        }
        put(buf, 2, 0, "signals", st.add_modifier(Modifier::BOLD));
    }
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
            None if n.open => gl().group_open,
            None => gl().group_closed,
            Some(v) if app.ws.shown(v) => gl().shown,
            Some(v) if app.panel.marked.contains(&v) => '+',
            Some(_) => ' ',
        };
        let text = format!("{}{mark} {}", " ".repeat(2 * n.depth), n.name);
        // too long: keep the end, where the leaf name is
        let len = text.chars().count();
        let text = if len > inner - 1 { format!("{}{}", gl().cut, text.chars().skip(len + 2 - inner).collect::<String>()) } else { text };
        let marked = n.var.is_some_and(|v| app.panel.marked.contains(&v));
        let st = match (Some(i) == cur, marked) {
            (true, _) => Style::new().bg(pal().selected).fg(pal().cursor).add_modifier(Modifier::BOLD | pal().accent()),
            (false, true) => Style::new().fg(pal().cursor),
            (false, false) => Style::new(),
        };
        let st = if range.contains(&i) { st.bg(pal().selected) } else { st };
        put(buf, 1, y, &format!(" {text:<w$}", w = inner - 1), st);
    }
    let q = app.panel.query.as_ref()?;
    Some(((3 + q.chars().count()).min(pw - 2) as u16, 1))
}

type Keys = &'static [(&'static str, &'static [(&'static str, &'static str)])];
type Examples = &'static [(&'static str, &'static str)];

/// The keys of each mode for the `?` box: its title, the keys under section headings, and a few
/// things to type, each with what it does.
fn help_text(app: &App) -> (&'static str, Keys, &'static str, Examples) {
    if app.panel_shown() {
        let keys: Keys = &[
            ("Move", &[("j k", "move"), ("l h", "open / close a scope"), ("gg G", "first / last line"), ("/", "search paths"), ("n N", "next / previous match")]),
            ("Mark", &[("Space", "mark a signal or scope"), ("V", "mark a range"), ("Ctrl-a", "mark all matches")]),
            ("Add", &[("Enter", "add as rows"), ("Ctrl-g", "add as a group"), ("u Ctrl-r", "undo / redo"), ("Esc", "clear marks / close")]),
        ];
        return ("signal panel", keys, "", &[("", "New signals go below the selected row, at the accent line.")]);
    }
    if app.editing_rows() {
        let keys: Keys = &[
            ("Rows", &[("j k", "select a row"), ("J K", "move down / up"), ("V", "select a range"), ("Space", "fold a group, bus or stream"), ("e", "examine: the whole value")]),
            ("Change", &[("a", "add signals"), ("/", "add signals, searching"), ("dd", "remove"), ("r", "next radix"), ("u Ctrl-r", "undo / redo")]),
            ("Other", &[(":", "command"), ("Esc", "back to View")]),
        ];
        let commands: Examples = &[
            (":group <name>", "group the selected rows"),
            (":rename <name>", "rename the group"),
            (":radix q4.12", "a radix: hex bin dec ascii, fixed point"),
            (":color cyan", "colour the row"),
            (":w", "save the session"),
        ];
        return ("EDIT", keys, "Commands", commands);
    }
    let keys: Keys = &[
        (
            "Move",
            &[
                ("h l", "cursor left / right"),
                ("w b", "next / prev change"),
                ("gg G", "start / end"),
                ("z x", "zoom in / out"),
                ("c", "center the cursor"),
                ("H L", "screen left / right"),
                ("Ctrl-d/u", "half a screen"),
            ],
        ),
        (
            "Rows",
            &[
                ("j k", "select a row"),
                ("Ctrl-f/b", "a screen of rows"),
                ("Space", "fold a group or bus"),
                ("e", "the whole value"),
                ("yy yt", "copy value / time"),
                ("i", "edit rows"),
            ],
        ),
        ("Search", &[("/", "search"), ("n N", "next / previous match")]),
        ("Marks", &[("m M", "put / remove a mark"), ("Tab", "next mark"), ("'2..'9", "jump to a mark"), ("''", "back before the jump")]),
        ("Other", &[(":", "command"), ("5l 3w", "a count repeats a key"), ("q", "quit")]),
    ];
    let search: Examples = &[
        ("/0x40", "the selected row's value, in any radix"),
        ("/x  /z", "unknown, high impedance"),
        ("/valid && ready && data == 0x40", "several signals at once"),
        ("/@clk fell(valid) && !past(ready)", "sampled at clk; also rose() stable()"),
    ];
    ("VIEW", keys, "Search expressions", search)
}

/// A line of text in parts, each in its own style.
type Line = Vec<(String, Style)>;

/// The `?` box: the current mode's keys in the accent, under dim headings, in as many columns as
/// fit; the things to type below them.
fn help(app: &App, buf: &mut Buffer, w: usize, body: usize) {
    let (title, sections, heading, examples) = help_text(app);
    let key = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    let dim = Style::new().fg(pal().dim);
    // each column's keys as wide as its widest
    let kw = |sec: Keys| sec.iter().flat_map(|(_, k)| k.iter()).map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let width = |sec: Keys| sec.iter().flat_map(|(_, k)| k.iter()).map(|(_, d)| kw(sec) + 2 + d.chars().count()).max().unwrap_or(0);
    const GAP: usize = 3;
    let lines_of = |sec: Keys| sec.iter().map(|(_, k)| k.len() + 1).sum::<usize>() + sec.len().saturating_sub(1);
    // the most columns that fit side by side: sections fill them in order, each about as tall
    let mut cols: Vec<Keys> = vec![sections];
    for n in (2..=3).rev() {
        let target = lines_of(sections).div_ceil(n);
        let mut split = Vec::new();
        let mut start = 0;
        for i in 1..sections.len() {
            if split.len() + 1 < n && lines_of(&sections[start..i + 1]) > target {
                split.push(&sections[start..i]);
                start = i;
            }
        }
        split.push(&sections[start..]);
        if split.iter().map(|c| width(c) + GAP).sum::<usize>() <= w.saturating_sub(4) + GAP {
            cols = split;
            break;
        }
    }
    let mut lines: Vec<Line> = Vec::new();
    for (x, col) in cols.iter().enumerate() {
        let (cw, kw) = (width(col) + if x + 1 < cols.len() { GAP } else { 0 }, kw(col));
        let mut y = 0;
        for (i, (head, keys)) in col.iter().enumerate() {
            let mut text = vec![vec![(format!("{head:<cw$}"), dim)]];
            text.extend(keys.iter().map(|(k, d)| vec![(format!("{k:<kw$}  "), key), (format!("{d:<w$}", w = cw - kw - 2), Style::new())]));
            if i > 0 {
                text.insert(0, vec![(" ".repeat(cw), Style::new())]);
            }
            for t in text {
                if lines.len() <= y {
                    lines.push(Vec::new());
                }
                // a shorter column before this one: pad to where this one starts
                let have: usize = lines[y].iter().map(|(s, _)| s.chars().count()).sum();
                let at: usize = cols[..x].iter().map(|c| width(c) + GAP).sum();
                if have < at {
                    lines[y].push((" ".repeat(at - have), Style::new()));
                }
                lines[y].extend(t);
                y += 1;
            }
        }
    }
    if !examples.is_empty() {
        lines.push(Vec::new());
        if !heading.is_empty() {
            lines.push(vec![(heading.to_string(), dim)]);
        }
        let ew = examples.iter().map(|(e, _)| e.chars().count()).max().unwrap_or(0);
        for (e, d) in examples {
            lines.push(match e.is_empty() {
                true => vec![(d.to_string(), dim)],
                false => vec![(format!("{e:<ew$}  "), Style::new().fg(pal().cursor)), (d.to_string(), dim)],
            });
        }
    }
    boxed(buf, w, body, &format!("help {} {title}", gl().dot), &lines, " Esc ? close ");
}

/// A box over the middle of the screen: a title, lines, and how to close it. Lines it can't hold
/// end in `… n more lines`.
fn boxed(buf: &mut Buffer, w: usize, body: usize, title: &str, lines: &[Line], close: &str) {
    let mut lines = lines.to_vec();
    let room = body.saturating_sub(2);
    if lines.len() > room && room > 0 {
        // shortcut: a box can't scroll; add scrolling when values longer than a screen matter
        let more = lines.len() - room + 1;
        lines.truncate(room - 1);
        lines.push(vec![(format!("{} {more} more lines", gl().cut), Style::new().fg(pal().dim))]);
    }
    let len = |l: &Line| l.iter().map(|(s, _)| s.chars().count()).sum::<usize>();
    let top = format!(" {title} ");
    let widest = lines.iter().map(len).max().unwrap_or(0).max(top.chars().count()).max(close.chars().count());
    // margins too narrow to read what is under them: the whole width
    let inner = if widest + 4 + 12 > w { w.saturating_sub(4) } else { widest };
    let (bw, bh) = (inner + 4, lines.len() + 2);
    if bw > w || bh > body || bw < close.chars().count() + 3 {
        return;
    }
    let (x0, y0) = ((w - bw) / 2, (body - bh) / 2);
    // opaque: the waveforms and cursor columns must not show through
    let st = Style::new().bg(pal().panel).fg(Color::Reset);
    let dim = st.fg(pal().dim);
    let top: String = top.chars().take(bw - 2).collect();
    if pal().mono {
        // no background to tell the box by: a border
        put(buf, x0, y0, &format!("{}{top}{}{}", gl().border[0], gl().border[4].to_string().repeat(bw - 2 - top.chars().count()), gl().border[1]), dim);
        put(buf, x0, y0 + bh - 1, &format!("{}{}{close}{}{}", gl().border[2], gl().border[4].to_string().repeat(bw - 3 - close.chars().count()), gl().border[4], gl().border[3]), dim);
    } else {
        // a card: its own background, the title in bold, how to close it dim in the corner
        for y in 0..bh {
            put(buf, x0, y0 + y, &" ".repeat(bw), st);
        }
        put(buf, x0 + 1, y0, &top, st.add_modifier(Modifier::BOLD));
        put(buf, x0 + bw - 1 - close.chars().count(), y0 + bh - 1, close, dim);
    }
    for (i, l) in lines.iter().enumerate() {
        if pal().mono {
            put(buf, x0, y0 + 1 + i, &format!("{} ", gl().border[5]), dim);
            put(buf, x0 + bw - 2, y0 + 1 + i, &format!(" {}", gl().border[5]), dim);
        }
        put(buf, x0 + 2, y0 + 1 + i, &" ".repeat(inner), st);
        let mut x = 0;
        for (text, s) in l {
            let text: String = text.chars().take(inner - x).collect();
            put(buf, x0 + 2 + x, y0 + 1 + i, &text, st.patch(*s));
            x += text.chars().count();
        }
    }
}

/// Lines in one style.
fn plain(lines: &[String]) -> Vec<Line> {
    lines.iter().map(|l| vec![(l.clone(), Style::new())]).collect()
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
    // EDIT in the accent: the keys change the layout; VIEW stays quiet
    let st = match app.mode == Mode::Edit && !pal().mono {
        true => Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(Modifier::BOLD),
        false => Style::new().add_modifier(Modifier::REVERSED),
    };
    put(buf, 0, y, mode, st);
    let plus = if app.ws.modified() { " [+]" } else { "" };
    let cursor = time::format(app.ws.view.cursor, fpt);
    let follow = app.follow.as_ref().filter(|f| f.on).map_or(String::new(), |f| format!("following {}   ", f.id));
    let right = format!("{follow}{cursor}   {}   {}{plus} ", zoom_text(&app.ws.view, fpt), app.ws.file_name);
    let (left, st) = match &app.msg {
        Some((m, err)) => (m.clone(), if *err { red() } else { Style::new() }),
        None => match app.ws.rows.get(app.ws.sel) {
            // a stream's record at the cursor, as much of it as fits (`e` shows all of it)
            Some(Row::Signal(r)) if app.ws.record_at(r.var).is_some() => {
                let (s, i) = app.ws.record_at(r.var).unwrap();
                let rec = &s.records[i];
                let fields: Vec<String> = s.fields_text(i, fpt).into_iter().map(|(n, v)| format!("{n}={v}")).collect();
                let times = format!("{}{}{}", time::format(rec.start, fpt), gl().dash, time::format(rec.end, fpt));
                let text = format!("{} #{i}  {times}  {}", s.name, fields.join(" "));
                let room = w.saturating_sub(mode.len() + 1 + right.chars().count() + 2);
                let text = if text.chars().count() > room { text.chars().take(room.saturating_sub(1)).chain([gl().cut]).collect() } else { text };
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
    let left = if len > room { format!("{}{}", gl().cut, left.chars().skip(len + 1 - room).collect::<String>()) } else { left };
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
        let mark = if k == app.hl { format!(" {} ", gl().hl) } else { "   ".to_string() };
        let text = format!("{mark}{:<uw$}  {}", c.usage, c.desc);
        // opaque: the cursor column, selection and grid lines must not show through
        let st = Style::new().bg(pal().panel).fg(Color::Reset);
        let st = if k == app.hl { st.add_modifier(Modifier::BOLD) } else { st };
        put(buf, 0, y - 1 - k, &format!("{text:<w$}"), st);
    }
    // without a background, a rule between the list and the waveforms above it
    if let Some(ry) = y.checked_sub(items.len() + 1).filter(|_| !items.is_empty() && pal().mono) {
        put(buf, 0, ry, &gl().border[4].to_string().repeat(w), Style::new().fg(pal().dim).bg(Color::Reset));
    }
    Some(((1 + app.cursor()).min(w.saturating_sub(1)) as u16, y as u16))
}
