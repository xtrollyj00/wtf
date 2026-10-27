//! The `:` commands on a workspace (spec §7), as both interfaces run them.

mod common;
use common::*;
use wtf_core::command::{self, no_match};

fn run(ws: &mut wtf_core::workspace::Workspace, line: &str) -> Result<Option<(String, bool)>, String> {
    command::run(ws, line)
}

#[test]
fn view_and_cursors() {
    let mut ws = open();
    assert_eq!(run(&mut ws, ""), Ok(None));
    run(&mut ws, "25ns").unwrap();
    assert_eq!(ws.view.cursor, 25);
    run(&mut ws, "zoom 2ns").unwrap();
    assert_eq!((ws.view.ticks, ws.view.cols), (2, 1));
    assert_eq!(run(&mut ws, "zoom 0ns"), Err("zoom must be above zero".into()));
    assert!(run(&mut ws, "zoom soon").is_err());
    run(&mut ws, "fit").unwrap();
    assert_eq!(ws.view.start, 0);
    // cursor 3 is a mark: placed, jumped to, and left where it is when the cursor moves on
    run(&mut ws, "cursor 3 60ns").unwrap();
    assert_eq!(ws.cursor_list(), [(0, 25), (2, 60)]);
    run(&mut ws, "cursor 3").unwrap();
    assert_eq!(ws.view.cursor, 60);
    run(&mut ws, "cursor 1 70ns").unwrap();
    assert_eq!(ws.cursor_list(), [(0, 70), (2, 60)]);
    run(&mut ws, "cursor 3 off").unwrap();
    assert_eq!(ws.cursor_list(), [(0, 70)]);
    assert_eq!(run(&mut ws, "cursor 1"), Ok(None));
    assert_eq!(run(&mut ws, "cursor 1 off"), Err("cursor 1 can't be removed".into()));
    assert_eq!(run(&mut ws, "cursor 4"), Err("no cursor 4".into()));
    assert_eq!(run(&mut ws, "cursor 10 1ns"), Err("usage: :cursor <1-9> [time|off]".into()));
    assert_eq!(run(&mut ws, "warp"), Err("unknown command: warp".into()));
}

#[test]
fn fit_a_range() {
    let mut ws = open();
    ws.view.set_width(40);
    run(&mut ws, "25ns").unwrap();
    run(&mut ws, "cursor 3 65ns").unwrap();
    // the cursor to mark 3, in either order; the cursor stays
    run(&mut ws, "fit '3 '1").unwrap();
    assert_eq!((ws.view.start, ws.view.ticks, ws.view.cols, ws.view.cursor), (25, 1, 1, 25));
    assert!(ws.view.last_visible() + 1 >= 65);
    run(&mut ws, "fit 10ns 20ns").unwrap();
    assert_eq!((ws.view.start, ws.view.ticks, ws.view.cols), (10, 1, 4));
    assert_eq!(run(&mut ws, "fit '1 '2"), Err("no cursor 2".into()));
    assert_eq!(run(&mut ws, "fit '0 '1"), Err("no cursor 0: cursors are '1 to '9".into()));
    assert_eq!(run(&mut ws, "fit '1 25ns"), Err("nothing between them: the same time".into()));
    assert_eq!(run(&mut ws, "fit 10ns"), Err("usage: :fit, or :fit <from> <to> as cursors ('2 '3) or times".into()));
}

#[test]
fn adding_and_removing() {
    let mut ws = open();
    assert_eq!(run(&mut ws, "add"), Err("usage: :add <path|pattern>...".into()));
    // a scope's direct signals become a group named after it
    run(&mut ws, "add top.cpu").unwrap();
    assert_eq!(names(&ws), ["▾ cpu", "  state", "  data", "  busy"]);
    assert_eq!(run(&mut ws, "add top.cpu.*"), Err("already shown: top.cpu.*".into()));
    assert_eq!(run(&mut ws, "add top.nope"), Err("no match: top.nope".into()));
    assert_eq!(run(&mut ws, "add top.zz*"), Err("no match: top.zz* (a pattern matches the whole path: **.*top.zz* for top.zz anywhere)".into()));
    assert_eq!(run(&mut ws, "add top.clk nope"), Ok(Some(("no match: nope".into(), true))));
    assert_eq!(run(&mut ws, "rm top.cpu.*a*"), Ok(None));
    // clk went in below busy, a row of the group, so it joined the group
    assert_eq!(names(&ws), ["▾ cpu", "  busy", "  clk"]);
    assert_eq!(run(&mut ws, "rm top.cpu.data"), Err("no matching rows: top.cpu.data".into()));
    // without an argument: the selected row
    ws.sel = 2;
    run(&mut ws, "rm").unwrap();
    run(&mut ws, "rm **").unwrap();
    assert!(ws.rows.is_empty());
    assert_eq!(run(&mut ws, "rm"), Err("no rows".into()));
}

