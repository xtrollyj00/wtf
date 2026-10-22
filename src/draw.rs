//! Drawing a workspace into a ratatui Buffer (spec §4, §14): the time axis, the cursor ruler and
//! the rows with their waveforms. The terminal UI draws its screen around it; the MCP server and
//! `--snapshot` turn it into text.

use wtf_core::rows::{self, Row, Sig};
use wtf_core::stream::Stream;
use wtf_core::time::{self, Tick};
use wtf_core::trace::{Changes, Trace, Val};
use wtf_core::view::View;
use wtf_core::workspace::Workspace;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub const MAX_NAME: usize = 30;
pub const MAX_VALUE: usize = 20;

pub struct Palette {
    /// 1-bit lines and edges.
    pub line: Color,
    /// Fill for a 1-bit signal toggling faster than a column.
    pub fill: Color,
    pub unknown: Color,
    pub highz: Color,
    /// The cursor is a highlighted column, so the data under it stays readable.
    pub cursor: Color,
    pub cursor_text: Color,
    /// The other cursors' columns.
    pub cursor_dim: Color,
    /// Background behind both lines of the selected signal.
    pub selected: Color,
    pub hatch: Color,
    /// Scope prefixes, column separators and time axis labels.
    pub dim: Color,
    /// Grid lines under the time axis labels.
    pub grid: Color,
    /// The shades of the signal colours, in the order of `rows::COLORS`.
    pub hues: [Color; 8],
    /// Without colour (`NO_COLOR`): the cursor column and the selected name are reversed, and
    /// marks are drawn with their glyph (spec §14 "Colours").
    pub mono: bool,
}

/// Truecolor when the terminal says it supports it, otherwise the nearest 256-colour shades.
/// COLORTERM is often lost over ssh, so known truecolor terminals are also recognised by TERM.
pub fn truecolor() -> bool {
    let env = |k| std::env::var(k).unwrap_or_default();
    let term = env("TERM");
    matches!(env("COLORTERM").as_str(), "truecolor" | "24bit")
        || term.ends_with("-direct")
        || ["ghostty", "kitty", "wezterm", "alacritty", "foot"].iter().any(|t| term.contains(t))
}

/// `#rrggbb` as it is, or its 256-colour shade where the terminal has no truecolor.
fn shade(rgb: bool) -> impl Fn(u32, u8) -> Color {
    move |hex, i| if rgb { Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8) } else { Color::Indexed(i) }
}

impl Palette {
    pub fn dark(rgb: bool) -> Palette {
        let c = shade(rgb);
        Palette {
            line: c(0x56b6c2, 73),
            fill: c(0x2e6b73, 30),
            unknown: c(0xe06c75, 168),
            highz: c(0x98c379, 114),
            cursor: c(0xe5c07b, 180),
            cursor_text: c(0x1e1e1e, 234),
            cursor_dim: c(0x4b422c, 58),
            selected: c(0x2c313c, 236),
            hatch: c(0x7f848e, 245),
            dim: c(0x5c6370, 241),
            grid: c(0x2c313c, 236),
            hues: [c(0xff6b6b, 203), c(0xd19a66, 173), c(0xe5c07b, 180), c(0x98c379, 114), c(0x56b6c2, 73), c(0x61afef, 75), c(0xc678dd, 176), c(0xabb2bf, 249)],
            mono: false,
        }
    }

    /// For terminals with a light background: darker lines, pale highlights.
    pub fn light(rgb: bool) -> Palette {
        let c = shade(rgb);
        Palette {
            line: c(0x0e7490, 30),
            fill: c(0x9fd3da, 152),
            unknown: c(0xc62828, 160),
            highz: c(0x2e7d32, 28),
            cursor: c(0xf0c674, 221),
            cursor_text: c(0x1e1e1e, 234),
            cursor_dim: c(0xf3e6c4, 230),
            selected: c(0xe4e8ee, 254),
            hatch: c(0x8a8f98, 245),
            dim: c(0x7a808a, 244),
            grid: c(0xe4e8ee, 254),
            hues: [c(0xc62828, 160), c(0xc2581c, 166), c(0xa67c00, 136), c(0x2e7d32, 28), c(0x0e7490, 30), c(0x1d4ed8, 26), c(0xa21caf, 127), c(0x6b7280, 243)],
            mono: false,
        }
    }

