//! The workspace through its public API, as the interfaces use it (spec §5, §6, §9, §11).

mod common;
use common::*;
use wtf_core::workspace::Workspace;

#[test]
fn opens_a_trace() {
    let ws = open();
    assert_eq!((ws.file_name.as_str(), ws.trace.end(), ws.rows.len()), ("simple.vcd", 80, 0));
    assert_eq!(ws.cursor_list(), [(0, 0)]);
    let e = Workspace::open(&fixture("nope")).err().unwrap();
    assert!(e.contains("simple.nope"), "{e}");
}

#[test]
fn names_signals() {
    let mut ws = open();
    let (data, cpu_state) = (var(&ws, "top.cpu.data"), var(&ws, "top.cpu.state"));
    assert_eq!(ws.resolve("top.cpu.data"), Ok((data, None)));
    assert_eq!(ws.resolve("data"), Ok((data, None)));
    assert_eq!(ws.resolve("cpu.data"), Ok((data, None)));
    assert_eq!(ws.resolve("data[7]"), Ok((data, Some(7))));
    assert_eq!(ws.resolve("data[8]"), Err("data has no bit 8".into()));
    assert_eq!(ws.resolve("nope"), Err("unknown signal: nope".into()));
    assert_eq!(ws.resolve("state"), Err("ambiguous: state (2 signals: top.cpu.state, top.dma.state)".into()));
    // ambiguous in the trace, but only one of them shown: that one
    wtf_core::command::run(&mut ws, "add top.cpu.state").unwrap();
    assert_eq!(ws.resolve("state"), Ok((cpu_state, None)));
}

#[test]
fn names_the_shallowest_of_a_port_passed_down() {
    let vcd = "$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! clk $end\n\
        $scope module u0 $end\n$var wire 1 \" clk $end\n$scope module core $end\n$var wire 1 # clk $end\n$upscope $end\n$upscope $end\n\
        $scope module u1 $end\n$var wire 1 $ clk $end\n$var wire 1 % ready $end\n$upscope $end\n\
        $scope module u2 $end\n$var wire 1 & ready $end\n$upscope $end\n$upscope $end\n\
        $enddefinitions $end\n#0\n0!\n0\"\n0#\n0$\n0%\n0&\n#10\n1!\n";
    let path = std::env::temp_dir().join(format!("wtf-depth-{}.vcd", std::process::id()));
    std::fs::write(&path, vcd).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    // clk is in top, u0, u0.core and u1: top's is the one meant
    assert_eq!(ws.resolve("clk"), Ok((var(&ws, "top.clk"), None)));
    // two at the same depth stay ambiguous
    assert_eq!(ws.resolve("ready"), Err("ambiguous: ready (2 signals: top.u1.ready, top.u2.ready)".into()));
}

#[test]
fn undo_steps() {
    let mut ws = open();
    assert_eq!(ws.undo(), Err("already at oldest change".into()));
    ws.undoable(|ws| wtf_core::command::run(ws, "add top.clk")).unwrap();
    ws.undoable(|ws| wtf_core::command::run(ws, "add top.rst")).unwrap();
    // a step that changes nothing records nothing
    ws.undoable(|ws| wtf_core::command::run(ws, "zoom 2ns")).unwrap();
    assert_eq!(names(&ws), ["clk", "rst"]);
    ws.undo().unwrap();
    assert_eq!(names(&ws), ["clk"]);
    ws.undo().unwrap();
    assert!(ws.rows.is_empty());
    assert_eq!(ws.undo(), Err("already at oldest change".into()));
    ws.redo().unwrap();
    assert_eq!(names(&ws), ["clk"]);
    // a new change drops what could be redone
    ws.undoable(|ws| wtf_core::command::run(ws, "add top.cpu.busy")).unwrap();
    assert_eq!(ws.redo(), Err("already at newest change".into()));
    // mark / record: what the terminal UI does around a key
    let m = ws.mark();
    wtf_core::command::run(&mut ws, "rm top.clk").unwrap();
    ws.record(m);
    ws.undo().unwrap();
    assert_eq!(names(&ws), ["clk", "busy"]);
}