#[test]
fn groups() {
    let mut ws = with(&["top.clk", "top.cpu.data", "top.cpu.busy"]);
    assert_eq!(run(&mut ws, "group"), Err("usage: :group <name>".into()));
    run(&mut ws, "select data busy").unwrap();
    run(&mut ws, "group cpu").unwrap();
    run(&mut ws, "select busy").unwrap();
    run(&mut ws, "rename core").unwrap();
    assert_eq!(names(&ws), ["clk", "▾ core", "  data", "  busy"]);
    assert_eq!(run(&mut ws, "rename"), Err("usage: :rename <name>".into()));
    run(&mut ws, "select clk").unwrap();
    assert_eq!(run(&mut ws, "rename x"), Err("not in a group".into()));
    assert_eq!(run(&mut ws, "ungroup"), Err("not in a group".into()));
    run(&mut ws, "select core").unwrap();
    run(&mut ws, "ungroup").unwrap();
    assert_eq!(names(&ws), ["clk", "data", "busy"]);
    assert_eq!(run(&mut ws, "select nope"), Err("unknown signal: nope".into()));
    assert_eq!(run(&mut ws, "select top.rst"), Err("not shown: top.rst".into()));
    assert_eq!(run(&mut ws, "select"), Err("usage: :select <row> [<last row>]".into()));
}

#[test]
fn radix_and_undo() {
    let mut ws = with(&["top.cpu.data"]);
    ws.sel = 0;
    run(&mut ws, "radix sdec").unwrap();
    assert_eq!(radix_of(&ws, "top.cpu.data"), "sdec");
    assert_eq!(run(&mut ws, "radix roman"), Err("unknown radix: roman".into()));
    ws.undoable(|ws| run(ws, "add top.clk")).unwrap();
    run(&mut ws, "undo").unwrap();
    assert_eq!(names(&ws), ["data"]);
    run(&mut ws, "redo").unwrap();
    assert_eq!(names(&ws), ["data", "clk"]);
}

#[test]
fn color_and_undo() {
    let mut ws = with(&["top.cpu"]);
    ws.sel = 0; // the group: every signal in it
    ws.undoable(|ws| run(ws, "color red")).unwrap();
    assert!(ws.rows.iter().filter_map(|r| r.sig()).all(|s| s.color == Some(0)));
    assert_eq!(run(&mut ws, "color pink"), Err("unknown colour: pink (colours: red, orange, yellow, green, cyan, blue, magenta, gray)".into()));
    run(&mut ws, "undo").unwrap();
    assert!(ws.rows.iter().filter_map(|r| r.sig()).all(|s| s.color.is_none()));
    run(&mut ws, "redo").unwrap();
    ws.sel = 1;
    run(&mut ws, "color off").unwrap();
    assert_eq!(ws.rows[1].sig().unwrap().color, None);
}

#[test]
fn reload_answers() {
    let mut ws = with(&["top.clk"]);
    assert_eq!(run(&mut ws, "e"), Ok(Some(("reloaded".into(), false))));
}

#[test]
fn patterns() {
    let ws = open();
    let paths = |arg: &str| command::resolve(&ws, arg).into_iter().map(|v| ws.trace.vars[v].path.clone()).collect::<Vec<_>>();
    assert_eq!(paths("top.cpu"), ["top.cpu.state", "top.cpu.data", "top.cpu.busy"]);
    assert_eq!(paths("**.state"), ["top.cpu.state", "top.dma.state"]);
    assert_eq!(paths("top.clk"), ["top.clk"]);
    assert!(paths("top.nope").is_empty());
    assert_eq!(no_match("x"), "no match: x");
    assert_eq!(no_match("**.x*"), "no match: **.x*");
}