    /// `NO_COLOR`: the terminal's own colours everywhere.
    pub fn mono() -> Palette {
        let r = Color::Reset;
        Palette { line: r, fill: r, unknown: r, highz: r, cursor: r, cursor_text: r, cursor_dim: r, selected: r, hatch: r, dim: r, grid: r, hues: [r; 8], mono: true }
    }

    /// Reversed where there is no colour: the cursor column, the selected name.
    pub fn accent(&self) -> Modifier {
        if self.mono { Modifier::REVERSED } else { Modifier::empty() }
    }
}

static PALETTE: std::sync::OnceLock<Palette> = std::sync::OnceLock::new();

/// Set once at start, before anything is drawn; later calls change nothing. Snapshots and the
/// agents' `render` keep the default, as `text` tells the cursor columns by their colours.
pub fn set_palette(p: Palette) {
    let _ = PALETTE.set(p);
}

pub fn pal() -> &'static Palette {
    PALETTE.get_or_init(|| Palette::dark(truecolor()))
}

/// The glyphs drawing uses (spec §14 "Glyphs"); one set per process, chosen at start.
pub struct Glyphs {
    /// 1-bit high / low lines and bus outlines.
    pub line: char,
    /// A 1-bit edge on the upper / lower line, by the lines before and after it:
    /// [rise, fall, both (pulse), neither].
    pub edge_up: [char; 4],
    pub edge_down: [char; 4],
    /// Upper and lower halves of a bus crossing, a segment's opening and its closing.
    pub crossing: [char; 2],
    pub open: [char; 2],
    pub close: [char; 2],
    /// Busy 1-bit columns and a collapsed group's activity, upper and lower.
    pub fill: [char; 2],
    pub busy_bus: char,
    pub cut: char,
    pub grid: char,
    pub sep: char,
    pub cursor: char,
    pub mark: char,
    /// A cursor off screen to the left / right.
    pub left: char,
    pub right: char,
    pub group_open: char,
    pub group_closed: char,
    pub shown: char,
    /// The highlighted completion, and the insertion point in Edit mode.
    pub hl: char,
    pub insert: char,
    /// Borders: top-left, top-right, bottom-left, bottom-right, horizontal, vertical.
    pub border: [char; 6],
    /// Separators in text: `help · keys`, `65ns–85ns`.
    pub dot: char,
    pub dash: char,
}

pub const UNICODE: Glyphs = Glyphs {
    line: '─',
    edge_up: ['┌', '┐', '┬', '╷'],
    edge_down: ['└', '┘', '┴', '╵'],
    crossing: ['🮦', '🮧'],
    open: ['🮣', '🮡'],
    close: ['🮢', '🮠'],
    fill: ['▄', '▀'],
    busy_bus: '╳',
    cut: '…',
    grid: '┊',
    sep: '│',
    cursor: '┃',
    mark: '╎',
    left: '◂',
    right: '▸',
    group_open: '▾',
    group_closed: '▸',
    shown: '●',
    hl: '▌',
    insert: '▸',
    border: ['┌', '┐', '└', '┘', '─', '│'],
    dot: '·',
    dash: '–',
};

/// Box drawing only: the legacy computing glyphs' pieces become corners.
pub const BOX: Glyphs = Glyphs { crossing: ['┬', '┴'], open: ['┌', '└'], close: ['┐', '┘'], ..UNICODE };

/// 7-bit only. An edge and a crossing are both `+`; a 1-bit signal has one line beside it, a bus two.
pub const ASCII: Glyphs = Glyphs {
    line: '-',
    edge_up: ['+'; 4],
    edge_down: ['+'; 4],
    crossing: ['+'; 2],
    open: ['+'; 2],
    close: ['+'; 2],
    fill: ['#'; 2],
    busy_bus: '#',
    cut: '~',
    grid: '.',
    sep: '|',
    cursor: '|',
    mark: ':',
    left: '<',
    right: '>',
    group_open: 'v',
    group_closed: '>',
    shown: '*',
    hl: '>',
    insert: '>',
    border: ['+', '+', '+', '+', '-', '|'],
    dot: '-',
    dash: '-',
};

