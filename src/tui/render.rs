//! The terminal UI's screen (spec §4, §14): the workspace's rows (`draw`), with the signal panel,
//! the `?` box, the launch guide, the status line and the command line around them.

use crate::tui::app::{App, Held, Mode};
use crate::tui::keys::Action as A;
use crate::draw::{self, gl, pal, put, ROW_LINES};
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
    app.hit = crate::tui::app::Hit { body, ..Default::default() };
    app.note_msg();
    // computed signals made since (`:signal`, a slice named in a search)
    if app.panel.version != app.ws.trace.version {
        app.panel.refresh(&app.ws.trace);
    }
    // the signal panel takes the left of the screen; the rows are drawn beside it
    // (not on screens too small to hold it)
    let pw = if app.panel_shown() && w >= 30 && body >= 4 { (w / 3).clamp(24, 48).min(w / 2) } else { 0 };
    app.hit.x0 = pw;
    let mut right = Buffer::empty(Rect::new(0, 0, (w - pw) as u16, h as u16));
    if app.ws.rows.is_empty() {
        app.ws.view.set_width((w - pw) as u64);
        guide(app, &mut right, w - pw, body);
    } else if body > ROW_LINES {
        // the time axis and at least one row
        let adding = app.panel_shown();
        let d = draw::rows(&mut app.ws, &mut app.top, adding, &mut right, w - pw, body);
        (app.page, app.wave_x) = (d.fit, pw + d.wave_x);
        (app.hit.head, app.hit.name_w, app.hit.lines) = (d.head, d.name_w, d.lines);
    }
    right.area.x = area.x + pw as u16;
    buf.merge(&right);
    // a row being moved, lifted: its name in the accent, its lines tinted (spec §2 "Drags")
    if let Some(Held::Move { row, len, moved: true, .. }) = app.held {
        let name = if pal().mono { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(Modifier::BOLD) };
        for &(ly, _, _) in app.hit.lines.iter().filter(|(_, (r, _), _)| (row..row + len).contains(r)) {
            for y in (ly..ly + ROW_LINES).filter(|&y| y < body) {
                for x in pw..w {
                    let cell = &mut buf[(x as u16, y as u16)];
                    match x < pw + app.hit.name_w {
                        true => _ = cell.set_style(name),
                        false => _ = cell.set_bg(pal().cursor_dim),
                    }
                }
            }
        }
    }
    // a zoom being dragged: its columns tinted as the marks are (spec §2 "Drags")
    if let Some(Held::Zoom { from, to, moved: true, .. }) = &app.held {
        for x in (app.wave_x + *from.min(to) as usize..=app.wave_x + *from.max(to) as usize).filter(|&x| x < w) {
            (0..body).for_each(|y| _ = buf[(x as u16, y as u16)].set_bg(pal().cursor_dim));
        }
    }
    let search = if pw > 0 { panel(app, buf, pw, body) } else { None };
    if let (label, Some((x, y))) = app.drag_label() {
        // what a drop adds, beside the pointer
        let text = format!(" + {label} ");
        let x = (x + 1).min(w.saturating_sub(text.chars().count()));
        put(buf, x, y, &text, Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(Modifier::BOLD));
    }
    // under what has the keys, so they don't show around its edges
    let covered = app.help || app.examine.is_some() || app.records.is_some() || app.messages.is_some() || app.menu.is_some() || app.leader.is_some() || app.picker.is_some();
    let drawn = if covered { 0 } else { notes(app, buf, w, body) };
    app.hit.unshown = drawn == 0 && !app.shown_notes(std::time::Instant::now()).is_empty();
    if app.help {
        let mut scroll = app.scroll;
        app.hit.boxed = help(app, buf, w, body, &mut scroll);
        app.scroll = scroll;
    }
    if let Some(ex) = &app.examine {
        // as a hover: beside the cursor, under the selected row
        let x = app.wave_x + app.ws.view.col_of(app.ws.view.cursor).map_or(0, |c| c as usize);
        let y = app.hit.lines.iter().find(|(_, (r, b), _)| *r == app.ws.sel && *b == app.ws.bit).map(|l| l.0);
        let nw = ex.lines.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(0);
        let name = Style::new().fg(pal().hues[5]);
        let lines: Vec<Line> = ex.lines.iter().map(|(n, v)| match n.is_empty() {
            true => vec![(v.clone(), Style::new())],
            false => vec![(format!("{n:<nw$}  "), name), (v.clone(), Style::new())],
        }).collect();
        let ic = if ex.record { draw::icon::STREAM } else { draw::icon::WIRE };
        app.hit.boxed = boxed(buf, w, body, &float_title(ic, &ex.title), &format!("{}{}", icon_text(draw::icon::CLOCK), ex.at), &lines, "e Esc close", &mut app.scroll, y.map(|y| (x, y)));
    }
    if let Some(r) = &mut app.records {
        // the title line and a blank, then as many lines as fit; records scroll to show the
        // highlight, a text by the line (`hl` is its first line shown)
        let text = r.title.is_empty();
        r.page = body.saturating_sub(if text { 2 } else { 3 }).max(1);
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
        let mut lines = if text { Vec::new() } else { vec![format!("{:<ww$}", r.title)] };
        for (k, (_, l)) in r.items.iter().enumerate().skip(r.top).take(r.page) {
            lines.push(match text {
                true => format!("{l:<ww$}"),
                false => format!("{}{l}", if k == r.hl { format!("{} ", gl().hl) } else { "  ".into() }),
            });
        }
        let shown = (r.top..r.items.len()).take(r.page);
        app.hit.boxed = boxed(buf, w, body, &float_title(if text { draw::icon::INFO } else { draw::icon::RECORD }, &r.name), "", &plain(&lines), if text { "j k scroll  Esc close" } else { "Enter jump  Esc close" }, &mut 0, None);
        if let Some(b) = app.hit.boxed.filter(|_| !text) {
            // under the title line
            app.hit.records = shown.enumerate().map(|(k, i)| (b.y as usize + 2 + k, i)).collect();
        }
    }
    messages(app, buf, w, body);
    crate::tui::picker::draw(app, buf, w, body);
    if app.leader.is_some() {
        leader(app, buf, w, body);
    } else if app.which_key {
        which_key(app, buf, w, body);
    }
    if app.menu.is_some() {
        menu(app, buf, w, body);
    }
    let at = if matches!(app.mode, Mode::Command | Mode::Search) {
        command_line(app, buf, w, h)
    } else {
        app.hit.button = Some(status(app, buf, w, h - 1));
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
        app.hit.panel.push((y, i));
        let mark = match n.var {
            None if n.open => gl().group_open,
            None => gl().group_closed,
            Some(v) if app.ws.shown(v) => gl().shown,
            Some(v) if app.panel.marked.contains(&v) => '+',
            Some(_) => ' ',
        };
        // a struct or array shown or marked as one signal says so after its name
        let whole = match n.whole {
            Some(v) if app.ws.shown(v) => format!(" {}", gl().shown),
            Some(v) if app.panel.marked.contains(&v) => " +".into(),
            _ => String::new(),
        };
        let text = format!("{}{mark} {}{whole}", " ".repeat(2 * n.depth), n.name);
        // too long: keep the end, where the leaf name is
        let len = text.chars().count();
        let text = if len > inner - 1 { format!("{}{}", gl().cut, text.chars().skip(len + 2 - inner).collect::<String>()) } else { text };
        let marked = n.var.or(n.whole).is_some_and(|v| app.panel.marked.contains(&v));
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

/// Sections of the `?` box: a heading, then each key and what it does.
type Section = (&'static str, Vec<(String, &'static str)>);
type Keys<'a> = &'a [Section];
type Examples = &'static [(&'static str, &'static str)];

