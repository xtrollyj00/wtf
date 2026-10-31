//! Session files (spec §11): what they hold, how they read back, and what they warn about.

mod common;
use common::*;
use wtf_core::session;

#[test]
fn round_trip() {
    let mut ws = with(&["top.clk", "top.cpu"]);
    for c in ["scope top.cpu", "select data", "radix dec", "color red", "select busy", "cursor 2 35ns", "cursor 3 50ns", "cursor 2", "zoom 2ns", "unit ps"] {
        wtf_core::command::run(&mut ws, c).unwrap();
    }
    ws.sel = 2;
    ws.toggle_group(); // cpu.state expanded into its bits
    let text = session::to_toml(&ws);
    assert!(text.starts_with("# wtf session\nversion = 1\n"), "{text}");
    assert!(text.contains("radix = \"dec\", color = \"red\"") && text.contains("open = [\"state\"]") && text.contains("[cursors]\n1 = \"35ns\"\n2 = \"35ns\"\n3 = \"50ns\"\n"), "{text}");
    let mut back = open();
    assert_eq!(session::apply(&mut back, &text), Ok(vec![]));
    assert_eq!(session::to_toml(&back), text);
    assert_eq!((&back.rows, back.marks, back.view.cursor), (&ws.rows, ws.marks, 35));
    assert_eq!(back.scope.as_deref(), Some("top.cpu"));
    assert!(text.contains("unit = \"ps\"\n"), "{text}");
    assert_eq!(back.unit, ws.unit);
    // what was loaded is what `modified` compares with
    assert!(!back.modified());
    assert_eq!(session::rows_toml(&back), back.saved);
}