#[test]
fn marks_stay_where_they_are_put() {
    let mut ws = open();
    ws.view.set_cursor(10);
    ws.set_mark(None).unwrap();
    ws.view.set_cursor(30);
    ws.set_mark(Some(4)).unwrap();
    assert_eq!(ws.cursor_list(), [(0, 30), (1, 10), (4, 30)]);
    // jumping to a mark and moving on leaves the mark where it was
    ws.goto_mark(1).unwrap();
    assert_eq!(ws.view.cursor, 10);
    ws.view.set_cursor(12);
    assert_eq!(ws.cursor_list(), [(0, 12), (1, 10), (4, 30)]);
    // Tab: the next mark in time after the cursor, wrapping around
    ws.next_mark(true).unwrap();
    assert_eq!(ws.view.cursor, 30);
    ws.next_mark(true).unwrap();
    assert_eq!(ws.view.cursor, 10);
    ws.next_mark(false).unwrap();
    assert_eq!(ws.view.cursor, 30);
    // a mark is moved only on purpose
    ws.view.set_cursor(50);
    ws.set_mark(Some(1)).unwrap();
    assert_eq!(ws.cursor_list(), [(0, 50), (1, 50), (4, 30)]);
    // M removes the mark under the cursor; NM a given one
    ws.remove_mark(None).unwrap();
    assert_eq!(ws.remove_mark(None), Err("no mark under the cursor".into()));
    ws.remove_mark(Some(4)).unwrap();
    assert_eq!(ws.cursor_list(), [(0, 50)]);
    assert_eq!(ws.next_mark(true), Err("no marks: m puts one at the cursor".into()));
    // cursor 1 is the cursor, not a mark
    assert_eq!(ws.set_mark(Some(0)), Err("cursor 1 is the cursor; marks are 2-9".into()));
    assert_eq!(ws.remove_mark(Some(0)), Err("cursor 1 can't be removed".into()));
    assert_eq!(ws.remove_mark(Some(3)), Err("no cursor 4".into()));
    assert_eq!(ws.goto_mark(6), Err("no cursor 7".into()));
    // `'1` centers on the cursor, which stays
    ws.view.start = 0;
    assert_eq!(ws.goto_mark(0), Ok(()));
    assert_eq!(ws.view.cursor, 50);
    assert_eq!(ws.view.start, 50 - ws.view.width / 2 * ws.view.ticks / ws.view.cols);
    assert_eq!(ws.set_mark(Some(9)), Err("no cursor 10: marks are 2-9".into()));
    for _ in 0..8 {
        ws.set_mark(None).unwrap();
    }
    assert_eq!(ws.set_mark(None), Err("all 8 marks in use".into()));
}

#[test]
fn far_jumps_center() {
    let mut ws = open();
    // 4 columns per tick: 20 ticks across 80 columns
    wtf_core::command::run(&mut ws, "zoom 0.25").unwrap();
    assert_eq!((ws.view.start, ws.view.last_visible()), (0, 19));
    ws.marks[1] = Some(70);
    ws.view.set_cursor(5);
    assert_eq!(ws.view.start, 0, "a move on screen doesn't scroll");
    // off screen: centered on the mark
    ws.goto_mark(1).unwrap();
    assert_eq!((ws.view.cursor, ws.view.start), (70, 60));
    // on screen: no scroll
    wtf_core::command::run(&mut ws, "65ns").unwrap();
    assert_eq!((ws.view.cursor, ws.view.start), (65, 60));
    // a time, and a search match, off screen
    wtf_core::command::run(&mut ws, "20ns").unwrap();
    assert_eq!((ws.view.cursor, ws.view.start), (20, 10));
    ws.search("@clk !busy", true, 2).unwrap();
    assert_eq!((ws.view.cursor, ws.view.start), (45, 35));
}