/// The keys for the `?` box, the panel's while it is open: its title, the keys under section
/// headings, and a few things to type, each with what it does. Normal mode's keys are the
/// keymap's, so a key changed in the config file shows here (spec §19 "Keys").
fn help_text(app: &App) -> (&'static str, Vec<Section>, &'static str, Examples) {
    let fixed = |keys: &[(&str, &'static str)]| keys.iter().map(|&(k, d)| (k.to_string(), d)).collect::<Vec<_>>();
    if app.panel_shown() {
        let keys = vec![
            ("Move", fixed(&[("j k", "move"), ("l h", "open / close a scope"), ("gg G", "first / last line"), ("/", "search paths"), ("n N", "next / previous match")])),
            ("Mark", fixed(&[("Space", "mark a signal or scope"), ("V", "mark a range"), ("Ctrl-a", "mark all matches")])),
            ("Add", fixed(&[("Enter", "add as rows"), ("Ctrl-g", "add as a group"), ("u Ctrl-r", "undo / redo"), ("a", "close"), ("Esc", "clear marks / close")])),
        ];
        return ("signal panel", keys, "", &[("", "New signals go below the selected row, at the accent line.")]);
    }
    let m = &app.keymap;
    let k = |acts: &[A], d: &'static str| {
        let keys: Vec<String> = acts.iter().map(|&a| m.show(a)).collect();
        // `Ctrl-d/u`: one Ctrl for both, so the column stays narrow
        let rest: Option<Vec<&str>> = keys.iter().map(|k| k.strip_prefix("Ctrl-")).collect();
        match rest {
            Some(r) if r.len() > 1 => (format!("Ctrl-{}", r.join("/")), d),
            _ => (keys.join(" "), d),
        }
    };
    let j = m.show(A::Jump);
    let keys = vec![
        (
            "Move",
            vec![
                k(&[A::Left, A::Right], "cursor left / right"),
                k(&[A::NextChange, A::PrevChange], "next / prev change"),
                k(&[A::Start, A::End], "start / end"),
                k(&[A::ZoomIn, A::ZoomOut], "zoom in / out"),
                k(&[A::Center], "center the cursor"),
                k(&[A::ScreenLeft, A::ScreenRight], "screen left / right"),
                k(&[A::HalfRight, A::HalfLeft], "half a screen"),
            ],
        ),
        (
            "Rows",
            vec![
                k(&[A::Down, A::Up], "select a row"),
                k(&[A::PageDown, A::PageUp], "a screen of rows"),
                k(&[A::Fold], "fold a group or bus"),
                k(&[A::Examine], "the whole value"),
                k(&[A::CopyValue, A::CopyTime], "copy value / time"),
            ],
        ),
        (
            "Change",
            vec![
                k(&[A::Panel], "add signals"),
                k(&[A::Remove], "remove"),
                k(&[A::MoveDown, A::MoveUp], "move down / up"),
                k(&[A::Range], "select rows; Esc ends"),
                k(&[A::Radix], "next radix"),
                k(&[A::Undo, A::Redo], "undo / redo"),
            ],
        ),
        ("Search", vec![k(&[A::Search], "search"), k(&[A::NextMatch, A::PrevMatch], "next / previous match")]),
        (
            "Marks",
            vec![
                k(&[A::Mark, A::Unmark], "put / remove a mark"),
                k(&[A::NextMark], "next mark"),
                (format!("{j}2..{j}9"), "jump to a mark"),
                k(&[A::JumpBack], "back before the jump"),
            ],
        ),
        ("Other", vec![k(&[A::Command], "command"), k(&[A::Leader], "leader menu"), (format!("5{} 3{}", m.show(A::Right), m.show(A::NextChange)), "a count repeats a key"), k(&[A::Quit], "quit")]),
    ];
    let search: Examples = &[
        ("/0x40", "the selected row's value, in any radix"),
        ("/x  /z", "unknown, high impedance"),
        ("/valid && ready && data == 0x40", "several signals at once"),
        ("/@clk fell(valid) && !past(ready)", "sampled at clk; also rose() stable()"),
    ];
    ("keys", keys, "Search expressions", search)
}

/// A line of text in parts, each in its own style.
type Line = Vec<(String, Style)>;

/// The `?` box: the current mode's keys in the accent, under dim headings, in as many columns as
/// fit; the things to type below them.
fn help(app: &App, buf: &mut Buffer, w: usize, body: usize, scroll: &mut usize) -> Option<Rect> {
    let (title, sections, heading, examples) = help_text(app);
    let sections = &sections[..];
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
        // too narrow for both side by side: what it does under each
        let stacked = examples.iter().any(|(_, d)| ew + 2 + d.chars().count() > w.saturating_sub(4));
        for (e, d) in examples {
            let ex = (if stacked { e.to_string() } else { format!("{e:<ew$}  ") }, Style::new().fg(pal().cursor));
            match (e.is_empty(), stacked) {
                (true, _) => lines.push(vec![(d.to_string(), dim)]),
                (false, true) => lines.extend([vec![ex], vec![(format!("  {d}"), dim)]]),
                (false, false) => lines.push(vec![ex, (d.to_string(), dim)]),
            }
        }
    }
    boxed(buf, w, body, &float_title(draw::icon::KEYS, &format!("help {} {title}", gl().dot)), "", &lines, "? Esc close", scroll, None)
}

