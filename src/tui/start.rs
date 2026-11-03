//! The start screen (spec §4 "Start screen"): what `wtf` shows without a trace. The logo, the
//! actions with their keys, the traces opened lately, the agents' workspaces to join.

use crate::draw::{gl, icon, pal};
use crate::history::{self, Open};
use crate::live;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::path::{Path, PathBuf};

/// The logo's letters, `w t f` in a pixel font: one pixel a column and half a line, `#` ink.
/// Two pixel lines make a cell (▀ ▄); the shadow is the ink moved 2 right and 2 down.
const INK: [&str; 20] = [
    "",
    "                          ##                    ######",
    "                         ###                   #######",
    "                         ###                  ########",
    "                         ###                  ###",
    "                         ###                  ###",
    "###         ###      ############         ############",
    "###         ###      ############         ############",
    "###         ###      ############         ############",
    "###   ###   ###          ###                  ###",
    "###   ###   ###          ###                  ###",
    "###   ###   ###          ###                  ###",
    "###   ###   ###          ###                  ###",
    "###   ###   ###          ###                  ###",
    "###   ###   ###          ###                  ###",
    "###   ###   ###          ###                  ###",
    "###   ###   ###          ###                  ###",
    "###############          #########            ###",
    "###############          #########            ###",
    " #############            ########            ###",
];
/// The ink's width, the cursor's column (in the gap between t and f) and where the bus's first
/// value ends (between w and t).
const INK_W: usize = 54;
const CURSOR: usize = 38;
const SPLIT: usize = 18;
const SHADOW: usize = 2;
/// The pixel logo's size in cells, with its shadow.
const LOGO_W: usize = INK_W + SHADOW;
const LOGO_H: usize = (INK.len() + SHADOW).div_ceil(2);
/// Below these the logo is one line.
const BIG_H: usize = 26;
const BIG_W: usize = LOGO_W + 2;
/// The column of actions and lists.
const COL_W: usize = 54;
/// Recent's strip: the last 7 days, half a day a cell.
const STRIP: usize = 14;
const CELL_SECS: u64 = 12 * 3600;


/// What the user picked.
#[derive(Debug, PartialEq)]
pub enum Choice {
    Open { trace: PathBuf, session: Option<PathBuf> },
    /// Open the agent workspace's trace and follow it.
    Join { trace: PathBuf, id: String },
    Quit,
}

/// A trace (with the session it was opened with) in Recent.
struct Recent {
    trace: PathBuf,
    session: Option<PathBuf>,
    last: u64,
    /// Each half day of the last 7, oldest first: opened by an agent (true), by the user (false).
    strip: [Option<bool>; STRIP],
}

/// What the list under the actions shows.
#[derive(PartialEq)]
enum List {
    Recent,
    Agents,
    Keys,
}

pub struct Start {
    recent: Vec<Recent>,
    agents: Vec<live::Session>,
    list: List,
    /// The path being typed after `o`.
    typing: Option<String>,
    /// An error, on the footer's line.
    pub msg: Option<String>,
    /// Recent's times are as of this.
    now: u64,
    /// Drawn for plain text (`--snapshot` without `--ansi`): the logo in plain characters.
    pub plain: bool,
}

impl Start {
    /// From the history (oldest first), as of `now`, and the agents' workspaces.
    pub fn new(opens: &[Open], now: u64, agents: Vec<live::Session>) -> Start {
        let mut recent: Vec<Recent> = Vec::new();
        for o in opens {
            let i = match recent.iter().position(|r| r.trace == o.trace && r.session == o.session) {
                Some(i) => i,
                None => {
                    recent.push(Recent { trace: o.trace.clone(), session: o.session.clone(), last: 0, strip: [None; STRIP] });
                    recent.len() - 1
                }
            };
            let r = &mut recent[i];
            r.last = r.last.max(o.at);
            let ago = (now.saturating_sub(o.at) / CELL_SECS) as usize;
            if ago < STRIP {
                // the latest open in a cell colours it
                r.strip[STRIP - 1 - ago] = Some(o.agent);
            }
        }
        recent.sort_by_key(|r| std::cmp::Reverse(r.last));
        Start { recent, agents: agents.into_iter().filter(|s| s.kind == "agent").collect(), list: List::Recent, typing: None, msg: None, now, plain: false }
    }

