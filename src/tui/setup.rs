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

/// A clock and a bus that starts unknown, for the samples.
const SAMPLE: &str = "$timescale 1ns $end
$scope module top $end
$var wire 1 ! clk $end
$var wire 8 # data $end
$upscope $end
$enddefinitions $end
#0
0!
bxxxxxxxx #
#5
1!
#10
0!
b10100101 #
#15
1!
#20
0!
#25
1!
#30
0!
b111100 #
#35
1!
#40
0!
b11110000 #
#45
1!
#50
0!
";

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
        let mut ws = Workspace::new(Trace::from_bytes(SAMPLE.as_bytes().to_vec()).expect("the sample is a VCD"), String::new());
        command::run(&mut ws, "add **").expect("the sample has signals");
        ws.view.cursor = 30;
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

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer) {
        let (w, h) = (area.width as usize, area.height as usize);
        draw::set_palette_ref(self.base);
        draw::set_glyphs(self.glyphs());
        let accent = Style::new().fg(pal().cursor).add_modifier(Modifier::BOLD);
        let dim = Style::new().fg(pal().dim);
        let x = 2;
        put(buf, x, 1, "wtf", accent);
        put(buf, x + 4, 1, &format!("{} first start", gl().dot), dim);
        let (question, hint) = match self.theme_step {
            false => ("Which of these does your terminal draw cleanly?", "Lines should join up; no character should be a box or a question mark."),
            true => ("Dark or light?", "Pick the one that suits your terminal's background."),
        };
        put(buf, x, 3, question, Style::new().add_modifier(Modifier::BOLD));
        put(buf, x, 4, hint, dim);
        let names: Vec<&str> = match self.theme_step {
            false => draw::SETS.iter().map(|(n, _)| *n).collect(),
            true => self.themes.iter().map(|(n, _)| *n).collect(),
        };
        let label = 14;
        let sw = w.saturating_sub(x + label + 2).min(64);
        let mut y = 6;
        for (i, name) in names.iter().enumerate() {
            if y + SAMPLE_LINES > h.saturating_sub(2) {
                break;
            }
            let on = i == self.sel;
            let mark = if on { gl().hl } else { ' ' };
            put(buf, x, y + SAMPLE_LINES / 2, &format!("{mark} {} {name}", i + 1), if on { accent } else { Style::new() });
            // the sample in its own glyphs and palette, then back to the screen's
            let (g, p) = match self.theme_step {
                false => (draw::SETS[i].1, self.base),
                true => (self.glyphs(), self.themes[i].1),
            };
            draw::set_glyphs(g);
            draw::set_palette_ref(p);
            // drawn once to learn the width the waveforms get, so `fit` can fill it
            let full = Rect::new(0, 0, sw as u16, 1 + SAMPLE_LINES as u16);
            draw::rows(&mut self.ws, &mut 0, false, &mut Buffer::empty(full), sw, 1 + SAMPLE_LINES);
            command::run(&mut self.ws, "fit").ok();
            let mut s = Buffer::empty(full);
            draw::rows(&mut self.ws, &mut 0, false, &mut s, sw, 1 + SAMPLE_LINES);
            draw::fill_screen(&mut s);
            draw::set_glyphs(self.glyphs());
            draw::set_palette_ref(self.base);
            for (dy, dx) in (0..SAMPLE_LINES).flat_map(|dy| (0..sw).map(move |dx| (dy, dx))) {
                if let Some(cell) = buf.cell_mut((area.x + (x + label + dx) as u16, area.y + (y + dy) as u16)) {
                    *cell = s[(dx as u16, 1 + dy as u16)].clone();
                }
            }
            y += SAMPLE_LINES + 1;
        }
        let keys = format!("1{}{} or j k, Enter to pick {} Esc for the defaults {} wtf --setup asks again", gl().dash, names.len(), gl().dot, gl().dot);
        put(buf, x, h.saturating_sub(2), &keys, dim);
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
        assert!(press(&mut Setup::new(false, true, None), KeyCode::Char('4')).is_none());
        // on a light terminal light is selected, and the default
        let mut s = Setup::new(false, true, Some((0xfa, 0xfa, 0xfa)));
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.sel, 1);
        let a = press(&mut Setup::new(false, true, Some((0xfa, 0xfa, 0xfa))), KeyCode::Esc).unwrap().unwrap();
        assert_eq!(a.theme, Some("light"));
        assert!(Setup::new(false, true, None).key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap().is_none());
    }
}
