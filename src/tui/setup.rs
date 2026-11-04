//! The first start (spec §19 "First start"): which glyph set the terminal's font draws, then dark
//! or light, each picked from samples drawn as wtf draws a trace. The answers go to the config file.

use crate::draw::{self, gl, pal, put, Glyphs, Palette};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use wtf_core::command;
use wtf_core::trace::Trace;
use wtf_core::workspace::Workspace;

/// A clock and a bus that starts unknown, for the samples: long enough to fill the widest card
/// at the samples' zoom.
fn sample() -> String {
    let mut v = String::from("$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! clk $end\n$var wire 8 # data $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\nbxxxxxxxx #\n");
    let data = ["10100101", "111100", "11110000", "1111", "10011001"];
    for t in (5..=400).step_by(5) {
        v.push_str(&format!("#{t}\n{}!\n", (t / 5) % 2));
        if t % 20 == 10 {
            v.push_str(&format!("b{} #\n", data[(t / 20) as usize % data.len()]));
        }
    }
    v
}

/// Lines a sample takes: two rows, without the time axis above them.
const SAMPLE_LINES: usize = 2 * draw::ROW_LINES;

pub struct Setup {
    /// Answering the theme question; before, the glyphs one.
    pub theme_step: bool,
    pub sel: usize,
    /// The glyph set picked, an index into `draw::SETS`.
    glyph: usize,
    /// Without colour there is no theme to pick.
    mono: bool,
    themes: Vec<(&'static str, &'static Palette)>,
    /// The theme selected at first: light on a light background.
    suited: usize,
    base: &'static Palette,
    ws: Workspace,
}

/// What the setup chose; `None` when it was left with Ctrl-c.
pub struct Answer {
    pub glyphs: &'static str,
    pub theme: Option<&'static str>,
}

impl Setup {
    /// `bg` is the terminal's background, if it answered: the theme that suits it comes first.
    pub fn new(mono: bool, rgb: bool, bg: Option<crate::theme::Rgb>) -> Setup {
        let leak = |p: Palette| -> &'static Palette { Box::leak(Box::new(p)) };
        let themes = ["dark", "light"].map(|n| (n, leak(crate::theme::builtin_on(n, rgb, bg).expect("built in")))).to_vec();
        let suited = bg.is_some_and(crate::theme::is_light) as usize;
        let base = if mono { leak(Palette::mono()) } else { themes[0].1 };
        let mut ws = Workspace::new(Trace::from_bytes(sample().into_bytes()).expect("the sample is a VCD"), String::new());
        command::run(&mut ws, "add **").expect("the sample has signals");
        // two ticks a column: a clock period is five columns
        command::run(&mut ws, "zoom 2").expect("a zoom");
        ws.view.cursor = 20;
        Setup { theme_step: false, sel: 0, glyph: 0, mono, themes, suited, base, ws }
    }

    fn options(&self) -> usize {
        if self.theme_step { self.themes.len() } else { draw::SETS.len() }
    }

    /// Takes a key; `Some` when the setup is over.
    pub fn key(&mut self, k: KeyEvent) -> Option<Option<Answer>> {
        let ctrl = k.modifiers.contains(crossterm::event::KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('c') if ctrl => return Some(None),
            KeyCode::Char('j') | KeyCode::Down => self.sel = (self.sel + 1) % self.options(),
            KeyCode::Char('k') | KeyCode::Up => self.sel = (self.sel + self.options() - 1) % self.options(),
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < self.options() => {
                self.sel = c as usize - '1' as usize;
                return self.choose();
            }
            KeyCode::Enter => return self.choose(),
            // the defaults for what is left
            KeyCode::Esc => {
                if !self.theme_step {
                    self.glyph = 0;
                }
                self.sel = self.suited;
                return Some(Some(self.answer()));
            }
            _ => {}
        }
        None
    }

    fn choose(&mut self) -> Option<Option<Answer>> {
        if !self.theme_step {
            self.glyph = self.sel;
            if !self.mono {
                (self.theme_step, self.sel) = (true, self.suited);
                return None;
            }
        }
        Some(Some(self.answer()))
    }