    /// The user's history, its traces that are gone left out, and the agents on this machine.
    pub fn load() -> Start {
        let opens: Vec<Open> = history::path().map(|f| history::read(&f)).unwrap_or_default().into_iter().filter(|o| o.trace.exists()).collect();
        Start::new(&opens, history::now(), live::sessions().unwrap_or_default())
    }

    /// Lists the agents' workspaces again.
    pub fn refresh_agents(&mut self) {
        self.agents = live::sessions().unwrap_or_default().into_iter().filter(|s| s.kind == "agent").collect();
    }

    /// The keys (spec §4 "Start screen"). Returns what was picked, if anything.
    pub fn key(&mut self, k: KeyEvent) -> Option<Choice> {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return Some(Choice::Quit);
        }
        if let Some(t) = &mut self.typing {
            match k.code {
                KeyCode::Char(c) => t.push(c),
                KeyCode::Backspace => _ = t.pop(),
                KeyCode::Esc => self.typing = None,
                KeyCode::Enter => {
                    let t = self.typing.take().unwrap_or_default();
                    let trace = match t.strip_prefix("~/") {
                        Some(rest) => std::env::var_os("HOME").map_or(PathBuf::from(&t), |h| Path::new(&h).join(rest)),
                        None => PathBuf::from(t.trim()),
                    };
                    return (!t.trim().is_empty()).then_some(Choice::Open { trace, session: None });
                }
                _ => {}
            }
            return None;
        }
        self.msg = None;
        match k.code {
            KeyCode::Char('o') => self.typing = Some(String::new()),
            KeyCode::Char('a') => match self.agents.as_slice() {
                [] => self.msg = Some("no agent works on this machine; one opens a trace with wtf --mcp".into()),
                [s] => return Some(Choice::Join { trace: s.trace.clone(), id: s.id.clone() }),
                _ => self.list = List::Agents,
            },
            KeyCode::Char('?') => self.list = if self.list == List::Keys { List::Recent } else { List::Keys },
            KeyCode::Char('q') => return Some(Choice::Quit),
            KeyCode::Esc => self.list = List::Recent,
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                match self.list {
                    List::Agents => return self.agents.get(i).map(|s| Choice::Join { trace: s.trace.clone(), id: s.id.clone() }),
                    _ => return self.recent.get(i).map(|r| Choice::Open { trace: r.trace.clone(), session: r.session.clone() }),
                }
            }
            _ => {}
        }
        None
    }

    /// The actions: icon, its colour, label, a dim note, key.
    fn actions(&self) -> Vec<Action> {
        let p = pal();
        let (blue, cyan) = (p.hues[5], p.hues[4]);
        let mut a = vec![(icon::FOLDER, blue, "Open a trace".to_string(), String::new(), "o".to_string())];
        match self.agents.as_slice() {
            [] => {}
            [s] => a.push((icon::AGENT, cyan, format!("Join {}", s.id), format!("on {}", name(&s.trace)), "a".into())),
            all => a.push((icon::AGENT, cyan, "Join an agent".into(), format!("{} workspaces", all.len()), "a".into())),
        }
        a.push((icon::KEYS, blue, "Keys".into(), String::new(), "?".into()));
        a.push((icon::ERROR, p.dim, "Quit".into(), String::new(), "q".into()));
        a
    }

    /// The list's heading and how many lines it has (none: no list).
    fn list_len(&self) -> usize {
        match self.list {
            List::Recent => self.recent.len(),
            List::Agents => self.agents.len(),
            List::Keys => KEYS.len(),
        }
        .min(9)
    }

    /// Lines the screen takes at its full size.
    pub fn height(&self) -> u16 {
        let n = self.list_len();
        (LOGO_H + 4 + self.actions().len() + if n > 0 { n + 2 } else { 0 } + 2) as u16
    }

    /// Draws the screen; returns where the terminal cursor goes while a path is typed.
    pub fn draw(&self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
        let (w, h) = (area.width as usize, area.height as usize);
        let p = pal();
        let accent = Style::new().fg(p.cursor).add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(p.dim);
        let big = h >= BIG_H && w >= BIG_W;
        // the logo, a blank, the bus (the big logo's), a blank
        let logo_h = if big { BIG_LOGO.0 } else { 2 };
        let actions = self.actions();
        // the footer and the blank above it
        let fixed = logo_h + actions.len() + 2;
        // the list, a blank and its heading above it, takes the lines left, up to 9
        let n = self.list_len().min(h.saturating_sub(fixed + 2));
        let list_h = if n > 0 { n + 2 } else { 0 };
        let y0 = h.saturating_sub(fixed + list_h) / 2;
        if big {
            big_logo(buf, area, y0, self.plain);
        } else {
            small_logo(buf, area, w.saturating_sub(7) / 2, y0);
        }

        // the actions, a column with the keys at its right
        let cw = COL_W.min(w.saturating_sub(2));
        let x = w.saturating_sub(cw) / 2;
        let ix = if gl().icons { 3 } else { 0 };
        let mut y = y0 + logo_h;
        action_rows(buf, area, x, y, cw, &actions);
        y += actions.len();
        let mut put = |x: usize, y: usize, s: &str, st: Style| {
            if x < w && y < h {
                buf.set_stringn(area.x + x as u16, area.y + y as u16, s, w - x, st);
            }
        };

        // the list: Recent, the agents to join, or the keys
        if n > 0 {
            y += 1;
            let (head, ic, c, note) = match self.list {
                List::Recent => ("Recent", icon::HISTORY, p.hues[6], "last 7 days"),
                List::Agents => ("Join an agent", icon::AGENT, p.hues[4], "Esc back"),
                List::Keys => ("Keys", icon::KEYS, p.hues[6], "Esc back"),
            };
            if gl().icons {
                put(x, y, &ic.to_string(), Style::new().fg(c));
            }
            put(x + ix, y, head, Style::new().fg(c).add_modifier(Modifier::BOLD));
            put(x + 22, y, note, dim);
            y += 1;
            for (i, num) in (1..=n).map(|k| k.to_string()).enumerate() {
                match self.list {
                    List::Recent => {
                        let r = &self.recent[i];
                        if gl().icons {
                            put(x, y, &icon::FILE.to_string(), Style::new().fg(p.hues[5]));
                        }
                        let room = 21 - ix;
                        let file = name(&r.trace);
                        put(x + ix, y, &cut(&file, room), Style::new());
                        if let Some(s) = &r.session {
                            let used = file.chars().count() + 1;
                            put(x + ix + used, y, &cut(&name(s), room.saturating_sub(used)), dim);
                        }
                        for (k, on) in r.strip.iter().enumerate() {
                            let (ch, c) = match on {
                                // you in the bus's colour, an agent in cyan; without colour the
                                // agent's line is thin
                                Some(false) => (gl().unknown, p.bus),
                                Some(true) => (if p.mono { gl().line } else { gl().unknown }, p.hues[4]),
                                None => (if gl().line.is_ascii() { '.' } else { '·' }, p.cursor_dim),
                            };
                            put(x + 22 + k, y, &ch.to_string(), Style::new().fg(c));
                        }
                        put(x + 40, y, &ago(self.now.saturating_sub(r.last)), dim);
                    }
                    List::Agents => {
                        let s = &self.agents[i];
                        if gl().icons {
                            put(x, y, &icon::AGENT.to_string(), Style::new().fg(p.hues[4]));
                        }
                        put(x + ix, y, &cut(&s.id, 21 - ix), Style::new());
                        put(x + 22, y, &cut(&name(&s.trace), cw.saturating_sub(24)), dim);
                    }
                    List::Keys => {
                        let (k, d) = KEYS[i];
                        put(x, y, k, accent);
                        put(x + 8, y, d, Style::new());
                    }
                }
                if self.list != List::Keys {
                    put(x + cw - 1, y, &num, accent);
                }
                y += 1;
            }
        }

        // the footer: the version and the agents, or the path being typed, or an error
        y += 1;
        let mut at = None;
        if let Some(t) = &self.typing {
            let line = format!("open {t}");
            let fx = x.min(w.saturating_sub(line.chars().count() + 1));
            put(fx, y, "open ", dim);
            put(fx + 5, y, t, Style::new());
            at = (y < h).then_some((area.x + (fx + line.chars().count()).min(w - 1) as u16, area.y + y as u16));
        } else if let Some(m) = &self.msg {
            put(w.saturating_sub(m.chars().count()) / 2, y, m, Style::new().fg(p.unknown));
        } else {
            let agents = {
                let mut pids: Vec<&str> = self.agents.iter().map(|s| s.id.split('-').next().unwrap_or(&s.id)).collect();
                pids.sort();
                pids.dedup();
                pids.len()
            };
            let mut foot = format!("wtf {}", env!("CARGO_PKG_VERSION"));
            if agents > 0 {
                foot += &format!("   {agents} agent{} on this machine", if agents == 1 { "" } else { "s" });
            }
            let icon = if gl().icons { format!("{} ", icon::STREAM) } else { String::new() };
            let fx = w.saturating_sub(icon.chars().count() + foot.len()) / 2;
            put(fx, y, &icon, Style::new().fg(p.hues[1]));
            put(fx + icon.chars().count(), y, &foot, dim);
        }
        crate::draw::fill_screen(buf);
        at
    }
}