#[test]
fn value_search() {
    let mut ws = with(&["top.cpu.data", "top.cpu.busy"]);
    ws.sel = 0;
    // a value searches the selected signal
    ws.search("0x96", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 20);
    // a match is where the signal changes to the value: X from the start isn't one
    assert_eq!(ws.search("x", false, 1).map(|_| ()), Err("not found before 20ns: x".into()));
    ws.search("z", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 50);
    ws.view.set_cursor(0);
    // an expression searches anything; `n` matches on
    ws.search("@clk busy", true, 2).unwrap();
    assert_eq!(ws.view.cursor, 25);
    assert_eq!(ws.search("data == 0x99", true, 1).map(|_| ()), Err("not found after 25ns: data == 0x99".into()));
    assert_eq!(ws.view.cursor, 25);
    // fewer matches than asked for: the last one
    ws.search("busy", false, 9).unwrap();
    assert_eq!(ws.view.cursor, 20);
    ws.sel = 1;
    ws.search("0", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 40);
    assert!(open().search("1", true, 1).is_err(), "no rows to search");
}

#[test]
fn presents_a_session() {
    let mut ws = with(&["top.clk"]);
    ws.saved = wtf_core::session::rows_toml(&ws);
    let text = "[cursors]\n2 = \"30ns\"\n[[rows]]\nsignal = \"top.cpu.data\"\n[[rows]]\nsignal = \"top.nope\"\n";
    let (msg, warned) = ws.present(text).unwrap();
    assert_eq!((msg.as_str(), warned), ("agent: presented 1 signals, 2 cursors; not in trace: top.nope", true));
    assert_eq!(names(&ws), ["data"]);
    // not saved: the user decides, and can go back
    assert!(ws.modified());
    ws.undo().unwrap();
    assert_eq!(names(&ws), ["clk"]);
    // a session that doesn't read changes nothing
    assert!(ws.present("rows = [").unwrap_err().starts_with("session: "));
    assert_eq!(names(&ws), ["clk"]);
}

#[test]
fn reloads() {
    let dir = Dir::new("reload");
    let trace = dir.0.join("t.vcd");
    std::fs::copy(fixture("vcd"), &trace).unwrap();
    let mut ws = Workspace::open(&trace).unwrap();
    for r in ["add top.clk", "add top.dma.state", "cursor 2 70ns"] {
        wtf_core::command::run(&mut ws, r).unwrap();
    }
    ws.undoable(|ws| wtf_core::command::run(ws, "add top.rst")).unwrap();
    assert_eq!(ws.reload(), Ok(("reloaded".into(), false)));
    assert_eq!(names(&ws), ["clk", "state", "rst"]);
    // a reload isn't undone, and clears undo
    assert!(ws.undo().is_err());
    wtf_core::command::run(&mut ws, "decode d check clock=clk busy=top.dma.state != 0").unwrap();
    wtf_core::command::run(&mut ws, "protocol dp clock=clk when=top.dma.state != 0").unwrap();
    // the simulation ran again: shorter, and without dma
    let text = std::fs::read_to_string(&trace).unwrap();
    let short: String = text.lines().filter(|l| !l.contains("dma") && !l.ends_with('&')).take_while(|l| *l != "#60").map(|l| format!("{l}\n")).collect();
    std::fs::write(&trace, short).unwrap();
    let (msg, _) = ws.reload().unwrap();
    assert_eq!(msg, "reloaded; not in trace: dp (unknown signal: top.dma.state), d (d: busy: unknown signal: top.dma.state), top.dma.state");
    assert_eq!(names(&ws), ["clk", "rst"]);
    assert_eq!((ws.trace.end(), ws.marks[1]), (55, Some(55)));
    // the instance that failed is kept, and made again once it can be
    assert_eq!((ws.decodes.len(), ws.protocols.len()), (1, 1));
    std::fs::write(&trace, &text).unwrap();
    assert_eq!(ws.reload(), Ok(("reloaded".into(), false)));
    assert!(ws.streams.iter().any(|s| s.name == "d") && ws.streams.iter().any(|s| s.name == "dp"));
    // a file that can't be read changes nothing
    std::fs::remove_file(&trace).unwrap();
    assert!(ws.reload().is_err());
    assert_eq!(names(&ws), ["clk", "rst"]);
}

