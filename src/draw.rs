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

/// The name and value columns' widest, without `name_width` / `value_width` in the config file.
pub const COLUMNS: (usize, usize) = (30, 20);

pub struct Palette {
    /// 1-bit lines and edges.
    pub line: Color,
    /// Bus and stream outlines; their labels are in the text's colour.
    pub bus: Color,
    /// Fill for a 1-bit signal toggling faster than a column.
    pub fill: Color,
    pub unknown: Color,
    pub highz: Color,
    /// The accent: the cursor's head on the time axis, the selected name.
    pub cursor: Color,
    pub cursor_text: Color,
    /// Behind the cursor's column: a tint, so the waveform under it stays as it is.
    pub column: Color,
    /// The other cursors' columns, fainter.
    pub cursor_dim: Color,
    /// Behind a mark's number and name on the ruler.
    pub mark: Color,
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
        Palette { line: r, bus: r, fill: r, unknown: r, highz: r, cursor: r, cursor_text: r, column: r, cursor_dim: r, mark: r, selected: r, panel: r, hatch: r, dim: r, hues: [r; 8], background: r, text: r, mono: true }
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

type Rgb = (u8, u8, u8);
static TERMINAL: std::sync::OnceLock<(Option<Rgb>, Option<Rgb>)> = std::sync::OnceLock::new();

/// The terminal's background and text colours, as far as it answered: a theme's `"default"`
/// is these (spec §19 "The terminal's background").
pub fn set_terminal_colors(bg: Option<Rgb>, fg: Option<Rgb>) {
    let _ = TERMINAL.set((bg, fg));
}

/// `c` as `#rrggbb`: its own, or the terminal's for `Reset` (`bg` for a background), if known.
pub fn rgb_of(c: Color, bg: bool) -> Option<Rgb> {
    match c {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Reset => TERMINAL.get().and_then(|t| if bg { t.0 } else { t.1 }),
        _ => None,
    }
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
    /// The highlighted completion, and the insertion point while the signal panel is open.
    pub hl: char,
    pub insert: char,
    /// Borders: top-left, top-right, bottom-left, bottom-right, horizontal, vertical.
    pub border: [char; 6],
    /// The cards' borders (which-key, the leader menu, notifications, the picker), as `border`
    /// with round corners.
    pub card: [char; 6],
    /// Between a key and what it does: `g → start`.
    pub arrow: char,
    /// Separators in text: `help · keys`, `65ns–85ns`.
    pub dot: char,
    pub dash: char,
    /// Nerd Font icons beside labels (the start screen's); the other sets draw none.
    pub icons: bool,
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
    card: ['╭', '╮', '╰', '╯', '─', '│'],
    arrow: '→',
    dot: '·',
    dash: '–',
    icons: false,
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
    card: ['+', '+', '+', '+', '-', '|'],
    arrow: '>',
    dot: '-',
    dash: '-',
    icons: false,
};

/// Nerd Font icons (`nf-cod-*` codicons), drawn with the `nerd` glyphs only (spec §14).
pub mod icon {
    pub const WIRE: char = '\u{eb31}';
    pub const CLOCK: char = '\u{eb7c}';
    pub const MARK: char = '\u{eaa5}';
    // cod-sparkle: in Nerd Fonts 3.4 too; cod-robot (U+EC20) only came with 3.5
    pub const AGENT: char = '\u{ec10}';
    pub const ERROR: char = '\u{ea87}';
    pub const INFO: char = '\u{ea74}';
    pub const OK: char = '\u{eab2}';
    pub const SEARCH: char = '\u{ea6d}';
    pub const CMD: char = '\u{ea85}';
    pub const HISTORY: char = '\u{ea82}';
    pub const FILE: char = '\u{ea7b}';
    pub const SESSION: char = '\u{ebd2}';
    pub const EYE: char = '\u{ea70}';
    pub const TRAIL: char = '\u{eb86}';
    pub const BELL: char = '\u{eaa2}';
    pub const KEYS: char = '\u{ea65}';
    pub const PLUG: char = '\u{eb2d}';
    pub const RECORD: char = '\u{eba7}';
    pub const FOLDER: char = '\u{ea83}';
    pub const STREAM: char = '\u{ea86}';
    pub const SAVE: char = '\u{eb4b}';
    pub const ZOOM: char = '\u{eb81}';
    pub const EDIT: char = '\u{ea73}';
    // what a row is (spec §14 "Glyphs"), Material Design icons of the Nerd Font
    pub const CLOCK_WAVE: char = '\u{f147b}';
    pub const BUS: char = '\u{f0688}';
    pub const ENUM: char = '\u{f04fc}';
    pub const REAL: char = '\u{f095b}';
    pub const TEXT: char = '\u{f027e}';
    pub const STRUCT: char = '\u{f0169}';
    pub const ARRAY: char = '\u{f0168}';
    pub const COMPUTED: char = '\u{f0871}';
    pub const RECORDS: char = '\u{f066c}';
    pub const FIELD: char = '\u{f0316}';
    pub const VIOLATION: char = '\u{f0029}';
    pub const GROUP: char = '\u{f0256}';
    pub const GROUP_OPEN: char = '\u{f0dcf}';
    pub const SCOPE: char = '\u{f061a}';
}

/// The name and value columns' headers, on the time axis' line (spec §4).
const NAME_HEAD: &str = "Name";
const VALUE_HEAD: &str = "Value";

/// What a var is, as its icon and colour (spec §14): a clock (some decode's), a 1-bit signal or
/// a bit of a bus, a bus, one with value names, a real, text, a struct or an array, a computed
/// signal, a stream (its violations apart) or a stream's field. None without the `nerd` glyphs.
pub fn kind(ws: &Workspace, var: usize, bit: Option<u32>) -> Option<(char, Color)> {
    gl().icons.then_some(())?;
    let (t, p) = (&ws.trace, pal());
    let v = &t.vars[var];
    let stream = ws.stream_of(var);
    let field = ws.streams.iter().any(|s| v.path.strip_prefix(s.name.as_str()).is_some_and(|f| f.starts_with('.')));
    Some(match () {
        _ if bit.is_some() => (icon::WIRE, p.line),
        _ if stream.is_some_and(|s| s.violations) => (icon::VIOLATION, p.unknown),
        _ if stream.is_some() => (icon::RECORDS, p.hues[5]),
        _ if field => (icon::FIELD, p.hues[5]),
        _ if t.is_composite(var) && t.is_array(var) => (icon::ARRAY, p.bus),
        _ if t.is_composite(var) => (icon::STRUCT, p.bus),
        _ if ws.decodes.iter().any(|d| d.clock == v.path) => (icon::CLOCK_WAVE, p.line),
        _ if t.is_computed(var) => (icon::COMPUTED, p.dim),
        _ if v.real => (icon::REAL, p.bus),
        _ if v.width.is_none() => (icon::TEXT, p.bus),
        _ if v.names.is_some() => (icon::ENUM, p.bus),
        _ if v.width == Some(1) => (icon::WIRE, p.line),
        _ => (icon::BUS, p.bus),
    })
}

/// `c` with the `nerd` glyphs, none with the others.
pub fn icon(c: char) -> Option<char> {
    gl().icons.then_some(c)
}

/// `unicode` with Nerd Font icons (the `nf-cod-*` codicons), for terminals set to a Nerd Font.
pub const NERD: Glyphs = Glyphs { icons: true, ..UNICODE };

pub const SETS: [(&str, &Glyphs); 4] = [("unicode", &UNICODE), ("box", &BOX), ("ascii", &ASCII), ("nerd", &NERD)];

pub fn glyphs_named(name: &str) -> Result<&'static Glyphs, String> {
    SETS.iter().find(|(n, _)| *n == name).map(|(_, g)| *g).ok_or_else(|| format!("unknown glyphs: {name} (unicode, box, ascii, nerd)"))
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

thread_local! {
    static COLUMN_MAX: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new(COLUMNS) };
    /// A line's widest value, by (trace version, var, radix, frac): a scan of its values is done once.
    static WIDEST: std::cell::RefCell<std::collections::HashMap<(u64, usize, usize, u32), usize>> = Default::default();
}