/// The keys, for `?`.
const KEYS: [(&str, &str); 5] = [
    ("o", "open a trace: type its path, Enter"),
    ("1-9", "open a recent trace, with its session"),
    ("a", "join an agent's workspace and follow it"),
    ("? Esc", "these keys, and back"),
    ("q", "quit"),
];

/// An action of a dashboard (this screen, and the trace's before any row): icon, its colour,
/// label, a dim note after it, key.
pub type Action = (char, Color, String, String, String);

/// The actions in a column `cw` wide at (`x`, `y`) of `area`: the icon (with the `nerd` glyphs),
/// the label, its note dim, the key at the right in the accent.
pub fn action_rows(buf: &mut Buffer, area: Rect, x: usize, y: usize, cw: usize, actions: &[Action]) {
    let (w, h) = (area.width as usize, area.height as usize);
    let accent = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    let mut put = |x: usize, y: usize, s: &str, st: Style| {
        if x < w && y < h {
            buf.set_stringn(area.x + x as u16, area.y + y as u16, s, w - x, st);
        }
    };
    let ix = if gl().icons { 3 } else { 0 };
    for (k, (ic, c, label, note, key)) in actions.iter().enumerate() {
        if gl().icons {
            put(x, y + k, &ic.to_string(), Style::new().fg(*c));
        }
        put(x + ix, y + k, label, Style::new());
        put(x + ix + label.chars().count() + 1, y + k, note, Style::new().fg(pal().dim));
        put((x + cw).saturating_sub(key.chars().count()), y + k, key, accent);
    }
}