#[test]
fn groups_and_selection() {
    let mut ws = with(&["top.clk", "top.cpu.data", "top.cpu.busy", "top.rst"]);
    (ws.sel, ws.anchor) = (2, Some(1));
    assert_eq!(ws.selection(), 1..3);
    ws.group("cpu").unwrap();
    assert_eq!(names(&ws), ["clk", "▾ cpu", "  data", "  busy", "rst"]);
    assert_eq!(selected(&ws), "▾ cpu");
    assert_eq!(ws.sel_vars(), [(var(&ws, "top.cpu.data"), None), (var(&ws, "top.cpu.busy"), None)]);
    // a header brings its whole group
    (ws.sel, ws.anchor) = (1, None);
    assert_eq!(ws.selection(), 1..4);
    ws.toggle_group();
    assert_eq!(names(&ws), ["clk", "▸ cpu (2)", "  data", "  busy", "rst"]);
    // the selection never rests on a hidden row
    ws.sel = 2;
    ws.fix_sel();
    assert_eq!(ws.sel, 1);
    ws.toggle_group();
    // a bus expands into its bits
    ws.sel = 2;
    ws.toggle_group();
    assert!(matches!(&ws.rows[2], wtf_core::rows::Row::Signal(s) if s.bits == 8));
    ws.sel = 1;
    ws.delete().unwrap();
    assert_eq!(names(&ws), ["clk", "rst"]);
    ws.rows.clear();
    assert_eq!(ws.delete(), Err("no rows".into()));
    assert_eq!(ws.group("g"), Err("no rows".into()));
}

#[test]
fn radixes() {
    let mut ws = with(&["top.cpu.data", "top.clk", "top.cpu.state"]);
    ws.sel = 0;
    // None steps to the next radix that fits
    ws.set_radix(None).unwrap();
    assert_eq!(radix_of(&ws, "top.cpu.data"), "bin");
    ws.set_radix(Some(wtf_core::radix::parse_name("q4.4").unwrap())).unwrap();
    assert_eq!(radix_of(&ws, "top.cpu.data"), "q");
    assert_eq!(ws.set_radix(Some(wtf_core::radix::parse_name("q4.12").unwrap())), Err("q4.12 doesn't fit a 8-bit signal".into()));
    ws.sel = 1;
    assert_eq!(ws.set_radix(None), Err("1-bit signals have no radix".into()));
    // a group: every signal it fits
    (ws.sel, ws.anchor) = (2, Some(0));
    ws.group("g").unwrap();
    ws.set_radix(Some(wtf_core::radix::parse_name("dec").unwrap())).unwrap();
    assert_eq!((radix_of(&ws, "top.cpu.data"), radix_of(&ws, "top.cpu.state")), ("dec", "dec"));
    ws.rows.clear();
    assert_eq!(ws.set_radix(None), Err("no rows".into()));
}

#[test]
fn adds_rows_where_the_selection_is() {
    let mut ws = with(&["top.clk", "top.cpu"]);
    assert_eq!(names(&ws), ["clk", "▾ cpu", "  state", "  data", "  busy"]);
    // below a row in a group: into it
    ws.sel = 3;
    ws.add_rows(&[var(&ws, "top.rst")]);
    assert_eq!(names(&ws), ["clk", "▾ cpu", "  state", "  data", "  rst", "  busy"]);
    // with the header selected: after the group
    ws.sel = 1;
    ws.add_rows(&[var(&ws, "top.dma.state")]);
    assert_eq!(names(&ws)[6], "dma.state");
    // a new group goes after the group the selection is in
    ws.sel = 2;
    ws.add_group("more", &[]);
    assert_eq!(names(&ws)[6], "▾ more");
    assert!(ws.shown(var(&ws, "top.rst")) && !open().shown(0));
    ws.add_rows(&[]);
}

