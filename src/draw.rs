//! Drawing a workspace into a ratatui Buffer (spec §4, §14): the time axis, the cursor ruler and
//! the rows with their waveforms. The terminal UI draws its screen around it; the MCP server and
//! `--snapshot` turn it into text.

use wtf_core::rows::{self, Row, Sig};
use wtf_core::stream::Stream;
use wtf_core::time::{self, Tick};
use wtf_core::trace::{Changes, Trace, Val};
use wtf_core::view::View;
use wtf_core::workspace::{Sub, Workspace};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub const MAX_NAME: usize = 30;
pub const MAX_VALUE: usize = 20;

pub struct Palette {
    /// 1-bit lines and edges.
    pub line: Color,
    /// Bus and stream outlines; their labels are in the text's colour.
    pub bus: Color,
    /// Fill for a 1-bit signal toggling faster than a column.
    pub fill: Color,
    pub unknown: Color,
    pub highz: Color,
    /// The accent: the cursor's head on the time axis, the selected name, EDIT.
    pub cursor: Color,
    pub cursor_text: Color,
    /// Behind the cursor's column: a tint, so the waveform under it stays as it is.
    pub column: Color,
    /// The other cursors' columns, fainter.
    pub cursor_dim: Color,
    /// Background behind both lines of the selected signal.
    pub selected: Color,
    /// Background of the boxes over the waveforms: the signal panel, help, completions.
    pub panel: Color,
    pub hatch: Color,
    /// Scope prefixes, column separators and time axis labels.
    pub dim: Color,
    /// The shades of the signal colours, in the order of `rows::COLORS`.
    pub hues: [Color; 8],
    /// The screen's background and text; Reset keeps the terminal's.
    pub background: Color,
    pub text: Color,
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

impl Palette {
    /// `NO_COLOR`: the terminal's own colours everywhere.
    pub fn mono() -> Palette {
        let r = Color::Reset;
        Palette { line: r, bus: r, fill: r, unknown: r, highz: r, cursor: r, cursor_text: r, column: r, cursor_dim: r, selected: r, panel: r, hatch: r, dim: r, hues: [r; 8], background: r, text: r, mono: true }
    }

    /// Reversed where there is no colour: the cursor column, the selected name.
    pub fn accent(&self) -> Modifier {
        if self.mono { Modifier::REVERSED } else { Modifier::empty() }
    }
}

thread_local! {
    // per thread, as the glyphs: the window draws on one thread, tests in several palettes
    static PALETTE: std::cell::Cell<Option<&'static Palette>> = const { std::cell::Cell::new(None) };
}

/// Set at start, before anything is drawn. Snapshots and the agents' `render` keep the default,
/// as `text` tells the cursor columns by their colours.
pub fn set_palette(p: Palette) {
    set_palette_ref(Box::leak(Box::new(p)));
}

/// For a screen that switches palettes as it draws (the first start's samples).
pub fn set_palette_ref(p: &'static Palette) {
    PALETTE.with(|c| c.set(Some(p)));
}

pub fn pal() -> &'static Palette {
    static DARK: std::sync::OnceLock<Palette> = std::sync::OnceLock::new();
    PALETTE.with(|c| c.get()).unwrap_or_else(|| DARK.get_or_init(|| crate::theme::builtin("dark", truecolor()).expect("dark is built in")))
}

/// The theme's background and text wherever the screen was left at the terminal's.
pub fn fill_screen(buf: &mut Buffer) {
    let p = pal();
    if (p.background, p.text) == (Color::Reset, Color::Reset) {
        return;
    }
    for cell in buf.content.iter_mut() {
        if cell.bg == Color::Reset {
            cell.bg = p.background;
        }
        if cell.fg == Color::Reset {
            cell.fg = p.text;
        }
    }
}