    fn answer(&self) -> Answer {
        Answer { glyphs: draw::SETS[self.glyph].0, theme: (!self.mono).then(|| self.themes[self.sel].0) }
    }

    fn glyphs(&self) -> &'static Glyphs {
        draw::SETS[self.glyph].1
    }

    /// The question, then a card per answer (spec §19 "First start"): titled with its key and
    /// name, the sample inside, the highlighted one's border in the accent. Each card is drawn in
    /// its own glyphs (the glyphs question) or theme (the theme question), its frame too, so the
    /// card itself shows whether the terminal draws it cleanly. Two cards a row where they fit.
    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let (w, h) = (area.width as usize, area.height as usize);
        draw::set_palette_ref(self.base);
        draw::set_glyphs(self.glyphs());
        let accent = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(pal().dim);
        let names: Vec<&str> = match self.theme_step {
            false => draw::SETS.iter().map(|(n, _)| *n).collect(),
            true => self.themes.iter().map(|(n, _)| *n).collect(),
        };
        // two cards a row where each gets 34 columns, as wide as 66
        let cols = if w >= 2 * 34 + 6 { 2 } else { 1 };
        let cw = ((w.saturating_sub(4 + 2 * (cols - 1))) / cols).min(66);
        let total = cols * cw + 2 * (cols - 1);
        let x = (w.saturating_sub(total) / 2).max(1);
        put(buf, x, 1, "wtf", accent);
        put(buf, x + 4, 1, &format!("{} first start", gl().dot), dim);
        if !self.mono {
            let step = format!("{} of 2", 1 + self.theme_step as usize);
            put(buf, (x + total).saturating_sub(step.chars().count()), 1, &step, dim);
        }
        let (question, hint) = match self.theme_step {
            false => ("Which of these does your terminal draw cleanly?", "Lines should join up; no character should be a box or a question mark."),
            true => ("Dark or light?", "Pick the one that suits your terminal's background."),
        };
        put(buf, x, 3, question, Style::new().add_modifier(Modifier::BOLD));
        put(buf, x, 4, hint, dim);
        let ch = SAMPLE_LINES + 2;
        let rows = names.len().div_ceil(cols);
        // a blank line between rows of cards where all fit with it
        let gap = (6 + rows * (ch + 1) - 1 <= h.saturating_sub(2)) as usize;
        let sw = cw.saturating_sub(4);
        for (i, name) in names.iter().enumerate() {
            let (cx, cy) = (x + (i % cols) * (cw + 2), 6 + (i / cols) * (ch + gap));
            if cy + ch > h.saturating_sub(2) || sw < 10 {
                break;
            }
            let on = i == self.sel;
            // the card in its own glyphs and palette, then back to the screen's
            let (g, p) = match self.theme_step {
                false => (draw::SETS[i].1, self.base),
                true => (self.glyphs(), self.themes[i].1),
            };
            draw::set_glyphs(g);
            draw::set_palette_ref(p);
            // a set with icons shows one: a font without them draws a box
            let icon = if !self.theme_step && g.icons { " \u{ea83}" } else { "" };
            let mark = if on && pal().mono { format!("{} ", gl().hl) } else { String::new() };
            let title = vec![(format!("{mark}{} {name}{icon}", i + 1), if on { accent } else { Style::new().add_modifier(Modifier::BOLD) })];
            let r = Rect::new(cx as u16, cy as u16, cw as u16, ch as u16);
            super::render::card_frame(buf, r, if on { pal().cursor } else { pal().dim }, &title, "", "");
            let full = Rect::new(0, 0, sw as u16, 1 + SAMPLE_LINES as u16);
            let mut s = Buffer::empty(full);
            draw::rows(&mut self.ws, &mut 0, false, &mut s, sw, 1 + SAMPLE_LINES);
            draw::fill_screen(&mut s);
            draw::set_glyphs(self.glyphs());
            draw::set_palette_ref(self.base);
            for (dy, dx) in (0..SAMPLE_LINES).flat_map(|dy| (0..sw).map(move |dx| (dy, dx))) {
                if let Some(cell) = buf.cell_mut((area.x + (cx + 2 + dx) as u16, area.y + (cy + 1 + dy) as u16)) {
                    *cell = s[(dx as u16, 1 + dy as u16)].clone();
                }
            }
        }
        // the keys, in the accent, what they do dim
        let keys = [(format!("1{}{}", gl().dash, names.len()), " pick   "), ("j k".into(), " move   "), ("Enter".into(), " pick   "), ("Esc".into(), " the defaults")];
        let mut kx = x;
        for (k, what) in keys {
            put(buf, kx, h.saturating_sub(2), &k, accent);
            put(buf, kx + k.chars().count(), h.saturating_sub(2), what, dim);
            kx += k.chars().count() + what.chars().count();
        }
        let again = "wtf --setup asks again";
        if kx + 2 + again.len() <= x + total {
            put(buf, x + total - again.len(), h.saturating_sub(2), again, dim);
        }
        draw::fill_screen(buf);
    }
}