#[test]
fn computed_vars_work_like_file_vars() {
    let mut ws = open();
    let b = |s: &str| wtf_core::trace::Val::Bits(s.into());
    let v = ws.trace.add_computed("calc.mode", Some(4), vec![(0, b("0000")), (25, b("0011")), (55, b("1010"))]).unwrap();
    // a row, by its path
    wtf_core::command::run(&mut ws, "add calc.mode").unwrap();
    assert_eq!(names(&ws), ["mode"]);
    assert_eq!(ws.resolve("mode"), Ok((v, None)));
    // value search on the row, an expression over it, w / b's changes
    ws.search("0xA", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 55);
    let q = ws.query("@clk calc.mode == 3").unwrap();
    let found = wtf_core::query::find(&ws.trace, &q, &wtf_core::query::Search { from: 0, to: None, forward: true, merge: true, runs: 9, matches: 99 });
    assert_eq!(found.runs.iter().map(|r| (r.first, r.last)).collect::<Vec<_>>(), [(25, 45)]);
    let mut budget = 9;
    assert_eq!(wtf_core::query::changes(&mut ws.trace, (v, None), 0, 80, &mut budget), (vec![25, 55], false));
    // in a session it is named like any signal
    assert!(wtf_core::session::rows_toml(&ws).contains("signal = \"calc.mode\""));
}

#[test]
fn slices_and_concatenations() {
    use wtf_core::trace::Val;
    let mut ws = with(&["top.cpu.data[7:4]"]);
    let hi = var(&ws, "top.cpu.data[7:4]");
    assert_eq!(names(&ws), ["data[7:4]"]);
    // named in any form, it is the same var; data: 0x43 @10, 0x96 @20
    assert_eq!(ws.resolve("data[7:4]"), Ok((hi, None)));
    assert_eq!(ws.resolve("cpu.data[ 7:4 ]"), Ok((hi, None)));
    assert_eq!((ws.trace.vars[hi].width, ws.trace.value_at(hi, 12), ws.trace.value_at(hi, 20)), (Some(4), Some(Val::Bits("0100".into())), Some(Val::Bits("1001".into()))));
    assert_eq!(ws.resolve("data[8:0]"), Err("top.cpu.data has no bits 8:0 (8 bits)".into()));
    assert_eq!(wtf_core::command::run(&mut ws, "add data[7:4]"), Err("already shown: data[7:4]".into()));
    // busy (high 20 - 40) above data's bit 0 (1, 0 @20, 1 @30)
    ws.search("{busy, data[0]} == 2", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 20);
    let cat = var(&ws, "{top.cpu.busy,top.cpu.data[0:0]}");
    assert_eq!(ws.resolve("{cpu.busy,data[0]}"), Ok((cat, None)));
    wtf_core::command::run(&mut ws, "add {busy,data[0]}").unwrap();
    assert_eq!(names(&ws), ["data[7:4]", "{top.cpu.busy,top.cpu.data[0:0]}"]);
    // sessions and :e make them again
    let toml = wtf_core::session::to_toml(&ws);
    let mut other = open();
    assert_eq!(wtf_core::session::apply(&mut other, &toml), Ok(vec![]));
    assert_eq!(names(&other), names(&ws));
    ws.reload().unwrap();
    assert_eq!(names(&ws), ["data[7:4]", "{top.cpu.busy,top.cpu.data[0:0]}"]);
    let hi = var(&ws, "top.cpu.data[7:4]");
    assert_eq!(ws.trace.value_at(hi, 20), Some(Val::Bits("1001".into())));
}