pub const SETS: [(&str, &Glyphs); 3] = [("unicode", &UNICODE), ("box", &BOX), ("ascii", &ASCII)];

pub fn glyphs_named(name: &str) -> Result<&'static Glyphs, String> {
    SETS.iter().find(|(n, _)| *n == name).map(|(_, g)| *g).ok_or_else(|| format!("unknown glyphs: {name} (unicode, box, ascii)"))
}

thread_local! {
    // per thread, so tests can draw in several sets at once; the window draws on one thread
    static GLYPHS: std::cell::Cell<&'static Glyphs> = const { std::cell::Cell::new(&UNICODE) };
}

pub fn set_glyphs(g: &'static Glyphs) {
    GLYPHS.with(|c| c.set(g));
}

pub fn gl() -> &'static Glyphs {
    GLYPHS.with(|c| c.get())
}

pub fn red() -> Style {
    Style::new().fg(pal().unknown)
}
pub fn green() -> Style {
    Style::new().fg(pal().highz)
}
pub fn line() -> Style {
    Style::new().fg(pal().line)
}

pub fn put(buf: &mut Buffer, x: usize, y: usize, s: &str, style: Style) {
    buf.set_string(x as u16, y as u16, s, style);
}

/// Lines a workspace's drawing takes: the time axis, the cursor ruler if there is one, and the
/// rows.
pub fn height(ws: &Workspace) -> u16 {
    (rows::lines(&ws.rows).len() * ROW_LINES) as u16 + 1 + (ws.cursor_list().len() > 1) as u16
}

/// A drawing as plain text, one character per cell. Colours are dropped, so cursor columns show
/// as ┃ (the cursor) and ╎ (marks) except over label text.
pub fn text(buf: &Buffer) -> String {
    let (cursor, mark) = (gl().cursor.to_string(), gl().mark.to_string());
    let mut out = String::new();
    for y in 0..buf.area.height {
        let line: String = (0..buf.area.width)
            .map(|x| {
                let cell = &buf[(x, y)];
                match cell.symbol() {
                    s if cell.bg == pal().cursor && !s.chars().all(char::is_alphanumeric) => cursor.as_str(),
                    s if cell.bg == pal().cursor_dim && !s.chars().all(char::is_alphanumeric) => mark.as_str(),
                    s => s,
                }
            })
            .collect();
        out += line.trim_end();
        out.push('\n');
    }
    out
}

/// The workspace's rows as text, `w` characters wide, all of them.
pub fn waveform(ws: &mut Workspace, w: u16) -> String {
    let h = height(ws);
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    rows(ws, &mut 0, false, &mut buf, w as usize, h as usize);
    text(&buf)
}

/// Unknown bits (x, u, w, -) beat high impedance (z), as in the radix digits (spec §10).
/// Decided from the bits, never from the label: hex labels start with `0x`.
pub fn state_style(v: &Val) -> Style {
    match v {
        Val::Bits(b) if b.contains(['x', 'u', 'w', '-']) => red(),
        Val::Bits(b) if b.contains('z') => green(),
        _ => Style::new(),
    }
}

/// Terminal lines per signal: two, so 1-bit edges and bus outlines connect.
pub const ROW_LINES: usize = 2;

