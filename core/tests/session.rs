//! Session files (spec §11): what they hold, how they read back, and what they warn about.

mod common;
use common::*;
use wtf_core::session;

#[test]
fn round_trip() {
    let mut ws = with(&["top.clk", "top.cpu"]);
    for c in [
        "scope top.cpu",
        "select data",
        "radix dec",
        "color red",
        "select busy",
        "cursor 2 35ns",
        "cursor 3 50ns",
        "cursor 2",
        "zoom 2ns",
        "unit ps",
    ] {
        wtf_core::command::run(&mut ws, c).unwrap();
    }
    ws.sel = 2;
    ws.toggle_group(); // cpu.state expanded into its bits
    let text = session::to_toml(&ws);
    assert!(text.starts_with("# wtf session\nversion = 1\n"), "{text}");
    assert!(
        text.contains("radix = \"dec\"\ncolor = \"red\"")
            && text.contains("open = [\"state\"]")
            && text.contains("[cursors]\n1 = \"35ns\"\n2 = \"35ns\"\n3 = \"50ns\"\n"),
        "{text}"
    );
    let mut back = open();
    assert_eq!(session::apply(&mut back, &text), Ok(vec![]));
    assert_eq!(session::to_toml(&back), text);
    // a named mark is a table; one without a name stays a time
    wtf_core::command::run(&mut back, "cursor 3 name=done note=\"last \"").unwrap();
    let text = session::to_toml(&back);
    assert!(
        text.contains(
            "2 = \"35ns\"\n\n[cursors.3]\ntime = \"50ns\"\nname = \"done\"\nnote = \"last\"\n"
        ),
        "{text}"
    );
    let mut again = open();
    assert_eq!(session::apply(&mut again, &text), Ok(vec![]));
    assert_eq!(session::to_toml(&again), text);
    // the marks' times, not their names: `back` was renamed above
    let times = |ws: &wtf_core::workspace::Workspace| {
        ws.marks()
            .iter()
            .map(|m| m.as_ref().map(|m| m.time))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        (&back.rows, times(&back), back.view.cursor),
        (&ws.rows, times(&ws), 35)
    );
    assert_eq!(back.scope.as_deref(), Some("top.cpu"));
    assert!(text.contains("unit = \"ps\"\n"), "{text}");
    assert_eq!(back.unit, ws.unit);
    // what was loaded is what `modified` compares with; a mark named since is a change (§5 "Undo")
    assert!(!again.modified() && back.modified());
    assert_eq!(session::layout_toml(&again), again.saved);
}

