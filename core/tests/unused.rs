//! Computed vars nothing uses any more are dropped (spec §16), and `present` shows only what the
//! session names (spec §15).
mod common;
use common::*;
use wtf_core::command::run;

fn has(ws: &wtf_core::workspace::Workspace, path: &str) -> bool {
    ws.trace.by_path.contains_key(path)
}

/// A command as the window and the agent run one: an undo step recorded after it.
fn cmd(ws: &mut wtf_core::workspace::Workspace, line: &str) {
    ws.undoable(|ws| run(ws, line)).unwrap();
}

#[test]
fn a_slice_goes_when_nothing_shows_it() {
    let mut ws = with(&["top.clk"]);
    ws.query("cpu.data[3:0] == 1").unwrap();
    assert!(has(&ws, "top.cpu.data[3:0]"));
    cmd(&mut ws, "unit ns");
    assert!(!has(&ws, "top.cpu.data[3:0]"));

    // a row keeps it, and so does the undo step that still shows it
    cmd(&mut ws, "add cpu.data[3:0]");
    cmd(&mut ws, "select cpu.data[3:0]");
    cmd(&mut ws, "rm cpu.data[3:0]");
    assert!(has(&ws, "top.cpu.data[3:0]"));
    cmd(&mut ws, "undo");
    assert_eq!(names(&ws), ["clk", "data[3:0]"]);
    assert_eq!(ws.trace.vars[ws.rows[1].sig().unwrap().var].path, "top.cpu.data[3:0]");

    // a named signal stays without a row
    cmd(&mut ws, "signal lo = cpu.data[1:0]");
    assert!(has(&ws, "lo"));
}

#[test]
fn sampling_again_with_fewer_fields_drops_the_old_ones() {
    let mut ws = with(&["top.clk"]);
    cmd(&mut ws, "protocol p clock=top.clk when=!top.rst d=top.cpu.data s=top.cpu.state");
    assert!(has(&ws, "p.s"));
    cmd(&mut ws, "protocol p clock=top.clk when=!top.rst d=top.cpu.data");
    assert!(has(&ws, "p.d") && !has(&ws, "p.s"));
}

#[test]
fn present_shows_only_what_the_session_names() {
    let mut ws = with(&["top.clk"]);
    cmd(&mut ws, "protocol p clock=top.clk when=!top.rst d=top.cpu.data");
    cmd(&mut ws, "signal hi = cpu.data[7:4]");
    let clk = ws.trace.by_path["top.clk"];
    let session = wtf_core::session::to_toml(&with(&["top.clk"]));
    ws.present(&session, false).unwrap();
    assert!(ws.streams.is_empty() && ws.protocols.is_empty() && ws.signals.is_empty());
    assert!(!has(&ws, "p") && !has(&ws, "p.d") && !has(&ws, "hi"));
    assert_eq!(ws.rows[0].sig().unwrap().var, clk);
}