/// The rows (names, values at the cursor, waveforms) under the time axis and the cursor ruler,
/// scrolled so the selection shows: `top` is the first row on screen, kept up to date. With
/// `adding`, a line marks where new signals go and the selection isn't accented (the keys are
/// elsewhere).
pub fn rows(ws: &mut Workspace, top: &mut usize, adding: bool, buf: &mut Buffer, w: usize, body: usize) {
    // line 0 is the time axis
    // while adding, one more line marks where new signals go
    // line 1 is the cursor ruler when there is more than one cursor
    let cursors = ws.cursor_list();
    let head = 1 + (cursors.len() > 1) as usize;
    let fit = (body.saturating_sub(head + adding as usize) / ROW_LINES).max(1);
    let lines = rows::lines(&ws.rows);
    let p = lines.iter().position(|&l| l == (ws.sel, ws.bit)).unwrap_or(0);
    if p < *top {
        *top = p;
    } else if p >= *top + fit {
        *top = p + 1 - fit;
    }
    let mut row_names = rows::display_names(&ws.trace, &ws.rows);
    // the core marks group headers `▾` / `▸`; drawn in the glyph set's
    for (n, r) in row_names.iter_mut().zip(&ws.rows) {
        if let Row::Group { collapsed, .. } = r {
            let g = if *collapsed { gl().group_closed } else { gl().group_open };
            *n = format!("{g}{}", n.chars().skip(1).collect::<String>());
        }
    }
    // a bit line is named after its bus, indented under it: `data[3]`; a field after its stream:
    // `frame.len`
    let names: Vec<String> = lines
        .iter()
        .map(|&(i, bit)| match (bit, ws.rows[i].sig()) {
            (Some(b), Some(s)) => match ws.probe(s.var, bit) {
                (v, None) => format!("  {}{}", row_names[i], &ws.trace.vars[v].path[ws.trace.vars[s.var].path.len()..]),
                _ => format!("  {}[{b}]", row_names[i]),
            },
            _ => row_names[i].clone(),
        })
        .collect();
    let (values, value_styles): (Vec<String>, Vec<Style>) = lines
        .iter()
        .map(|&(i, bit)| {
            ws.rows[i]
                .sig()
                .and_then(|s| {
                    let (var, bit) = ws.probe(s.var, bit);
                    let v = ws.trace.value_of(var, bit, ws.view.cursor)?;
                    let stream = ws.stream_of(var);
                    let st = if stream.is_some_and(|s| s.violations) { red() } else { state_style(&v) };
                    Some((label(&ws.trace, stream, s, &v), st))
                })
                .unwrap_or_default()
        })
        .unzip();
    let width = |v: &[String], max| v.iter().map(|s| s.chars().count()).max().unwrap_or(0).min(max);
    // hidden rows count too, so collapsing a group doesn't shift the waveforms
    let hidden: Vec<String> = ws
        .rows
        .iter()
        .filter_map(|r| r.sig())
        .filter_map(|s| ws.trace.value_at(s.var, ws.view.cursor).map(|v| label(&ws.trace, ws.stream_of(s.var), s, &v)))
        .collect();
    let nw = width(&names, MAX_NAME).max(width(&row_names, MAX_NAME));
    let vw = width(&values, MAX_VALUE).max(width(&hidden, MAX_VALUE));
    // name │ value │wave
    let (sep1, sep2) = (nw + 1, nw + 3 + vw + 1);
    let wx = sep2 + 1;
    ws.view.set_width(w.saturating_sub(wx) as u64);
    let ww = w.saturating_sub(wx);

    // bit lines of an expanded bus share one pass over the bus (row, [bit][column])
    let mut bit_changes: Option<(usize, Vec<Vec<Changes>>)> = None;
    let mut marker = None;
    let mut y = head;
    for (l, &(i, bit)) in lines.iter().enumerate().skip(*top).take(fit) {
        // name and value sit on the bottom line, level with a low signal
        let yb = (y + 1).min(body - 1);
        let name: String = names[l].chars().take(nw).collect();
        let selected = (i, bit) == (ws.sel, ws.bit);
        if ws.anchor.is_some() && ws.selection().contains(&i) {
            let lines = ROW_LINES.min(body - y) as u16;
            buf.set_style(Rect::new(0, y as u16, w as u16, lines), Style::new().bg(pal().selected));
        }
        let sel = if selected {
            let lines = ROW_LINES.min(body - y) as u16;
            buf.set_style(Rect::new(0, y as u16, w as u16, lines), Style::new().bg(pal().selected));
            // the accent shows where the keys go: not here while the panel has them
            match adding {
                true => Style::new().add_modifier(Modifier::BOLD),
                false => Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD | pal().accent()),
            }
        } else {
            Style::new()
        };
        if selected {
            put(buf, 0, yb, &format!("{name:<nw$}"), sel);
        } else if ws.rows[i].is_group() {
            put(buf, 0, yb, &name, Style::new().add_modifier(Modifier::BOLD));
        } else {
            // the scope is greyed so the leaf names stand out
            let leaf = name.rfind('.').map_or(0, |d| d + 1);
            put(buf, 0, yb, &name[..leaf], Style::new().fg(pal().dim));
            put(buf, leaf, yb, &name[leaf..], Style::new());
        }
        let value: String = values[l].chars().take(vw).collect();
        put(buf, nw + 3, yb, &value, value_styles[l]);
        let cells = match &ws.rows[i] {
            Row::Signal(s) if ws.probe(s.var, bit).0 != s.var => {
                let field = Sig { var: ws.probe(s.var, bit).0, ..s.clone() };
                wave(&ws.trace, &ws.view, &field, None, None, ws.stream_of(field.var), ww)
            }
            Row::Signal(s) => {
                let bits = bit.map(|b| {
                    if bit_changes.as_ref().is_none_or(|(r, _)| *r != i) {
                        let v = &ws.view;
                        let ranges: Vec<_> = (0..ww as u64).map(|c| (v.col_start(c), v.col_start(c + 1))).collect();
                        bit_changes = Some((i, ws.trace.bit_changes(s.var, &ranges)));
                    }
                    bit_changes.as_ref().map_or(&[][..], |(_, c)| &c[b as usize][..])
                });
                wave(&ws.trace, &ws.view, s, bit, bits, ws.stream_of(s.var), ww)
            }
            Row::Group { collapsed: true, .. } => activity(&ws.trace, &ws.view, &rows::members(&ws.rows, i), ww),
            Row::Group { .. } => Vec::new(),
        };
        for (c, cells) in cells.into_iter().enumerate() {
            for (dy, (ch, st)) in cells.into_iter().enumerate().filter(|(dy, _)| y + dy < body) {
                put(buf, wx + c, y + dy, ch.encode_utf8(&mut [0; 4]), st);
            }
        }
        y += ROW_LINES;
        // under the selected row's last line (after its bits)
        let last = lines.get(l + 1).is_none_or(|n| n.0 != i);
        if adding && i == ws.sel && last && y < body {
            let indent = if rows::header_of(&ws.rows, i).is_some() { "  " } else { "" };
            let text = format!("{indent}{} new signals go here ", gl().insert);
            let fill = w.saturating_sub(text.chars().count());
            put(buf, 0, y, &format!("{text}{}", gl().line.to_string().repeat(fill)), Style::new().fg(pal().cursor));
            marker = Some(y);
            y += 1;
        }
    }
    let dim = Style::new().fg(pal().dim);
    for y in (0..body).filter(|y| Some(*y) != marker) {
        put(buf, sep1, y, &gl().sep.to_string(), dim);
        put(buf, sep2, y, &gl().sep.to_string(), dim);
    }
    let mut free = 0;
    for (c, t) in axis(&ws.view, ws.trace.fs_per_tick) {
        let x = wx + c;
        // labels that would run into the previous one or off the edge are left out; their grid line stays
        if c >= free && c + t.chars().count() <= ww {
            put(buf, x, 0, &t, dim);
            free = c + t.chars().count() + 2;
        }
        for y in 1..body {
            // only cells nothing was drawn in: label gaps are styled spaces
            let cell = &buf[(x as u16, y as u16)];
            if cell.symbol() == " " && cell.fg == Color::Reset {
                put(buf, x, y, &gl().grid.to_string(), Style::new().fg(pal().grid));
            }
        }
    }
    if head > 1 {
        ruler(ws, buf, wx, ww, 1);
    }
    // the marks first, so the cursor is on top where they share a column
    for (i, t) in cursors.into_iter().filter(|&(i, _)| i != 0).chain([(0, ws.view.cursor)]) {
        if let Some(c) = ws.view.col_of(t) {
            let area = Rect::new((wx + c as usize) as u16, 0, 1, body as u16);
            let st = match i == 0 {
                true => Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(pal().accent()),
                false => Style::new().bg(pal().cursor_dim),
            };
            buf.set_style(area, st);
            if pal().mono && i != 0 {
                for y in area.top()..area.bottom() {
                    if buf[(area.x, y)].symbol() == " " {
                        buf[(area.x, y)].set_char(gl().mark);
                    }
                }
            }
        }
    }
}

