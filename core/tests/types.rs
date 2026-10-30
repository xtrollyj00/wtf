//! Composite HDL types as simulators dump them: the paths wtf gets, a value, and the names a user
//! writes in either HDL's form (spec §8).
//!
//! `tests/data/types.sh` dumps two designs. types.sv (Verilator, Icarus): a packed struct `pkt` (valid,
//! enum state IDLE/BUSY/DONE, pair.hi / pair.lo; 11 bits), an unpacked struct `loose` (a, b),
//! packed `packed2[3:0][7:0]`, unpacked `mem[0:3]` and `grid[0:1][0:2]`, structs `pkts[0:1]`, a
//! packed union `word` (byte_ / pair) and `gen[g].p` (a pair). types.vhd (nvc): records `pkt` and
//! `pkts(0 to 1)` as above (state idle/busy/done), a 2-D `grid(0 to 1, 0 to 2)`, an array of
//! arrays `rows`, a record `link` (addr, data(0 to 2)) and `gen(i).p`. Values change once, at
//! "10ns": pkt valid 1, BUSY, pair 0xA / 0x5; loose.a 0x3C; packed2 0x44332211; mem[i] 0x10 + i;
//! grid / rows [i][j] 3 * i + j; pkts[1].pair.lo 0x7; word 0xC3; gen[1].p 0x96; link.addr 0x3C,
//! link.data (1, 2, 3).
//!
//! types-verilator.{vcd,fst} are without --trace-structs, types-verilator-structs.* with it;
//! types-icarus.{vcd,fst} are Icarus's (`vvp -fst`); types-nvc.{vcd,fst} with --dump-arrays,
//! types-nvc-noarrays.fst without.

use std::path::PathBuf;
use wtf_core::time::Tick;
use wtf_core::trace::Val;
use wtf_core::workspace::Workspace;

fn fixture(name: &str) -> Workspace {
    Workspace::open(&PathBuf::from(format!("{}/../tests/data/{name}", env!("CARGO_MANIFEST_DIR")))).unwrap()
}

/// What a name resolves to and its value at `t`, as `path = value` (an enum value by its name),
/// or `unknown` when it doesn't resolve.
fn at(ws: &mut Workspace, name: &str, t: Tick) -> String {
    let (v, bit) = match ws.resolve(name) {
        Ok(p) => p,
        Err(e) if e.starts_with("unknown signal") => return "unknown".into(),
        Err(e) => panic!("{name}: {e}"),
    };
    ws.trace.load(&[v]);
    let var = &ws.trace.vars[v];
    let path = format!("{}{}", var.path, bit.map(|b| format!("[{b}]")).unwrap_or_default());
    let value = match ws.trace.value_of(v, bit, t) {
        Some(Val::Bits(b)) => var.name_of(&b).map_or(b.clone(), str::to_string),
        Some(Val::Text(s)) => s,
        None => "none".into(),
    };
    format!("{path} = {value}")
}

fn check(file: &str, cases: &[(&str, &str)]) {
    let mut ws = fixture(file);
    let t = (10_000_000 / ws.trace.fs_per_tick.unwrap()) as Tick; // 10ns
    let got: Vec<_> = cases.iter().map(|(n, _)| (*n, at(&mut ws, n, t))).collect();
    let want: Vec<_> = cases.iter().map(|(n, w)| (*n, w.to_string())).collect();
    assert_eq!(got, want, "{file}");
}

/// Without --trace-structs a packed struct, packed array or union is one wide var.
const VERILATOR_PLAIN: &[(&str, &str)] = &[
    ("pkt", "types.pkt = 10110100101"),
    ("pkt[9:8]", "types.pkt[9:8] = 01"), // the state, by bits: no field names, no enum names
    // shortcut: a packed struct's fields don't resolve on the one var, fixed by a later piece
    ("pkt.pair.hi", "unknown"),
    ("pkt.state", "unknown"),
    ("loose.a", "types.loose.a = 00111100"),
    ("packed2[15:8]", "types.packed2[15:8] = 00100010"),
    // shortcut: SV's packed2[1] is element 1 (0x22); wtf takes bit 1, fixed by a later piece
    ("packed2[1]", "types.packed2[1] = 0"),
    ("mem[2]", "types.mem[2] = 00010010"),
    ("mem(2)", "types.mem[2] = 00010010"),
    ("grid[1][2]", "types.grid[1][2] = 0101"),
    ("grid(1)(2)", "types.grid[1][2] = 0101"),
    ("grid(1, 2)", "types.grid[1][2] = 0101"),
    ("grid[1]", "unknown"),
    ("pkts(1)", "types.pkts[1] = 00000000111"),
    ("pkts[1].pair.lo", "unknown"), // shortcut: as pkt.pair.hi
    ("word", "types.word = 11000011"),
    ("gen(1).p", "types.gen[1].p = 10010110"),
    ("gen(1).p(7 downto 4)", "types.gen[1].p[7:4] = 1001"),
];