/// A struct or array row keeps its open levels, named from the row's name
/// (tests/data/types-verilator-structs.fst, as in types.rs).
#[test]
fn composite_rows() {
    let types = || {
        wtf_core::workspace::Workspace::open(&std::path::PathBuf::from(format!(
            "{}/../tests/data/types-verilator-structs.fst",
            env!("CARGO_MANIFEST_DIR")
        )))
        .unwrap()
    };
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
    let warn = session::apply(
        &mut back,
        &text.replace(r#""pkts[1]""#, r#""pkts[7]", "pkt.pair""#),
    )
    .unwrap();
    assert_eq!(
        warn,
        [
            "types.pkts has no pkts[7] to open",
            "types.pkts has no pkt.pair to open"
        ]
    );
}

/// A field's radix and colour are its own (spec §6): set on its line, kept in `fields`, a
/// colour taken from the row where it has none.
#[test]
fn field_looks() {
    use wtf_core::{command, radix};
    let types = || {
        wtf_core::workspace::Workspace::open(&std::path::PathBuf::from(format!(
            "{}/../tests/data/types-verilator-structs.fst",
            env!("CARGO_MANIFEST_DIR")
        )))
        .unwrap()
    };
    let mut ws = types();
    wtf_core::command::run(&mut ws, "add types.pkt").unwrap();
    // select a line by its signal's path
    let to = |ws: &mut wtf_core::workspace::Workspace, path: &str| {
        let s = ws.rows[0].sig().unwrap().clone();
        let at = ws
            .lines()
            .into_iter()
            .find(|&(_, b)| ws.trace.vars[ws.probe(&s, b).0].path == path)
            .unwrap_or_else(|| panic!("{path} not open"));
        (ws.sel, ws.bit) = at;
    };
    ws.toggle_group(); // pkt
    to(&mut ws, "types.pkt.pair");
    ws.toggle_group();
    to(&mut ws, "types.pkt.pair.hi");
    let var = |ws: &wtf_core::workspace::Workspace, p: &str| ws.trace.by_path[p];
    let (pkt, hi, lo) = (
        var(&ws, "types.pkt"),
        var(&ws, "types.pkt.pair.hi"),
        var(&ws, "types.pkt.pair.lo"),
    );
    let look = |ws: &wtf_core::workspace::Workspace, v| ws.look(ws.rows[0].sig().unwrap(), v);
    // the field's radix, not the row's
    command::run(&mut ws, "radix dec").unwrap();
    assert_eq!(
        (
            look(&ws, hi).radix,
            look(&ws, lo).radix,
            look(&ws, pkt).radix
        ),
        (2, radix::DEFAULT, radix::DEFAULT)
    );
    // the row's colour reaches the fields without one of their own
    to(&mut ws, "types.pkt");
    command::run(&mut ws, "color red").unwrap();
    to(&mut ws, "types.pkt.pair.hi");
    command::run(&mut ws, "color blue").unwrap();
    assert_eq!(
        (look(&ws, hi).color, look(&ws, lo).color),
        (Some(5), Some(0))
    );
    let text = session::to_toml(&ws);
    assert!(
        text.contains(
            r#"color = "red"
open = ["pkt", "pkt.pair"]

[[rows.fields]]
field = "pkt.pair.hi"
radix = "dec"
color = "blue"
"#
        ),
        "{text}"
    );
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
    assert_eq!(
        warn,
        [
            "unit: bad time unit: furlong (s, ms, us, ns, ps, fs)",
            "rows: top.cpu.data: unknown radix: roman (radixes: hex, bin, dec, sdec, oct, ascii)",
            "rows: top.clk: radix q4.4 doesn't fit top.clk",
            "rows: top.clk: unknown colour: pink (colours: red, orange, yellow, green, cyan, blue, magenta, gray)",
            "nested group flattened: inner",
            "rows [5]: entry without signal or group",
            "rows [5]: unknown key: label (keys: signal, group)",
            "not in trace: top.gone",
            "view.zoom: bad time",
            "view.time: bad time: soon (a number and a unit, 20ns or 1.5us; a plain number is file ticks)",
            "view: unknown key: speed (keys: time, to, zoom)",
            "cursors: no cursor 12 (1-9)",
            "session: unknown key: colour (keys: version, scope, unit, signal, names, protocol, decode, rows, view, cursors, trail)",
        ]
    );
    // what could be read was: a signal shown twice only once
    assert_eq!(names(&ws), ["data", "clk", "▾ g", "  rst"]);
    assert_eq!(ws.view.cursor, 20);
}

#[test]
fn refuses_what_it_cant_read() {
    let mut ws = open();
    assert_eq!(
        session::apply(&mut ws, "version = 2"),
        Err("session version 2 is newer than this wtf supports (1)".into())
    );
    assert_eq!(
        session::apply(&mut ws, "version = \"one\""),
        Err("session: version must be a number".into())
    );
    assert!(
        session::apply(&mut ws, "[[rows]\n")
            .unwrap_err()
            .starts_with("session: ")
    );
    assert_eq!(
        session::apply(&mut ws, "rows = 3"),
        Ok(vec!["rows: must be a list".to_string()])
    );
    // a trail that isn't a list warns as `rows` does, and the session still loads
    assert_eq!(
        session::apply(&mut ws, "trail = \"oops\""),
        Ok(vec!["trail: must be a list".to_string()])
    );
    // a time as plain ticks, and one that isn't a time
    assert_eq!(
        session::apply(&mut ws, "[cursors]\n1 = 30\n2 = true"),
        Ok(vec!["cursors.2: bad time".to_string()])
    );
    assert_eq!(
        (ws.view.cursor, ws.marks()[1].as_ref().map(|m| m.time)),
        (30, None)
    );
    // a section in the wrong shape holds nothing and says so where it sits
    for (bad, msg) in [
        ("signal = 3", "signal: must be a list"),
        ("names = 3", "names: must be a list"),
        ("protocol = 3", "protocol: must be a list"),
        ("decode = 3", "decode: must be a list"),
        ("view = 3", "view: must be a table"),
        ("cursors = 3", "cursors: must be a table"),
    ] {
        assert_eq!(session::apply(&mut open(), bad), Ok(vec![msg.to_string()]));
    }
    // at its place in the file, with the rest of the session still loaded
    let mut next = open();
    let warn = session::apply(
        &mut next,
        "unit = \"ns\"\nsignal = 3\n[[rows]]\nsignal = \"top.clk\"",
    )
    .unwrap();
    assert_eq!(warn, ["signal: must be a list"]);
    assert!(next.unit.is_some());
    assert_eq!(names(&next), ["clk"]);
    // a nested `rows` that isn't a list names the group it sits in
    assert_eq!(
        session::apply(&mut open(), "[[rows]]\ngroup = \"g\"\nrows = 3"),
        Ok(vec!["rows [1].rows: must be a list".to_string()])
    );
}

/// A key an entry doesn't take warns as one of the file's does (spec §11); the entry still loads.
#[test]
fn unknown_entry_keys_warn() {
    let mut ws = open();
    let text = r#"
        [[signal]]
        name = "low"
        expr = "top.cpu.data[3:0]"
        colour = "red"
        [[trail]]
        tool = "find"
        ask = "top.cpu.busy"
        answer = "1 matches"
        when = "now"
        [[signal]]
        name = ""
        expr = "top.clk"
        [[rows]]
        signal = "top.clk"
        colour = "red"
        radix = "enum"
        fields = [{ field = "x", colour = "red" }]
        [[rows]]
        group = "g"
        colapsed = true
    "#;
    assert_eq!(
        session::apply(&mut ws, text).unwrap(),
        [
            "signal low: unknown key: colour (keys: name, expr)",
            "signal [2]: needs a name and an expr",
            "rows: top.clk: enum: the file names no values of top.clk",
            "top.clk has no field x",
            "rows: top.clk: field x: unknown key: colour (keys: field, radix, color)",
            "rows: top.clk: unknown key: colour (keys: signal, radix, color, open, fields)",
            "rows [2]: unknown key: colapsed (keys: group, collapsed, rows)",
            "trail [1]: unknown key: when (keys: tool, ask, answer, time)",
        ]
    );
    assert!(ws.signals.iter().any(|s| s.0 == "low"));
    assert_eq!(ws.trail.len(), 1);
}

#[test]
fn files() {
    let dir = Dir::new("session");
    let trace = dir.0.join("run.fst");
    assert_eq!(
        session::default_path(&trace),
        dir.0.join("run.fst.session.toml")
    );
    let mut ws = with(&["top.clk"]);
    ws.session_path = session::default_path(&trace);
    assert!(ws.modified());
    let msg = wtf_core::command::save(&mut ws, "").unwrap();
    assert_eq!(
        msg.text(),
        format!("written: {}", ws.session_path.display())
    );
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

/// A `[[protocol]] wasm=` entry the file is loaded for: the entry is the file's, so it reads and
/// writes back as its name and that file (spec §11).
#[test]
fn wasm_protocol_round_trip() {
    let mut ws = open();
    ws.push_loaded(loaded("probe", &[]));
    let text = "\n[[protocol]]\nname = \"probe\"\nwasm = \"dir/probe.wasm\"\n";
    assert_eq!(session::apply(&mut ws, text), Ok(vec![]));
    let p = ws.protocols().iter().find(|p| p.name == "probe").unwrap();
    assert_eq!(
        (p.wasm.as_deref(), p.when.as_str(), p.roles.len()),
        (Some("dir/probe.wasm"), "", 0)
    );
    let back = session::to_toml(&ws);
    assert!(
        back.contains("[[protocol]]\nname = \"probe\"\nwasm = \"dir/probe.wasm\"\n"),
        "{back}"
    );
    assert!(!back.contains("roles = []"), "{back}");
}

/// The same entry with the file not loaded: it says so and keeps the entry, so the session still
/// names its protocol and the next `:e` tries the file again (spec §11).
#[test]
fn wasm_protocol_without_the_load() {
    let mut ws = open();
    let text = "\n[[protocol]]\nname = \"probe\"\nwasm = \"dir/probe.wasm\"\n";
    assert_eq!(
        session::apply(&mut ws, text),
        Ok(vec![
            "protocol probe: wasm dir/probe.wasm is not loaded".into()
        ])
    );
    assert!(
        session::to_toml(&ws)
            .contains("[[protocol]]\nname = \"probe\"\nwasm = \"dir/probe.wasm\"\n")
    );
}

/// The trail is last, so a link's view is the shared file without it (spec §11).
#[test]
fn trail_comes_last() {
    let mut ws = open();
    let text =
        "[cursors]\n1 = \"20ns\"\n\n[[trail]]\ntool = \"find\"\nask = \"q\"\nanswer = \"a\"\n";
    assert_eq!(session::apply(&mut ws, text), Ok(vec![]));
    assert_eq!(ws.trail.len(), 1);
    let link = session::to_link_toml(&ws);
    assert_eq!(session::without_trail(&link), session::to_shared_toml(&ws));
    assert!(
        link.contains("\n[[trail]]\ntool = \"find\"\nask = \"q\"\nanswer = \"a\"\n"),
        "{link}"
    );
}

/// Splitting the trail off skips a `"""` string that holds the same line.
#[test]
fn trail_split_skips_strings() {
    let text = "version = 1\n\n[cursors.3]\ntime = \"20ns\"\nnote = \"\"\"\na\n[[trail]]\nb\"\"\"\n\n[[trail]]\ntool = \"find\"\nask = \"q\"\nanswer = \"a\"\n";
    assert_eq!(
        session::without_trail(text),
        "version = 1\n\n[cursors.3]\ntime = \"20ns\"\nnote = \"\"\"\na\n[[trail]]\nb\"\"\"\n"
    );
    assert_eq!(session::read_trail(&open(), text).len(), 1);
}

/// A note with `"""` in one line is written as a literal string: the trail still splits off.
#[test]
fn trail_split_one_line_quotes() {
    let mut ws = open();
    let text = "[cursors.3]\ntime = \"20ns\"\nnote = 'say \"\"\" here'\n\n[[trail]]\ntool = \"find\"\nask = \"q\"\nanswer = \"a\"\n";
    assert_eq!(session::apply(&mut ws, text), Ok(vec![]));
    let link = session::to_link_toml(&ws);
    assert!(link.contains("say \"\"\" here"), "{link}");
    assert_eq!(session::without_trail(&link), session::to_shared_toml(&ws));
}

/// A row that is neither a signal nor a group is dropped, and says so.
#[test]
fn row_with_bad_signal_warns() {
    let mut ws = open();
    let warn = session::apply(&mut ws, "[[rows]]\nsignal = 3\n").unwrap();
    assert_eq!(warn, ["rows [1]: entry without signal or group"]);
    assert!(ws.rows.is_empty());
}

/// A hand-written file describes without failing: junk rows skip, unparseable times clear, and
/// the cursor is listed even when the file names none.
#[test]
fn describe_skips_gracefully() {
    use toml::Value;
    let ws = open();
    assert!(!ws.modified()); // a fresh window: the layout has no version line
    let mut f = session::File::default();
    f.rows
        .push(session::model::Row::Other(Value::String("x".into())));
    f.cursors.insert("2".into(), Value::String("soon".into()));
    f.cursors.insert("x".into(), Value::String("1ns".into()));
    let names: Vec<String> = Vec::new();
    let show = |t: u64| format!("{t}");
    let text = session::describe(
        &f,
        &session::Describe {
            names: &names,
            sel: 999,
            show: &show,
            visible: (String::from("0"), String::from("0")),
            trace: &ws.trace,
        },
    );
    assert_eq!(
        text,
        "rows (> selected):\ncursors, in time:\n  1: 0\nvisible: 0 - 0\n"
    );
}