/// The cursor ruler (spec §9): the cursor's and each mark's number at its column, the cursor highlighted, and
/// the Δt between neighbouring cursors in time. Cursors outside the view sit at the edge (`◂1`, `3▸`).
fn ruler(ws: &Workspace, buf: &mut Buffer, wx: usize, ww: usize, y: usize) {
    let v = &ws.view;
    let dim = Style::new().fg(pal().dim);
    let mut list = ws.cursor_list();
    list.sort_by_key(|&(i, t)| (t, i));
    let col = |t: Tick| match v.col_of(t) {
        Some(c) => c as usize,
        None if t < v.start => 0,
        None => ww.saturating_sub(1),
    };
    for pair in list.windows(2) {
        let (a, b) = (col(pair[0].1), col(pair[1].1));
        let span = b.saturating_sub(a + 1);
        put(buf, wx + a + 1, y, &gl().line.to_string().repeat(span), dim);
        // like bus labels: only if it fits with a spare column on each side
        let label = format!(" {} ", time::format(pair[1].1 - pair[0].1, ws.trace.fs_per_tick));
        let len = label.chars().count();
        if len + 2 <= span {
            put(buf, wx + a + 1 + (span - len) / 2, y, &label, dim);
        }
    }
    list.sort_by_key(|&(i, _)| i == 0);
    for (i, t) in list {
        let n = i + 1;
        let st = match i == 0 {
            true => Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD),
            false => Style::new(),
        };
        match v.col_of(t) {
            Some(c) => put(buf, wx + c as usize, y, &n.to_string(), st),
            None if t < v.start => put(buf, wx, y, &format!("{}{n}", gl().left), st),
            None => put(buf, wx + ww.saturating_sub(2), y, &format!("{n}{}", gl().right), st),
        }
    }
}