#[test]
fn verilator() {
    check("types-verilator.vcd", VERILATOR_PLAIN);
    check("types-verilator.vcd", &[("grid[1][1]", "types.grid[1][1] = 0100")]);
    check("types-verilator.fst", VERILATOR_PLAIN);
}

/// Icarus dumps a packed struct, packed array or union as one wide var, as Verilator without
/// --trace-structs does, and drops unpacked arrays (`$dumpvars` doesn't take them) and `loose`.
#[test]
fn icarus() {
    for file in ["types-icarus.vcd", "types-icarus.fst"] {
        check(
            file,
            &[
                ("pkt", "types.pkt = 10110100101"),
                ("pkt[9:8]", "types.pkt[9:8] = 01"),
                ("pkt.state", "unknown"), // shortcut: as Verilator's without --trace-structs
                ("packed2[15:8]", "types.packed2[15:8] = 00100010"),
                ("word", "types.word = 11000011"),
                ("gen(1).p", "types.gen[1].p = 10010110"),
                ("mem[2]", "unknown"),
                ("pkts(1)", "unknown"),
            ],
        );
    }
}

/// With --trace-structs a struct or union is a scope of its fields, a packed array a scope of
/// elements; the FST gives an enum field its names, the VCD can't.
const VERILATOR_STRUCTS: &[(&str, &str)] = &[
    // shortcut: a struct is a scope, so it has no whole value or slice, fixed by a later piece
    ("pkt", "unknown"),
    ("pkt.pair", "unknown"),
    ("pkt.valid", "types.pkt.valid = 1"),
    ("pkt.pair.hi", "types.pkt.pair.hi = 1010"),
    ("loose.a", "types.loose.a = 00111100"),
    ("packed2[1]", "types.packed2[1] = 00100010"),
    ("packed2[15:8]", "unknown"), // shortcut: as pkt
    ("mem(2)", "types.mem[2] = 00010010"),
    ("grid(1)(2)", "types.grid[1][2] = 0101"),
    ("pkts[1]", "unknown"), // shortcut: as pkt
    ("word", "unknown"),    // shortcut: as pkt
    ("word.byte_", "types.word.byte_ = 11000011"),
    ("word.pair.hi", "types.word.pair.hi = 1100"),
    ("gen(1).p", "unknown"), // shortcut: as pkt
    ("gen[1].p.hi", "types.gen[1].p.hi = 1001"),
    ("gen(1).p.hi", "types.gen[1].p.hi = 1001"),
];

#[test]
fn verilator_structs() {
    check("types-verilator-structs.vcd", VERILATOR_STRUCTS);
    check("types-verilator-structs.fst", VERILATOR_STRUCTS);
    // the VCD writes an array of structs' scopes pkts[1], the FST pkts / [1]
    check(
        "types-verilator-structs.vcd",
        &[("pkt.state", "types.pkt.state = 01"), ("pkts(1).pair.lo", "types.pkts[1].pair.lo = 0111")],
    );
    check(
        "types-verilator-structs.fst",
        &[("pkt.state", "types.pkt.state = BUSY"), ("pkts(1).pair.lo", "types.pkts[1].pair.lo = 0111")],
    );
}

#[test]
fn verilator_fst_grid() {
    // shortcut: read from Verilator's FST, grid's diagonal is named types.grid[i].grid[i]., so
    // grid[1][1] and grid[0][0] don't resolve (the VCD is right), fixed by a later piece
    for file in ["types-verilator.fst", "types-verilator-structs.fst"] {
        let mut ws = fixture(file);
        assert!(ws.trace.by_path.contains_key("types.grid[1].grid[1]."), "{file}");
        check(file, &[("grid[1][1]", "unknown"), ("grid[0][0]", "unknown"), ("grid[1][0]", "types.grid[1][0] = 0011")]);
        assert!(ws.resolve("grid(0)(1)").is_ok());
    }
}

