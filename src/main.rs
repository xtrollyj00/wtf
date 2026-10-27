mod config;
mod draw;
mod live;
mod mcp;
mod theme;
mod tui;

use tui::app::App;
use tui::render;
use wtf_core::{session, workspace};
use clap::Parser;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "wtf", version, about = "Waveform Terminal Frontend")]
struct Args {
    /// The VCD or FST file to open
    #[arg(required_unless_present_any = ["mcp", "attach"])]
    trace: Option<PathBuf>,
    /// Load this session instead of <trace>.session.toml
    #[arg(short = 'S', value_name = "session")]
    session: Option<PathBuf>,
    /// Render one screen as plain text to stdout and exit
    #[arg(long, value_name = "width[xheight]", value_parser = parse_size)]
    snapshot: Option<(u16, Option<u16>)>,
    /// Open a window on a session (an agent's workspace or another window); without an id, list
    /// the open sessions
    #[arg(long, value_name = "id", conflicts_with = "trace")]
    attach: Option<Option<String>>,
    /// Draw with this glyph set: unicode, box (no legacy computing glyphs) or ascii
    #[arg(long, value_name = "set", value_parser = draw::glyphs_named)]
    glyphs: Option<&'static draw::Glyphs>,
    /// Serve AI agents over MCP on stdin / stdout, e.g. `claude mcp add wtf -- wtf --mcp`
    #[arg(long, conflicts_with_all = ["trace", "session", "snapshot", "attach", "glyphs"])]
    mcp: bool,
}

fn parse_size(s: &str) -> Result<(u16, Option<u16>), String> {
    let num = |x: &str| x.parse::<u16>().ok().filter(|n| *n > 0).ok_or(format!("bad size: {s}"));
    match s.split_once('x') {
        Some((w, h)) => Ok((num(w)?, Some(num(h)?))),
        None => Ok((num(s)?, None)),
    }
}