/// Time axis ticks as (column, label), at a round step wide enough for the labels.
fn axis(v: &View, fpt: Option<u128>) -> Vec<(usize, String)> {
    let spacing = time::format(v.last_visible(), fpt).chars().count() as u64 + 4;
    let step = nice((spacing * v.ticks).div_ceil(v.cols));
    let first = v.start.div_ceil(step) * step;
    (first..=v.last_visible().min(v.end))
        .step_by(step as usize)
        .filter_map(|t| Some((v.col_of(t)? as usize, time::format(t, fpt))))
        .collect()
}

/// Smallest 1, 2 or 5 times a power of ten that is at least `min`.
fn nice(min: u64) -> u64 {
    let mut p = 1;
    loop {
        if let Some(n) = [p, 2 * p, 5 * p].into_iter().find(|n| *n >= min) {
            return n;
        }
        p *= 10;
    }
}

/// The index into `edge_up` / `edge_down` for an edge from `before` to `after` (high = true).
fn edge(before: bool, after: bool) -> usize {
    match (before, after) {
        (false, true) => 0,
        (true, false) => 1,
        (true, true) => 2,
        _ => 3,
    }
}

/// Which of a signal's two lines a value draws: low, high, or both (buses, X, Z).
fn lines_of(v: &Option<Val>, own: Style) -> (bool, bool, Style) {
    let Some(v) = v else { return (false, false, Style::new()) };
    let st = match state_style(v) {
        s if s == Style::new() => own,
        s => s,
    };
    match v {
        Val::Bits(b) if b.len() == 1 => match b.as_str() {
            "1" | "h" => (true, false, st),
            "0" | "l" => (false, true, st),
            _ => (true, true, st),
        },
        _ => (true, true, st),
    }
}

