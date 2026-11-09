//! The decodes list (spec §5 "Decodes"): every decode made, with its protocol, where it is and its
//! records; from it a decode's records open, its results are added as rows, it is edited in the
//! decode form or removed.

use crate::draw::{gl, pal, put};
use crate::tui::app::{App, Target};
use crate::tui::keys::{self, Fixed, Key};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use std::time::Instant;
use wtf_core::workspace::Workspace;

pub struct Decodes {
    /// The highlighted decode, the first on screen and how many fit, as last drawn.
    pub hl: usize,
    pub top: usize,
    pub page: usize,
    /// `d` pressed once: a second removes the decode.
    pub d: bool,
}

/// What a key does in the decodes list (`App::DECODES_KEYS`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dec {
    Down,
    Up,
    Records,
    Rows,
    Edit,
    Remove,
    Close,
}

/// A decode's line: its name, protocol, where it is (its prefix, else its clock, else the signals
/// or streams it takes), and its
/// results' and violations' records.
fn line(ws: &Workspace, i: usize) -> (String, String, String, String) {
    let d = &ws.decodes[i];
    let signals = || d.signals.iter().map(|s| s.1.as_str()).collect::<Vec<_>>().join(" ");
    let at = d.params.iter().find(|p| p.0 == "prefix").map(|p| p.1.clone()).or_else(|| (!d.clock.is_empty()).then(|| d.clock.clone())).unwrap_or_else(signals);
    let short = |s: &str| s.strip_prefix(&format!("{}.", d.name)).unwrap_or(s).to_string();
    let streams = ws.results(&d.name);
    let of = |violations: bool| streams.iter().map(|&s| &ws.streams[s]).filter(|s| s.violations == violations && (!violations || !s.records.is_empty())).map(|s| format!("{} {}", short(&s.name), s.records.len())).collect::<Vec<_>>();
    let (ok, bad) = (of(false), of(true));
    let counts = if bad.is_empty() { ok.join("  ") } else { format!("{}  {} {}", ok.join("  "), gl().dot, bad.join("  ")) };
    (d.name.clone(), d.protocol.clone(), at, counts)
}

impl App {
    /// The list's keys (spec §5 "Decodes"): `decodes_key` and the keys page both read them.
    pub const DECODES_KEYS: &Fixed<Dec> = &[
        (&[(Key::ch('j'), Dec::Down), (Key::ch('k'), Dec::Up), (Key::code(KeyCode::Down), Dec::Down), (Key::code(KeyCode::Up), Dec::Up)], "move"),
        (&[(Key::code(KeyCode::Enter), Dec::Records)], "its records"),
        (&[(Key::ch('a'), Dec::Rows)], "add its results as a group"),
        (&[(Key::ch('e'), Dec::Edit)], "edit it in the decode form"),
        (&[(Key::ch('d'), Dec::Remove)], "dd: remove it, its streams and their rows"),
        (&[(Key::code(KeyCode::Esc), Dec::Close), (Key::ch('q'), Dec::Close)], "close"),
    ];

    /// `:decodes`: the list, or why there is none.
    pub fn open_decodes(&mut self) -> Result<(), String> {
        if self.ws.decodes.is_empty() {
            return Err("no decodes yet: Space d d decodes an interface found, Space d D with a form".into());
        }
        self.decodes = Some(Decodes { hl: 0, top: 0, page: 1, d: false });
        Ok(())
    }

    /// The list has the keys; they are fixed.
    pub fn decodes_key(&mut self, k: KeyEvent) {
        let Some(l) = &mut self.decodes else { return };
        let act = keys::fixed(Self::DECODES_KEYS, k);
        let second_d = std::mem::take(&mut l.d) && act == Some(Dec::Remove);
        let last = self.ws.decodes.len().saturating_sub(1);
        let name = self.ws.decodes.get(l.hl).map(|d| d.name.clone()).unwrap_or_default();
        let r = match act {
            None => return,
            Some(Dec::Down) => return l.hl = (l.hl + 1).min(last),
            Some(Dec::Up) => return l.hl = l.hl.saturating_sub(1),
            Some(Dec::Close) => return self.decodes = None,
            Some(Dec::Remove) if !second_d => return l.d = true,
            Some(Dec::Remove) => self.run(&format!("decode {name} off")).map(|()| {
                if self.ws.decodes.is_empty() {
                    self.decodes = None;
                } else if let Some(l) = &mut self.decodes {
                    l.hl = l.hl.min(self.ws.decodes.len() - 1);
                }
            }),
            Some(Dec::Records) => {
                self.decodes = None;
                self.open_browser(&name)
            }
            Some(Dec::Rows) => self.show_decode(&name).map(|added| {
                if !added {
                    self.msg = Some((format!("{name}: its results are shown already"), false));
                }
            }),
            Some(Dec::Edit) => {
                self.decodes = None;
                self.edit_decode(&name)
            }
        };
        self.report(r);
    }