/// A struct or array row keeps its open levels, named from the row's name
/// (tests/data/types-verilator-structs.fst, as in types.rs).
#[test]
fn composite_rows() {
    let types = || wtf_core::workspace::Workspace::open(&std::path::PathBuf::from(format!("{}/../tests/data/types-verilator-structs.fst", env!("CARGO_MANIFEST_DIR")))).unwrap();
    let mut ws = types();
    wtf_core::command::run(&mut ws, "add types.pkts types.pkt").unwrap();
    ws.sel = 0;
    ws.toggle_group(); // pkts
    ws.bit = Some(0);
    ws.toggle_group(); // pkts[1]
    let text = session::to_toml(&ws);
    assert!(text.contains("signal = \"types.pkts\"\nopen = [\"pkts\", \"pkts[1]\"]\n\n[[rows]]\nsignal = \"types.pkt\"\n"), "{text}");
    let mut back = types();
    assert_eq!(session::apply(&mut back, &text), Ok(vec![]));
    assert_eq!(session::to_toml(&back), text);
    assert_eq!(back.rows, ws.rows);
    // a level the row doesn't have is skipped with a warning
    let mut back = types();
    let warn = session::apply(&mut back, &text.replace(r#""pkts[1]""#, r#""pkts[7]", "pkt.pair""#)).unwrap();
    assert_eq!(warn, ["types.pkts has no pkts[7] to open", "types.pkts has no pkt.pair to open"]);
}

/// A field's radix and colour are its own (spec §6): set on its line, kept in `fields`, a
/// colour taken from the row where it has none.
#[test]
fn field_looks() {
    use wtf_core::{command, radix};
    let types = || wtf_core::workspace::Workspace::open(&std::path::PathBuf::from(format!("{}/../tests/data/types-verilator-structs.fst", env!("CARGO_MANIFEST_DIR")))).unwrap();
    let mut ws = types();
    wtf_core::command::run(&mut ws, "add types.pkt").unwrap();
    // select a line by its signal's path
    let to = |ws: &mut wtf_core::workspace::Workspace, path: &str| {
        let s = ws.rows[0].sig().unwrap().clone();
        let at = ws.lines().into_iter().find(|&(_, b)| ws.trace.vars[ws.probe(&s, b).0].path == path).unwrap_or_else(|| panic!("{path} not open"));
        (ws.sel, ws.bit) = at;
    };
    ws.toggle_group(); // pkt
    to(&mut ws, "types.pkt.pair");
    ws.toggle_group();
    to(&mut ws, "types.pkt.pair.hi");
    let var = |ws: &wtf_core::workspace::Workspace, p: &str| ws.trace.by_path[p];
    let (pkt, hi, lo) = (var(&ws, "types.pkt"), var(&ws, "types.pkt.pair.hi"), var(&ws, "types.pkt.pair.lo"));
    let look = |ws: &wtf_core::workspace::Workspace, v| ws.look(ws.rows[0].sig().unwrap(), v);
    // the field's radix, not the row's
    command::run(&mut ws, "radix dec").unwrap();
    assert_eq!((look(&ws, hi).radix, look(&ws, lo).radix, look(&ws, pkt).radix), (2, radix::DEFAULT, radix::DEFAULT));
    // the row's colour reaches the fields without one of their own
    to(&mut ws, "types.pkt");
    command::run(&mut ws, "color red").unwrap();
    to(&mut ws, "types.pkt.pair.hi");
    command::run(&mut ws, "color blue").unwrap();
    assert_eq!((look(&ws, hi).color, look(&ws, lo).color), (Some(5), Some(0)));
    let text = session::to_toml(&ws);
    assert!(text.contains(r#"color = "red"
open = ["pkt", "pkt.pair"]
fields = [{ field = "pkt.pair.hi", radix = "dec", color = "blue" }]"#), "{text}");
    let mut back = types();
    assert_eq!(session::apply(&mut back, &text), Ok(vec![]));
    assert_eq!(back.rows, ws.rows);
    // back to the defaults, the field keeps no look of its own
    command::run(&mut ws, "color off").unwrap();
    command::run(&mut ws, "radix hex").unwrap();
    assert!(ws.rows[0].sig().unwrap().looks.is_empty());
    // a field the row doesn't have is skipped with a warning
    let mut back = types();
    let warn = session::apply(&mut back, &text.replace("pkt.pair.hi", "pkt.pair.mid")).unwrap();
    assert_eq!(warn, ["types.pkt has no field pkt.pair.mid"]);
}

#[test]
fn warns_and_keeps_going() {
    let mut ws = open();
    let text = r#"
        version = 1
        colour = "pink"
        unit = "furlong"
        [view]
        zoom = "fast"
        time = "soon"
        speed = 3
        [cursors]
        1 = "20ns"
        12 = "30ns"
        [[rows]]
        signal = "top.cpu.data"
        radix = "roman"
        [[rows]]
        signal = "top.clk"
        radix = "q4.4"
        color = "pink"
        [[rows]]
        signal = "top.gone"
        [[rows]]
        group = "g"
        rows = [{ group = "inner", rows = [{ signal = "top.rst" }] }, { signal = "top.cpu.data" }]
        [[rows]]
        label = "what"
    "#;
    let warn = session::apply(&mut ws, text).unwrap();
    assert_eq!(warn, [
        "bad time unit: furlong (s, ms, us, ns, ps, fs)",
        "unknown radix: roman",
        "radix q4.4 doesn't fit top.clk",
        "unknown colour: pink (colours: red, orange, yellow, green, cyan, blue, magenta, gray)",
        "nested group flattened: inner",
        "rows: entry without signal or group",
        "unknown key: rows.label",
        "not in trace: top.gone",
        "view.zoom: bad time",
        "bad time: soon",
        "unknown key: view.speed",
        "cursors: no cursor 12 (1-9)",
        "unknown key: colour",
    ]);
    // what could be read was: a signal shown twice only once
    assert_eq!(names(&ws), ["data", "clk", "▾ g", "  rst"]);
    assert_eq!(ws.view.cursor, 20);
}

#[test]
fn refuses_what_it_cant_read() {
    let mut ws = open();
    assert_eq!(session::apply(&mut ws, "version = 2"), Err("session version 2 is newer than this wtf supports (1)".into()));
    assert_eq!(session::apply(&mut ws, "version = \"one\""), Err("session: version must be a number".into()));
    assert!(session::apply(&mut ws, "[[rows]\n").unwrap_err().starts_with("session: "));
    assert_eq!(session::apply(&mut ws, "rows = 3"), Ok(vec!["rows: must be a list".to_string()]));
    // a time as plain ticks, and one that isn't a time
    assert_eq!(session::apply(&mut ws, "[cursors]\n1 = 30\n2 = true"), Ok(vec!["bad time".to_string()]));
    assert_eq!((ws.view.cursor, ws.marks[1]), (30, None));
}

#[test]
fn files() {
    let dir = Dir::new("session");
    let trace = dir.0.join("run.fst");
    assert_eq!(session::default_path(&trace), dir.0.join("run.fst.session.toml"));
    let mut ws = with(&["top.clk"]);
    ws.session_path = session::default_path(&trace);
    assert!(ws.modified());
    let (msg, _) = wtf_core::command::save(&mut ws, "").unwrap();
    assert_eq!(msg, format!("written: {}", ws.session_path.display()));
    assert!(!ws.modified());
    // :w <file> makes that the session file in use
    let other = dir.0.join("other.toml");
    wtf_core::command::save(&mut ws, other.to_str().unwrap()).unwrap();
    assert_eq!(ws.session_path, other);
    let mut back = open();
    session::apply(&mut back, &std::fs::read_to_string(&other).unwrap()).unwrap();
    assert_eq!(names(&back), ["clk"]);
    assert!(session::write(&dir.0.join("no/such/dir/s.toml"), "x").is_err());
}