#[test]
fn concatenation_of_sv_numbers() {
    use wtf_core::trace::Val;
    let mut ws = open();
    let b = |s: &str| Val::Bits(s.into());
    // data: x @0, 0x43 @10, 0x96 @20; busy high 20 - 40. A sized number is a constant part.
    let (cat, _) = ws.resolve("{cpu.data, 1'b0}").unwrap();
    assert_eq!((ws.trace.vars[cat].width, ws.trace.value_at(cat, 12), ws.trace.value_at(cat, 20)), (Some(9), Some(b("010000110")), Some(b("100101100"))));
    let (cat, _) = ws.resolve("{1'b1, cpu.busy}").unwrap();
    assert_eq!((ws.trace.value_at(cat, 25), ws.trace.value_at(cat, 45)), (Some(b("11")), Some(b("10"))));
    // it is named again as the same var, and a number needs a width
    assert_eq!(ws.resolve("{cpu.data,1'b0}").unwrap().0, ws.resolve("{cpu.data, 1'b0}").unwrap().0);
    assert_eq!(ws.resolve("{cpu.data, 0}"), Err("0: a number in a concatenation needs a width, as 1'b0".into()));
    assert_eq!(ws.resolve("{'1, cpu.busy}"), Err("'1: a number in a concatenation needs a width, as 1'b0".into()));
}

#[test]
fn computed_expressions() {
    use wtf_core::command::run;
    use wtf_core::trace::Val;
    let dir = Dir::new("arith");
    let trace = dir.0.join("t.vcd");
    std::fs::copy(fixture("vcd"), &trace).unwrap();
    let mut ws = Workspace::open(&trace).unwrap();
    // data: x @0, 0x43 @10, 0x96 @20; an expression's var is as wide as its value can need
    run(&mut ws, "signal next = data + 1").unwrap();
    let next = var(&ws, "next");
    assert_eq!(ws.trace.vars[next].width, Some(9));
    assert_eq!(ws.trace.value_at(next, 0), Some(Val::Bits("xxxxxxxxx".into())));
    assert_eq!(ws.trace.value_at(next, 22), Some(Val::Bits("010010111".into())));
    // one that can be negative is signed: two's complement, sdec, signed in searches
    run(&mut ws, "signal d = data - 0x80").unwrap();
    let d = var(&ws, "d");
    assert_eq!((ws.trace.vars[d].width, ws.trace.vars[d].signed), (Some(10), true));
    assert_eq!(ws.trace.value_at(d, 12), Some(Val::Bits(format!("{:010b}", -61i32 as u32 & 0x3FF))));
    run(&mut ws, "add d").unwrap();
    assert_eq!(ws.rows.last().and_then(|r| r.sig()).map(|s| s.radix), Some(wtf_core::radix::SDEC));
    ws.search("d < 0", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 10);
    ws.search("d > 0", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 20);
    // named in a row as written, by full paths: naming it again finds the same var
    run(&mut ws, "add (data + 1) top.cpu.busy").unwrap();
    assert_eq!(names(&ws)[names(&ws).len() - 2], "(top.cpu.data+1)");
    assert_eq!(ws.resolve("(top.cpu.data+1)"), ws.resolve("( data+ 1)"));
    // a search computes without a var
    ws.view.cursor = 0;
    ws.search("(data + 1)[7:0] == 0x97", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 20);
    assert_eq!(run(&mut ws, "signal p = data + past(data)"), Err("data + past(data): past, rose, fell and stable need a clock; use them in find with @clk".into()));
    // a session keeps them; :e works them out again
    let toml = wtf_core::session::to_toml(&ws);
    assert!(toml.contains("expr = \"data - 0x80\""), "{toml}");
    let mut other = Workspace::open(&trace).unwrap();
    assert_eq!(wtf_core::session::apply(&mut other, &toml), Ok(vec![]));
    assert_eq!((names(&other), wtf_core::session::to_toml(&other)), (names(&ws), toml));
    ws.reload().unwrap();
    assert!(ws.trace.vars[var(&ws, "d")].signed);
}