/// Runs the setup in the terminal and writes the answers to the config file. Returns a warning
/// for the status line if the file couldn't be written.
pub fn run(mono: bool, bg: Option<crate::theme::Rgb>) -> Option<String> {
    let mut setup = Setup::new(mono, draw::truecolor(), bg);
    let mut term = ratatui::init();
    let answer = loop {
        if let Err(e) = term.draw(|f| setup.draw(f.area(), f.buffer_mut())) {
            break Err(e.to_string());
        }
        match crossterm::event::read() {
            Ok(crossterm::event::Event::Key(k)) if k.kind == crossterm::event::KeyEventKind::Press => {
                if let Some(a) = setup.key(k) {
                    break Ok(a);
                }
            }
            Ok(_) => {}
            Err(e) => break Err(e.to_string()),
        }
    };
    ratatui::restore();
    let a = match answer {
        Ok(Some(a)) => a,
        Ok(None) => return None,
        Err(e) => return Some(format!("setup: {e}")),
    };
    let dir = crate::config::dir()?;
    let mut keys = vec![("glyphs", a.glyphs)];
    keys.extend(a.theme.map(|t| ("theme", t)));
    crate::config::save(&dir, &keys).err().map(|e| format!("setup: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn press(s: &mut Setup, code: KeyCode) -> Option<Option<Answer>> {
        s.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn screen(s: &mut Setup) -> String {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        s.draw(area, &mut buf);
        draw::text(&buf)
    }

    #[test]
    fn screens() {
        let mut s = Setup::new(false, true, None);
        insta::assert_snapshot!("setup_glyphs", screen(&mut s));
        assert!(press(&mut s, KeyCode::Char('2')).is_none());
        insta::assert_snapshot!("setup_theme", screen(&mut s));
    }

    #[test]
    fn answers() {
        let mut s = Setup::new(false, true, None);
        press(&mut s, KeyCode::Char('j'));
        assert!(press(&mut s, KeyCode::Enter).is_none());
        let a = press(&mut s, KeyCode::Char('2')).unwrap().unwrap();
        assert_eq!((a.glyphs, a.theme), ("box", Some("light")));
        // Esc: the defaults for what is left
        let a = press(&mut Setup::new(false, true, None), KeyCode::Esc).unwrap().unwrap();
        assert_eq!((a.glyphs, a.theme), ("unicode", Some("dark")));
        let mut s = Setup::new(false, true, None);
        press(&mut s, KeyCode::Char('3'));
        let a = press(&mut s, KeyCode::Esc).unwrap().unwrap();
        assert_eq!((a.glyphs, a.theme), ("ascii", Some("dark")));
        // without colour, no theme question
        let a = press(&mut Setup::new(true, true, None), KeyCode::Char('3')).unwrap().unwrap();
        assert_eq!((a.glyphs, a.theme), ("ascii", None));
        assert!(press(&mut Setup::new(false, true, None), KeyCode::Char('5')).is_none());
        // on a light terminal light is selected, and the default
        let mut s = Setup::new(false, true, Some((0xfa, 0xfa, 0xfa)));
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.sel, 1);
        let a = press(&mut Setup::new(false, true, Some((0xfa, 0xfa, 0xfa))), KeyCode::Esc).unwrap().unwrap();
        assert_eq!(a.theme, Some("light"));
        assert!(Setup::new(false, true, None).key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap().is_none());
    }
}