/// Lines the big logo takes with its bus and the blank under each, and the width it needs.
pub const BIG_LOGO: (usize, usize) = (LOGO_H + 1 + 2 + 1, BIG_W);

/// The pixel logo centred in `area` from line `y`, the bus under it carrying the letters' codes
/// (unknown from the cursor on), the cursor's column through both. `plain` (and the `ascii`
/// glyphs, and no colour) draw it in plain characters.
pub fn big_logo(buf: &mut Buffer, area: Rect, y: usize, plain: bool) {
    let (w, h) = (area.width as usize, area.height as usize);
    let p = pal();
    let mut put = |x: usize, y: usize, s: &str, st: Style| {
        if x < w && y < h {
            buf.set_stringn(area.x + x as u16, area.y + y as u16, s, w - x, st);
        }
    };
    // amber up to the cursor, unknown (red, as an X is drawn) after it
    let lx = w.saturating_sub(LOGO_W) / 2;
    logo(&mut put, lx, y, plain || gl().line.is_ascii() || p.mono);
    let by = y + LOGO_H + 1;
    let segs = [(0, SPLIT, "0x77", p.bus, gl().line), (SPLIT, CURSOR, "0x74", p.bus, gl().line), (CURSOR, INK_W, "0xXX", p.unknown, gl().unknown)];
    for (i, (a, b, label, c, ln)) in segs.into_iter().enumerate() {
        let st = Style::new().fg(c);
        if i > 0 {
            put(lx + a, by, &gl().crossing[0].to_string(), st);
            put(lx + a, by + 1, &gl().crossing[1].to_string(), st);
        }
        let x0 = a + (i > 0) as usize;
        let (n, label) = (b - x0, format!(" {label} "));
        let pad = n - label.len();
        let line = |k: usize| ln.to_string().repeat(k);
        put(lx + x0, by, &line(n), st);
        put(lx + x0, by + 1, &format!("{}{label}{}", line(pad / 2), line(pad - pad / 2)), st);
    }
    for y in (y.saturating_sub(1)..y + LOGO_H + 3).filter(|&y| y < h && lx + CURSOR < w) {
        cursor_cell(buf, area, lx + CURSOR, y);
    }
}