/// A float (spec §2 `e`, `?`): a card with the title and `right` in its top border, the keys in
/// its bottom one; under the row line at `at` (screen x, y), or above it where there is no room,
/// else in the middle. Too long for the screen it takes the whole width and scrolls from `top`,
/// its bottom border saying which lines show. Returns where it was drawn.
#[allow(clippy::too_many_arguments)]
fn boxed(buf: &mut Buffer, w: usize, body: usize, title: &[(String, Style)], right: &str, lines: &[Line], keys: &str, top: &mut usize, at: Option<(usize, usize)>) -> Option<Rect> {
    let all = lines.len();
    let room = body.saturating_sub(2);
    let full = all > room && room > 0;
    let mut lines = lines.to_vec();
    let mut footer = keys.to_string();
    if full {
        *top = (*top).min(all - room);
        lines = lines[*top..*top + room].to_vec();
        footer = format!("{}{}{} of {all}  j k scroll  {keys}", *top + 1, gl().dash, *top + room);
    } else {
        *top = 0;
    }
    let len = |l: &Line| l.iter().map(|(s, _)| s.chars().count()).sum::<usize>();
    let head = title.iter().map(|(t, _)| t.chars().count()).sum::<usize>() + right.chars().count() + 8;
    let widest = lines.iter().map(len).max().unwrap_or(0).max(head).max(footer.chars().count() + 6);
    // margins too narrow to read what is under them, or too long for the screen: the whole width
    let inner = if widest + 4 + 12 > w || full { w.saturating_sub(4) } else { widest };
    let (bw, bh) = (inner + 4, lines.len() + 2);
    if bw > w || bh > body || bh < 3 {
        return None;
    }
    let below = |y: usize| y + draw::ROW_LINES;
    let (x0, y0) = match at.filter(|_| !full) {
        Some((x, y)) if below(y) + bh <= body || y >= bh => {
            let x0 = if x + 2 + bw <= w { x + 2 } else { x.saturating_sub(bw + 1) }.min(w - bw);
            (x0, if below(y) + bh <= body { below(y) } else { y - bh })
        }
        _ => ((w - bw) / 2, (body - bh) / 2),
    };
    let r = Rect::new(x0 as u16, y0 as u16, bw as u16, bh as u16);
    card_frame(buf, r, pal().dim, title, right, &footer);
    let st = card_style();
    for (i, l) in lines.iter().enumerate() {
        let mut x = 0;
        for (text, s) in l {
            let text: String = text.chars().take(inner - x).collect();
            put(buf, x0 + 2 + x, y0 + 1 + i, &text, st.patch(*s));
            x += text.chars().count();
        }
    }
    Some(r)
}

/// A float's title: an icon and the name in the accent.
fn float_title(ic: char, name: &str) -> Vec<(String, Style)> {
    vec![(format!("{}{name}", icon_text(ic)), Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD))]
}

/// The right click's menu (spec §2 "Menu"), with its corner at the pointer moved to fit, and
/// its second menu beside the item that opened it.
fn menu(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    let Some(m) = &app.menu else { return };
    let main = card(buf, w, body, m.at, &m.items, m.hl, true);
    app.hit.menus = main.into_iter().collect();
    if let (Some(r), Some((p, sub))) = (main, &m.sub) {
        let sw = card_width(&sub.items);
        // right of the menu, or left where there is no room
        let x = if r.right() as usize + sw <= w { r.right() as usize } else { (r.x as usize).saturating_sub(sw) };
        if let Some(r) = card(buf, w, body, (x, r.y as usize + p), &sub.items, sub.hl, false) {
            app.hit.menus.push(r);
        }
    }
}

/// The newest messages in the top right, newest first (spec §4 "Notifications"): an agent's
/// card cyan with its id (a step's tool at the right, what it asked, its answer in green), an
/// error's red, the user's dim with the word before a `:` as its title; each fades over its last
/// second.
/// Returns how many it drew.
fn notes(app: &App, buf: &mut Buffer, w: usize, body: usize) -> usize {
    use crate::draw::icon;
    let now = std::time::Instant::now();
    let nw = (w * 2 / 5).clamp(24, 49);
    if nw + 2 > w {
        return 0;
    }
    let mut drawn = 0;
    let x0 = w - nw - 1;
    let mut y = 2.min(body);
    let green = pal().hues[3];
    for n in app.shown_notes(now).into_iter().take(3) {
        let fading = crate::tui::app::note_life(n.err).saturating_sub(now.duration_since(n.at)) < std::time::Duration::from_secs(1);
        // the title (icon, word, colour), the tool at the right, the lines and their colours
        let short = n.text.split_once(": ").filter(|(t, _)| !t.contains(' ') && t.chars().count() <= 16);
        let (ic, word, col, right, body_lines): (char, String, Color, String, Vec<(String, Color)>) = match (&n.step, &n.who) {
            (_, _) if n.err => (icon::ERROR, "error".into(), pal().unknown, String::new(), vec![(n.text.clone(), Color::Reset)]),
            (Some((tool, ask, answer)), Some(id)) => (icon::AGENT, id.clone(), pal().hues[4], tool.clone(), vec![(ask.clone(), Color::Reset), (answer.clone(), green)]),
            (_, Some(id)) => (icon::AGENT, id.clone(), pal().hues[4], String::new(), vec![(n.text.clone(), Color::Reset)]),
            (_, None) => match short {
                Some((t, rest)) => (icon::OK, t.into(), green, String::new(), vec![(rest.into(), Color::Reset)]),
                None => (icon::INFO, String::new(), pal().dim, String::new(), vec![(n.text.clone(), Color::Reset)]),
            },
        };
        let mut lines: Vec<(String, Color)> = Vec::new();
        for (t, c) in body_lines {
            let room = 3usize.saturating_sub(lines.len()).max(1);
            lines.extend(wrap_text(&t, nw - 4, room).into_iter().map(|l| (l, c)));
        }
        lines.truncate(3);
        let bh = lines.len() + 2;
        if y + bh > body {
            break;
        }
        drawn += 1;
        let edge = if fading || (n.who.is_none() && !n.err) { pal().dim } else { col };
        let tcol = if fading { pal().dim } else { col };
        let title_st = Style::new().fg(tcol).add_modifier(Modifier::BOLD);
        let mut title = Vec::new();
        if let Some(c) = draw::icon(ic) {
            title.push((c.to_string(), title_st));
        }
        if !word.is_empty() {
            title.push((if title.is_empty() { word } else { format!(" {word}") }, title_st));
        }
        card_frame(buf, Rect::new(x0 as u16, y as u16, nw as u16, bh as u16), edge, &title, &right, "");
        let st = card_style();
        for (i, (l, c)) in lines.iter().enumerate() {
            put(buf, x0 + 2, y + 1 + i, l, st.fg(if fading { pal().dim } else { *c }));
        }
        y += bh;
    }
    drawn
}