/// The name and value columns' widest (spec §19).
pub fn set_columns(c: (usize, usize)) {
    COLUMN_MAX.with(|x| x.set(c));
}

/// The widest value a line can show (spec §4 "Value column"), so the column keeps its width as
/// the cursor moves: a bit vector's from its width and radix, anything else (records, text,
/// reals, times) from its values, scanned once.
fn widest(ws: &Workspace, var: usize, bit: Option<u32>, row: &Sig) -> usize {
    let (t, v) = (&ws.trace, &ws.trace.vars[var]);
    if bit.is_some() {
        return 1;
    }
    let stream = ws.stream_of(var);
    if let (None, Some(w), false, None) = (stream, v.width, v.real, v.time) {
        let w = w as usize;
        let bits = |b: String| label(t, None, row, &Val::Bits(b)).chars().count();
        // all ones the most digits, a one and zeros the most negative
        let names = v.names.iter().flat_map(|n| n.iter()).map(|(_, n)| n.chars().count()).max().unwrap_or(0);
        return bits("1".repeat(w)).max(bits(format!("1{}", "0".repeat(w - 1)))).max(names);
    }
    let key = (t.version, var, row.look.radix, row.look.frac);
    if let Some(n) = WIDEST.with(|c| c.borrow().get(&key).copied()) {
        return n;
    }
    let n = match stream {
        Some(s) => (0..s.records.len()).map(|i| s.label(i, t.fs_per_tick).chars().count()).max().unwrap_or(0),
        None => std::iter::once(t.value_at(var, t.start())).chain(t.walk(var, None, t.start(), true).map(|(_, v)| v)).flatten().map(|v| label(t, None, row, &v).chars().count()).max().unwrap_or(0),
    };
    WIDEST.with(|c| c.borrow_mut().insert(key, n));
    n
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

/// A line's signal: `var` in the look it has under row `s` (`Workspace::look`).
pub fn line_sig(ws: &Workspace, s: &Sig, var: usize) -> Sig {
    Sig { var, look: ws.look(s, var), ..s.clone() }
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
                    // the column, and its head on the time axis; bold is text on the accent (the
                    // time axis pill, a mark's tab under the cursor)
                    s if (cell.bg == pal().column || cell.bg == pal().cursor) && !cell.modifier.contains(Modifier::BOLD) && !s.chars().all(char::is_alphanumeric) => cursor.as_str(),
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

/// Between a stream's records (its own var, `stream`, empty there), a field of it has no value
/// to draw (spec §15), though its var holds z; a record's field that really is all z is drawn.
fn gap(trace: &Trace, stream: Option<usize>, t: Tick) -> bool {
    stream.is_some_and(|s| !matches!(trace.value_at(s, t), Some(Val::Text(r)) if !r.is_empty()))
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

/// What `rows` drew: how many lines of rows fit, the column the waveforms start at, the name
/// column's width, and each line on screen with its first screen line, the line it shows
/// (`Workspace::lines`) and the column of its `▸` / `▾` if it opens.
pub struct Drawn {
    pub fit: usize,
    pub wave_x: usize,
    /// The first screen line of rows, under the time axis and the ruler.
    pub head: usize,
    pub name_w: usize,
    pub lines: Vec<RowLine>,
}

/// A row line on screen: its first screen line, the line it shows (`Workspace::lines`) and the
/// column of its `▸` / `▾` if it opens.
pub type RowLine = (usize, (usize, Option<u32>), Option<usize>);

/// The column of a line's `▸` / `▾`, with the line (`Workspace::lines`).
pub type Marked = ((usize, Option<u32>), Option<usize>);

/// The rows (names, values at the cursor, waveforms) under the time axis and the cursor ruler,
/// scrolled so the selection shows: `top` is the first row on screen, kept up to date. With
/// `adding`, a line marks where new signals go and the selection isn't accented (the keys are
/// elsewhere). Returns how many lines of rows fit, where the waveforms start and where each line
/// was drawn, for the mouse.
pub fn rows(ws: &mut Workspace, top: &mut usize, adding: bool, buf: &mut Buffer, w: usize, body: usize) -> Drawn {
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
    // `data[3]`; a stream's field by its name, `len`; a composite's child by its field
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
    // each line's name, and the column of its `▸` / `▾` if it opens
    let (names, markers): (Vec<String>, Vec<Option<usize>>) = lines
        .iter()
        .zip(&shows)
        .map(|(&(i, line), l)| {
            let (Some(s), Some(l)) = (ws.rows[i].sig(), l) else { return (row_names[i].clone(), ws.rows[i].is_group().then_some(0)) };
            let n = &row_names[i];
            let indent = &n[..n.len() - n.trim_start().len()];
            let opens = |var: usize| ws.stream_of(var).is_some() || ws.trace.is_composite(var);
            let marked = |var: usize, name: &str| match opens(var) {
                true => format!("{} {name}", if s.open.contains(&var) { gl().group_open } else { gl().group_closed }),
                false => name.to_string(),
            };
            let at = indent.chars().count();
            if line.is_none() {
                return (format!("{indent}{}", marked(s.var, n.trim_start())), opens(s.var).then_some(at));
            }
            let own = |v: usize| match v == s.var {
                true => n.trim_start().to_string(),
                false if ws.stream_of(s.var).is_some() => ws.trace.vars[v].path[ws.trace.vars[s.var].path.len() + 1..].to_string(),
                false => rows::child_name(&ws.trace.vars[v].path),
            };
            let (name, opens) = match l.bit {
                Some(bit) => (format!("{}[{bit}]", own(l.var)), false),
                None => (marked(l.var, &own(l.var)), opens(l.var)),
            };
            (format!("{indent}{}{name}", "  ".repeat(l.depth + 1)), opens.then_some(at + 2 * (l.depth + 1)))
        })
        .unzip();
    // a stream's field, as the stream's own var: between records it has no value to show
    let field_of = |var: usize| {
        let s = ws.streams.iter().find(|s| ws.trace.vars[var].path.strip_prefix(s.name.as_str()).is_some_and(|f| f.starts_with('.')))?;
        ws.trace.by_path.get(&s.name).copied()
    };
    let (values, value_styles): (Vec<String>, Vec<Style>) = lines
        .iter()
        .zip(&shows)
        .map(|(&(i, _), l)| {
            let (s, l) = (ws.rows[i].sig()?, l.as_ref()?);
            let var = l.var;
            let v = ws.trace.value_of(var, l.bit, ws.view.cursor).filter(|_| !gap(&ws.trace, field_of(var), ws.view.cursor))?;
            let stream = ws.stream_of(var);
            let st = if stream.is_some_and(|s| s.violations) { red() } else { state_style(&v) };
            Some((label(&ws.trace, stream, &line_sig(ws, s, var), &v), st))
        })
        .map(Option::unwrap_or_default)
        .unzip();
    // each line's icon (spec §14), in two columns after its `▸` / `▾` or its indent
    let icons: Vec<Option<(usize, char, Color)>> = lines
        .iter()
        .zip(&shows)
        .zip(names.iter().zip(&markers))
        .map(|((&(i, _), l), (n, m))| {
            let (c, col) = match (&ws.rows[i], l) {
                (Row::Group { collapsed, .. }, _) => (if *collapsed { icon::GROUP } else { icon::GROUP_OPEN }, pal().cursor),
                (_, l) => kind(ws, l.as_ref()?.var, l.as_ref()?.bit)?,
            };
            gl().icons.then_some((m.map_or(n.len() - n.trim_start().len(), |m| m + 2), c, col))
        })
        .collect();
    let names: Vec<String> = names
        .into_iter()
        .zip(&icons)
        .map(|(n, ic)| match ic {
            Some((at, ..)) => n.chars().take(*at).chain("  ".chars()).chain(n.chars().skip(*at)).collect(),
            None => n,
        })
        .collect();
    let width = |v: &[String], max| v.iter().map(|s| s.chars().count()).max().unwrap_or(0).min(max);
    // the widest each line and row can show, hidden rows too, so neither the cursor moving nor a
    // group collapsing shifts the waveforms (spec §4 "Value column")
    let lines_widest = lines.iter().zip(&shows).filter_map(|(&(i, _), l)| Some(widest(ws, l.as_ref()?.var, l.as_ref()?.bit, &line_sig(ws, ws.rows[i].sig()?, l.as_ref()?.var))));
    let rows_widest = ws.rows.iter().filter_map(Row::sig).map(|s| widest(ws, s.var, None, s));
    let (max_name, max_value) = COLUMN_MAX.with(|c| c.get());
    // at least as wide as the headers on the time axis' line
    let nw = width(&names, max_name).max(width(&row_names, max_name)).max(NAME_HEAD.len());
    let vw = lines_widest.chain(rows_widest).max().unwrap_or(0).min(max_value).max(VALUE_HEAD.len());
    // name, value, wave, a dim line in the gap of three between them (spec §4)
    let (vx, wx) = (nw + 3, nw + 3 + vw + 3);
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

    put(buf, 0, 0, NAME_HEAD, dim);
    put(buf, vx, 0, VALUE_HEAD, dim);
    for x in [nw + 1, vx + vw + 1] {
        for y in 0..body {
            put(buf, x, y, &gl().border[5].to_string(), Style::new().fg(pal().hatch));
        }
    }

    // bit lines of an expanded bus share one pass over the bus (var, [bit][column])
    let mut bit_changes: Option<(usize, Vec<Vec<Changes>>)> = None;
    let mut y = head;
    let mut drawn = Drawn { fit, wave_x: wx, head, name_w: nw, lines: Vec::new() };
    for (l, &(i, bit)) in lines.iter().enumerate().skip(*top).take(fit) {
        drawn.lines.push((y, (i, bit), markers[l].filter(|&m| m < nw)));
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
        if let Some((at, c, col)) = icons[l].filter(|ic| ic.0 < nw) {
            put(buf, at, yb, &c.to_string(), Style::new().fg(col));
        }
        // cut with a mark, so a cut value isn't read as a whole one; `e` shows it whole
        let value: String = match values[l].chars().count() > vw {
            true => values[l].chars().take(vw - 1).chain([gl().cut]).collect(),
            false => values[l].clone(),
        };
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
                wave(&ws.trace, &ws.view, &line, l.bit, bits, ws.stream_of(l.var), field_of(l.var), ww)
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
    // over the columns' tints, so a mark's tab keeps its own
    if head > 1 {
        ruler(ws, buf, wx, ww, 1);
    }
    // bold: `text` shows it as it is, not as the cursor's column
    if let Some((x, t)) = pill {
        put(buf, wx + x, 0, &t, Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(Modifier::BOLD | pal().accent()));
    }
    drawn
}

/// The cursor ruler (spec §9): the cursor's and each mark's number at its column, a mark's name on
/// a tab after it, and the Δt between neighbouring cursors in time. Cursors outside the view sit at
/// the edge (`◂3 first write`, `1▸`).
fn ruler(ws: &Workspace, buf: &mut Buffer, wx: usize, ww: usize, y: usize) {
    let v = &ws.view;
    let dim = Style::new().fg(pal().dim);
    let mut list = ws.cursor_list();
    list.sort_by_key(|&(i, t)| (t, i));
    // Cursors sharing a place (a column, or an edge) draw as one: the mark nearest the view, which
    // the cursor, if there too, lends its accent (spec §9).
    #[derive(Clone, Copy, PartialEq)]
    enum Slot {
        Left,
        At(usize),
        Right,
    }
    struct Group {
        slot: Slot,
        lo: usize,
        hi: usize,
        first: Tick,
        last: Tick,
        mark: Option<usize>,
        cursor: bool,
    }
    let mut groups: Vec<Group> = Vec::new();
    for (i, t) in list {
        let slot = match v.col_of(t) {
            Some(c) => Slot::At(c as usize),
            None if t < v.start => Slot::Left,
            None => Slot::Right,
        };
        if groups.last().is_none_or(|g| g.slot != slot) {
            let (lo, hi) = match slot {
                Slot::At(c) => (c, c),
                Slot::Left => (0, 1),
                Slot::Right => (ww.saturating_sub(2), ww.saturating_sub(1)),
            };
            groups.push(Group { slot, lo, hi, first: t, last: t, mark: None, cursor: false });
        }
        let g = groups.last_mut().unwrap();
        g.last = t;
        match i {
            0 => g.cursor = true,
            // in time order: the last one on the left and in a column, the first on the right
            _ if slot != Slot::Right || g.mark.is_none() => g.mark = Some(i),
            _ => {}
        }
    }
    // (number, name) styles of a mark, with the cursor on it or not
    let tint = |cursor: bool| match cursor {
        true => (Style::new().bg(pal().cursor).fg(pal().cursor_text).add_modifier(Modifier::BOLD | pal().accent()), Style::new().fg(pal().cursor).bg(pal().column).add_modifier(Modifier::BOLD)),
        false => (Style::new().fg(pal().text).bg(pal().mark).add_modifier(Modifier::BOLD), Style::new().fg(pal().text).bg(pal().mark)),
    };
    // a mark's name: a tab after its number (before it at the right edge) with a space on each
    // side, up to the next cursor; cut with … there, and left out with room for under three columns
    let mut tabs: Vec<(usize, String, Style)> = Vec::new();
    let mut free = 0;
    for (k, g) in groups.iter().enumerate() {
        if let Some(i) = g.mark.filter(|&i| !ws.labels[i].name.is_empty()) {
            let (from, to) = match g.slot {
                Slot::Right => (free, g.lo),
                _ => (g.hi + 1, groups.get(k + 1).map_or(ww, |n| n.lo)),
            };
            let room = to.saturating_sub(from).saturating_sub(2);
            let name = &ws.labels[i].name;
            let fit: Option<String> = match name.chars().count() {
                n if n <= room => Some(name.clone()),
                _ if room >= 3 => Some(name.chars().take(room - 1).chain([gl().cut]).collect()),
                _ => None,
            };
            if let Some(fit) = fit {
                let text = format!(" {fit} ");
                let len = text.chars().count();
                let x = if g.slot == Slot::Right { to - len } else { from };
                free = x + len;
                tabs.push((x, text, tint(g.cursor).1));
            }
        }
        free = free.max(g.hi + 1);
    }
    for pair in groups.windows(2) {
        let (a, b) = (pair[0].hi + 1, pair[1].lo);
        let span = b.saturating_sub(a);
        put(buf, wx + a, y, &gl().line.to_string().repeat(span), dim);
        // like bus labels: only if it fits with a spare column on each side, and no name is there
        let label = format!(" {} ", time::format(pair[1].first - pair[0].last, ws.trace.fs_per_tick));
        let len = label.chars().count();
        if len + 2 <= span && !tabs.iter().any(|t| (a..b).contains(&t.0)) {
            put(buf, wx + a + (span - len) / 2, y, &label, dim);
        }
    }
    for (x, text, st) in tabs {
        put(buf, wx + x, y, &text, st);
    }
    for g in groups {
        let (text, st) = match g.mark {
            Some(i) => ((i + 1).to_string(), tint(g.cursor).0),
            None => ("1".to_string(), Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD)),
        };
        let arrow = if g.cursor { Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD) } else { dim };
        match g.slot {
            Slot::At(c) => put(buf, wx + c, y, &text, st),
            Slot::Left => {
                put(buf, wx, y, &gl().left.to_string(), arrow);
                put(buf, wx + 1, y, &text, st);
            }
            Slot::Right => {
                put(buf, wx + g.lo, y, &text, st);
                put(buf, wx + g.hi, y, &gl().right.to_string(), arrow);
            }
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
pub fn wave(trace: &Trace, view: &View, row: &Sig, bit: Option<u32>, bits: Option<&[Changes]>, stream: Option<&Stream>, field: Option<usize>, ww: usize) -> Vec<[(char, Style); 2]> {
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
    let value = |t: Tick| trace.value_of(var, bit, t).filter(|_| !gap(trace, field, t));
    let val = |c: usize| value(range(c).1 - 1);
    let blank = (' ', Style::new());
    let mut out = vec![[blank; 2]; ww];
    // the row's own colour, or the theme's: a bus's outline in a hue of its own, its labels in
    // the text's colour so they read first
    let own = match row.look.color {
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
                let text_st = if st == own && row.look.color.is_none() { Style::new() } else { st };
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
            let text = rows::label(&trace.vars[var], &row.look, &v);
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
        _ => rows::label(&trace.vars[row.var], &row.look, v),
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