/// The logo on one line, `w t` in the accent and `f` unknown, the cursor's column between them.
pub fn small_logo(buf: &mut Buffer, area: Rect, x: usize, y: usize) {
    if x + 7 > area.width as usize || y >= area.height as usize {
        return;
    }
    let bold = Style::new().add_modifier(Modifier::BOLD);
    buf.set_string(area.x + x as u16, area.y + y as u16, "w t", bold.fg(pal().cursor));
    buf.set_string(area.x + (x + 6) as u16, area.y + y as u16, "f", bold.fg(pal().unknown));
    cursor_cell(buf, area, x + 4, y);
}

/// A cell of the cursor's column, as a waveform's: tinted, `┃` where it is blank.
fn cursor_cell(buf: &mut Buffer, area: Rect, x: usize, y: usize) {
    let cell = &mut buf[(area.x + x as u16, area.y + y as u16)];
    cell.set_bg(pal().column);
    if cell.symbol() == " " {
        cell.set_char(gl().cursor).set_fg(pal().cursor);
    }
}

/// The pixel logo at (`x`, `y`): ▀ ▄ in the accent before the cursor's column and in the unknown
/// colour after, with a shadow; or in plain characters (`plain`), without the shadow.
fn logo(put: &mut impl FnMut(usize, usize, &str, Style), x: usize, y: usize, plain: bool) {
    let p = pal();
    let ink = |py: usize, px: usize| INK.get(py).is_some_and(|l| l.as_bytes().get(px) == Some(&b'#'));
    let colour = |py: usize, px: usize| match ink(py, px) {
        true => Some(if px < CURSOR { p.cursor } else { p.unknown }),
        false if !plain && py >= SHADOW && px >= SHADOW && ink(py - SHADOW, px - SHADOW) => Some(p.cursor_dim),
        false => None,
    };
    for r in 0..LOGO_H {
        for c in 0..LOGO_W {
            let (t, b) = (colour(2 * r, c), colour(2 * r + 1, c));
            let (ch, st) = match (t, b, plain) {
                (None, None, _) => continue,
                (Some(t), Some(_), true) => ('#', Style::new().fg(t)),
                (Some(t), None, true) => ('"', Style::new().fg(t)),
                (None, Some(b), true) => (',', Style::new().fg(b)),
                (Some(t), b, false) => ('▀', Style::new().fg(t).bg(b.unwrap_or(Color::Reset))),
                (None, Some(b), false) => ('▄', Style::new().fg(b)),
            };
            put(x + c, y + r, &ch.to_string(), st);
        }
    }
}

fn name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// `s` in `n` columns, cut with `…`.
fn cut(s: &str, n: usize) -> String {
    match s.chars().count() <= n {
        true => s.to_string(),
        false => s.chars().take(n.saturating_sub(1)).chain(std::iter::once(gl().cut)).collect(),
    }
}

/// How long ago, in the largest unit.
fn ago(s: u64) -> String {
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        86400..1_209_600 => format!("{}d ago", s / 86400),
        _ => format!("{}w ago", s / 604_800),
    }
}