/// `:messages` (spec §4): a panel across the bottom, a message a line: its clock time, an icon
/// and who caused it in the kind's colour, the text; the highlight scrolled into view.
fn messages(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    use crate::draw::icon;
    let Some(hl) = app.messages else { return };
    let page = app.notes.len().min(10).min(body.saturating_sub(4));
    if page == 0 || w < 30 {
        return;
    }
    let bh = page + 3;
    let y0 = body - bh;
    let st = card_style();
    let rule = Style::new().fg(pal().dim);
    put(buf, 0, y0, &gl().card[4].to_string().repeat(w), if pal().mono { rule } else { rule.bg(pal().panel) });
    for y in y0 + 1..body {
        put(buf, 0, y, &" ".repeat(w), st);
    }
    let acc = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    put(buf, 2, y0, &format!(" {}messages ", icon_text(icon::BELL)), st.patch(acc));
    let keys = " j k move  y copy  Esc close ";
    put(buf, w.saturating_sub(keys.chars().count() + 2), y0, keys, st.fg(pal().dim));
    let top = hl.saturating_sub(page - 1).min(app.notes.len() - page);
    let ix = if gl().icons { 2 } else { 0 };
    for (k, n) in app.notes.iter().enumerate().skip(top).take(page) {
        let y = y0 + 2 + k - top;
        let on = k == hl;
        let row = if on { st.bg(pal().selected) } else { st };
        put(buf, 0, y, &" ".repeat(w), row);
        if on {
            put(buf, 0, y, &gl().hl.to_string(), row.fg(pal().cursor));
        }
        let (ic, col) = match (n.err, &n.who) {
            (true, _) => (icon::ERROR, pal().unknown),
            (false, Some(_)) => (icon::AGENT, pal().hues[4]),
            (false, None) => (icon::INFO, pal().hues[5]),
        };
        put(buf, 2, y, &crate::tui::app::clock_of(n), row.fg(pal().dim));
        if let Some(c) = draw::icon(ic) {
            put(buf, 12, y, &c.to_string(), row.fg(col));
        }
        let who = n.who.as_deref().unwrap_or("you");
        put(buf, 12 + ix, y, who, row.fg(col).add_modifier(Modifier::BOLD));
        let x = 12 + ix + who.chars().count().max(8) + 2;
        let room = w.saturating_sub(x + 1);
        let text: String = if n.text.chars().count() > room { n.text.chars().take(room.saturating_sub(1)).chain([gl().cut]).collect() } else { n.text.clone() };
        put(buf, x, y, &text, if on { row.add_modifier(Modifier::BOLD) } else { row });
    }
    app.hit.boxed = Some(Rect::new(0, y0 as u16, w as u16, bh as u16));
}