#[test]
fn named_signals() {
    use wtf_core::command::run;
    use wtf_core::trace::Val;
    let dir = Dir::new("named");
    let trace = dir.0.join("t.vcd");
    std::fs::copy(fixture("vcd"), &trace).unwrap();
    let mut ws = Workspace::open(&trace).unwrap();
    // data: 0x43 @10, 0x96 @20
    run(&mut ws, "signal hi = data[7:4]").unwrap();
    run(&mut ws, "signal pair = {busy, data[0]}").unwrap();
    run(&mut ws, "add hi pair {busy, data[1]}").unwrap();
    assert_eq!(names(&ws), ["hi", "pair", "{top.cpu.busy,top.cpu.data[1:1]}"]);
    let hi = var(&ws, "hi");
    assert_eq!((ws.trace.vars[hi].width, ws.trace.value_at(hi, 12)), (Some(4), Some(Val::Bits("0100".into()))));
    ws.search("pair == 2", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 20);
    assert_eq!(run(&mut ws, "signal a b = clk"), Err("not a signal name: a b".into()));
    assert_eq!(run(&mut ws, "signal top.clk = rst"), Err("top.clk is a signal of the file".into()));
    assert_eq!(run(&mut ws, "signal x = nope"), Err("unknown signal: nope".into()));
    // defining it again replaces it, in place
    run(&mut ws, "signal hi = data[7:5]").unwrap();
    assert_eq!((var(&ws, "hi"), ws.trace.vars[hi].width), (hi, Some(3)));
    run(&mut ws, "signal hi = data[7:4]").unwrap();
    // :rm by the names rows show
    run(&mut ws, "rm {busy, data[1]}").unwrap();
    assert_eq!(names(&ws), ["hi", "pair"]);
    // a session keeps them, before the rows that show them
    let toml = wtf_core::session::to_toml(&ws);
    assert!(toml.contains("[[signal]]\nname = \"hi\"\nexpr = \"data[7:4]\"\n"), "{toml}");
    let mut other = Workspace::open(&trace).unwrap();
    assert_eq!(wtf_core::session::apply(&mut other, &toml), Ok(vec![]));
    assert_eq!((names(&other), wtf_core::session::to_toml(&other)), (names(&ws), toml));
    let bad = "[[signal]]\nname = \"x\"\nexpr = \"nope\"\n[[signal]]\nname = \"y\"\n";
    assert_eq!(wtf_core::session::apply(&mut other, bad), Ok(vec!["signal x: unknown signal: nope".into(), "signal: needs a name and an expr".into()]));
    // the simulation ran again: :e works them out from the new values
    let text = std::fs::read_to_string(&trace).unwrap().replacen("b1000011 $", "b11110011 $", 1);
    std::fs::write(&trace, text).unwrap();
    ws.reload().unwrap();
    assert_eq!(names(&ws), ["hi", "pair"]);
    assert_eq!(ws.trace.value_at(var(&ws, "hi"), 12), Some(Val::Bits("1111".into())));
    // ran again without busy: pair can't be made, and its row goes
    let text: String = std::fs::read_to_string(&trace).unwrap().lines().filter(|l| !l.contains('%')).map(|l| format!("{l}\n")).collect();
    std::fs::write(&trace, text).unwrap();
    assert_eq!(ws.reload(), Ok(("reloaded; not in trace: pair (unknown signal: busy), pair".into(), true)));
    assert_eq!(names(&ws), ["hi"]);
}

