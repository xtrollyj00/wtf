//! Names in either HDL's form (spec §8): case, VHDL's parentheses, array elements.
//!
//! `tests/data/hdl.vcd` names scopes as nvc and Verilog simulators do: `dut.clk` (rising at 5,
//! 15, 25), `dut.headers_vld` (high 10ns - 20ns), `dut.data` (8 bits, 0xA5 from 10ns),
//! `dut.gen_stages(0).x` (high from 20ns) and `dut.gen_stages(1).x` (from 10ns), `dut.blk[2].y`
//! (from 10ns); 0 - 30ns at 1ns ticks. `tests/data/vhdl.fst` is what nvc dumps of vhdl.vhd:
//! `vhdl.clk`, the array elements `vhdl.arr[0]` and `vhdl.arr[1]`, `vhdl.gen(0).x` and
//! `vhdl.gen(1).x`.

use std::path::PathBuf;
use wtf_core::query::{self, Search};
use wtf_core::trace::{Val, brackets};
use wtf_core::workspace::Workspace;

fn fixture(name: &str) -> Workspace {
    Workspace::open(&PathBuf::from(format!("{}/../tests/data/{name}", env!("CARGO_MANIFEST_DIR")))).unwrap()
}

fn open() -> Workspace {
    fixture("hdl.vcd")
}

fn path(ws: &mut Workspace, name: &str) -> String {
    let (v, bit) = ws.resolve(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    format!("{}{}", ws.trace.vars[v].path, bit.map(|b| format!("[{b}]")).unwrap_or_default())
}

#[test]
fn either_case() {
    let mut ws = open();
    assert_eq!(path(&mut ws, "dut.HEADERS_VLD"), "dut.headers_vld");
    assert_eq!(path(&mut ws, "Headers_Vld"), "dut.headers_vld");
    assert!(ws.resolve("headers_valid").unwrap_err().starts_with("unknown signal"));
}

#[test]
fn either_indexing() {
    let mut ws = open();
    // generate scopes, whichever the simulator wrote
    assert_eq!(path(&mut ws, "gen_stages(1).x"), "dut.gen_stages(1).x");
    assert_eq!(path(&mut ws, "dut.GEN_STAGES[1].X"), "dut.gen_stages(1).x");
    assert_eq!(path(&mut ws, "blk(2).y"), "dut.blk[2].y");
    // bits and slices
    assert_eq!(path(&mut ws, "data(7)"), "dut.data[7]");
    assert_eq!(path(&mut ws, "data[7]"), "dut.data[7]");
    let (v, _) = ws.resolve("DATA(7 downto 4)").unwrap();
    assert_eq!(ws.trace.value_of(v, None, 10), Some(Val::Bits("1010".into())));
    assert_eq!(ws.resolve("data(3 DOWNTO 0)").unwrap().0, ws.resolve("data[3:0]").unwrap().0);
    assert_eq!(ws.resolve("data(8)").unwrap_err(), "data has no bit 8");
}

#[test]
fn nvc_names() {
    let mut ws = fixture("vhdl.fst");
    // array elements, not bits of a bus
    assert_eq!(path(&mut ws, "ARR(1)"), "vhdl.arr[1]");
    assert_eq!(path(&mut ws, "vhdl.arr[0]"), "vhdl.arr[0]");
    assert_eq!(path(&mut ws, "arr(1)(3)"), "vhdl.arr[1][3]");
    assert_eq!(path(&mut ws, "GEN[1].X"), "vhdl.gen(1).x");
    // a session saved when paths kept nvc's form still finds the element
    let v = ws.var_named("vhdl.arr.[1]").unwrap();
    assert_eq!(ws.trace.vars[v].path, "vhdl.arr[1]");
    let q = ws.query("@clk gen(1).x && arr(1) == 0xC && arr(0)(1)").unwrap();
    let s = Search { from: 0, to: None, forward: true, merge: true, runs: usize::MAX, matches: u64::MAX };
    assert_eq!(query::find(&ws.trace, &q, &s).runs.len(), 1);
}

#[test]
fn in_expressions() {
    let mut ws = open();
    let s = Search { from: 0, to: None, forward: true, merge: false, runs: usize::MAX, matches: u64::MAX };
    let q = ws.query("@CLK gen_stages(1).x && !gen_stages[0].x && DATA(7 downto 4) == 0xA").unwrap();
    assert_eq!(query::find(&ws.trace, &q, &s).runs.iter().map(|r| r.first).collect::<Vec<_>>(), [5]);
    // a function's parenthesis is the call's
    let q = ws.query("@clk rose(HEADERS_VLD) || fell(gen_stages(1).x)").unwrap();
    assert_eq!(query::find(&ws.trace, &q, &s).runs.iter().map(|r| r.first).collect::<Vec<_>>(), [5]);
}

#[test]
fn add_either_form() {
    let mut ws = open();
    wtf_core::command::run(&mut ws, "add dut.GEN_STAGES[0] dut.HEADERS_VLD dut.blk(2).y").unwrap();
    let names = wtf_core::rows::display_names(&ws.trace, &ws.rows);
    let names: Vec<&str> = names.iter().map(|n| n.trim()).collect();
    // a scope is a group, a signal a row of its own
    assert_eq!(names, ["▾ gen_stages(0)", "x", "headers_vld", "y"]);
}

#[test]
fn bracket_forms() {
    assert_eq!(brackets("top.axi_tdata.[6]"), "top.axi_tdata[6]");
    assert_eq!(brackets("arr..[0]"), "arr[0]");
    assert_eq!(brackets("x(7 DownTo 4)"), "x[7:4]");
    assert_eq!(brackets("grid(1, 2)"), "grid[1][2]");
    assert_eq!(brackets("{a(1), gen(2).b}"), "{a[1], gen[2].b}");
    assert_eq!(brackets("countdownto"), "countdownto");
}
