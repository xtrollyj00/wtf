//! Why a name is no signal (`Workspace::unknown`), apart from how the error says it (spec §15).
//!
//! `simple.vcd` (see `common`); `types-verilator-structs.fst`: the struct `types.pkt` (valid,
//! state, pair with hi and lo) and the array `types.pkts` of two of them.

mod common;
use common::*;
use std::path::PathBuf;
use wtf_core::workspace::Workspace;
use wtf_core::workspace::unknown::Why;

fn structs() -> Workspace {
    Workspace::open(&PathBuf::from(format!("{}/../tests/data/types-verilator-structs.fst", env!("CARGO_MANIFEST_DIR")))).unwrap()
}

#[test]
fn what_is_there_instead() {
    let ws = open();
    let why = |name| ws.unknown(name).why;
    assert_eq!(why("cpu"), Why::Scope { signals: 3, first: vec!["top.cpu.state".into(), "top.cpu.data".into(), "top.cpu.busy".into()] });
    // a field of a signal the trace keeps as one vector, as Verilator's VCD keeps a packed struct
    assert_eq!(why("cpu.data.valid"), Why::NoFields { signal: "top.cpu.data".into(), width: Some(8), real: false, start: "cpu.data".into() });
    assert_eq!(why("top.cpu.data[0].valid"), Why::NoFields { signal: "top.cpu.data".into(), width: Some(8), real: false, start: "top.cpu.data".into() });
    assert_eq!(why("cpu.bsy"), Why::Nearest(vec!["top.cpu.busy".into()]));
    assert_eq!(why("nope"), Why::Nearest(vec![]));
    let ws = structs();
    let why = |name| ws.unknown(name).why;
    assert_eq!(why("pkt.pair.mid"), Why::NoField { whole: "types.pkt.pair".into(), missing: "mid".into(), fields: vec!["hi".into(), "lo".into()] });
    assert_eq!(why("pkts[5].valid"), Why::NoField { whole: "types.pkts".into(), missing: "[5]".into(), fields: vec!["[0]".into(), "[1]".into()] });
}

#[test]
fn messages() {
    let ws = open();
    let msg = |name| ws.unknown(name).to_string();
    assert_eq!(msg("cpu.data.valid"), "unknown signal: cpu.data.valid: top.cpu.data is one 8-bit signal in this trace, without fields or elements; slice it, `cpu.data[7:0]`");
    assert_eq!(msg("cpu.busy.x"), "unknown signal: cpu.busy.x: top.cpu.busy is one 1-bit signal in this trace, without fields or elements");
    assert_eq!(msg("cpu.bsy"), "unknown signal: cpu.bsy (nearest: top.cpu.busy)");
    assert_eq!(msg("nope"), "unknown signal: nope");
    let ws = structs();
    assert_eq!(ws.unknown("pkts[1].bad").to_string(), "unknown signal: pkts[1].bad: types.pkts[1] has no bad; it has valid, state, pair");
    // what resolve answers is that message
    let mut ws = structs();
    assert_eq!(ws.resolve("pkt.pair.mid"), Err("unknown signal: pkt.pair.mid: types.pkt.pair has no mid; it has hi, lo".into()));
}