/// Shows the start screen until the user picks.
pub fn run(start: &mut Start) -> Result<Choice, String> {
    let mut term = ratatui::init();
    let res = (|| -> std::io::Result<Choice> {
        loop {
            term.draw(|f| {
                if let Some(at) = start.draw(f.area(), f.buffer_mut()) {
                    f.set_cursor_position(at);
                }
            })?;
            // agents come and go: listed again while no key comes
            if !crossterm::event::poll(std::time::Duration::from_secs(1))? {
                start.refresh_agents();
                continue;
            }
            if let crossterm::event::Event::Key(k) = crossterm::event::read()?
                && k.kind == crossterm::event::KeyEventKind::Press
                && let Some(c) = start.key(k)
            {
                return Ok(c);
            }
        }
    })();
    ratatui::restore();
    res.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw;

    const DAY: u64 = 86400;
    const NOW: u64 = 1_800_000_000;

    fn start() -> Start {
        let o = |at: u64, agent: bool, t: &str, s: Option<&str>| Open { at: NOW - at, agent, trace: t.into(), session: s.map(PathBuf::from) };
        let opens = [
            o(6 * DAY, false, "/w/tx_drift.fst", None),
            o(5 * DAY, false, "/w/eth_parser.fst", None),
            o(3 * DAY, false, "/w/eth_parser.fst", None),
            o(2 * DAY, true, "/w/eth_parser.fst", None),
            o(DAY, false, "/w/axilite.vcd", Some("/w/bug.toml")),
            o(7200, false, "/w/axilite.vcd", None),
            o(30, true, "/w/axilite.vcd", None),
        ];
        let agent = live::Session { id: "4123-1".into(), kind: "agent".into(), trace: "/w/axilite.vcd".into() };
        Start::new(&opens, NOW, vec![agent])
    }

    fn screen(s: &Start, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        s.draw(buf.area, &mut buf);
        // the cells as drawn: `draw::text` would read the logo's colours as cursor columns
        (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string() + "\n").collect()
    }

    fn press(s: &mut Start, keys: &str) -> Option<Choice> {
        crate::tui::app::parse_keys(keys).unwrap().into_iter().filter_map(|k| s.key(k)).last()
    }

    #[test]
    fn unicode() {
        insta::assert_snapshot!(screen(&start(), 100, 32));
    }

    #[test]
    fn ascii() {
        draw::set_glyphs(&draw::ASCII);
        insta::assert_snapshot!(screen(&start(), 100, 32));
        draw::set_glyphs(&draw::UNICODE);
    }

    #[test]
    fn nerd_icons() {
        draw::set_glyphs(&draw::NERD);
        let s = screen(&start(), 100, 32);
        draw::set_glyphs(&draw::UNICODE);
        assert!(s.contains("\u{ea83}  Open a trace") && s.contains("\u{ea7b}  axilite.vcd"), "{s}");
    }

    #[test]
    fn small() {
        insta::assert_snapshot!(screen(&start(), 60, 16));
    }

    #[test]
    fn keys() {
        let mut s = start();
        assert_eq!(press(&mut s, "1"), Some(Choice::Open { trace: "/w/axilite.vcd".into(), session: None }));
        assert_eq!(press(&mut s, "2"), Some(Choice::Open { trace: "/w/axilite.vcd".into(), session: Some("/w/bug.toml".into()) }));
        assert_eq!(press(&mut s, "a"), Some(Choice::Join { trace: "/w/axilite.vcd".into(), id: "4123-1".into() }));
        assert_eq!(press(&mut s, "8"), None);
        assert_eq!(press(&mut s, "o<Esc>q"), Some(Choice::Quit));
        assert_eq!(press(&mut s, "ox.vcdd<BS><CR>"), Some(Choice::Open { trace: "x.vcd".into(), session: None }));
        assert_eq!(press(&mut s, "o<CR>"), None);
        assert!(screen(&s, 100, 32).contains("wtf "));
        // ? lists the keys in Recent's place
        press(&mut s, "?");
        assert!(screen(&s, 100, 32).contains("join an agent's workspace"));
        // several agents: a lists them, a number joins one
        let mut s = start();
        s.agents.push(live::Session { id: "5000-2".into(), kind: "agent".into(), trace: "/w/eth_parser.fst".into() });
        assert_eq!(press(&mut s, "a"), None);
        assert!(screen(&s, 100, 32).contains("5000-2"));
        assert_eq!(press(&mut s, "2"), Some(Choice::Join { trace: "/w/eth_parser.fst".into(), id: "5000-2".into() }));
        assert!(screen(&s, 100, 32).contains("2 agents on this machine"));
        // no agents: said
        let mut s = Start::new(&[], NOW, Vec::new());
        assert_eq!(press(&mut s, "a"), None);
        assert!(screen(&s, 100, 32).contains("no agent works on this machine"));
    }
}