/// `text` in lines of `width` at most, broken between words, `max` lines (the last cut with `…`).
fn wrap_text(text: &str, width: usize, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        // a word longer than a line is cut into pieces
        let chars: Vec<char> = word.chars().collect();
        for piece in chars.chunks(width.max(1)) {
            let piece: String = piece.iter().collect();
            if !cur.is_empty() && cur.chars().count() + 1 + piece.chars().count() > width {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(&piece);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > max {
        lines.truncate(max);
        let last = &mut lines[max - 1];
        if last.chars().count() >= width {
            *last = last.chars().take(width - 1).collect();
        }
        last.push(gl().cut);
    }
    lines
}

/// A card (which-key, the leader menu, notifications): its background, a round border in
/// `edge`, the title's parts set in the top border at the left, `right` dim in it at the right,
/// `footer` dim in the bottom border at the right; a column cleared on each side.
fn card_frame(buf: &mut Buffer, r: Rect, edge: Color, title: &[(String, Style)], right: &str, footer: &str) {
    let (x0, y0, bw, bh) = (r.x as usize, r.y as usize, r.width as usize, r.height as usize);
    let st = card_style();
    let b = gl().card;
    let w = buf.area.width as usize;
    for y in 0..bh {
        let line = match y {
            0 => format!("{}{}{}", b[0], b[4].to_string().repeat(bw - 2), b[1]),
            _ if y == bh - 1 => format!("{}{}{}", b[2], b[4].to_string().repeat(bw - 2), b[3]),
            _ => format!("{}{}{}", b[5], " ".repeat(bw - 2), b[5]),
        };
        put(buf, x0, y0 + y, &line, st.fg(edge));
        for x in [x0.wrapping_sub(1), x0 + bw].into_iter().filter(|&x| x < w) {
            buf[(x as u16, (y0 + y) as u16)].set_char(' ');
        }
    }
    let mut x = x0 + 2;
    for (i, (t, ts)) in title.iter().enumerate() {
        let t = if i == 0 { format!(" {t}") } else { t.clone() };
        put(buf, x, y0, &t, st.patch(*ts));
        x += t.chars().count();
    }
    if !title.is_empty() {
        put(buf, x, y0, " ", st);
    }
    if !right.is_empty() {
        put(buf, (x0 + bw).saturating_sub(right.chars().count() + 4), y0, &format!(" {right} "), st.fg(pal().dim));
    }
    if !footer.is_empty() {
        put(buf, (x0 + bw).saturating_sub(footer.chars().count() + 4), y0 + bh - 1, &format!(" {footer} "), st.fg(pal().dim));
    }
}

/// A card's inside: the panel's colour (none without colour, where the border tells it).
fn card_style() -> Style {
    if pal().mono { Style::new() } else { Style::new().bg(pal().panel).fg(Color::Reset) }
}

/// An icon and a space before a label, with the `nerd` glyphs (spec §14); nothing with the others.
fn icon_text(c: char) -> String {
    draw::icon(c).map_or(String::new(), |c| format!("{c} "))
}

/// Which-key (spec §2): what can follow the keys pending, in a card in the bottom right; after
/// `'`, the cursors with their names and times; after a count, the keys it goes to.
fn which_key(app: &App, buf: &mut Buffer, w: usize, body: usize) {
    use crate::draw::icon;
    use crate::tui::keys::Keymap;
    let ws = &app.ws;
    let fpt = ws.trace.fs_per_tick;
    // key, icon and its colour, what it does, a time
    type Line = (String, Option<(char, Color)>, String, String);
    let mut lines: Vec<Line> = Vec::new();
    let typed = app.pending();
    let count = typed.chars().all(|c| c.is_ascii_digit());
    let next = if count { app.keymap.counted() } else { app.keymap.next(&app.typed_keys()) };
    let mut group = if count { "times" } else { "" };
    for (k, a) in next {
        if group.is_empty() {
            group = Keymap::group(a);
        }
        if a == A::Jump {
            for (i, t) in ws.cursor_list() {
                let (name, ic) = match i {
                    0 => ("cursor".into(), (icon::CLOCK, pal().hues[4])),
                    _ if ws.labels[i].name.is_empty() => (format!("mark {}", i + 1), (icon::MARK, pal().bus)),
                    _ => (ws.labels[i].name.clone(), (icon::MARK, pal().bus)),
                };
                lines.push((format!("{k}{}", i + 1), Some(ic), name, time::format(t, fpt)));
            }
        } else {
            lines.push((k, None, Keymap::desc(a).into(), String::new()));
        }
    }
    if lines.is_empty() {
        return;
    }
    let ix = if gl().icons && lines.iter().any(|l| l.1.is_some()) { 2 } else { 0 };
    let kw = lines.iter().map(|l| l.0.chars().count()).max().unwrap_or(0);
    let dw = lines.iter().map(|l| l.2.chars().count()).max().unwrap_or(0);
    let tw = lines.iter().map(|l| l.3.chars().count()).max().unwrap_or(0);
    let inner = kw + 3 + ix + dw + if tw > 0 { 3 + tw } else { 0 };
    let title = format!("{typed} {group}");
    let (bw, bh) = ((inner + 4).max(title.chars().count() + 16), lines.len().min(body.saturating_sub(1)) + 2);
    if bw + 1 > w || bh > body || bh < 3 {
        return;
    }
    let r = Rect::new((w - bw - 1) as u16, (body - bh) as u16, bw as u16, bh as u16);
    let acc = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    card_frame(buf, r, pal().dim, &[(typed.clone(), acc), (format!(" {group}"), acc)], "", "Esc close");
    let st = card_style();
    for (y, (k, ic, d, t)) in lines.iter().take(bh - 2).enumerate() {
        let (x, y) = (r.x as usize + 2, r.y as usize + 1 + y);
        put(buf, x, y, k, st.patch(acc));
        put(buf, x + kw + 1, y, &gl().arrow.to_string(), st.fg(pal().dim));
        if let Some((c, col)) = ic.and_then(|(c, col)| draw::icon(c).map(|c| (c, col))) {
            put(buf, x + kw + 3, y, &c.to_string(), st.fg(col));
        }
        put(buf, x + kw + 3 + ix, y, d, if t.is_empty() && k.ends_with('\'') { st.fg(pal().dim) } else { st });
        put(buf, x + inner - t.chars().count(), y, t, st.fg(pal().dim));
    }
}

/// The leader menu (spec §2): the open group's entries across the bottom of the screen, groups
/// first, in magenta with `+`. In columns, each entry's command dim at its column's right; a group
/// whose entries say what happens, one a line: label, command in blue, what happens dim.
fn leader(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    use crate::draw::icon;
    use crate::tui::keys::Entry;
    let Some(path) = &app.leader else { return };
    let Some(Entry::Group { name, items }) = app.keymap.leader.at(path) else { return };
    let mut items: Vec<&(crate::tui::keys::Key, Entry)> = items.iter().collect();
    items.sort_by_key(|(_, e)| !matches!(e, Entry::Group { .. }));
    let why = items.iter().any(|(_, e)| matches!(e, Entry::Run { why, .. } if !why.is_empty()));
    let cols = if why { 1 } else { (w.saturating_sub(3) / 39).clamp(1, 3) };
    let rows = items.len().div_ceil(cols).max(1);
    let bh = rows + 4;
    if bh > body || w < 20 {
        return;
    }
    let r = Rect::new(0, (body - bh) as u16, w as u16, bh as u16);
    let acc = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    let mut title = vec![(app.pending(), acc)];
    if !name.is_empty() {
        title.push((format!("  {name}"), acc));
    }
    card_frame(buf, r, pal().dim, &title, "", &format!("Esc close   Backspace {}", if path.is_empty() { "close" } else { "up" }));
    let st = card_style();
    let ix = if gl().icons { 2 } else { 0 };
    let cw = if why { w - 6 } else { (w - 6) / cols };
    let cut = |t: &str, n: usize| if t.chars().count() > n { t.chars().take(n.saturating_sub(1)).chain([gl().cut]).collect() } else { t.to_string() };
    let mut hits = Vec::new();
    for (i, (k, e)) in items.into_iter().enumerate() {
        let (x, y) = (3 + (i / rows) * cw, r.y as usize + 2 + i % rows);
        put(buf, x, y, &k.show(), st.patch(acc));
        put(buf, x + 2, y, &gl().arrow.to_string(), st.fg(pal().dim));
        let lx = x + 4 + ix;
        match e {
            Entry::Group { name, .. } => {
                let ic = match name.as_str() {
                    "signals" => icon::WIRE,
                    "cursors" => icon::MARK,
                    "decode" => icon::PLUG,
                    "agent" => icon::AGENT,
                    "session" => icon::SESSION,
                    _ => icon::FOLDER,
                };
                put(buf, x + 4, y, &icon_text(ic), st.fg(pal().bus));
                let mag = if pal().mono { st.add_modifier(Modifier::BOLD) } else { st.fg(pal().hues[6]).add_modifier(Modifier::BOLD) };
                put(buf, lx, y, &cut(&format!("+{name}"), cw.saturating_sub(6 + ix)), mag);
            }
            Entry::Run { label, line, why: text } => {
                put(buf, x + 4, y, &icon_text(crate::tui::commands::icon(line)), st.fg(pal().hues[4]));
                let cmd = line.trim_end();
                if why {
                    // label, command, what happens: at fixed columns, as far as the screen goes
                    let (cx, wx) = (x + 39, x + 61);
                    put(buf, lx, y, &cut(label, cx.saturating_sub(lx + 1)), st);
                    put(buf, cx, y, &cut(cmd, 21), st.fg(pal().hues[5]));
                    put(buf, wx, y, &cut(text, w.saturating_sub(wx + 3)), st.fg(pal().dim));
                } else {
                    let room = cw.saturating_sub(6 + ix);
                    let lw = label.chars().count().min(room);
                    put(buf, lx, y, &cut(label, room), st);
                    let cl = cmd.chars().count();
                    if lw + 2 + cl <= room.saturating_sub(4) {
                        put(buf, x + cw - 4 - cl, y, cmd, st.fg(pal().dim));
                    }
                }
            }
        }
        hits.push((Rect::new(x as u16, y as u16, cw as u16, 1), *k));
    }
    app.hit.leader = hits;
}

/// A menu card's width: a column for the highlight, the labels, two spaces, the keys, and a
/// column of margin on each side.
fn card_width(items: &[crate::tui::app::Item]) -> usize {
    let lw = items.iter().map(|i| i.label.chars().count()).max().unwrap_or(0);
    let kw = items.iter().map(|i| i.key.chars().count()).max().unwrap_or(0);
    1 + lw + 2 + kw + 2
}

/// A menu card at `at` (moved left and up to fit with `fit`, else placed as given). Returns
/// where it was drawn.
fn card(buf: &mut Buffer, w: usize, body: usize, at: (usize, usize), items: &[crate::tui::app::Item], hl: Option<usize>, fit: bool) -> Option<Rect> {
    let lw = items.iter().map(|i| i.label.chars().count()).max().unwrap_or(0);
    let kw = items.iter().map(|i| i.key.chars().count()).max().unwrap_or(0);
    let (bw, bh) = (card_width(items), items.len() + 2);
    if bw > w || bh > body || items.is_empty() {
        return None;
    }
    let x0 = at.0.min(w - bw);
    let y0 = if fit || at.1 + bh > body { at.1.min(body - bh) } else { at.1 };
    let st = Style::new().bg(pal().panel).fg(Color::Reset);
    let dim = st.fg(pal().dim);
    let b = gl().border;
    for y in 0..bh {
        let line = match (pal().mono, y) {
            (true, 0) => format!("{}{}{}", b[0], b[4].to_string().repeat(bw - 2), b[1]),
            (true, _) if y == bh - 1 => format!("{}{}{}", b[2], b[4].to_string().repeat(bw - 2), b[3]),
            (true, _) => format!("{}{}{}", b[5], " ".repeat(bw - 2), b[5]),
            (false, _) => " ".repeat(bw),
        };
        put(buf, x0, y0 + y, &line, if pal().mono { dim } else { st });
    }
    for (k, i) in items.iter().enumerate() {
        let on = hl == Some(k);
        let row = if on { st.bg(pal().selected).add_modifier(Modifier::BOLD) } else { st };
        let label = match i.hue {
            Some(h) => row.fg(pal().hues[h]),
            None => row,
        };
        let y = y0 + 1 + k;
        put(buf, x0 + 1, y, &format!("{}{:<lw$}  ", if on { gl().hl } else { ' ' }, i.label), row);
        put(buf, x0 + 2, y, &i.label, label);
        put(buf, x0 + 1 + 1 + lw + 2, y, &format!("{:>kw$}", i.key), if on { row } else { dim });
    }
    Some(Rect::new(x0 as u16, y0 as u16, bw as u16, bh as u16))
}

/// Lines in one style.
fn plain(lines: &[String]) -> Vec<Line> {
    lines.iter().map(|l| vec![(l.clone(), Style::new())]).collect()
}

/// The launch guide: a small wordmark, the trace's name and what is in it, the keys to start with.
fn guide(app: &App, buf: &mut Buffer, w: usize, body: usize) {
    let keys: &[(&str, &str)] = if app.panel_shown() {
        // the panel is open: how to add from it
        &[("/", "search"), ("Space", "mark"), ("Enter", "add"), ("Ctrl-g", "add as a group"), ("Esc", "close")]
    } else {
        &[("a", "add signals"), (":", "commands"), ("?", "help"), ("q", "quit")]
    };
    let t = &app.ws.trace;
    let fpt = t.fs_per_tick;
    let signals = (0..t.vars.len()).filter(|&v| !t.is_computed(v)).count();
    let facts = format!("{signals} signals  {}  {}{}{}", gl().dot, time::format(t.start(), fpt), gl().dash, time::format(t.end(), fpt));
    let (dim, bold) = (Style::new().fg(pal().dim), Style::new().add_modifier(Modifier::BOLD));
    let centred: [(String, Style); 6] = [
        ("w t f".into(), bold.fg(pal().cursor)),
        ("waveform terminal frontend".into(), dim),
        (String::new(), dim),
        (app.ws.file_name.clone(), bold),
        (facts, dim),
        (String::new(), dim),
    ];
    let kw = keys.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let bw = keys.iter().map(|(_, d)| kw + 2 + d.chars().count()).max().unwrap_or(0);
    let y0 = body.saturating_sub(centred.len() + keys.len()) / 2;
    for (i, (l, st)) in centred.iter().enumerate().filter(|(i, _)| y0 + i < body) {
        put(buf, w.saturating_sub(l.chars().count()) / 2, y0 + i, l, *st);
    }
    // the keys as a block
    for (i, (k, d)) in keys.iter().enumerate().filter(|(i, _)| y0 + centred.len() + i < body) {
        let (x, y) = (w.saturating_sub(bw) / 2, y0 + centred.len() + i);
        put(buf, x, y, k, bold.fg(pal().cursor));
        put(buf, x + kw + 2, y, d, Style::new());
    }
}

/// Lines the launch guide takes, with a blank above and below.
pub fn guide_height(app: &App) -> u16 {
    if app.panel_shown() { 13 } else { 12 }
}

fn zoom_text(v: &View, fpt: Option<u128>) -> String {
    if v.cols == 1 {
        format!("{}/col", time::format(v.ticks, fpt))
    } else {
        format!("{}/{}col", time::format(1, fpt), v.cols)
    }
}

/// The status line in segments (spec §4): the mode, the keys pending, the panel's button, then
/// dim in the middle the mark under the cursor (its name and note), the stream's record there, or
/// the selected row's path; at the right the agent followed, the cursor's time, the zoom, the
/// agents' time unit and the trace. Segments that don't fit go, the least needed first.
fn status(app: &App, buf: &mut Buffer, w: usize, y: usize) -> Rect {
    use crate::draw::icon;
    let fpt = app.ws.trace.fs_per_tick;
    let mono = pal().mono;
    let seg = |fg: Color, bg: Color| if mono { Style::new() } else { Style::new().fg(fg).bg(bg) };
    let (panel, sel) = (pal().panel, pal().selected);
    put(buf, 0, y, &" ".repeat(w), Style::new());
    let mode = if app.picker.is_some() { "PICK" } else if app.panel_shown() { "SIGNALS" } else { "NORMAL" };
    let badge = if mono { Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD) } else { Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(Modifier::BOLD) };
    let mut x = 0;
    // as wide as the longest, so the button doesn't move when the mode changes
    x += put_seg(buf, x, y, &format!("{mode:<7}"), badge);
    let pending = app.pending();
    if !pending.is_empty() {
        x += put_seg(buf, x, y, &pending, seg(pal().cursor, sel).add_modifier(Modifier::BOLD));
    }
    // the panel's button
    let button = format!("signals {}", if app.adding { gl().left } else { gl().right });
    let bx = x + if mono { 1 } else { 0 };
    x += put_seg(buf, bx, y, &button, seg(Color::Reset, panel).add_modifier(Modifier::BOLD)) + (bx - x);
    let at = Rect::new(bx as u16, y as u16, (button.chars().count() + 2) as u16, 1);
    // the right: (text, style, how much it is needed)
    let mut right: Vec<(String, Style, u8)> = Vec::new();
    if let Some(f) = app.follow.as_ref() {
        right.push((format!("{}following {}", icon_text(icon::EYE), f.id), seg(pal().hues[4], sel), 3));
    }
    right.push((format!("{}{}", icon_text(icon::CLOCK), time::format(app.ws.view.cursor, fpt)), seg(pal().cursor, panel).add_modifier(Modifier::BOLD), 5));
    right.push((zoom_text(&app.ws.view, fpt), seg(Color::Reset, panel), 1));
    if let Some(u) = app.ws.unit {
        right.push((format!("unit {}", time::unit_name(u)), seg(pal().dim, panel), 0));
    }
    right.push((format!("{}{}", icon_text(icon::FILE), app.ws.file_name), seg(Color::Reset, sel), 2));
    let modified = app.ws.modified();
    let width = |r: &[(String, Style, u8)]| r.iter().map(|(t, ..)| t.chars().count() + 2 + mono as usize).sum::<usize>() + if modified { 4 } else { 0 } + 1;
    while width(&right) + x + 12 > w {
        let Some(i) = (0..right.len()).min_by_key(|&i| right[i].2).filter(|_| right.len() > 1) else { break };
        right.remove(i);
    }
    // the middle: the mark under the cursor, else the record or path of the selected row
    let dim = Style::new().fg(pal().dim);
    let msg = app.msg.as_ref().filter(|_| app.hit.unshown);
    let mid = match app.ws.label_at_cursor() {
        // a message no card could show
        _ if msg.is_some() => msg.unwrap().0.clone(),
        Some((i, l)) => {
            let text = [l.name.as_str(), l.note.as_str()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(": ");
            format!("{}{} {text}", icon_text(icon::MARK), i + 1)
        }
        None => match app.ws.rows.get(app.ws.sel) {
            // a stream's record at the cursor, as much of it as fits (`e` shows all of it)
            Some(Row::Signal(r)) if app.ws.record_at(r.var).is_some() => {
                let (s, i) = app.ws.record_at(r.var).unwrap();
                let rec = &s.records[i];
                let fields: Vec<String> = s.fields_text(i, fpt).into_iter().map(|(n, v)| format!("{n}={v}")).collect();
                let times = format!("{}{}{}", time::format(rec.start, fpt), gl().dash, time::format(rec.end, fpt));
                format!("{} #{i}  {times}  {}", s.name, fields.join(" "))
            }
            Some(Row::Signal(r)) => {
                // a bit as `data[3]`, a stream's field by its own path
                let (var, bit) = app.ws.probe(r, app.ws.bit);
                let bit = bit.map_or(String::new(), |b| format!("[{b}]"));
                format!("{}{bit}", app.ws.trace.vars[var].path)
            }
            Some(Row::Group { name, .. }) => name.clone(),
            None => String::new(),
        },
    };
    let room = w.saturating_sub(x + 1 + width(&right) + 1);
    let len = mid.chars().count();
    let mid = if len > room { mid.chars().take(room.saturating_sub(1)).chain([gl().cut]).collect() } else { mid };
    put(buf, x + 1, y, &mid, if msg.is_some_and(|m| m.1) { crate::draw::red() } else if msg.is_some() { Style::new() } else { dim });
    let mut x = w.saturating_sub(width(&right));
    let n = right.len();
    for (i, (t, st, _)) in right.into_iter().enumerate() {
        let file = i == n - 1;
        let shown = t.chars().count() + 2 + mono as usize;
        put_seg(buf, x, y, &t, st);
        if file && modified {
            put(buf, x + shown - 1, y, " [+] ", st.fg(pal().cursor));
        }
        x += shown;
    }
    at
}

/// A segment of the status line: its text with a space each side; returns its width.
fn put_seg(buf: &mut Buffer, x: usize, y: usize, text: &str, st: Style) -> usize {
    put(buf, x, y, &format!(" {text} "), st);
    text.chars().count() + 2
}

fn command_line(app: &mut App, buf: &mut Buffer, w: usize, h: usize) -> Option<(u16, u16)> {
    let y = h - 1;
    let search = app.mode == Mode::Search;
    // the mode's badge, as on the status line (spec §4), where it leaves room for the line
    let (name, colour) = if search { ("SEARCH", pal().hues[1]) } else { ("CMD", pal().hues[5]) };
    let badge = if pal().mono { Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD) } else { Style::new().bg(colour).fg(pal().cursor_text).add_modifier(Modifier::BOLD) };
    let bw = if w >= 40 { name.len() + 3 } else { 0 };
    put(buf, 0, y, &" ".repeat(w), Style::new());
    if bw > 0 {
        put_seg(buf, 0, y, name, badge);
    }
    let line = format!("{}{}", if search { '/' } else { ':' }, app.line);
    put(buf, bw, y, &line, Style::new());
    let comp = (!search).then(|| crate::tui::commands::complete(&app.line, app));
    let items = comp.as_ref().map_or(&[][..], |c| &c.items[..]);
    // what the highlighted item adds, dim after the line, when the cursor is at its end
    if let Some(c) = &comp
        && app.cursor() == app.line.chars().count()
    {
        let x = bw + line.chars().count();
        put(buf, x, y, &c.ghost(&app.line, app.hl).chars().take(w.saturating_sub(x)).collect::<String>(), Style::new().fg(pal().dim));
    }
    // up to ten, scrolled to the highlight; the labels in a column up to 40% of the width (a
    // longer one is cut: the description says it again), the descriptions dim after them
    let shown = items.len().min(10).min(y);
    let first = app.hl.saturating_sub(shown.saturating_sub(1));
    // the highlighted item's docs in a card at the right, where it leaves the list 40 columns
    let label_w = |lw: usize| items.iter().map(|c| c.label.chars().count()).max().unwrap_or(0).min(lw * 2 / 5).max(4);
    let doc = match items.get(app.hl) {
        Some(it) if !it.doc.is_empty() => it.doc.as_str(),
        Some(it) if 3 + label_w(w) + 3 + it.desc.chars().count() > w => it.desc.as_str(),
        _ => "",
    };
    let dw = if doc.is_empty() || w < 90 { 0 } else { (w * 2 / 5).clamp(44, 76) };
    let lw = w - dw;
    let uw = label_w(lw);
    let cut = |s: &str, n: usize| if s.chars().count() > n { s.chars().take(n.saturating_sub(1)).chain([gl().cut]).collect() } else { s.to_string() };
    for (k, c) in items.iter().enumerate().skip(first).take(shown) {
        let ly = y - 1 - (k - first);
        app.hit.completions.push((ly, k));
        // opaque: the cursor column and the selection must not show through
        let st = Style::new().bg(if k == app.hl { pal().selected } else { pal().panel }).fg(Color::Reset);
        let mark = if k == app.hl { format!(" {} ", gl().hl) } else { "   ".to_string() };
        put(buf, 0, ly, &format!("{mark:<lw$}"), st);
        put(buf, 3, ly, &cut(&c.label, uw), if k == app.hl { st.add_modifier(Modifier::BOLD) } else { st });
        put(buf, 3 + uw + 2, ly, &cut(&c.desc, lw.saturating_sub(3 + uw + 3)), st.fg(pal().dim));
    }
    if dw > 0 && shown > 0 {
        let it = &items[app.hl];
        let tw = dw - 4;
        // each line of the docs wrapped on its own, keeping its indent
        let mut lines: Vec<String> = Vec::new();
        for l in doc.lines() {
            let indent = l.len() - l.trim_start().len();
            let wrapped = wrap_text(l, tw.saturating_sub(indent).max(8), usize::MAX);
            lines.extend(if wrapped.is_empty() { vec![String::new()] } else { wrapped.into_iter().map(|t| format!("{}{t}", " ".repeat(indent))).collect() });
        }
        let bh = (lines.len() + 2).min(y);
        if bh >= 3 {
            let r = Rect::new(lw as u16, (y - bh) as u16, dw as u16, bh as u16);
            let what = comp.as_ref().map_or("", |c| c.what);
            card_frame(buf, r, pal().dim, &[(it.label.clone(), Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD))], what, "Tab takes it");
            let st = card_style();
            for (i, l) in lines.iter().take(bh - 2).enumerate() {
                let found = l.starts_with("This trace:");
                put(buf, lw + 2, y - bh + 1 + i, &cut(l, tw), if found { st.fg(pal().hues[3]) } else { st });
            }
        }
    }
    // without a background, a rule between the list and the waveforms above it
    if let Some(ry) = y.checked_sub(shown + 1).filter(|_| shown > 0 && pal().mono) {
        put(buf, 0, ry, &gl().border[4].to_string().repeat(w), Style::new().fg(pal().dim).bg(Color::Reset));
    }
    Some(((bw + 1 + app.cursor()).min(w.saturating_sub(1)) as u16, y as u16))
}
