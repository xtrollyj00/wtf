//! Enum names from the file (spec §10 `enum`, §8 names): `tests/data/enum.fst` has top.clk, top.state
//! (3 bits, FST enum state_t: IDLE 000, READ 001, WRITE 010, DONE 011) and top.count (3 bits, no
//! names; 0 at 0ns, +1 every 10ns). state is IDLE at 0ns, READ 10, WRITE 20, DONE 30, 101 (no name)
//! 40, xxx 50, IDLE 60; 0 - 80ns.

use wtf_core::command::run;
use wtf_core::rows::{self, Row};
use wtf_core::workspace::Workspace;
use wtf_core::{radix, session};

fn open() -> Workspace {
    let mut ws = Workspace::open(format!("{}/../tests/data/enum.fst", env!("CARGO_MANIFEST_DIR")).as_ref()).unwrap();
    run(&mut ws, "add top.state top.count").unwrap();
    run(&mut ws, "select state").unwrap();
    ws
}

/// The selected row's value at `t` ns, as the row shows it.
fn shown(ws: &Workspace, t: u64) -> String {
    let Row::Signal(s) = &ws.rows[ws.sel] else { panic!() };
    rows::label(&ws.trace.vars[s.var], s, &ws.trace.value_at(s.var, t).unwrap())
}

#[test]
fn shown_by_name() {
    let mut ws = open();
    let Row::Signal(s) = &ws.rows[ws.sel] else { panic!() };
    assert_eq!(s.radix, radix::ENUM);
    let all: Vec<String> = [0, 10, 20, 30, 40, 50].iter().map(|&t| shown(&ws, t)).collect();
    assert_eq!(all, ["IDLE", "READ", "WRITE", "DONE", "0x5", "0xX"]); // no name: hex
    run(&mut ws, "radix hex").unwrap();
    assert_eq!(shown(&ws, 10), "0x1");
    // r cycles through enum on a signal with names, and only there
    ws.set_radix(None).unwrap(); // bin
    assert_eq!(radix::next(ws.rows[ws.sel].sig().unwrap().radix, 3, true), 2);
    run(&mut ws, "radix float").unwrap_err();
    run(&mut ws, "select count").unwrap();
    assert_eq!(ws.rows[ws.sel].sig().unwrap().radix, radix::DEFAULT);
    assert_eq!(run(&mut ws, "radix enum"), Err("enum: the file names no values of this signal".into()));
}

#[test]
fn searched_by_name() {
    let mut ws = open();
    ws.search("write", true, 1).unwrap(); // any case
    assert_eq!(ws.view.cursor, 20);
    ws.search("state == DONE", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 30);
    ws.search("5", true, 1).unwrap(); // numbers still work
    assert_eq!(ws.view.cursor, 40);
    ws.search("idle", true, 1).unwrap(); // IDLE at 0 is the first value, no change
    assert_eq!(ws.view.cursor, 60);
    // count has no names: READ is a signal name there, and there is none
    assert!(ws.search("count == READ", true, 1).unwrap_err().contains("READ"));
}

#[test]
fn saved_only_when_not_enum() {
    let mut ws = open();
    let text = session::to_toml(&ws);
    assert!(!text.contains("radix"), "{text}");
    run(&mut ws, "radix hex").unwrap();
    let text = session::to_toml(&ws);
    assert!(text.contains("signal = \"top.state\"\nradix = \"hex\""), "{text}");
    let mut back = Workspace::open(format!("{}/../tests/data/enum.fst", env!("CARGO_MANIFEST_DIR")).as_ref()).unwrap();
    assert_eq!(session::apply(&mut back, &text), Ok(vec![]));
    assert_eq!(back.rows, ws.rows);
}