/// One signal's waveform as (top, bottom) cells per column. Each column costs two binary
/// searches, so drawing depends on the screen size, not on the number of transitions (spec §18).
/// For a bit of a bus, `bits` holds its changes per column (`Trace::bit_changes`). A stream's own
/// var draws its records' labels, red for violations, and nothing between records.
pub fn wave(trace: &Trace, view: &View, row: &Sig, bit: Option<u32>, bits: Option<&[Changes]>, stream: Option<&Stream>, ww: usize) -> Vec<[(char, Style); 2]> {
    let var = row.var;
    let end = trace.end();
    let bus = bit.is_none() && trace.vars[var].width != Some(1);
    let range = |c: usize| (view.col_start(c as u64), view.col_start(c as u64 + 1));
    // columns starting after the end of the simulation stay empty
    // (an empty range continues the previous tick below one tick per column)
    let live = |c: usize| {
        let (a, b) = range(c);
        a <= end || (a == b && a - 1 <= end)
    };
    let n: Vec<Changes> = match bits {
        Some(b) => b.to_vec(),
        None => (0..ww).map(&range).map(|(a, b)| trace.changes_in(var, a, b)).collect(),
    };
    let val = |c: usize| trace.value_of(var, bit, range(c).1 - 1);
    let blank = (' ', Style::new());
    let mut out = vec![[blank; 2]; ww];
    // the row's own colour, or the theme's line
    let own = row.color.map_or_else(line, |c| Style::new().fg(pal().hues[c]));

    // Busy: two or more changes in a column, or a run of segments narrower than two columns.
    // A run of k changed columns holds k-1 one-column segments; one alone is a short pulse.
    let mut busy: Vec<bool> = n.iter().map(|x| *x == Changes::Many).collect();
    let mut c = 0;
    while c < ww {
        let run = n[c..].iter().take_while(|x| **x != Changes::None).count();
        if run >= 3 {
            busy[c..c + run - 1].iter_mut().for_each(|b| *b = true);
        }
        c += run.max(1);
    }

    if bus {
        // One hexagon per value. A change column holds the crossing (🮦 over 🮧), whose ends sit
        // mid-cell like the ─ edges; a value with nothing drawn before it opens with 🮣 over 🮡.
        let mut c = 0;
        while c < ww && live(c) {
            if busy[c] {
                // too many values to show: crossings
                out[c] = [(gl().busy_bus, Style::new().fg(pal().hatch)); 2];
                c += 1;
                continue;
            }
            let mut e = c + 1;
            while e < ww && live(e) && !busy[e] && n[e] == Changes::None {
                e += 1;
            }
            let opens = n[c] == Changes::One;
            if let Some(v) = val(c).filter(|v| *v != Val::Text(String::new())) {
                let text = label(trace, stream, row, &v);
                let st = match stream.is_some_and(|s| s.violations) {
                    true => red(),
                    false => Some(state_style(&v)).filter(|s| *s != Style::new()).unwrap_or(own),
                };
                out[c..e].iter_mut().for_each(|o| *o = [(gl().line, st); 2]);
                if opens {
                    let crossing = c > 0 && !busy[c - 1] && out[c - 1][0].0 != ' ';
                    out[c] = if crossing { gl().crossing.map(|g| (g, st)) } else { gl().open.map(|g| (g, st)) };
                }
                // centred between the opening piece and the next crossing, with a gap on each side;
                // cut with … if it doesn't fit, as long as three characters of it do (not just `0x`)
                let a = c + opens as usize;
                let room = (e - a).saturating_sub(2);
                let text: String = match text.chars().count() {
                    n if n <= room => text,
                    _ if room >= 4 => text.chars().take(room - 1).chain([gl().cut]).collect(),
                    _ => String::new(),
                };
                let len = text.chars().count();
                if len > 0 {
                    let start = a + (e - a - len) / 2;
                    for (k, ch) in text.chars().enumerate() {
                        out[start + k][1] = (ch, st);
                    }
                    // a gap in the bottom edge around the label
                    for x in [start - 1, start + len] {
                        if out[x][1].0 == gl().line {
                            out[x][1].0 = ' ';
                        }
                    }
                }
            } else if opens && c > 0 && !busy[c - 1] && out[c - 1][0].0 != ' ' {
                // a value ending in nothing (a stream between records): a closing piece
                let st = out[c - 1][0].1;
                out[c] = gl().close.map(|g| (g, st));
            }
            c = e;
        }
        return out;
    }

    for c in (0..ww).filter(|&c| live(c)) {
        if busy[c] {
            // toggling faster than a column: fill between the high and low lines
            let st = Style::new().fg(pal().fill);
            out[c] = gl().fill.map(|g| (g, st));
            continue;
        }
        let (top, bot, st) = lines_of(&val(c), own);
        out[c] = if n[c] == Changes::One {
            // an edge joins the lines on its left (before the change) to those on its right
            let before = range(c).0.checked_sub(1).and_then(|t| trace.value_of(var, bit, t));
            let (pt, pb, _) = lines_of(&before, own);
            let t = gl().edge_up[edge(pt, top)];
            let b = gl().edge_down[edge(pb, bot)];
            [(t, own), (b, own)]
        } else {
            [if top { (gl().line, st) } else { blank }, if bot { (gl().line, st) } else { blank }]
        };
    }
    // X and Z draw both lines as a bus does, and get its label: colour isn't always there to
    // tell them apart (NO_COLOR, snapshots, the agents' render)
    let mut c = 0;
    while c < ww && live(c) {
        let mut e = c + 1;
        while e < ww && live(e) && !busy[e] && n[e] == Changes::None {
            e += 1;
        }
        if let Some(v) = val(c).filter(|v| !busy[c] && state_style(v) != Style::new()) {
            let text = rows::label(&trace.vars[var], row, &v);
            let a = c + (n[c] == Changes::One) as usize;
            let len = text.chars().count();
            if len + 2 <= e - a {
                let start = a + (e - a - len) / 2;
                for (k, ch) in text.chars().enumerate() {
                    out[start + k][1].0 = ch;
                }
                for x in [start - 1, start + len] {
                    if out[x][1].0 == gl().line {
                        out[x][1].0 = ' ';
                    }
                }
            }
        }
        c = e;
    }
    out
}