fn main() {
    let args = Args::parse();
    if let Err(e) = run(args) {
        eprintln!("wtf: {e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    if args.mcp {
        return mcp::serve();
    }
    let config = config::load();
    // --glyphs over the config file over unicode (spec §19 "Precedence")
    draw::set_glyphs(args.glyphs.unwrap_or(config.glyphs));
    let mut warnings = config.warnings;
    let (path, shared, attached) = match args.attach {
        Some(None) => {
            let list: Vec<String> = live::sessions()?.iter().map(|s| format!("{}  {}  {}", s.id, s.kind, s.trace.display())).collect();
            println!("{}", if list.is_empty() { "no sessions are open".into() } else { list.join("\n") });
            return Ok(());
        }
        Some(Some(id)) => {
            let s = live::sessions()?.into_iter().find(|s| s.id == id).ok_or(format!("no session {id}; `wtf --attach` lists them"))?;
            let answer = live::request(&id, live::Request::Get)?;
            let text = answer.split_once("\n---\n").map_or(String::new(), |(_, t)| t.to_string());
            (s.trace, Some(text), Some(id))
        }
        None => (args.trace.expect("clap requires it"), None, None),
    };
    let mut app = App::new(workspace::Workspace::open(&path)?);

    // -S may name a file that doesn't exist yet: that starts an empty session, and :w creates it.
    let session = args.session.unwrap_or_else(|| session::default_path(&path));
    app.ws.session_path = session.clone();
    if session.exists() {
        let text = std::fs::read_to_string(&session).map_err(|e| format!("{}: {e}", session.display()))?;
        warnings.extend(session::apply(&mut app.ws, &text)?);
    }
    if !warnings.is_empty() {
        app.msg = Some((warnings.join("; "), true));
    }
    // a copy of the session on top of the usual one, as the agent's `present` does: u goes back
    if let Some(text) = shared {
        app.present(&text)?;
        app.follow = attached.map(|id| tui::app::Follow::new(&id, &text));
    }

    match args.snapshot {
        Some((w, h)) => {
            print!("{}", snapshot(&mut app, w, h));
            Ok(())
        }
        None => {
            // as no-color.org says: set and not empty
            let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
            draw::set_palette(if no_color { draw::Palette::mono() } else { config.palette });
            tui(&mut app)
        }
    }
}

/// Without a height, as tall as the content: the rows (or the guide) plus the status line.
fn snapshot(app: &mut App, w: u16, h: Option<u16>) -> String {
    let h = h.unwrap_or(if app.ws.rows.is_empty() { 7 } else { draw::height(&app.ws) + 1 });
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    render::draw(app, area, &mut buf);
    draw::text(&buf)
}

fn tui(app: &mut App) -> Result<(), String> {
    // the agent's way in (`wtf --mcp`); without it the window works as before, and says why
    let live = match live::window(&std::process::id().to_string(), &app.ws.trace_path) {
        Ok(l) => Some(l),
        Err(e) => {
            app.msg = Some((format!("no link for agents: {e}"), true));
            None
        }
    };
    // the agent's session, read every half second while attached, each change sent on
    let agent = app.follow.as_ref().map(|f| {
        let (tx, rx) = std::sync::mpsc::channel();
        let id = f.id.clone();
        std::thread::spawn(move || {
            let mut last = String::new();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let now = live::request(&id, live::Request::Get).map(|a| a.split_once("\n---\n").map_or(String::new(), |(_, t)| t.to_string()));
                let gone = now.is_err();
                if now.as_ref().is_ok_and(|t| *t == last) {
                    continue;
                }
                if let Ok(t) = &now {
                    last = t.clone();
                }
                if tx.send(now).is_err() || gone {
                    return;
                }
            }
        });
        rx
    });
    let mut term = ratatui::init();
    let res = (|| -> std::io::Result<()> {
        while !app.quit {
            term.draw(|f| {
                if let Some((x, y)) = render::draw(app, f.area(), f.buffer_mut()) {
                    f.set_cursor_position((x, y));
                }
            })?;
            // wait for a key, answering the agent in between
            while !crossterm::event::poll(std::time::Duration::from_millis(50))? {
                if let Some(inc) = live.as_ref().and_then(|(_, rx)| rx.try_recv().ok()) {
                    let answer = match inc.request {
                        live::Request::Get => Ok(mcp::link_answer(&app.ws)),
                        live::Request::Put(text) => app.present(&text),
                    };
                    let _ = inc.reply.send(answer);
                    break;
                }
                if app.watch(tui::app::stamp(&app.ws.trace_path), std::time::Instant::now()) {
                    break;
                }
                if let Some(now) = agent.as_ref().and_then(|rx| rx.try_recv().ok())
                    && app.agent_changed(now)
                {
                    break;
                }
            }
            if !crossterm::event::poll(std::time::Duration::ZERO)? {
                continue;
            }
            // handle every event already waiting before drawing again, so held keys (and slow
            // links like ssh) don't queue up frames that are stale by the time they arrive
            let mut ev = crossterm::event::read()?;
            loop {
                if let crossterm::event::Event::Key(k) = ev
                    && k.kind == crossterm::event::KeyEventKind::Press
                {
                    app.key(k);
                }
                // to the terminal's clipboard; one that can't take it ignores the sequence
                if let Some(text) = app.yank.take() {
                    use crossterm::clipboard::{ClipboardSelection, ClipboardType, CopyToClipboard};
                    crossterm::execute!(std::io::stdout(), CopyToClipboard { content: text, destination: ClipboardSelection(vec![ClipboardType::Clipboard]) })?;
                }
                if app.quit || !crossterm::event::poll(std::time::Duration::ZERO)? {
                    break;
                }
                ev = crossterm::event::read()?;
            }
        }
        Ok(())
    })();
    ratatui::restore();
    res.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app;
    use wtf_core::{radix, rows, trace};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use crate::tui::commands;
    use ratatui::style::Color;

    fn open(ext: &str, session: Option<&str>) -> App {
        let t = trace::Trace::open(format!("tests/data/simple.{ext}").as_ref()).unwrap();
        let mut app = App::new(workspace::Workspace::new(t, "simple".into()));
        if let Some(s) = session {
            assert!(session::apply(&mut app.ws, &std::fs::read_to_string(s).unwrap()).unwrap().is_empty());
        }
        app
    }

    fn keys(app: &mut App, s: &str) {
        for c in s.chars() {
            let k = match c {
                '\n' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                '\x1b' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                '\x08' => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                // other control characters: Ctrl and the letter, e.g. \x01 is Ctrl-a
                c if c < ' ' => KeyEvent::new(KeyCode::Char((c as u8 + b'a' - 1) as char), KeyModifiers::CONTROL),
                c => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            };
            app.key(k);
        }
    }

    const SESSION: &str = "tests/data/simple.session.toml";

    #[test]
    fn guide() {
        insta::assert_snapshot!(snapshot(&mut open("vcd", None), 60, None));
    }

    #[test]
    fn session_view() {
        insta::assert_snapshot!(snapshot(&mut open("vcd", Some(SESSION)), 100, None));
    }

    #[test]
    fn vcd_equals_fst() {
        for w in [40, 100, 300] {
            let a = snapshot(&mut open("vcd", Some(SESSION)), w, None);
            assert_eq!(a, snapshot(&mut open("fst", Some(SESSION)), w, None), "width {w}");
        }
    }

    #[test]
    fn unit_is_for_agents() {
        let mut app = open("vcd", None);
        keys(&mut app, ":unit ns\n");
        assert!(app.msg.as_ref().unwrap().0.starts_with("unit sets the time unit of an agent's answers"));
        assert_eq!(app.ws.unit, None);
    }

    #[test]
    fn add_walk_zoom() {
        let mut app = open("vcd", None);
        keys(&mut app, ":add top.cpu\n");
        assert_eq!(app.ws.rows.len(), 4); // a group: header and three signals
        keys(&mut app, ":add top.cpu.*\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "already shown: top.cpu.*");
        keys(&mut app, "kww");
        assert_eq!(app.ws.view.cursor, 20);
        keys(&mut app, "3z");
        insta::assert_snapshot!(snapshot(&mut app, 60, None));
        keys(&mut app, "b");
        assert_eq!(app.ws.view.cursor, 10);
        keys(&mut app, "G");
        assert_eq!(app.ws.view.cursor, 80);
        keys(&mut app, "gg0");
        assert_eq!(app.ws.view.cursor, 0);
        keys(&mut app, "5l");
        assert_eq!(app.ws.view.cursor, 5);
        keys(&mut app, ":rm\n:rm top.cpu.state\n");
        assert_eq!(app.ws.rows.len(), 2);
        keys(&mut app, ":rm\n:rm\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "no rows");
        keys(&mut app, ":add top.cpu\n");
        keys(&mut app, ":bogus\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "unknown command: bogus");
        keys(&mut app, ":42ns\n");
        assert_eq!(app.ws.view.cursor, 42);
        keys(&mut app, "q");
        assert_eq!(app.msg.as_ref().unwrap().0, "unsaved session (:w to save, :q! to quit)");
        keys(&mut app, ":q!\n");
        assert!(app.quit);
    }

    #[test]
    fn paging() {
        let mut app = open("vcd", None);
        keys(&mut app, ":add top.cpu\n:add top.clk\n:zoom 1ns\n");
        snapshot(&mut app, 40, None); // sets the width: 40 columns less the names, 1 tick each
        let half = app.ws.view.width / 2;
        keys(&mut app, "\x04");
        assert_eq!((app.ws.view.start, app.ws.view.cursor), (half, half));
        keys(&mut app, "\x15\x15");
        assert_eq!((app.ws.view.start, app.ws.view.cursor), (0, 0));
        // the screen alone: the cursor keeps its time, off screen too
        keys(&mut app, "2L");
        assert_eq!((app.ws.view.start, app.ws.view.cursor), (2 * half, 0));
        keys(&mut app, "H");
        assert_eq!(app.ws.view.start, half);
        // L stops with the end at the left edge
        keys(&mut app, "99L");
        assert_eq!((app.ws.view.start, app.ws.view.cursor), (app.ws.view.end, 0));
        keys(&mut app, "99H");
        assert_eq!(app.ws.view.start, 0);
        keys(&mut app, "2L");
        keys(&mut app, "5lc");
        assert_eq!(app.ws.view.cursor, 5);
        assert_eq!(app.ws.view.start, 0);
        keys(&mut app, ":30ns\nc");
        assert_eq!(app.ws.view.start, 30 - app.ws.view.width / 2);
        // rows: a screen down, clamped at the last
        keys(&mut app, "\x06");
        assert_eq!(app.ws.sel, app.ws.rows.len() - 1);
        keys(&mut app, "\x02");
        assert!(app.ws.sel < app.ws.rows.len() - 1);
        // other Ctrl keys do nothing, not their letter
        keys(&mut app, "\x0c");
        assert_eq!(app.ws.view.cursor, 30);
    }

    #[test]
    fn collapsed_group() {
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, "j ");
        insta::assert_snapshot!(snapshot(&mut app, 60, None));
        keys(&mut app, "w");
        assert_eq!(app.ws.view.cursor, 20); // the next change of any signal in the group, after 12ns
        keys(&mut app, "bb");
        assert_eq!(app.ws.view.cursor, 10); // the group's first change, then it stops
        keys(&mut app, "j");
        assert_eq!(app.msg, None);
        assert!(matches!(&app.ws.rows[app.ws.sel], rows::Row::Signal(s) if app.ws.trace.vars[s.var].path == "top.dma.state"));
        keys(&mut app, "k ");
        assert_eq!(rows::visible(&app.ws.rows).len(), 6);
    }

    fn names(app: &App) -> String {
        wtf_core::rows::display_names(&app.ws.trace, &app.ws.rows).join(",")
    }

    #[test]
    fn edit_rows() {
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, "iJ");
        assert_eq!(names(&app), "▾ cpu,  clk,  cpu.state,  data,  busy,dma.state");
        keys(&mut app, "jVjj:group g\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "groups can't be nested yet");
        keys(&mut app, "\x1bjjj:group g\n");
        assert!(app.mode == app::Mode::Edit);
        assert_eq!(names(&app), "▾ cpu,  clk,  cpu.state,  data,  busy,▾ g,  dma.state");
        keys(&mut app, "kkr");
        assert_eq!(app.msg, None);
        assert!(matches!(&app.ws.rows[3], rows::Row::Signal(s) if radix::RADIXES[s.radix].name == "bin"));
        keys(&mut app, "kddkdd");
        assert_eq!(names(&app), "▾ cpu,  data,  busy,▾ g,  state");
        keys(&mut app, "uuu");
        assert_eq!(names(&app), "▾ cpu,  clk,  cpu.state,  data,  busy,▾ g,  dma.state");
        assert!(matches!(&app.ws.rows[3], rows::Row::Signal(s) if radix::RADIXES[s.radix].name == "hex"));
        keys(&mut app, ":redo\n9jkdd");
        assert_eq!(names(&app), "▾ cpu,  clk,  state,  data,  busy");
        assert!(matches!(&app.ws.rows[3], rows::Row::Signal(s) if radix::RADIXES[s.radix].name == "bin"));
        keys(&mut app, "\x1b");
        assert!(app.mode == app::Mode::View);
    }

    #[test]
    fn edit_panel() {
        let mut app = open("vcd", None);
        keys(&mut app, "ijl\n");
        assert_eq!(names(&app), "▾ cpu,  state,  data,  busy");
        keys(&mut app, "\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "already shown: top.cpu");
        keys(&mut app, "jjjjlj\n");
        assert_eq!(names(&app), "▾ cpu,  cpu.state,  data,  busy,  dma.state");
        keys(&mut app, "/c l\x08\x08lk\n\n");
        insta::assert_snapshot!(snapshot(&mut app, 80, Some(14)));
        for (w, h) in [(1, 1), (20, 3), (30, 4)] {
            snapshot(&mut app, w, Some(h)); // tiny screens draw without panicking
        }
        keys(&mut app, "/rst\x01\n\n\n");
        assert_eq!(names(&app), "▾ cpu,  cpu.state,  data,  busy,  dma.state,  clk,  rst");
        keys(&mut app, "/STATE\nn");
        assert_eq!(app.panel.nodes[app.panel.sel].path, "top.dma.state");
        keys(&mut app, "N/nope\n");
        assert_eq!(app.panel.nodes[app.panel.sel].path, "top.cpu.state");
        assert_eq!(app.msg.as_ref().unwrap().0, "not found: nope");
        keys(&mut app, "\x1b");
        assert!(!app.adding && app.mode == app::Mode::Edit);
        keys(&mut app, "u\x12\x1b");
        assert_eq!(names(&app), "▾ cpu,  cpu.state,  data,  busy,  dma.state,  clk,  rst");
        assert!(app.mode == app::Mode::View);
    }

    #[test]
    fn computed_var_draws() {
        // a computed var draws as a file's does: a bus with its values, and its bits
        let mut app = open("vcd", None);
        let b = |s: &str| trace::Val::Bits(s.into());
        app.ws.trace.add_computed("calc.mode", Some(4), vec![(0, b("0000")), (25, b("0011")), (55, b("1010"))]).unwrap();
        keys(&mut app, ":add calc.mode\n:add top.clk\n");
        app.ws.sel = 0;
        keys(&mut app, " ");
        insta::assert_snapshot!(snapshot(&mut app, 80, None));
    }

    #[test]
    fn named_signals_join_the_panel() {
        let mut app = open("vcd", None);
        keys(&mut app, ":signal hi = data[7:4]\n");
        snapshot(&mut app, 80, None);
        assert!(app.panel.nodes.iter().any(|n| n.var == Some(app.ws.trace.by_path["hi"])));
    }

    #[test]
    fn mark_and_group() {
        let mut app = open("vcd", None);
        keys(&mut app, "ijl  j j ");
        assert_eq!(app.panel.marked.len(), 2); // Space on cpu marked and unmarked its three signals
        insta::assert_snapshot!(snapshot(&mut app, 80, Some(12)));
        keys(&mut app, "\x07\x08\x08\x08pair\n");
        assert_eq!(names(&app), "▾ pair,  state,  data");
        assert!(app.mode == app::Mode::Edit && app.adding);
        assert!(app.panel.marked.is_empty());
        keys(&mut app, "jj j \n");
        assert_eq!(names(&app), "▾ pair,  cpu.state,  data,  dma.state,  clk"); // Enter: rows, joining the group
        keys(&mut app, "uuu");
        assert!(app.ws.rows.is_empty());
        // V: a range in the tree; the closed dma scope in it brings its signal
        keys(&mut app, "ggjj"); // top.cpu.state
        keys(&mut app, "Vjjj\n");
        assert_eq!(names(&app), "cpu.state,data,busy,dma.state");
        keys(&mut app, "GkVj \x07\n");
        assert_eq!(names(&app), "cpu.state,data,busy,dma.state,▾ top,  clk,  rst");
        keys(&mut app, "u");
        keys(&mut app, "\x1b\x1b");
        assert!(!app.adding);
    }

    #[test]
    fn expand_bus() {
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, "jjj ");
        assert!(matches!(&app.ws.rows[3], rows::Row::Signal(s) if s.bits == 8));
        keys(&mut app, "8j");
        assert_eq!(app.ws.bit, Some(0)); // data[0], the last bit line
        insta::assert_snapshot!(snapshot(&mut app, 80, Some(30)));
        keys(&mut app, "w");
        assert_eq!(app.ws.view.cursor, 20); // data[0] changes at 20 (0x43 -> 0x96)
        keys(&mut app, "kkkk2w");
        assert_eq!(app.ws.bit, Some(4));
        assert_eq!(app.ws.view.cursor, 40); // data[4] changes at 30 (0x96 -> 0xE1) and 40 (-> 0x3A)
        // the bits move with their bus, and expanding is not an undo step
        keys(&mut app, "iK");
        assert!(matches!(&app.ws.rows[2], rows::Row::Signal(s) if s.bits == 8));
        assert_eq!(app.ws.bit, Some(4));
        keys(&mut app, "u");
        assert!(matches!(&app.ws.rows[3], rows::Row::Signal(s) if s.bits == 8));
        keys(&mut app, " ");
        assert!(matches!(&app.ws.rows[3], rows::Row::Signal(s) if s.bits == 0) && app.ws.bit.is_none());
        keys(&mut app, "u");
        assert_eq!(app.msg.as_ref().unwrap().0, "already at oldest change");
    }

    #[test]
    fn follow_the_agent() {
        let mut app = open("vcd", None);
        let msg = |app: &App| app.msg.as_ref().map_or(String::new(), |m| m.0.clone());
        keys(&mut app, ":follow\n");
        assert_eq!(msg(&app), "not attached to an agent's session: wtf --attach <id> follows one");
        // what the agent shows: first its cpu group, then the dma signals too
        let mut agent = open("vcd", None);
        keys(&mut agent, ":add top.cpu\n:30ns\n");
        let first = session::to_toml(&agent.ws);
        app.present(&first).unwrap();
        app.follow = Some(tui::app::Follow::new("4123-1", &first));
        assert!(snapshot(&mut app, 100, None).contains("following 4123-1"));
        assert!(!app.agent_changed(Ok(first.clone()))); // no change, nothing to do
        keys(&mut app, "5l");
        assert_eq!(app.ws.view.cursor, 35); // the user's moves stay until the agent's next change
        keys(&mut agent, ":add top.dma.state\n");
        let second = session::to_toml(&agent.ws);
        assert!(app.agent_changed(Ok(second.clone())));
        assert_eq!((app.ws.rows.len(), app.ws.view.cursor), (5, 30));
        assert!(msg(&app).starts_with("agent: presented"));
        // each change is one undo step
        app.ws.undo().unwrap();
        assert_eq!(app.ws.rows.len(), 4);
        app.ws.redo().unwrap();
        assert_eq!(app.ws.rows.len(), 5);
        // off: changes wait; on again shows the newest
        keys(&mut app, ":follow off\n");
        assert_eq!(msg(&app), "not following 4123-1");
        app.msg = None;
        assert!(!snapshot(&mut app, 100, None).contains("following"));
        assert!(!app.agent_changed(Ok(first.clone())));
        assert_eq!(app.ws.rows.len(), 5);
        keys(&mut app, ":follow\n");
        assert_eq!(app.ws.rows.len(), 4);
        // the session gone: the window stays as it is
        assert!(app.agent_changed(Err("no session 4123-1".into())));
        assert_eq!((app.follow.is_none(), msg(&app)), (true, "not following 4123-1: no session 4123-1".into()));
        assert_eq!(app.ws.rows.len(), 4);
    }

    #[test]
    fn copy() {
        let mut app = open("vcd", Some(SESSION));
        // data: 0x96 @20
        keys(&mut app, ":25ns\njjjyy");
        assert_eq!(app.yank.take().as_deref(), Some("0x96"));
        assert_eq!(app.msg.as_ref().unwrap().0, "copied: 0x96");
        keys(&mut app, "yt");
        assert_eq!(app.yank.take().as_deref(), Some("25ns"));
        keys(&mut app, "kkyy");
        assert!(matches!(app.ws.rows[app.ws.sel], rows::Row::Group { .. }));
        assert_eq!((app.yank.take(), app.msg.as_ref().unwrap().0.as_str()), (None, "select a signal to copy its value"));
        // on a stream's row, its record at the cursor
        let t = trace::Trace::open("tests/data/axis.vcd".as_ref()).unwrap();
        let mut app = App::new(workspace::Workspace::new(t, "axis".into()));
        keys(&mut app, ":decode rx axis clock=clk tvalid=tvalid tready=tready tdata=tdata tlast=tlast tkeep=tkeep\n:add rx.frame\n:70ns\nyy");
        assert_eq!(app.yank.take().as_deref(), Some("rx.frame #2 65ns–85ns beats=0x2 len=0x8 data=c0 00 00 00 c1 00 00 00 complete=1"));
    }

    #[test]
    fn reload_when_the_file_changes() {
        let mut app = open("vcd", Some(SESSION));
        app.ws.trace_path = "tests/data/simple.vcd".into();
        let t0 = std::time::Instant::now();
        let at = |s: u64| t0 + std::time::Duration::from_millis(s);
        let seen = tui::app::stamp(&app.ws.trace_path);
        let new = |n: u64| Some((std::time::SystemTime::UNIX_EPOCH, n));
        assert!(!app.watch(seen, at(0)));
        // changed, then still changing: wait for it to stay the same a second
        assert!(!app.watch(new(1), at(0)));
        assert!(!app.watch(new(2), at(900)));
        assert!(!app.watch(new(2), at(1800)));
        assert!(app.watch(new(2), at(1900)));
        assert_eq!(app.msg.as_ref().unwrap().0, "reloaded");
        // reloaded: the file as it is now is the one seen
        assert!(!app.watch(tui::app::stamp(&app.ws.trace_path), at(5000)));
        // gone (a run deleting it first) is not a change
        assert!(!app.watch(None, at(6000)));
        // a change that can't be read keeps the old trace and says why
        let before = session::to_toml(&app.ws);
        app.ws.trace_path = "tests/data/gone.vcd".into();
        assert!(!app.watch(new(3), at(7000)));
        assert!(app.watch(new(3), at(8000)));
        assert!(app.msg.as_ref().unwrap().1, "{:?}", app.msg);
        assert_eq!(session::to_toml(&app.ws), before);
        assert_eq!(app.ws.trace.end(), 80);
    }

    #[test]
    fn save_and_reload() {
        let dir = std::env::temp_dir().join(format!("wtf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.toml");
        let mut app = open("fst", Some(SESSION));
        app.ws.trace_path = "tests/data/simple.fst".into();
        assert!(!app.ws.modified());
        // layout changes mark the session modified; undoing them clears it again
        keys(&mut app, "jjjir\x1b");
        assert!(app.ws.modified());
        keys(&mut app, "iu\x1b");
        assert!(!app.ws.modified());
        keys(&mut app, "ir\x1bk k 3z:30ns\n");
        keys(&mut app, &format!(":w {}\n", file.display()));
        assert_eq!(app.msg.as_ref().unwrap().0, format!("written: {}", file.display()));
        assert!(!app.ws.modified() && app.ws.session_path == file);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("radix = \"bin\"") && text.contains("expanded = true") && text.contains("collapsed = true"), "{text}");
        // the file reads back to the same session
        let mut back = open("fst", Some(file.to_str().unwrap()));
        assert_eq!(session::to_toml(&back.ws), session::to_toml(&app.ws));
        app.msg = None;
        back.ws.sel = app.ws.sel; // the selection is not part of a session
        assert_eq!(snapshot(&mut back, 100, None), snapshot(&mut app, 100, None));
        // :e keeps everything; a file that can't be read changes nothing
        keys(&mut app, ":e\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "reloaded");
        assert!(!app.ws.modified());
        assert_eq!(session::to_toml(&back.ws), session::to_toml(&app.ws));
        app.ws.trace_path = dir.join("gone.fst");
        keys(&mut app, ":e\n");
        assert!(app.msg.as_ref().unwrap().1);
        assert_eq!(session::to_toml(&back.ws), session::to_toml(&app.ws));
        // a failed save leaves no file behind and keeps [+]
        keys(&mut app, "idd\x1b");
        keys(&mut app, &format!(":w {}\n", dir.join("no/such/dir.toml").display()));
        assert!(app.msg.as_ref().unwrap().1 && app.ws.modified());
        keys(&mut app, ":wq\n");
        assert!(app.quit && !app.ws.modified());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_across_time() {
        // handshake.vcd: valid 65 - 85 dropped without ready, data changed at 75 while stalled
        let t = trace::Trace::open("tests/data/handshake.vcd".as_ref()).unwrap();
        let mut app = App::new(workspace::Workspace::new(t, "handshake".into()));
        let msg = |app: &App| app.msg.as_ref().map_or(String::new(), |m| m.0.clone());
        keys(&mut app, "/@clk fell(valid) && !past(ready)\n");
        assert_eq!(app.ws.view.cursor, 85);
        keys(&mut app, "n");
        assert_eq!((app.ws.view.cursor, msg(&app)), (85, "not found after 85ns: @clk fell(valid) && !past(ready)".into()));
        keys(&mut app, ":0\n/@clk past(valid) && !past(ready) && !stable(data[3:0] )\n");
        assert_eq!(app.ws.view.cursor, 75);
        keys(&mut app, "/rose(valid\n");
        assert_eq!(msg(&app), "missing )");
    }

    #[test]
    fn value_search() {
        let mut app = open("vcd", Some(SESSION));
        let msg = |app: &App| app.msg.as_ref().map_or(String::new(), |m| m.0.clone());
        // data: x, 0x43 @10, 0x96 @20, 0xE1 @30, 0x3A..0x7F @40..44, z @50, 0x40 @60
        keys(&mut app, "jjjgg/0x96\n");
        assert_eq!(app.ws.view.cursor, 20);
        keys(&mut app, "/64\n");
        assert_eq!(app.ws.view.cursor, 60);
        keys(&mut app, "N");
        assert_eq!((app.ws.view.cursor, msg(&app)), (60, "not found before 60ns: 64".into()));
        keys(&mut app, "/z\nN");
        assert_eq!(app.ws.view.cursor, 50);
        // n / N search the row selected now: cpu.state was 0b10 from 20ns
        keys(&mut app, "k/0b10\n");
        assert_eq!(msg(&app), "not found after 50ns: 0b10");
        keys(&mut app, "N");
        assert_eq!(app.ws.view.cursor, 20);
        keys(&mut app, "/0x10\n");
        assert_eq!(msg(&app), "0x10 doesn't fit in 3 bits");
        keys(&mut app, "k/1\n");
        assert_eq!(msg(&app), "select a signal to search");
        // expressions name signals as in the name column, by path or by the end of a path
        keys(&mut app, ":0\n/busy && data == 0x96\n");
        assert_eq!(app.ws.view.cursor, 20);
        keys(&mut app, "/@clk busy\n");
        assert_eq!(app.ws.view.cursor, 25);
        keys(&mut app, "n");
        assert_eq!((app.ws.view.cursor, msg(&app)), (25, "not found after 25ns: @clk busy".into()));
        keys(&mut app, "N");
        assert_eq!((app.ws.view.cursor, msg(&app)), (15, "match 1 of 2".into()));
        keys(&mut app, "n");
        assert_eq!(msg(&app), "match 2 of 2");
        // another search counts its own matches
        keys(&mut app, ":0\n/busy\n");
        assert_eq!(msg(&app), "match 1 of 1"); // where busy becomes true, not each cycle
        keys(&mut app, ":0\n/data[7] && top.cpu.busy\n");
        assert_eq!(app.ws.view.cursor, 20);
        keys(&mut app, "/state == 1\n");
        assert_eq!(msg(&app), "ambiguous: state (2 signals: top.cpu.state, top.dma.state)");
        keys(&mut app, "/rst || nope\n");
        assert_eq!(msg(&app), "unknown signal: nope");
        keys(&mut app, "/k\n");
        assert_eq!(msg(&app), "unknown signal: k");
        // earlier searches come back with Up, as on the command line
        keys(&mut app, "/");
        app.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.line, "k");
        app.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.line, "rst || nope");
        keys(&mut app, "\x1b");
        assert!(app.mode == app::Mode::View);
    }

    #[test]
    fn select_command() {
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, ":select dma.state\n:select cpu.state\n");
        assert_eq!((app.ws.sel, app.ws.anchor), (2, None));
        // a range, then grouped: the commands an agent uses for V and :group
        keys(&mut app, ":select top.clk nope\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "unknown signal: nope");
        keys(&mut app, ":select cpu.state data\n");
        assert_eq!((app.ws.sel, app.ws.anchor), (3, Some(2)));
        keys(&mut app, ":select top.clk dma.state\n:group edge\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "groups can't be nested yet");
        keys(&mut app, ":select top.dma.state\n:group edge\n");
        assert_eq!(names(&app), "clk,▾ cpu,  cpu.state,  data,  busy,▾ edge,  dma.state");
        keys(&mut app, ":select cpu\n:radix dec\n:select rst\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "not shown: rst");
    }

    #[test]
    fn line_editing() {
        let mut app = open("vcd", Some(SESSION));
        let key = |app: &mut App, code| app.key(KeyEvent::new(code, KeyModifiers::NONE));
        keys(&mut app, "/busy && data == 0x40");
        for _ in 0..15 {
            key(&mut app, KeyCode::Left);
        }
        keys(&mut app, "\x17"); // Ctrl-w: the word before the cursor, `busy `
        assert_eq!((app.line.as_str(), app.cursor()), ("&& data == 0x40", 0));
        keys(&mut app, "valid ");
        key(&mut app, KeyCode::End);
        keys(&mut app, "\x08\x080");
        key(&mut app, KeyCode::Home);
        key(&mut app, KeyCode::Delete);
        key(&mut app, KeyCode::Right);
        keys(&mut app, "é");
        assert_eq!((app.line.as_str(), app.cursor()), ("aélid && data == 0x0", 2));
        keys(&mut app, "\x05\x15"); // Ctrl-e, Ctrl-u
        assert_eq!(app.line, "");
        keys(&mut app, "\x1b");
    }

    #[test]
    fn cursors() {
        let mut app = open("vcd", Some(SESSION));
        let msg = |app: &App| app.msg.as_ref().map_or(String::new(), |m| m.0.clone());
        let at = |app: &App| app.ws.cursor_list().iter().map(|&(i, t)| format!("{}:{t}", i + 1)).collect::<Vec<_>>().join(" ");
        // measure: the cursor at 12, a mark there, the cursor on to data's next changes, mark 3 at the first
        keys(&mut app, "M");
        assert_eq!(msg(&app), "no mark under the cursor");
        keys(&mut app, "jjjmw3mw");
        assert_eq!(at(&app), "1:30 2:12 3:20");
        insta::assert_snapshot!(snapshot(&mut app, 80, None));
        // Tab jumps to the marks in time, wrapping around; 'N to one
        let tab = |app: &mut App, code| app.key(KeyEvent::new(code, KeyModifiers::NONE));
        tab(&mut app, KeyCode::Tab);
        assert_eq!(app.ws.view.cursor, 12);
        tab(&mut app, KeyCode::BackTab);
        assert_eq!(app.ws.view.cursor, 20);
        // after a jump, moving moves the cursor: the mark stays where it was put
        keys(&mut app, "'2h");
        assert_eq!(at(&app), "1:11 2:12 3:20");
        keys(&mut app, "'7");
        assert_eq!(msg(&app), "no cursor 7");
        keys(&mut app, "1m");
        assert_eq!(msg(&app), "cursor 1 is the cursor; marks are 2-9");
        // removing keeps the other numbers; a new mark takes the lowest free one
        keys(&mut app, "3Mm");
        assert_eq!(at(&app), "1:11 2:12 3:11");
        keys(&mut app, "M");
        assert_eq!(at(&app), "1:11 2:12");
        keys(&mut app, "mmmmmmmm");
        assert_eq!(msg(&app), "all 8 marks in use");
        // :cursor places and removes them by number, as an agent would
        let time = |app: &App, n: usize| app.ws.cursor_list().iter().find(|c| c.0 == n - 1).map(|c| c.1);
        keys(&mut app, ":cursor 9 50ns\n:cursor 2 off\n:cursor 1 5\n");
        assert_eq!((time(&app, 9), time(&app, 2), time(&app, 1)), (Some(50), None, Some(5)));
        keys(&mut app, ":cursor 9 off\n:cursor 1 off\n");
        assert_eq!(msg(&app), "cursor 1 can't be removed");
        // they are saved and come back
        let text = session::to_toml(&app.ws);
        let mut back = open("vcd", None);
        assert!(session::apply(&mut back.ws, &text).unwrap().is_empty());
        assert_eq!((at(&back), back.ws.view.cursor), (at(&app), 5));
    }

    #[test]
    fn jumps_to_marks() {
        let mut app = open("vcd", None);
        app.ws.view.set_width(20);
        app.ws.view.set_zoom(1.0);
        // as an agent presents them: the cursor on one event, marks on others
        app.ws.present("version = 1\n[cursors]\n1 = \"10\"\n2 = \"30\"\n3 = \"70\"\n[[rows]]\nsignal = \"top.clk\"\n").unwrap();
        app.ws.view.set_cursor(10);
        app.ws.view.start = 0;
        keys(&mut app, "''");
        assert_eq!(app.msg.as_ref().unwrap().0, "no jump to go back from");
        // '2 centres on mark 2, though it was on screen; looking around moves cursor 1 only
        keys(&mut app, "'2");
        assert_eq!((app.ws.view.cursor, app.ws.view.col_of(30)), (30, Some(10)));
        keys(&mut app, "ll");
        assert_eq!(app.ws.cursor_list(), [(0, 32), (1, 30), (2, 70)]);
        // '' puts cursor 1 back where the agent left it, and '' again returns
        keys(&mut app, "''");
        assert_eq!((app.ws.view.cursor, app.ws.view.col_of(10)), (10, Some(10)));
        keys(&mut app, "''");
        assert_eq!(app.ws.view.cursor, 32);
        // Tab and :cursor N centre too
        app.ws.view.start = 0;
        app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!((app.ws.view.cursor, app.ws.view.col_of(70)), (70, Some(10)));
        keys(&mut app, ":cursor 2\n''");
        assert_eq!(app.ws.view.cursor, 70);
    }

    #[test]
    fn radixes() {
        let mut app = open("vcd", Some(SESSION));
        let msg = |app: &App| app.msg.as_ref().map_or(String::new(), |m| m.0.clone());
        let value = |app: &App| {
            let s = app.ws.rows[app.ws.sel].sig().unwrap();
            wtf_core::rows::label(&app.ws.trace.vars[s.var], s, &app.ws.trace.value_at(s.var, app.ws.view.cursor).unwrap())
        };
        // data is 0x43 at 12ns
        keys(&mut app, "jjj:radix q4.4\n");
        assert_eq!(value(&app), "4.1875");
        keys(&mut app, ":radix q4.12\n");
        assert_eq!(msg(&app), "q4.12 doesn't fit a 8-bit signal");
        keys(&mut app, ":radix float\n");
        assert_eq!(msg(&app), "float doesn't fit a 8-bit signal");
        keys(&mut app, ":radix ascii\n");
        assert_eq!(value(&app), "C");
        // r goes on from ascii through what fits 8 bits (no float, no fixed point) back to hex
        let mut seen = Vec::new();
        for _ in 0..6 {
            keys(&mut app, "ir\x1b");
            seen.push(value(&app));
        }
        assert_eq!(seen, ["0x43", "0b1000011", "67", "67", "0o103", "C"]);
        keys(&mut app, ":radix uq2.6\n");
        let text = session::to_toml(&app.ws);
        assert!(text.contains("radix = \"uq2.6\""), "{text}");
        let mut back = open("vcd", None);
        assert!(session::apply(&mut back.ws, &text).unwrap().is_empty());
        back.ws.sel = app.ws.sel;
        back.ws.view.cursor = 12;
        assert_eq!(value(&back), "1.046875");
    }

    #[test]
    fn help() {
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, "?");
        insta::assert_snapshot!(snapshot(&mut app, 100, Some(20)));
        keys(&mut app, "jl");
        assert!(app.help && app.ws.view.cursor == 12 && app.ws.sel == 0); // the box takes no other keys
        keys(&mut app, "q");
        assert!(!app.help && !app.quit);
        keys(&mut app, "i?");
        assert!(snapshot(&mut app, 100, Some(20)).contains("help · EDIT"));
        keys(&mut app, "\x1ba?");
        assert!(snapshot(&mut app, 100, Some(20)).contains("help · signal panel"));
        keys(&mut app, "?\x1b:help\n");
        assert!(app.help && app.mode == app::Mode::Edit && snapshot(&mut app, 100, Some(20)).contains("help · EDIT"));
        // a topic: the agents' reference, scrolled by the line
        keys(&mut app, "?\x1b:help Decode\n");
        let screen = snapshot(&mut app, 100, Some(12));
        assert!(screen.contains("help · decode") && screen.contains("Decode first: records are exact"), "{screen}");
        // Enter on a text jumps nowhere
        let cursor = app.ws.view.cursor;
        keys(&mut app, "\n");
        assert!(app.records.is_some() && app.ws.view.cursor == cursor);
        keys(&mut app, "G");
        let screen = snapshot(&mut app, 100, Some(12));
        assert!(!screen.contains("Decode first: records are exact") && screen.contains("j k scroll  Esc close"), "{screen}");
        keys(&mut app, "\x1b:help nope\n");
        assert_eq!(app.msg.as_ref().unwrap().0, "no help topic nope; topics: names times radix expressions decode records workspace commands");
    }

    #[test]
    fn command_line() {
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, ":zo");
        insta::assert_snapshot!(snapshot(&mut app, 60, Some(5)));
        // the list is opaque: no grid line or cursor column colours its text
        keys(&mut app, "\x08\x08");
        let area = Rect::new(0, 0, 100, 30);
        let mut buf = Buffer::empty(area);
        render::draw(&mut app, area, &mut buf);
        let listed = commands::completions("").len() as u16;
        for y in 29 - listed..29 {
            assert!((0..100).all(|x| buf[(x, y)].fg == Color::Reset && buf[(x, y)].bg == Color::Reset), "line {y}");
        }
        keys(&mut app, "\x1b");
        assert!(app.mode == app::Mode::View);
    }

    #[test]
    fn glyph_sets() {
        // buses with crossings, X and Z, a busy column, a group, marks; the panel's borders and tree;
        // a stream's segments opening and closing
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, "jjjmw3mwia");
        let t = trace::Trace::open("tests/data/axis.vcd".as_ref()).unwrap();
        let mut axis = App::new(workspace::Workspace::new(t, "axis".into()));
        keys(&mut axis, ":decode rx axis clock=clk tvalid=tvalid tready=tready tdata=tdata tlast=tlast tkeep=tkeep\n:add rx.frame\n:fit\n");
        for (name, g) in [("box", &draw::BOX), ("ascii", &draw::ASCII)] {
            draw::set_glyphs(g);
            let text = format!("{}{}", snapshot(&mut app, 80, Some(16)), snapshot(&mut axis, 100, None));
            if name == "ascii" {
                assert!(text.is_ascii(), "{text}");
            }
            insta::assert_snapshot!(name, text);
        }
        draw::set_glyphs(&draw::UNICODE);
        assert!(Args::try_parse_from(["wtf", "--glyphs", "utf8", "x.vcd"]).err().unwrap().to_string().contains("unknown glyphs: utf8 (unicode, box, ascii)"));
    }

    #[test]
    fn signal_colour() {
        let mut app = open("vcd", Some(SESSION));
        keys(&mut app, ":select data\n:color blue\n");
        assert!(app.msg.is_none(), "{:?}", app.msg);
        let row = app.ws.rows[app.ws.sel].sig().unwrap().clone();
        let cells = draw::wave(&app.ws.trace, &app.ws.view, &row, None, None, None, 60);
        let fgs: Vec<_> = cells.iter().map(|c| c[0].1.fg).collect();
        // its values in its colour; X and Z keep theirs
        assert!(fgs.contains(&Some(draw::pal().hues[5])) && fgs.contains(&Some(draw::pal().unknown)) && fgs.contains(&Some(draw::pal().highz)), "{fgs:?}");
        assert!(!fgs.contains(&Some(draw::pal().line)));
    }

    #[test]
    fn palettes() {
        let t = trace::Trace::open("tests/data/axilite.vcd".as_ref()).unwrap();
        let mut app = App::new(workspace::Workspace::new(t, "axilite".into()));
        keys(&mut app, ":decode bus axilite clock=aclk prefix=s_axi_\n:add bus.wr bus.rd aclk s_axi_awaddr\n:fit\n");
        assert!(app.msg.is_none(), "{:?}", app.msg);
        let screen = |app: &mut App, w: u16| {
            let area = Rect::new(0, 0, w, 14);
            let mut buf = Buffer::empty(area);
            render::draw(app, area, &mut buf);
            buf
        };
        // the grid goes where the rows draw nothing, with colour or without: never into a label's gaps
        let grid = |buf: &Buffer| buf.content.iter().enumerate().filter(|(_, c)| c.symbol() == "┊").map(|(i, _)| i).collect::<Vec<_>>();
        let colour: Vec<_> = (60..130).map(|w| grid(&screen(&mut app, w))).collect();
        draw::set_palette(draw::Palette::mono());
        let mono: Vec<_> = (60..130).map(|w| grid(&screen(&mut app, w))).collect();
        assert!(colour == mono);
        // light paints the whole screen over the terminal's colours
        draw::set_palette(theme::builtin("light", true).unwrap());
        keys(&mut app, ":");
        let buf = screen(&mut app, 100);
        assert!(buf.content.iter().all(|c| c.bg != Color::Reset && c.fg != Color::Reset));
        draw::set_palette(theme::builtin("dark", true).unwrap());
    }

    #[test]
    fn records_list() {
        let t = trace::Trace::open("tests/data/axis.vcd".as_ref()).unwrap();
        let mut app = App::new(workspace::Workspace::new(t, "axis".into()));
        let msg = |app: &App| app.msg.as_ref().map_or(String::new(), |m| m.0.clone());
        keys(&mut app, ":records rx.frame\n");
        assert_eq!(msg(&app), "no streams yet: make one with :decode or :protocol");
        keys(&mut app, ":decode rx axis clock=clk tvalid=tvalid tready=tready tdata=tdata tlast=tlast tkeep=tkeep\n");
        keys(&mut app, ":records rx.fram\n");
        assert_eq!(msg(&app), "unknown stream: rx.fram (streams: rx.beat, rx.errors, rx.frame)");
        keys(&mut app, ":records rx.frame where len == 99\n");
        assert_eq!(msg(&app), "no matching records, of 4");
        keys(&mut app, ":add rx.frame rx.errors\n:fit\n:select rx.errors\n");
        keys(&mut app, ":records rx.frame where len >= 8\nj");
        insta::assert_snapshot!(snapshot(&mut app, 100, Some(10)));
        // Enter jumps to the highlighted record and selects its stream's row
        keys(&mut app, "\n");
        assert!(app.records.is_none());
        assert_eq!(app.ws.view.cursor, 65); // the third frame, 8 bytes
        assert_eq!(app.ws.sel, 0);
        keys(&mut app, "''");
        assert_eq!(app.ws.view.cursor, 0);
        keys(&mut app, ":records rx.frame\nGk\x1b");
        assert!(app.records.is_none());
        assert_eq!(app.ws.view.cursor, 0);
    }

    #[test]
    fn stream_rows() {
        let t = trace::Trace::open("tests/data/axis.vcd".as_ref()).unwrap();
        let mut app = App::new(workspace::Workspace::new(t, "axis".into()));
        keys(&mut app, ":decode rx axis clock=clk tvalid=tvalid tready=tready tdata=tdata tlast=tlast tkeep=tkeep\n");
        // what it made, violations apart
        assert_eq!(app.msg, Some(("rx: records rx.beat 7, rx.frame 4; violations rx.errors 2".into(), false)));
        keys(&mut app, ":add rx.frame rx.errors\n:fit\n");
        insta::assert_snapshot!(snapshot(&mut app, 100, None));
        // Space shows a row per field under the stream, as a bus's bits
        // the violations' segments are red, the frames' aren't
        let area = Rect::new(0, 0, 100, draw::height(&app.ws) + 1);
        let mut buf = Buffer::empty(area);
        render::draw(&mut app, area, &mut buf);
        let fg = |y: u16, ch: &str| (0..100).map(|x| &buf[(x, y)]).find(|c| c.symbol() == ch).map(|c| c.fg);
        assert_eq!(fg(4, "🮡"), Some(draw::red().fg.unwrap()));
        assert_eq!(fg(2, "🮡"), Some(draw::line().fg.unwrap()));
        keys(&mut app, ":select rx.frame\n ");
        insta::assert_snapshot!(snapshot(&mut app, 100, None));
        // the instance and the expanded row come back from the session
        let text = session::to_toml(&app.ws);
        let mut back = App::new(workspace::Workspace::new(trace::Trace::open("tests/data/axis.vcd".as_ref()).unwrap(), "axis".into()));
        assert!(session::apply(&mut back.ws, &text).unwrap().is_empty(), "{text}");
        assert_eq!(draw::waveform(&mut back.ws, 100), draw::waveform(&mut app.ws, 100));
        keys(&mut app, "jj");
        assert!(snapshot(&mut app, 100, None).contains("rx.frame.len"));
        keys(&mut app, "w/8\n");
        assert_eq!(app.ws.view.cursor, 65); // the third frame, 8 bytes
        // on the stream, the status line shows the record at the cursor; `e` all of it
        keys(&mut app, ":select rx.frame\n");
        let screen = snapshot(&mut app, 120, None);
        assert!(screen.contains(" VIEW  rx.frame #2  65ns–85ns  beats=0x2 len=0x8 data=c0 00 00 00 c1 00 00 00 complete=1"), "{screen}");
        keys(&mut app, "e");
        insta::assert_snapshot!(snapshot(&mut app, 100, Some(16)));
        keys(&mut app, "jl\x1b");
        assert!(app.examine.is_none() && app.ws.view.cursor == 65); // the box takes no other keys
        // any other signal: its value in the row's radix
        keys(&mut app, ":add top.tdata\ne");
        assert_eq!(app.examine.clone(), Some(("top.tdata @ 65ns".to_string(), vec!["0xC0".to_string()])));
        keys(&mut app, "e:rm top.tdata\n:select rx.frame\n");
        keys(&mut app, " ");
        assert!(matches!(&app.ws.rows[0], rows::Row::Signal(s) if s.bits == 0) && app.ws.bit.is_none());
        // a rule's value runs to the next key, spaces and all
        keys(&mut app, ":decode stall check clock=clk bad=tvalid && !tready\n");
        assert_eq!(app.msg, Some(("stall: violations stall 4".into(), false)));
        assert_eq!(app.ws.decodes.last().unwrap().params, [("bad".to_string(), "tvalid && !tready".to_string())]);
        keys(&mut app, ":decode x axis tvalid=nope\n");
        assert!(app.msg.as_ref().is_some_and(|(m, err)| *err && m.contains("no signal for tready")), "{:?}", app.msg);
    }

    /// `e` works on any stream: a protocol of the session's, a frame of it, a check.
    #[test]
    fn examine_declared_streams() {
        let t = trace::Trace::open("tests/data/handshake.vcd".as_ref()).unwrap();
        let mut app = App::new(workspace::Workspace::new(t, "hs".into()));
        let text = r#"
[[protocol]]
name = "cmd"
roles = ["valid", "ready", "cmd"]
when = "valid && ready"
fields = { op = "cmd[3:0]", arg = "cmd[7:4]" }

[[decode]]
name = "c0"
protocol = "cmd"
clock = "clk"
signals = { valid = "valid", ready = "ready", cmd = "data" }

[[decode]]
name = "burst"
protocol = "frame"
signals = { input = "c0" }
params = { gap = "100ns", data = "op" }

[[decode]]
name = "stall"
protocol = "check"
clock = "clk"
params = { waiting = "valid && !ready" }

[[rows]]
signal = "c0"

[[rows]]
signal = "burst"

[[rows]]
signal = "stall"
"#;
        assert_eq!(session::apply(&mut app.ws, text).unwrap(), Vec::<String>::new());
        let examine = |app: &mut App, row: &str, at: &str| {
            keys(app, &format!(":select {row}\n:{at}\ne"));
            let seen = app.examine.clone().unwrap_or_else(|| panic!("no box: {:?}", app.msg));
            keys(app, "\x1b");
            seen
        };
        let c0 = examine(&mut app, "c0", "35ns");
        assert_eq!((c0.0.as_str(), &c0.1[2..]), ("c0 #0", &["op   0x1".to_string(), "arg  0x1".to_string()][..]));
        let burst = examine(&mut app, "burst", "35ns");
        assert_eq!(burst.0, "burst #0"); // `data` stays out of the label
        assert!(burst.1.contains(&"data      01 03".to_string()), "{burst:?}");
        let first = app.ws.streams.iter().find(|s| s.name == "stall").unwrap().records[0].start;
        let stall = examine(&mut app, "stall", &format!("{first}ns"));
        assert!(stall.0.starts_with("stall #0 waiting") && stall.1.contains(&"rule  waiting".to_string()), "{stall:?}");
    }

    #[test]
    fn busy_when_zoomed_out() {
        let mut app = open("vcd", None);
        keys(&mut app, ":add top.cpu.data\n:zoom 4ns\n");
        insta::assert_snapshot!(snapshot(&mut app, 40, None));
    }
}

/// Random edits, then undo everything: the rows must be back where they started.
#[cfg(test)]
mod fuzz {
    use super::*;
    use crate::tui::app;
    use wtf_core::{rows, trace};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn undo_fuzz() {
        let set: Vec<KeyEvent> = "jkJKdVr u\x1b"
            .chars()
            .map(|c| match c {
                '\x1b' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                c => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            })
            .chain([KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)])
            .collect();
        let mut seed = 12345u64;
        for round in 0..500 {
            let t = trace::Trace::open("tests/data/simple.vcd".as_ref()).unwrap();
            let mut app = App::new(workspace::Workspace::new(t, "s".into()));
            session::apply(&mut app.ws, &std::fs::read_to_string("tests/data/simple.session.toml").unwrap()).unwrap();
            let start = app.ws.rows.clone();
            app.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
            let mut log = String::new();
            for _ in 0..30 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let k = set[(seed >> 33) as usize % set.len()];
                log.push_str(&format!("{:?}{} ", k.code, if k.modifiers.is_empty() { "" } else { "^" }));
                app.mode = app::Mode::Edit;
                app.adding = false;
                app.key(k);
                assert!(app.ws.rows.is_empty() || rows::visible(&app.ws.rows).contains(&app.ws.sel), "round {round}: sel hidden after {log}");
            }
            for _ in 0..100 {
                let _ = app.ws.undo();
            }
            assert!(rows::same_layout(&start, &app.ws.rows), "round {round}: undo all != start after {log}\n{:?}\n{:?}", start, app.ws.rows);
        }
    }
}