#[test]
fn computed_edge_cases() {
    use wtf_core::trace::Val;
    let mut ws = open();
    let b = |s: &str| Val::Bits(s.into());
    // a part with no value yet is unknown bits in a concatenation
    ws.trace.add_computed("calc.late", Some(1), vec![(40, b("1"))]).unwrap();
    let (cat, _) = ws.resolve("{calc.late, busy}").unwrap();
    assert_eq!((ws.trace.value_at(cat, 30), ws.trace.value_at(cat, 45)), (Some(b("x1")), Some(b("10"))));
    // weak values (h / l) match as 1 / 0
    ws.trace.add_computed("calc.weak", Some(1), vec![(0, b("l")), (50, b("h"))]).unwrap();
    ws.search("calc.weak == 1", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 50);
    // string signals compare as text, and have no edges
    ws.trace.add_computed("calc.txt", None, vec![(0, Val::Text("idle".into())), (30, Val::Text("run".into()))]).unwrap();
    ws.view.jump(0);
    ws.search("calc.txt == run", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 30);
    assert_eq!(ws.query("fell(calc.txt)").err().unwrap(), "fell(calc.txt): not a bit signal");
}

/// `:e` makes named signals and instances that use ones made after them.
#[test]
fn reloads_in_any_order() {
    let mut ws = open();
    for c in ["signal lo = top.cpu.data[3:0]", "signal hi = top.cpu.data[7:4]", "signal lo = hi[1:0]", "decode p check clock=clk b=top.cpu.busy", "decode f pair request=p response=p", "decode p check clock=clk b=lo == 1"] {
        wtf_core::command::run(&mut ws, c).unwrap();
    }
    assert_eq!(ws.signals.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["lo", "hi"]);
    // the pair before the check it reads, as a session written by hand may list them
    ws.decodes.reverse();
    assert_eq!(ws.reload(), Ok(("reloaded".into(), false)));
    assert!(ws.streams.iter().any(|s| s.name == "f"));
}

/// `:e` reports only what still can't be made after its passes: a named signal over one the rerun
/// dropped, and the one over it, but not one that uses a signal listed after it.
#[test]
fn reloads_and_reports_whats_left() {
    let dir = Dir::new("reload-left");
    let trace = dir.0.join("t.vcd");
    let text = std::fs::read_to_string(fixture("vcd")).unwrap();
    std::fs::write(&trace, &text).unwrap();
    let mut ws = Workspace::open(&trace).unwrap();
    for c in ["signal lo = top.cpu.data[3:0]", "signal hi = top.cpu.data[7:4]", "signal lo = hi[1:0]", "signal ds = top.dma.state[0]", "signal dd = ds[0]"] {
        wtf_core::command::run(&mut ws, c).unwrap();
    }
    std::fs::write(&trace, text.lines().filter(|l| !l.contains("dma") && !l.ends_with('&')).map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    let (msg, _) = ws.reload().unwrap();
    assert_eq!(msg, "reloaded; not in trace: ds (unknown signal: top.dma.state[0]), dd (unknown signal: ds[0])");
    assert!(ws.trace.by_path.contains_key("lo"), "lo is made in a second pass");
}

#[test]
fn first_value_after_the_start_is_a_change() {
    // axi4.vcd gives its signals their first value at 2ns, after the clock's at 0
    let mut ws = Workspace::open(format!("{}/../tests/data/axi4.vcd", env!("CARGO_MANIFEST_DIR")).as_ref()).unwrap();
    wtf_core::command::run(&mut ws, "add top.m_axi_rresp top.aclk").unwrap();
    let (t, v) = (&ws.trace, ws.trace.by_path["top.m_axi_rresp"]);
    assert_eq!((t.value_at(v, 1), t.next_change(v, 0)), (None, Some(2)));
    assert_eq!(t.changes_in(v, 0, 3), wtf_core::trace::Changes::One);
    // the clock's value at 0 is its initial value, not a change
    let clk = t.by_path["top.aclk"];
    assert_eq!(t.changes_in(clk, 0, 1), wtf_core::trace::Changes::None);
}
