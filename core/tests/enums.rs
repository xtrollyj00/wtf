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

#[test]
fn named_by_the_user() {
    // top.count has no names in the file: :names gives some, as a VCD's signals need
    let mut ws = open();
    run(&mut ws, "select count").unwrap();
    run(&mut ws, "names count 0=ZERO 0b1=ONE 'h2=TWO").unwrap();
    let Row::Signal(s) = &ws.rows[ws.sel] else { panic!() };
    assert_eq!(s.radix, radix::ENUM);
    let all: Vec<String> = [0, 10, 20, 30].iter().map(|&t| shown(&ws, t)).collect();
    assert_eq!(all, ["ZERO", "ONE", "TWO", "0x3"]);
    ws.search("two", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 20);
    // saved, and given again on :e and from a session
    let text = session::to_toml(&ws);
    assert!(text.contains("[[names]]\nsignal = \"top.count\"\nvalues = \"0=ZERO 0b1=ONE 'h2=TWO\"\n"), "{text}");
    ws.reload().unwrap();
    assert_eq!(shown(&ws, 10), "ONE");
    let mut back = Workspace::open(format!("{}/../tests/data/enum.fst", env!("CARGO_MANIFEST_DIR")).as_ref()).unwrap();
    assert!(session::apply(&mut back, &text).unwrap().is_empty());
    assert_eq!(session::to_toml(&back), text);
    // off: back to the default radix, no names
    run(&mut ws, "names count off").unwrap();
    assert_eq!(shown(&ws, 10), "0x1");
    assert!(!session::to_toml(&ws).contains("[[names]]"));
    assert_eq!(run(&mut ws, "names count 9=NINE"), Err("9 doesn't fit in 3 bits".into()));
    assert!(run(&mut ws, "names count ZERO").unwrap_err().starts_with("usage: :names"));
    assert!(run(&mut ws, "names count[0] 1=ON").unwrap_err().contains("not a bit"));
}

/// Names given to a signal a rerun no longer has: `:e` reports them.
#[test]
fn names_lost_on_reload() {
    let dir = std::env::temp_dir().join(format!("wtf-core-names-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let trace = dir.join("t.vcd");
    let text = std::fs::read_to_string(format!("{}/../tests/data/simple.vcd", env!("CARGO_MANIFEST_DIR"))).unwrap();
    std::fs::write(&trace, &text).unwrap();
    let mut ws = Workspace::open(&trace).unwrap();
    run(&mut ws, "names top.dma.state 0=IDLE").unwrap();
    // the simulation ran again, without dma
    std::fs::write(&trace, text.lines().filter(|l| !l.contains("dma") && !l.ends_with('&')).map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    let (msg, _) = ws.reload().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(msg, "reloaded; not in trace: names of top.dma.state (unknown signal: top.dma.state)");
}
