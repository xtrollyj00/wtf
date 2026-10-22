//! Session files (spec §11): what they hold, how they read back, and what they warn about.

mod common;
use common::*;
use wtf_core::session;

#[test]
fn round_trip() {
    let mut ws = with(&["top.clk", "top.cpu"]);
    for c in ["scope top.cpu", "select data", "radix dec", "color red", "select busy", "cursor 2 35ns", "cursor 3 50ns", "cursor 2", "zoom 2ns"] {
        wtf_core::command::run(&mut ws, c).unwrap();
    }
    ws.sel = 2;
    ws.toggle_group(); // cpu.state expanded into its bits
    let text = session::to_toml(&ws);
    assert!(text.starts_with("# wtf session\nversion = 1\n"), "{text}");
    assert!(text.contains("radix = \"dec\", color = \"red\"") && text.contains("expanded = true") && text.contains("[cursors]\n1 = \"35ns\"\n2 = \"35ns\"\n3 = \"50ns\"\n"), "{text}");
    let mut back = open();
    assert_eq!(session::apply(&mut back, &text), Ok(vec![]));
    assert_eq!(session::to_toml(&back), text);
    assert_eq!((&back.rows, back.marks, back.view.cursor), (&ws.rows, ws.marks, 35));
    assert_eq!(back.scope.as_deref(), Some("top.cpu"));
    // what was loaded is what `modified` compares with
    assert!(!back.modified());
    assert_eq!(session::rows_toml(&back), back.saved);
}

#[test]
fn warns_and_keeps_going() {
    let mut ws = open();
    let text = r#"
        version = 1
        colour = "pink"
        [view]
        active_cursor = 2
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
        "unknown radix: roman",
        "unknown colour: pink (colours: red, orange, yellow, green, cyan, blue, magenta, gray)",
        "radix q4.4 doesn't fit top.clk",
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