/// A value as a row shows it; a stream's own var (`#3`) as its record's label (`#3 64B`).
fn label(trace: &Trace, stream: Option<&Stream>, row: &Sig, v: &Val) -> String {
    match (stream, v) {
        (Some(s), Val::Text(t)) => match t.strip_prefix('#').and_then(|n| n.parse::<usize>().ok()).filter(|&i| i < s.records.len()) {
            Some(i) => s.label(i, trace.fs_per_tick),
            None => t.clone(),
        },
        _ => rows::label(&trace.vars[row.var], row, v),
    }
}

/// A collapsed group's row: a mark in each column where any of its signals changes (spec §6).
fn activity(trace: &Trace, view: &View, vars: &[usize], ww: usize) -> Vec<[(char, Style); 2]> {
    let end = trace.end();
    let blank = (' ', Style::new());
    (0..ww as u64)
        .map(|c| (view.col_start(c), view.col_start(c + 1)))
        .take_while(|&(a, b)| a <= end || (a == b && a - 1 <= end))
        .map(|(a, b)| match vars.iter().any(|&v| trace.changes_in(v, a, b) != Changes::None) {
            true => gl().fill.map(|g| (g, line())),
            false => [blank, (gl().line, Style::new().fg(pal().dim))],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_come_from_bits() {
        let bits = |b: &str| Val::Bits(b.into());
        assert_eq!(state_style(&bits("01000011")), Style::new()); // 0x43: the `x` of `0x` is not unknown
        assert_eq!(state_style(&bits("0100x011")), red());
        assert_eq!(state_style(&bits("zzzzzzzz")), green());
        assert_eq!(state_style(&bits("zzzzxzzz")), red());
        assert_eq!(state_style(&Val::Text("max".into())), Style::new());
    }

    #[test]
    fn nice_steps() {
        assert_eq!([0, 1, 3, 7, 10, 11, 4999].map(nice), [1, 1, 5, 10, 10, 20, 5000]);
    }
}