/// The glyphs drawing uses (spec §14 "Glyphs"); one set per process, chosen at start.
pub struct Glyphs {
    /// 1-bit high / low lines and bus outlines.
    pub line: char,
    /// The lines of an X (any unknown bit) and of a Z: a shape, not only a colour, tells them
    /// from a value and from each other.
    pub unknown: char,
    pub highz: char,
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
    unknown: '━',
    highz: '┄',
    edge_up: ['┌', '┐', '┬', '╷'],
    edge_down: ['└', '┘', '┴', '╵'],
    crossing: ['🮦', '🮧'],
    open: ['🮣', '🮡'],
    close: ['🮢', '🮠'],
    fill: ['▄', '▀'],
    busy_bus: '╳',
    cut: '…',
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
    unknown: '=',
    highz: '.',
    edge_up: ['+'; 4],
    edge_down: ['+'; 4],
    crossing: ['+'; 2],
    open: ['+'; 2],
    close: ['+'; 2],
    fill: ['#'; 2],
    busy_bus: '#',
    cut: '~',
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

/// A line's signal: a composite's child in the default radix for it (spec §6), a stream's field or
/// a bit in its row's.
pub fn line_sig(ws: &Workspace, s: &Sig, var: usize) -> Sig {
    match var == s.var || ws.stream_of(s.var).is_some() {
        true => Sig { var, ..s.clone() },
        false => Sig { color: s.color, ..Sig::of(var, &ws.trace.vars[var]) },
    }
}

/// Lines a workspace's drawing takes: the time axis, the cursor ruler if there is one, and the
/// rows.
pub fn height(ws: &Workspace) -> u16 {
    (ws.lines().len() * ROW_LINES) as u16 + 1 + (ws.cursor_list().len() > 1) as u16
}

/// A drawing as plain text, one character per cell. Colours are dropped, so cursor columns show
/// as ┃ (the cursor) and ╎ (marks) except over label text.
/// The screen as `text` prints it, but in its colours: an SGR code wherever the style changes,
/// one reset at each line's end (`--snapshot --ansi`).
pub fn ansi(buf: &Buffer) -> String {
    use crossterm::style::Colored;
    use ratatui::backend::IntoCrossterm;
    let sgr = |c: &ratatui::buffer::Cell| {
        let mut s = String::from("\x1b[0");
        for (m, code) in [(Modifier::BOLD, ";1"), (Modifier::DIM, ";2"), (Modifier::ITALIC, ";3"), (Modifier::UNDERLINED, ";4"), (Modifier::REVERSED, ";7")] {
            if c.modifier.contains(m) {
                s += code;
            }
        }
        if c.fg != Color::Reset {
            s += &format!(";{}", Colored::ForegroundColor(c.fg.into_crossterm()));
        }
        if c.bg != Color::Reset {
            s += &format!(";{}", Colored::BackgroundColor(c.bg.into_crossterm()));
        }
        s + "m"
    };
    let mut out = String::new();
    for y in 0..buf.area.height {
        let row: Vec<_> = (0..buf.area.width).map(|x| &buf[(x, y)]).collect();
        // trailing blanks in the terminal's colours are trimmed, as `text` does
        let end = row.iter().rposition(|c| c.symbol() != " " || c.bg != Color::Reset || !c.modifier.is_empty()).map_or(0, |i| i + 1);
        let mut last = String::from("\x1b[0m");
        for c in &row[..end] {
            let code = sgr(c);
            if code != last {
                out += &code;
                last = code;
            }
            out += c.symbol();
        }
        out += if last != "\x1b[0m" { "\x1b[0m\n" } else { "\n" };
    }
    out
}

pub fn text(buf: &Buffer) -> String {
    let (cursor, mark) = (gl().cursor.to_string(), gl().mark.to_string());
    let mut out = String::new();
    for y in 0..buf.area.height {
        let line: String = (0..buf.area.width)
            .map(|x| {
                let cell = &buf[(x, y)];
                match cell.symbol() {
                    // the column, and its head on the time axis (the EDIT badge shares the head's colour, but is bold)
                    s if (cell.bg == pal().column || cell.bg == pal().cursor && !cell.modifier.contains(Modifier::BOLD)) && !s.chars().all(char::is_alphanumeric) => cursor.as_str(),
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

/// A stream field's value between records (spec §15): all `z`.
// shortcut: a record whose field really is all z draws as a gap too; give gaps their own value if that matters
fn gap(v: &Val) -> bool {
    matches!(v, Val::Bits(b) if !b.is_empty() && b.chars().all(|c| c == 'z'))
}

/// The line a value is drawn with: X's, Z's (as `state_style` tells them), or a value's.
fn rail(v: &Val) -> char {
    match v {
        Val::Bits(b) if b.contains(['x', 'u', 'w', '-']) => gl().unknown,
        Val::Bits(b) if b.contains('z') => gl().highz,
        _ => gl().line,
    }
}

/// Terminal lines per signal: two, so 1-bit edges and bus outlines connect.
pub const ROW_LINES: usize = 2;

/// The rows (names, values at the cursor, waveforms) under the time axis and the cursor ruler,
/// scrolled so the selection shows: `top` is the first row on screen, kept up to date. With
/// `adding`, a line marks where new signals go and the selection isn't accented (the keys are
/// elsewhere). Returns how many lines of rows fit, and the column the waveforms start at.
pub fn rows(ws: &mut Workspace, top: &mut usize, adding: bool, buf: &mut Buffer, w: usize, body: usize) -> (usize, usize) {
    // line 0 is the time axis
    // while adding, one more line marks where new signals go
    // line 1 is the cursor ruler when there is more than one cursor
    let cursors = ws.cursor_list();
    let head = 1 + (cursors.len() > 1) as usize;
    let fit = (body.saturating_sub(head + adding as usize) / ROW_LINES).max(1);
    let lines = ws.lines();
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
    // the lines under a row, indented a level deeper each (spec §6): a bit after its bus,
    // `data[3]`; a stream's field after its stream, `frame.len`; a composite's child by its field
    // or index, `valid`, `pkts[0]`. A stream or composite folds and unfolds as a group does its
    // rows: the same marks
    let mut subs: Option<(usize, Vec<Sub>)> = None;
    let shows: Vec<Option<Sub>> = lines
        .iter()
        .map(|&(i, line)| {
            let s = ws.rows[i].sig()?;
            let Some(b) = line else { return Some(Sub { var: s.var, bit: None, depth: 0 }) };
            if subs.as_ref().is_none_or(|(r, _)| *r != i) {
                subs = Some((i, ws.subs(s)));
            }
            let list = &subs.as_ref().expect("set above").1;
            list.get(list.len().checked_sub(b as usize + 1)?).cloned()
        })
        .collect();
    let names: Vec<String> = lines
        .iter()
        .zip(&shows)
        .map(|(&(i, line), l)| {
            let (Some(s), Some(l)) = (ws.rows[i].sig(), l) else { return row_names[i].clone() };
            let n = &row_names[i];
            let indent = &n[..n.len() - n.trim_start().len()];
            let marked = |var: usize, name: &str| match ws.stream_of(var).is_some() || ws.trace.is_composite(var) {
                true => format!("{} {name}", if s.open.contains(&var) { gl().group_open } else { gl().group_closed }),
                false => name.to_string(),
            };
            if line.is_none() {
                return format!("{indent}{}", marked(s.var, n.trim_start()));
            }
            let own = |v: usize| match v == s.var {
                true => n.trim_start().to_string(),
                false if ws.stream_of(s.var).is_some() => format!("{}{}", n.trim_start(), &ws.trace.vars[v].path[ws.trace.vars[s.var].path.len()..]),
                false => rows::child_name(&ws.trace.vars[v].path),
            };
            let name = match l.bit {
                Some(bit) => format!("{}[{bit}]", own(l.var)),
                None => marked(l.var, &own(l.var)),
            };
            format!("{indent}{}{name}", "  ".repeat(l.depth + 1))
        })
        .collect();
    // a stream's field between records has no value to show
    let is_field = |var: usize| ws.streams.iter().any(|s| ws.trace.vars[var].path.strip_prefix(s.name.as_str()).is_some_and(|f| f.starts_with('.')));
    let (values, value_styles): (Vec<String>, Vec<Style>) = lines
        .iter()
        .zip(&shows)
        .map(|(&(i, _), l)| {
            let (s, l) = (ws.rows[i].sig()?, l.as_ref()?);
            let var = l.var;
            let v = ws.trace.value_of(var, l.bit, ws.view.cursor).filter(|v| !(is_field(var) && gap(v)))?;
            let stream = ws.stream_of(var);
            let st = if stream.is_some_and(|s| s.violations) { red() } else { state_style(&v) };
            Some((label(&ws.trace, stream, &line_sig(ws, s, var), &v), st))
        })
        .map(Option::unwrap_or_default)
        .unzip();
    let width = |v: &[String], max| v.iter().map(|s| s.chars().count()).max().unwrap_or(0).min(max);
    // hidden rows count too, so collapsing a group doesn't shift the waveforms
    let hidden: Vec<String> = ws
        .rows
        .iter()
        .filter_map(|r| r.sig())
        .filter_map(|s| ws.trace.value_at(s.var, ws.view.cursor).filter(|v| !(is_field(s.var) && gap(v))).map(|v| label(&ws.trace, ws.stream_of(s.var), s, &v)))
        .collect();
    let nw = width(&names, MAX_NAME).max(width(&row_names, MAX_NAME));
    let vw = width(&values, MAX_VALUE).max(width(&hidden, MAX_VALUE));
    // name, value, wave, with a gap of two between them
    let (vx, wx) = (nw + 2, nw + 2 + vw + 2);
    ws.view.set_width(w.saturating_sub(wx) as u64);
    let ww = w.saturating_sub(wx);

    // the time axis, with the cursor's time in the accent at its column (kept on screen); the
    // labels it would touch are left out, as are those that would run into the previous one
    let dim = Style::new().fg(pal().dim);
    let pill = ws.view.col_of(ws.view.cursor).map(|c| {
        let t = time::format(ws.view.cursor, ws.trace.fs_per_tick);
        ((c as usize).min(ww.saturating_sub(t.chars().count())), t)
    });
    let mut free = 0;
    for (c, t) in axis(&ws.view, ws.trace.fs_per_tick) {
        let len = t.chars().count();
        let clear = pill.as_ref().is_none_or(|(x, p)| c + len + 1 < *x || c > x + p.chars().count());
        if c >= free && c + len <= ww && clear {
            put(buf, wx + c, 0, &t, dim);
            free = c + len + 2;
        }
    }

    // bit lines of an expanded bus share one pass over the bus (var, [bit][column])
    let mut bit_changes: Option<(usize, Vec<Vec<Changes>>)> = None;
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
            // a column, not a byte: a marker before the name (`▸`) is one column of three bytes
            put(buf, name[..leaf].chars().count(), yb, &name[leaf..], Style::new());
        }
        let value: String = values[l].chars().take(vw).collect();
        put(buf, vx, yb, &value, value_styles[l]);
        let cells = match &ws.rows[i] {
            Row::Signal(_) if shows[l].is_none() => Vec::new(),
            Row::Signal(s) => {
                let l = shows[l].as_ref().expect("checked above");
                let line = line_sig(ws, s, l.var);
                let bits = l.bit.map(|b| {
                    if bit_changes.as_ref().is_none_or(|(v, _)| *v != l.var) {
                        let v = &ws.view;
                        let ranges: Vec<_> = (0..ww as u64).map(|c| (v.col_start(c), v.col_start(c + 1))).collect();
                        bit_changes = Some((l.var, ws.trace.bit_changes(l.var, &ranges)));
                    }
                    bit_changes.as_ref().map_or(&[][..], |(_, c)| &c[b as usize][..])
                });
                wave(&ws.trace, &ws.view, &line, l.bit, bits, ws.stream_of(l.var), is_field(l.var), ww)
            }
            Row::Group { collapsed: true, .. } => activity(&ws.trace, &ws.view, &rows::members(&ws.rows, i), ww),
            Row::Group { .. } => Vec::new(),
        };
        for (c, cells) in cells.into_iter().enumerate() {
            for (dy, (ch, st)) in cells.into_iter().enumerate().filter(|(dy, (ch, st))| y + dy < body && (*ch, *st) != (' ', Style::new())) {
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
            y += 1;
        }
    }
    let end = y.min(body);
    if head > 1 {
        ruler(ws, buf, wx, ww, 1);
    }
    // the marks first, so the cursor is on top where they share a column; down to the last row
    for (i, t) in cursors.into_iter().filter(|&(i, _)| i != 0).chain([(0, ws.view.cursor)]) {
        if let Some(c) = ws.view.col_of(t) {
            let area = Rect::new((wx + c as usize) as u16, 0, 1, end as u16);
            buf.set_style(area, Style::new().bg(if i == 0 { pal().column } else { pal().cursor_dim }));
            // without colour, a glyph where the column is blank, the waveform kept where it isn't
            if pal().mono {
                for y in area.top() + 1..area.bottom() {
                    if buf[(area.x, y)].symbol() == " " {
                        buf[(area.x, y)].set_char(if i == 0 { gl().cursor } else { gl().mark });
                    }
                }
            }
        }
    }
    // bold: `text` shows it as it is, not as the cursor's column
    if let Some((x, t)) = pill {
        put(buf, wx + x, 0, &t, Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(Modifier::BOLD | pal().accent()));
    }
    (fit, wx)
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
/// var draws its records' labels, red for violations, and nothing between records; so does a
/// stream's `field`, whose value there is all `z`.
#[allow(clippy::too_many_arguments)]
pub fn wave(trace: &Trace, view: &View, row: &Sig, bit: Option<u32>, bits: Option<&[Changes]>, stream: Option<&Stream>, field: bool, ww: usize) -> Vec<[(char, Style); 2]> {
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
    let value = |t: Tick| trace.value_of(var, bit, t).filter(|v| !(field && gap(v)));
    let val = |c: usize| value(range(c).1 - 1);
    let blank = (' ', Style::new());
    let mut out = vec![[blank; 2]; ww];
    // the row's own colour, or the theme's: a bus's outline in a hue of its own, its labels in
    // the text's colour so they read first
    let own = match row.color {
        Some(c) => Style::new().fg(pal().hues[c]),
        None if bus => Style::new().fg(pal().bus),
        None => line(),
    };

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
                out[c..e].iter_mut().for_each(|o| *o = [(rail(&v), st); 2]);
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
                let text_st = if st == own && row.color.is_none() { Style::new() } else { st };
                if len > 0 {
                    let start = a + (e - a - len) / 2;
                    for (k, ch) in text.chars().enumerate() {
                        out[start + k][1] = (ch, text_st);
                    }
                    // a gap in the bottom edge around the label
                    for x in [start - 1, start + len] {
                        if out[x][1].0 == rail(&v) {
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
            let before = range(c).0.checked_sub(1).and_then(value);
            let (pt, pb, _) = lines_of(&before, own);
            let t = gl().edge_up[edge(pt, top)];
            let b = gl().edge_down[edge(pb, bot)];
            [(t, own), (b, own)]
        } else {
            let r = val(c).as_ref().map_or(gl().line, rail);
            [if top { (r, st) } else { blank }, if bot { (r, st) } else { blank }]
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
                    if out[x][1].0 == rail(&v) {
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