/// nvc writes records as scopes of their fields, an enum as its names (a string var).
const NVC_COMMON: &[(&str, &str)] = &[
    ("pkt", "unknown"), // shortcut: a record is a scope, no whole value, fixed by a later piece
    ("pkt.valid", "types.pkt.valid = 1"),
    ("pkt.state", "types.pkt.state = busy"),
    ("PKT.PAIR.HI", "types.pkt.pair.hi = 1010"),
    ("link.addr", "types.link.addr = 00111100"),
    ("gen(1).p", "unknown"), // shortcut: as pkt
    ("gen[1].p.hi", "types.gen(1).p.hi = 1001"),
    // nvc drops a 2-D array, in VCD as in FST
    ("grid(1, 2)", "unknown"),
    ("grid(1)(2)", "unknown"),
];

#[test]
fn nvc() {
    let arrays: &[(&str, &str)] = &[
        ("pkts(1).pair.lo", "types.pkts[1].pair.lo = 0111"),
        ("pkts[1].pair.lo", "types.pkts[1].pair.lo = 0111"),
        ("pkts(1)", "unknown"), // shortcut: as pkt
        ("rows(1)(2)", "types.rows[1][2] = 0101"),
        ("rows[1][2]", "types.rows[1][2] = 0101"),
        ("rows(1)", "unknown"),
        ("link.data(1)", "types.link.data[1] = 0010"),
    ];
    for file in ["types-nvc.fst", "types-nvc.vcd"] {
        check(file, NVC_COMMON);
        check(file, arrays);
        assert!(!fixture(file).trace.vars.iter().any(|v| v.path.contains("grid")), "{file}");
    }
    // without --dump-arrays nvc drops every array, records' array fields too
    check("types-nvc-noarrays.fst", NVC_COMMON);
    check("types-nvc-noarrays.fst", &[("pkts(1).pair.lo", "unknown"), ("rows(1)(2)", "unknown"), ("link.data(1)", "unknown")]);
}

/// The rows `:add types.**` makes, as wtf names them: each keeps its composite's name.
fn row_names(file: &str) -> String {
    let mut ws = fixture(file);
    wtf_core::command::run(&mut ws, "add types.**").unwrap();
    wtf_core::rows::display_names(&ws.trace, &ws.rows).join(" ")
}

#[test]
fn row_names_keep_the_composite() {
    let pkts = "pkts[0].valid pkts[0].state pkts[0].pair.hi pkts[0].pair.lo pkts[1].valid pkts[1].state pkts[1].pair.hi pkts[1].pair.lo";
    let grid = "grid[0][0] grid[0][1] grid[0][2] grid[1][0] grid[1][1] grid[1][2]";
    // shortcut: wellen's names for grid's diagonal (verilator_fst_grid), fixed upstream
    let fst_grid = "grid[0].grid[0]. grid[0][1] grid[0][2] grid[1][0] grid[1].grid[1]. grid[1][2]";
    let generate = "gen[0].p.hi gen[0].p.lo gen[1].p.hi gen[1].p.lo";
    assert_eq!(
        row_names("types-verilator-structs.fst"),
        format!("clk pkt.valid pkt.state pkt.pair.hi pkt.pair.lo loose.a loose.b packed2[3] packed2[2] packed2[1] packed2[0] mem[0] mem[1] mem[2] mem[3] {fst_grid} {pkts} word.byte_ word.pair.hi word.pair.lo {generate}")
    );
    // shortcut: a Verilator VCD writes struct scopes as modules, so a one-deep field shows bare
    // (a, byte_ for loose.a, word.byte_), fixed by a later piece
    assert_eq!(
        row_names("types-verilator-structs.vcd"),
        format!("clk pkt.valid pkt.state pkt.pair.hi pkt.pair.lo a b packed2[3] packed2[2] packed2[1] packed2[0] mem[0] mem[1] mem[2] mem[3] {grid} {pkts} byte_ word.pair.hi word.pair.lo {generate}")
    );
    assert_eq!(row_names("types-verilator.vcd"), format!("clk pkt a b packed2 mem[0] mem[1] mem[2] mem[3] {grid} pkts[0] pkts[1] word gen[0].p gen[1].p"));
    // shortcut: Icarus's foreach loop variables are rows too, fixed by a later piece
    assert_eq!(
        row_names("types-icarus.fst"),
        "clk packed2 pkt word gen[0].g gen[0].p gen[1].g gen[1].p $ivl_foreach0.i $ivl_foreach1.i $ivl_foreach1.j $ivl_foreach2.i $ivl_foreach3.i $ivl_foreach4.i $ivl_foreach4.j"
    );
    for file in ["types-nvc.fst", "types-nvc.vcd"] {
        assert_eq!(
            row_names(file),
            format!("clk pkt.valid pkt.state pkt.pair.hi pkt.pair.lo {pkts} rows[0][0] rows[0][1] rows[0][2] rows[1][0] rows[1][1] rows[1][2] link.addr link.data[0] link.data[1] link.data[2] gen(0).p.hi gen(0).p.lo gen(1).p.hi gen(1).p.lo"),
            "{file}"
        );
    }
}

