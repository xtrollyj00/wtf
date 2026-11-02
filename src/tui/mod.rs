//! The terminal UI: an interface to the core's workspace, with modes, keys, the signal panel and
//! the command line (spec §2 - §9).

pub mod app;
pub mod commands;
pub mod keys;
pub mod panel;
pub mod render;
pub mod setup;

use crate::live;
use app::App;

/// Run the window until it quits: draw, then answer keys, the mouse, agents and the trace file
/// changing on disk.
pub fn run(app: &mut App) -> Result<(), String> {
    // the agent's way in (`wtf --mcp`); without it the window works as before, and says why
    let live = match live::window(&std::process::id().to_string(), &app.ws.trace_path) {
        Ok(l) => Some(l),
        Err(e) => {
            app.msg = Some((format!("no link for agents: {e}"), true));
            None
        }
    };
    // the agents on this trace and their sessions, read every half second, each change sent on
    // (spec §15 "Joining")
    let agents = {
        let (tx, rx) = std::sync::mpsc::channel();
        let trace = app.ws.trace_path.clone();
        std::thread::spawn(move || {
            let mut last: Vec<(String, String)> = Vec::new();
            loop {
                let now: Vec<(String, String)> = live::on_trace("agent", &trace).into_iter().filter_map(|id| live::get(&id).ok().map(|(_, t)| (id, t))).collect();
                if now != last {
                    last = now.clone();
                    if tx.send(now).is_err() {
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
        rx
    };
    let mut term = ratatui::init();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
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
                        live::Request::Get => Ok(crate::mcp::link_answer(&app.ws)),
                        live::Request::Put(text) => app.take_present(&text),
                    };
                    let _ = inc.reply.send(answer);
                    break;
                }
                if app.watch(app::stamp(&app.ws.trace_path), std::time::Instant::now()) {
                    break;
                }
                // a drag resting at an edge scrolls
                if app.tick(std::time::Instant::now()) {
                    break;
                }
                if let Ok(now) = agents.try_recv()
                    && app.agents_now(now)
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
                match ev {
                    crossterm::event::Event::Key(k) if k.kind == crossterm::event::KeyEventKind::Press => app.key(k),
                    crossterm::event::Event::Mouse(m) => app.mouse(m),
                    _ => {}
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
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    ratatui::restore();
    res.map_err(|e| e.to_string())
}