    /// The mouse over the list: the wheel moves, a click highlights, a double click opens the
    /// records, a click outside closes it.
    pub fn decodes_mouse(&mut self, m: MouseEvent, (x, y): (usize, usize), now: Instant) {
        let inside = self.hit.boxed.is_some_and(|r| r.contains((x as u16, y as u16).into()));
        let last = self.ws.decodes.len().saturating_sub(1);
        let Some(l) = &mut self.decodes else { return };
        match m.kind {
            MouseEventKind::ScrollDown => l.hl = (l.hl + 1).min(last),
            MouseEventKind::ScrollUp => l.hl = l.hl.saturating_sub(1),
            MouseEventKind::Down(_) if !inside => self.decodes = None,
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(&(_, k)) = self.hit.records.iter().find(|(ly, _)| *ly == y) else { return };
                l.hl = k;
                if self.double(Target::Record(k), now) {
                    self.decodes_key(KeyEvent::from(KeyCode::Enter));
                }
            }
            _ => {}
        }
    }
}

/// Draws the list: a card over the waveforms, a line per decode in columns, the highlighted one
/// marked.
pub fn draw(app: &mut App, buf: &mut Buffer, w: usize, body: usize) {
    let Some(l) = &mut app.decodes else { return };
    let n = app.ws.decodes.len();
    let (bw, bh) = (w.saturating_sub(8).min(110), (n + 2).min(body.saturating_sub(2)));
    if bw < 40 || bh < 3 {
        return;
    }
    let lines: Vec<_> = (0..n).map(|i| line(&app.ws, i)).collect();
    let (x0, y0) = ((w - bw) / 2, (body - bh) / 2);
    let r = Rect::new(x0 as u16, y0 as u16, bw as u16, bh as u16);
    let acc = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
    let title = vec![(format!("{}decodes", crate::tui::render::icon_text(crate::draw::icon::RECORD)), acc)];
    let keys = if l.d { "d again removes it  Esc keeps it" } else { "Enter records  a add rows  e edit  dd remove  Esc close" };
    crate::tui::render::card_frame(buf, r, pal().dim, &title, &format!("{n} decode{}", if n == 1 { "" } else { "s" }), keys);
    let card = crate::tui::render::card_style();
    // the columns as wide as their widest
    let wide = |f: fn(&(String, String, String, String)) -> &String| lines.iter().map(|x| f(x).chars().count()).max().unwrap_or(0);
    let (nw, pw, aw) = (wide(|x| &x.0), wide(|x| &x.1), wide(|x| &x.2).min(bw / 3));
    l.page = bh - 2;
    if l.hl < l.top {
        l.top = l.hl;
    } else if l.hl >= l.top + l.page {
        l.top = l.hl + 1 - l.page;
    }
    let mut hits = Vec::new();
    for (k, (name, protocol, at, counts)) in lines.iter().enumerate().skip(l.top).take(l.page) {
        let y = y0 + 1 + k - l.top;
        hits.push((y, k));
        let on = k == l.hl;
        let row = if on { card.bg(pal().selected) } else { card };
        put(buf, x0 + 1, y, &" ".repeat(bw - 2), row);
        if on {
            put(buf, x0 + 1, y, &gl().hl.to_string(), row.patch(acc));
        }
        let mut x = x0 + 3;
        let at: String = at.chars().take(aw).collect();
        for (text, st, width) in [(name, row.add_modifier(Modifier::BOLD), nw), (protocol, row.fg(pal().dim), pw), (&at, row.fg(pal().dim), aw)] {
            put(buf, x, y, text, st);
            x += width + 2;
        }
        let room = (x0 + bw - 2).saturating_sub(x);
        put(buf, x, y, &counts.chars().take(room).collect::<String>(), row);
    }
    app.hit.boxed = Some(r);
    app.hit.records = hits;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn app() -> App {
        let t = wtf_core::trace::Trace::open("tests/data/axilite.vcd".as_ref()).unwrap();
        let mut a = App::new(Workspace::new(t, "x".into()));
        a.keys(":decode s axilite clock=aclk prefix=s_axi_\n:decode lat pair request=s.aw response=s.b\n").unwrap();
        a
    }

    fn hl(a: &App) -> usize {
        a.decodes.as_ref().expect("the list").hl
    }

    /// The list (spec §5 "Decodes"): a line per decode; `Enter` its records, `a` its results as
    /// rows, `dd` removes it unless another reads it, `Esc` closes.
    #[test]
    fn lists_and_acts() {
        let t = wtf_core::trace::Trace::open("tests/data/axilite.vcd".as_ref()).unwrap();
        let mut empty = App::new(Workspace::new(t, "x".into()));
        assert_eq!(empty.open_decodes(), Err("no decodes yet: Space d d decodes an interface found, Space d D with a form".into()));
        let mut a = app();
        a.keys(":decodes\n").unwrap();
        insta::assert_snapshot!("decodes", crate::snapshot(&mut a, 100, Some(12)));
        // a key the list has none for does nothing; a screen too small for it leaves it out
        a.keys("z").unwrap();
        assert!(a.decodes.is_some());
        assert!(!crate::snapshot(&mut a, 40, Some(10)).contains("lat"));
        a.keys("jjk").unwrap();
        assert_eq!(hl(&a), 0);
        // dd on s: lat reads it; d then another key does nothing
        a.keys("dd").unwrap();
        assert_eq!(a.msg.as_ref().map(|m| m.0.as_str()), Some("s is read by lat: remove it first"));
        a.keys("djd").unwrap();
        assert_eq!((hl(&a), a.ws.decodes.len()), (1, 2));
        assert!(crate::snapshot(&mut a, 100, Some(12)).contains("d again removes it  Esc keeps it"));
        a.keys("d").unwrap();
        assert_eq!((a.ws.decodes.len(), hl(&a)), (1, 0));
        // a: its results as a group, once
        a.keys("a").unwrap();
        assert!(a.ws.rows.iter().any(|r| matches!(r, wtf_core::rows::Row::Group { name, .. } if name == "s")));
        a.keys("a").unwrap();
        assert_eq!(a.msg.as_ref().map(|m| m.0.as_str()), Some("s: its results are shown already"));
        // Enter: its records
        a.keys("\n").unwrap();
        assert!(a.decodes.is_none() && a.browser.is_some());
        a.keys("\x1b:decodes\nq").unwrap();
        assert!(a.decodes.is_none());
        // the last removed: the list closes, its rows go
        a.keys(":decodes\ndd").unwrap();
        assert!(a.decodes.is_none() && a.ws.decodes.is_empty() && a.ws.rows.is_empty());
    }

    /// `e` opens the decode form on it, its name not taken; `Enter` decodes it again in place,
    /// its rows kept.
    #[test]
    fn edits() {
        let mut a = app();
        a.keys(":decodes\nae").unwrap();
        let f = a.form.as_ref().expect("the form");
        assert_eq!((f.editing.as_deref(), f.at, f.check.clone()), (Some("s"), 1, Ok(())));
        assert!(crate::snapshot(&mut a, 100, Some(16)).contains("edit decode s"));
        let rows = a.ws.rows.len();
        a.keys("\n").unwrap();
        assert!(a.form.is_none());
        assert_eq!((a.ws.rows.len(), a.msg.as_ref().map(|m| m.0.as_str())), (rows, Some("s: decoded again")));
        // a clock that isn't one: the form stays, the old decode too
        a.keys(":decodes\ne\t<C-u>nope\n").unwrap();
        assert!(a.form.is_some() && a.ws.decodes[0].clock == "top.aclk");
        let _ = a.keys("\x1b");
        assert!(a.form.is_none());
        assert_eq!(a.edit_decode("nope"), Err("no decode nope".into()));
    }

    /// Edited into a protocol without the field a frame reads: decoded again, the frame failing
    /// says so (spec §5 "Decodes").
    #[test]
    fn edit_fails_readers() {
        let mut a = app();
        a.keys(":protocol pa roles=valid,ready when=valid && ready a=valid\n:protocol pb roles=valid,ready when=valid && ready b=valid\n").unwrap();
        a.keys(":decode x pa clock=aclk valid=s_axi_wvalid ready=s_axi_wready\n:decode f frame input=x last=a\n").unwrap();
        a.edit_decode("x").unwrap();
        let f = a.form.as_mut().expect("the form");
        f.fields[1].value = "pb".into();
        a.keys("\n").unwrap();
        let (text, error) = a.msg.clone().expect("a message");
        assert!(error && text.starts_with("x: decoded again, but its readers failed: "), "{text}");
    }

    /// The mouse: the wheel moves, a click highlights, a double click opens the records, a click
    /// outside closes the list.
    #[test]
    fn mouse() {
        let mut a = app();
        a.keys(":decodes\n").unwrap();
        crate::snapshot(&mut a, 100, Some(12));
        let ev = |kind, x: usize, y: usize| MouseEvent { kind, column: x as u16, row: y as u16, modifiers: KeyModifiers::NONE };
        let (x, y) = (a.hit.boxed.unwrap().x as usize + 4, a.hit.records[1].0);
        a.decodes_mouse(ev(MouseEventKind::ScrollDown, x, y), (x, y), Instant::now());
        a.decodes_mouse(ev(MouseEventKind::ScrollDown, x, y), (x, y), Instant::now());
        assert_eq!(hl(&a), 1);
        a.decodes_mouse(ev(MouseEventKind::ScrollUp, x, y), (x, y), Instant::now());
        assert_eq!(hl(&a), 0);
        let t0 = Instant::now();
        a.mouse_at(ev(MouseEventKind::Down(MouseButton::Left), x, y), t0);
        assert_eq!(hl(&a), 1);
        a.mouse_at(ev(MouseEventKind::Down(MouseButton::Left), x, y), t0);
        assert!(a.decodes.is_none() && a.browser.is_some());
        a.keys("\x1b:decodes\n").unwrap();
        a.mouse_at(ev(MouseEventKind::Down(MouseButton::Left), 0, 0), t0);
        assert!(a.decodes.is_none());
    }
}