/// `:add` of a struct, record or array: a group of all of it; of a plain scope, an error that
/// says how.
fn added(file: &str, cmd: &str) -> String {
    let mut ws = fixture(file);
    match wtf_core::command::run(&mut ws, cmd) {
        Ok(_) => wtf_core::rows::display_names(&ws.trace, &ws.rows).iter().map(|n| n.trim()).collect::<Vec<_>>().join(" "),
        Err(e) => e,
    }
}

#[test]
fn add_a_composite() {
    let pkt = "▾ pkt pkt.valid pkt.state pkt.pair.hi pkt.pair.lo";
    let pkts = "▾ pkts pkts[0].valid pkts[0].state pkts[0].pair.hi pkts[0].pair.lo pkts[1].valid pkts[1].state pkts[1].pair.hi pkts[1].pair.lo";
    for file in ["types-verilator-structs.fst", "types-nvc.fst"] {
        for (cmd, want) in [
            ("add types.pkt", pkt),
            ("add PKT", pkt),
            ("add pkt.pair", "▾ pkt.pair pkt.pair.hi pkt.pair.lo"),
            ("add types.pkts", pkts),
            // shortcut: the group and rows lose the generate scope (p, p.hi for gen(1).p), fixed
            // by a later piece
            ("add gen(1).p", "▾ p p.hi p.lo"),
        ] {
            assert_eq!(added(file, cmd), want, "{file}: {cmd}");
        }
    }
    let structs = "types-verilator-structs.fst";
    assert_eq!(added(structs, "add types.mem"), "▾ mem mem[0] mem[1] mem[2] mem[3]");
    assert_eq!(added(structs, "add word"), "▾ word word.byte_ word.pair.hi word.pair.lo");
    // shortcut: wellen's grid diagonal (verilator_fst_grid)
    assert_eq!(added(structs, "add types.grid[1]"), "▾ grid[1] grid[1][0] grid[1].grid[1]. grid[1][2]");
    assert_eq!(
        added(structs, "add types.gen"),
        "no signals directly in types.gen: add types.gen.** for all of it, or one of its scopes: types.gen[0], types.gen[1]"
    );
    // shortcut: the error mixes forms (types.gen(1), types.gen[1].p), fixed by a later piece
    assert_eq!(
        added("types-nvc.fst", "add types.gen[1]"),
        "no signals directly in types.gen(1): add types.gen(1).** for all of it, or one of its scopes: types.gen[1].p"
    );
    // shortcut: a Verilator VCD writes struct scopes as modules, so :add takes only the fields
    // directly in it (not pkt.pair) and pkts is a plain scope, fixed by a later piece
    let vcd = "types-verilator-structs.vcd";
    assert_eq!(added(vcd, "add types.pkt"), "▾ pkt valid state");
    assert!(added(vcd, "add types.pkts").starts_with("no signals directly in types.pkts"));
    assert_eq!(added(vcd, "add types.grid[1]"), "▾ grid[1] grid[1][0] grid[1][1] grid[1][2]");
    let mut ws = fixture(structs);
    let e = ws.resolve("pkt").unwrap_err();
    assert!(e.starts_with("unknown signal: pkt is a scope of 4 signals (types.pkt.valid"), "{e}");
}
