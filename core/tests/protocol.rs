//! Protocols written in Rust and the generic stateful decoders, put on a fixture's signals
//! (spec §16).

mod common;
use common::*;
use std::path::PathBuf;
use wtf_core::stream::{Decode, Sampler, Stream};
use wtf_core::trace::Val;
use wtf_core::workspace::{Msg, Workspace};

fn open(name: &str) -> Workspace {
    Workspace::open(&fixture("vcd").with_file_name(name)).unwrap()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|&(a, b)| (a.into(), b.into())).collect()
}

fn decode(
    name: &str,
    protocol: &str,
    clock: &str,
    signals: &[(&str, &str)],
    params: &[(&str, &str)],
) -> Decode {
    Decode {
        name: name.into(),
        protocol: protocol.into(),
        clock: clock.into(),
        signals: pairs(signals),
        params: pairs(params),
    }
}

const AXIS: &[(&str, &str)] = &[
    ("tvalid", "tvalid"),
    ("tready", "tready"),
    ("tdata", "tdata"),
    ("tlast", "tlast"),
    ("tkeep", "tkeep"),
    ("aresetn", "aresetn"),
];

fn stream<'a>(ws: &'a Workspace, name: &str) -> &'a Stream {
    ws.streams()
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no stream {name}"))
}

/// Each record: start, end, and each field as a number, or its text.
fn table(s: &Stream) -> Vec<(u64, u64, Vec<String>)> {
    let show = |v: &Option<Val>| match v {
        Some(Val::Bits(b)) => u64::from_str_radix(b, 2).map_or(b.clone(), |n| n.to_string()),
        Some(Val::Text(t)) => t.clone(),
        None => "-".into(),
    };
    s.records
        .iter()
        .map(|r| (r.start, r.end, r.fields.iter().map(show).collect()))
        .collect()
}

fn row(start: u64, end: u64, fields: &[&str]) -> (u64, u64, Vec<String>) {
    (start, end, fields.iter().map(|f| f.to_string()).collect())
}

/// The ten `<chan>_unknown` errors axilite.vcd's valids and readys make: every one is x from 2ns
/// to the first edge at 10ns, so the one cycle that samples them breaks `unknown` alone.
fn unknown_rows() -> Vec<(u64, u64, Vec<String>)> {
    let mut rows = Vec::new();
    for c in ["aw", "w", "b", "ar", "r"] {
        for side in ["valid", "ready"] {
            rows.push(row(
                10,
                20,
                &[
                    &format!("{c}_unknown"),
                    &format!("{c}{side} is x out of reset"),
                    "error",
                ],
            ));
        }
    }
    rows
}

/// axis.vcd: clk rises at 5, 15 ...; cycle k is driven at 5 + 10k. Frame A: beats 0xA0, 0xA1 (after
/// a stall that holds it), 0xA2 (tkeep 0b0011), 10 bytes; B: 0xB0; C: 0xC0, 0xC1; then 0xD0 is
/// dropped before its ready (105ns), 0xE0 changes to 0xE1 while stalled (125ns), and E: 0xE1.
#[test]
fn axis_beats_and_frames() {
    let mut ws = open("axis.vcd");
    let rx = ws
        .add_decode(decode("rx", "axis", "clk", AXIS, &[]))
        .unwrap();
    assert_eq!(rx, var(&ws, "rx.beat"));
    let beat = stream(&ws, "rx.beat");
    assert_eq!(
        beat.schema
            .iter()
            .map(|f| (f.name.as_str(), f.width))
            .collect::<Vec<_>>(),
        [("tdata", Some(32)), ("tkeep", Some(4)), ("tlast", Some(1))]
    );
    let data: Vec<(u64, String)> = table(beat)
        .into_iter()
        .map(|(s, _, f)| (s, format!("{:X}", f[0].parse::<u64>().unwrap())))
        .collect();
    let want = [
        (15, "A0"),
        (35, "A1"),
        (45, "A2"),
        (55, "B0"),
        (65, "C0"),
        (75, "C1"),
        (135, "E1"),
    ];
    assert_eq!(data, want.map(|(t, d)| (t, d.to_string())));
    // frames: beats, len in bytes, the bytes tkeep keeps (the last beat of the first: 2), complete
    let frames = table(stream(&ws, "rx.frame"));
    let want = [
        row(15, 55, &["3", "10", "a0 00 00 00 a1 00 00 00 a2 00", "1"]),
        row(55, 65, &["1", "4", "b0 00 00 00", "1"]),
        row(65, 85, &["2", "8", "c0 00 00 00 c1 00 00 00", "1"]),
        row(135, 145, &["1", "4", "e1 00 00 00", "1"]),
    ];
    assert_eq!(frames, want);
    // as signals; `tdata` still names the file's, not rx.beat.tdata
    assert_eq!(ws.resolve("tdata"), Ok((var(&ws, "top.tdata"), None)));
    ws.search("rx.frame.len == 8", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 65);
    wtf_core::command::run(&mut ws, "add rx.frame rx.frame.len").unwrap();
    // without tkeep a beat is all of tdata; without tlast there are no frames
    let mut ws = open("axis.vcd");
    ws.add_decode(decode(
        "a",
        "axis",
        "clk",
        &[&AXIS[..4], &AXIS[5..]].concat(),
        &[],
    ))
    .unwrap();
    assert_eq!(
        table(stream(&ws, "a.frame"))[0],
        row(
            15,
            55,
            &["3", "12", "a0 00 00 00 a1 00 00 00 a2 00 00 00", "1"]
        )
    );
    ws.add_decode(decode(
        "b",
        "axis",
        "clk",
        &[&AXIS[..3], &AXIS[5..]].concat(),
        &[],
    ))
    .unwrap();
    // tlast not given reads 1: every beat ends its own packet
    assert_eq!(
        table(stream(&ws, "b.frame"))[0],
        row(15, 25, &["1", "4", "a0 00 00 00", "1"])
    );
    assert_eq!(stream(&ws, "b.beat").schema.len(), 1);
}

/// Spec §16 "Results": a protocol declares what each of its streams is made from, so a decode's
/// own `frame` makes its `beat` intermediate. There, the 7 beats are what the 4 frames are made
/// from, and the decode still goes when no other one reads it.
#[test]
fn axis_stream_provenance() {
    let mut ws = open("axis.vcd");
    let names = |ws: &Workspace, v: Vec<usize>| {
        v.into_iter()
            .map(|i| ws.streams()[i].name.clone())
            .collect::<Vec<_>>()
    };
    // the line the TUI runs, and the sets its answer names
    let line = "decode rx axis clock=clk tvalid=tvalid tready=tready tdata=tdata tlast=tlast tkeep=tkeep aresetn=aresetn";
    assert_eq!(
        wtf_core::command::run(&mut ws, line),
        Ok(Some(Msg::Info(
            "rx: records rx.frame 4; violations rx.errors 2; made from rx.beat 7".into()
        )))
    );
    assert_eq!(names(&ws, ws.results("rx")), ["rx.frame", "rx.errors"]);
    assert_eq!(
        names(&ws, ws.stream_set("rx").unwrap()),
        ["rx.frame", "rx.errors"]
    );
    // named by the instance's names, and a stream made of the trace alone is made from nothing
    assert_eq!(stream(&ws, "rx.frame").inputs, ["rx.beat"]);
    assert!(stream(&ws, "rx.beat").inputs.is_empty() && stream(&ws, "rx.errors").inputs.is_empty());
    // its own frame does not read the decode; another decode's does
    ws.add_decode(decode(
        "f",
        "frame",
        "",
        &[("input", "rx.beat")],
        &[("gap", "100ns")],
    ))
    .unwrap();
    assert_eq!(
        wtf_core::command::run(&mut ws, "decode rx off"),
        Err("rx is read by f: remove it first".into())
    );
    assert_eq!(
        wtf_core::command::run(&mut ws, "decode f off"),
        Ok(Some(Msg::Info("f: removed".into())))
    );
    assert_eq!(
        wtf_core::command::run(&mut ws, "decode rx off"),
        Ok(Some(Msg::Info("rx: removed".into())))
    );
}

#[test]
fn axis_errors_are_the_injected_ones() {
    let mut ws = open("axis.vcd");
    ws.add_decode(decode("rx", "axis", "clk", AXIS, &[]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "rx.errors")),
        [
            row(105, 115, &["dropped", "tvalid fell before tready", "error"]),
            row(
                125,
                135,
                &["changed", "tdata changed while tvalid stalled", "error"]
            )
        ]
    );
    // without tkeep / tlast the payload is what there is
    ws.add_decode(decode(
        "b",
        "axis",
        "clk",
        &[&AXIS[..3], &AXIS[5..]].concat(),
        &[],
    ))
    .unwrap();
    assert_eq!(table(stream(&ws, "b.errors")).len(), 2);
    ws.search("rx.errors.rule == changed", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 125);
}

#[test]
fn generic_check() {
    let mut ws = open("axis.vcd");
    // rules in order at one time; each a record per cycle it holds
    let rules = pairs(&[
        ("stall", "tvalid && !tready"),
        ("short", "tvalid && tlast && tkeep != 0xF"),
    ]);
    let s = ws.check("chk", "clk", &rules).unwrap();
    assert_eq!(
        table(&s),
        [
            row(25, 35, &["stall"]),
            row(45, 55, &["short"]),
            row(95, 105, &["stall"]),
            row(115, 125, &["stall"]),
            row(125, 135, &["stall"])
        ]
    );
    assert_eq!(
        ws.check("chk", "clk", &pairs(&[("bad", "nope")]))
            .err()
            .unwrap(),
        "bad: unknown signal: nope"
    );
    // a signal compares with a signal: `x != past(x)` is `!stable(x)`
    let mut by = |rule: &str| {
        table(
            &ws.check("chk", "clk", &pairs(&[("changed", rule)]))
                .unwrap(),
        )
    };
    let stalled = "past(tvalid) && !past(tready)";
    assert_eq!(
        by(&format!("{stalled} && tdata != past(tdata)")),
        by(&format!("{stalled} && !stable(tdata)"))
    );
    assert!(!by(&format!("{stalled} && tdata != past(tdata)")).is_empty());
    assert!(by("tvalid && tdata == tdata").len() == by("tvalid").len());
    assert_eq!(
        ws.check("chk", "clk", &pairs(&[("bad", "tdata == 0xZQ")]))
            .err()
            .unwrap(),
        "bad: tdata: not a value: 0xZQ"
    );
    // as an instance, its params the rules
    let short = [("short", "tvalid && tlast && tkeep != 0xF")];
    ws.add_decode(decode("c", "check", "clk", &[], &short))
        .unwrap();
    assert_eq!(table(stream(&ws, "c")), [row(45, 55, &["short"])]);
    assert_eq!(
        ws.add_decode(decode("c", "check", "", &[("x", "tvalid")], &[]))
            .err()
            .unwrap(),
        "c: check has no role x; check: no rules (params = { rule = \"expression\" }); check takes no signals; no clock"
    );
    assert!(wtf_core::session::to_toml(&ws).contains("[[decode]]\nname = \"c\"\nprotocol = \"check\"\nclock = \"top.clk\"\n\n[decode.params]\nshort = \"tvalid && tlast && tkeep != 0xF\"\n"));
}

#[test]
fn builtin_instance_errors() {
    let mut ws = open("axis.vcd");
    let err = |ws: &mut Workspace, d| ws.add_decode(d).err().unwrap();
    assert_eq!(
        err(
            &mut ws,
            decode(
                "rx",
                "axis",
                "",
                &[("tfoo", "tdata"), ("tdata", "nope")],
                &[("x", "1")]
            )
        ),
        "rx: no signal for tvalid, aresetn (the file has: decode rx axis clock=top.clk aresetn=top.aresetn prefix=top.); axis has no role tfoo; tdata: unknown signal: nope; axis has no param x (params: prefix, reset_active); no clock"
    );
    assert_eq!(
        err(
            &mut ws,
            decode("rx", "axis", "clk", AXIS, &[("reset_active", "up")])
        ),
        "rx: reset_active: low or high, not up"
    );
    // its names are taken
    let p = Sampler {
        name: "axis".into(),
        when: "tvalid".into(),
        fields: vec![],
        roles: vec!["tvalid".into()],
        wasm: None,
    };
    assert_eq!(
        ws.add_protocol(p),
        Err("axis is a built-in protocol".into())
    );
    assert!(ws.streams().is_empty() && ws.decodes().is_empty());
}

/// reqresp.vcd: requests (req_valid, req_id) 1 at 15ns, 2 at 25, 3 at 45; responses (resp_valid,
/// resp_id) 2 at 35, 1 at 55, 0 at 85.
fn reqresp() -> Workspace {
    let mut ws = open("reqresp.vcd");
    // one protocol, put on each side
    ws.add_protocol(Sampler {
        name: "hs".into(),
        when: "valid".into(),
        fields: pairs(&[("id", "id")]),
        roles: vec!["valid".into(), "id".into()],
        wasm: None,
    })
    .unwrap();
    for (name, v, id) in [
        ("req", "req_valid", "req_id"),
        ("resp", "resp_valid", "resp_id"),
    ] {
        ws.add_decode(Decode {
            name: name.into(),
            protocol: "hs".into(),
            clock: "clk".into(),
            signals: pairs(&[("valid", v), ("id", id)]),
            params: vec![],
        })
        .unwrap();
    }
    ws
}

#[test]
fn pairs_by_id_and_by_order() {
    let mut ws = reqresp();
    let io = [("request", "req"), ("response", "resp")];
    // by id: delay, req_id, resp_id; the unmatched are violations
    ws.add_decode(decode("p", "pair", "", &io, &[("id", "id")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "p")),
        [
            row(15, 65, &["40", "1", "1"]),
            row(25, 45, &["10", "2", "2"])
        ]
    );
    assert_eq!(
        table(stream(&ws, "p.unmatched")),
        [row(45, 55, &["no response"]), row(85, 95, &["no request"])]
    );
    assert_eq!(
        stream(&ws, "p")
            .schema
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["delay", "req_id", "resp_id"]
    );
    // by order: the oldest request waiting
    ws.add_decode(decode("o", "pair", "", &io, &[])).unwrap();
    assert_eq!(
        table(stream(&ws, "o")),
        [
            row(15, 45, &["20", "1", "2"]),
            row(25, 65, &["30", "2", "1"]),
            row(45, 95, &["40", "3", "0"])
        ]
    );
    assert!(stream(&ws, "o.unmatched").records.is_empty());
    // as signals: overlapping pairs show the latest started one still open
    let p = var(&ws, "p");
    assert_eq!(
        [20, 30, 50, 70].map(|t| ws.trace.value_at(p, t)),
        ["#0", "#1", "#0", ""].map(|v| Some(Val::Text(v.into())))
    );
    // and kept by sessions and :e
    ws.search("p.delay == 10ns", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 25);
    let toml = wtf_core::session::to_toml(&ws);
    assert!(toml.contains("[[decode]]\nname = \"p\"\nprotocol = \"pair\"\n\n[decode.signals]\nrequest = \"req\"\nresponse = \"resp\"\n\n[decode.params]\nid = \"id\"\n"), "{toml}");
    let mut other = open("reqresp.vcd");
    assert_eq!(wtf_core::session::apply(&mut other, &toml), Ok(vec![]));
    assert_eq!(table(stream(&other, "p.unmatched")).len(), 2);
    assert_eq!(ws.reload(), Ok(Msg::Info("reloaded".into())));
    assert_eq!(table(stream(&ws, "o")).len(), 3);
}

#[test]
fn chained_frame_and_their_errors() {
    let mut ws = reqresp();
    // frame on a stream from a session protocol: requests back to back
    ws.add_decode(decode(
        "burst",
        "frame",
        "",
        &[("input", "req")],
        &[("gap", "0")],
    ))
    .unwrap();
    assert_eq!(
        table(stream(&ws, "burst")),
        [row(15, 35, &["2", "1"]), row(45, 55, &["1", "0"])]
    );
    let err = |ws: &mut Workspace, d| ws.add_decode(d).err().unwrap();
    assert_eq!(
        err(
            &mut ws,
            decode("f", "frame", "", &[("in", "req")], &[("gap", "0")])
        ),
        "f: no stream for input; frame has no role in"
    );
    assert_eq!(
        err(&mut ws, decode("f", "frame", "", &[("input", "nope")], &[])),
        "f: input: unknown stream: nope"
    );
    assert_eq!(
        err(
            &mut ws,
            decode("f", "frame", "", &[("input", "req")], &[("last", "x")])
        ),
        "f: frame: no field x (fields: id)"
    );
    let io = [("request", "req"), ("response", "resp")];
    assert_eq!(
        err(
            &mut ws,
            decode("q", "pair", "", &io, &[("req_id", "id"), ("by", "x")])
        ),
        "q: pair: no parameter by (id, req_id, resp_id); pair: req_id and resp_id go together"
    );
    assert_eq!(
        err(&mut ws, decode("q", "pair", "", &io, &[("id", "nope")])),
        "q: pair: req has no field nope; pair: resp has no field nope"
    );
}

/// A short name unique only thanks to the rows shown is kept by its full path, so the session,
/// which makes instances before rows, makes it again.
#[test]
fn instance_names_signals_in_full() {
    let mut ws = open("simple.vcd");
    let add = |ws: &mut Workspace, p: Sampler| ws.add_protocol(p).unwrap();
    add(
        &mut ws,
        Sampler {
            name: "busy".into(),
            when: "b".into(),
            fields: pairs(&[("s", "s")]),
            roles: vec!["b".into(), "s".into()],
            wasm: None,
        },
    );
    let state = ws.var_named("top.cpu.state").unwrap();
    ws.add_rows(&[state]);
    assert!(ws.resolve("state").is_ok() && ws.resolve("dma.state").is_ok());
    ws.add_decode(decode(
        "cpu",
        "busy",
        "clk",
        &[("b", "busy"), ("s", "state")],
        &[],
    ))
    .unwrap();
    assert_eq!(
        ws.decodes()[0].signals,
        pairs(&[("b", "top.cpu.busy"), ("s", "top.cpu.state")])
    );
    assert_eq!(ws.decodes()[0].clock, "top.clk");
    let text = wtf_core::session::to_toml(&ws);
    let mut back = open("simple.vcd");
    assert_eq!(
        wtf_core::session::apply(&mut back, &text).unwrap(),
        Vec::<String>::new(),
        "{text}"
    );
    assert_eq!(table(stream(&back, "cpu")), table(stream(&ws, "cpu")));
}

/// axilite.vcd with awaddr changed at 92ns, with the awready that ends the stall awvalid has been
/// in since 82ns: a payload that moves while stalled breaks `aw_changed`.
#[test]
fn axilite_payload_changed_while_stalled() {
    let dir = Dir::new("axi-changed");
    let path = dir.0.join("changed.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    let text = text.replacen("#92\n1#\n", "#92\n1#\nb10101 $\n", 1);
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert_eq!(
        // axilite.vcd's x's are the ten `<chan>_unknown` errors at 10..20ns: this is the tenth
        // row on
        table(stream(&ws, "s.errors"))[10],
        row(
            90,
            100,
            &[
                "aw_changed",
                "awaddr changed while awvalid stalled",
                "error"
            ]
        )
    );
}

/// The faults of axilite_errors.vcd, one per rule: an awvalid through the reset and its release
/// cycle, one dropped before awready, a wdata changed while stalled, an EXOKAY read, a B and an R
/// before their requests, an x arvalid and an x wdata. The warnings are the other half, pinned in
/// `tests/mcp.rs::axilite_warnings_filter_by_severity`.
///
/// Two shapes the fixture is built around: the stray B is the last B, and the x wdata is in the
/// last cycle: no cycle follows to make the stalled valid a dropped one.
#[test]
fn axilite_error_rules() {
    let mut ws = open("axilite_errors.vcd");
    // aresetn is bound by the prefix, at the port's polarity: nothing else is given
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let errors: Vec<_> = table(stream(&ws, "s.errors"))
        .into_iter()
        .filter(|(_, _, fields)| fields[2] == "error")
        .collect();
    assert_eq!(
        errors,
        [
            row(10, 20, &["aw_reset", "awvalid in reset", "error"]),
            row(
                20,
                30,
                &[
                    "aw_reset",
                    "awvalid in the cycle reset is released",
                    "error"
                ]
            ),
            row(
                50,
                60,
                &["aw_dropped", "awvalid fell before awready", "error"]
            ),
            row(
                90,
                100,
                &["w_changed", "wdata changed while wvalid stalled", "error"]
            ),
            row(150, 160, &["r_exokay", "rresp is EXOKAY", "error"]),
            row(
                160,
                170,
                &["b_early", "response 2 before write 2's AW and W", "error"]
            ),
            row(
                190,
                200,
                &["r_early", "response 4 before read 4's AR", "error"]
            ),
            row(
                200,
                210,
                &["ar_unknown", "arvalid is x out of reset", "error"]
            ),
            row(
                220,
                230,
                &["w_payload_unknown", "wdata is x while wvalid", "error"]
            ),
        ]
    );
}

/// axilite.vcd: every valid and ready is x from 2ns to the first edge at 10ns, so the cycle they
/// are sampled in breaks `<chan>_unknown` alone — a valid of x is no valid that fell. A payload of
/// x while its valid is high breaks `<chan>_payload_unknown`; x under a reset is neither.
#[test]
fn axilite_unknown_out_of_reset() {
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    // the clock's first edge is at 10ns, so one cycle samples the x's: all ten are errors,
    // the two rules that really break come after them
    let errors = table(stream(&ws, "s.errors"));
    assert_eq!(errors.len(), 13);
    assert_eq!(
        errors[..10]
            .iter()
            .map(|(_, _, f)| f[2].as_str())
            .collect::<Vec<_>>(),
        ["error"; 10]
    );
    assert_eq!((errors[0].0, errors[0].1), (10, 20));
    // a payload of x is the second rule, and it names the signal, not the valid
    let dir = Dir::new("axi-unknown");
    let path = dir.0.join("unknown.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    let text = text.replacen("b100010001000100010001000100010 '", "bx '", 1);
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "s.errors"))[10],
        row(
            60,
            70,
            &["w_payload_unknown", "wdata is x while wvalid", "error"]
        )
    );
    // x in reset is no unknown: the reset is what the port says about it, and the cycle the x's
    // are in is inside the reset
    let mut text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    text = text.replacen("#0\n1~~\n", "#0\n0~~\n", 1);
    text = text.replacen("#22\n", "#22\n1~~\n", 1);
    let path = dir.0.join("reset.vcd");
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "s.errors")),
        [
            row(180, 190, &["r_resp", "rresp is SLVERR", "warning"]),
            row(
                210,
                220,
                &["aw_dropped", "awvalid fell before awready", "error"]
            ),
            row(
                230,
                240,
                &["b_early", "response 3 before write 3's AW and W", "error"]
            )
        ]
    );
    // a reset of x, as a testbench's before it drives it, is no cycle to check either: the same
    // three rows, not the x's
    let mut text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    text = text.replacen("#0\n1~~\n", "#0\nx~~\n", 1);
    text = text.replacen("#22\n", "#22\n1~~\n", 1);
    let path = dir.0.join("x-reset.vcd");
    std::fs::write(&path, text).unwrap();
    let mut x = Workspace::open(&path).unwrap();
    x.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&x, "s.errors")),
        table(stream(&ws, "s.errors"))
    );
}

/// AXI-Lite in axilite.vcd: two writes (the second's W before its AW), two reads (the second
/// SLVERR), an AW valid dropped before its ready, a B with no write to answer (b_early), every
/// valid and ready X before the first edge (ten unknown errors), then six polling reads from
/// 260ns.
#[test]
fn axilite_transactions_and_errors() {
    let mut ws = open("axilite.vcd");
    // the prefix plays every role; kept by full path, as the clock
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert_eq!(
        (ws.decodes()[0].clock.as_str(), &ws.decodes()[0].params),
        ("top.aclk", &pairs(&[("prefix", "top.s_axi_")]))
    );
    let wr = stream(&ws, "s.wr");
    assert_eq!(
        wr.schema
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["delay", "addr", "prot", "data", "strb", "resp"]
    );
    let want = [
        row(30, 50, &["10", "16", "0", "286331153", "15", "0"]),
        row(60, 120, &["20", "20", "0", "572662306", "3", "0"]),
    ];
    assert_eq!(table(wr), want);
    let want = [
        row(130, 160, &["20", "16", "0", "286331153", "0"]),
        row(170, 190, &["10", "32", "0", "3735928559", "2"]),
    ];
    let rd = table(stream(&ws, "s.rd"));
    assert_eq!((&rd[..2], rd.len()), (&want[..], 8));
    // the port's valids and readys are x from 2ns to the first edge at 10ns, so the one cycle
    // that samples them breaks `unknown` alone: x is no valid that fell
    let mut errors = unknown_rows();
    errors.push(row(180, 190, &["r_resp", "rresp is SLVERR", "warning"]));
    errors.push(row(
        210,
        220,
        &["aw_dropped", "awvalid fell before awready", "error"],
    ));
    // both writes answered, so the B at 230ns is the third response and no write's
    errors.push(row(
        230,
        240,
        &["b_early", "response 3 before write 3's AW and W", "error"],
    ));
    assert_eq!(table(stream(&ws, "s.errors")), errors);
    assert!(stream(&ws, "s.wr.unmatched").records.is_empty());
    assert!(stream(&ws, "s.rd.unmatched").records.is_empty());
    // the session makes it again
    let text = wtf_core::session::to_toml(&ws);
    let mut back = open("axilite.vcd");
    assert_eq!(
        wtf_core::session::apply(&mut back, &text).unwrap(),
        Vec::<String>::new(),
        "{text}"
    );
    assert_eq!(table(stream(&back, "s.wr")), table(stream(&ws, "s.wr")));
    // a role missing under the prefix is reported
    let err = ws
        .add_decode(decode("t", "axilite", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap_err();
    assert!(
        err.starts_with("t: awvalid: unknown signal: m_axi_awvalid"),
        "{err}"
    );
}

/// axilite.vcd with `wstrb`, `bresp` and `rresp` named so the prefix doesn't bind them: the spec's
/// defaults fill them — every write byte a data byte, every response OKAY.
#[test]
fn axilite_defaults_for_absent_roles() {
    let dir = Dir::new("axi-defaults");
    let path = dir.0.join("defaults.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    // a name ending in the role's would still bind, by the path it ends
    let text = text
        .replacen("s_axi_wstrb", "s_axi_wstrb_old", 1)
        .replacen("s_axi_bresp", "s_axi_bresp_old", 1)
        .replacen("s_axi_rresp", "s_axi_rresp_old", 1);
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    // strb all ones, prot 0, resp OKAY: the fields are there, the SLVERR is not
    assert_eq!(
        table(stream(&ws, "s.wr")),
        [
            row(30, 50, &["10", "16", "0", "286331153", "15", "0"]),
            row(60, 120, &["20", "20", "0", "572662306", "15", "0"]),
        ]
    );
    let want = [
        row(130, 160, &["20", "16", "0", "286331153", "0"]),
        row(170, 190, &["10", "32", "0", "3735928559", "0"]),
    ];
    let rd = table(stream(&ws, "s.rd"));
    assert_eq!((&rd[..2], rd.len()), (&want[..], 8));
    let mut errors = unknown_rows();
    errors.push(row(
        210,
        220,
        &["aw_dropped", "awvalid fell before awready", "error"],
    ));
    errors.push(row(
        230,
        240,
        &["b_early", "response 3 before write 3's AW and W", "error"],
    ));
    assert_eq!(table(stream(&ws, "s.errors")), errors);
}

/// axilite.vcd with bresp 2'b01 at the first write's B (back to 0 once it is answered) and rresp
/// 2'b01 at the first read's R: EXOKAY is exclusive access, which AXI4-Lite has none of.
#[test]
fn axilite_exokay_is_an_error() {
    let dir = Dir::new("axi-exokay");
    let path = dir.0.join("exokay.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    let text = text.replacen("1)\n1*\nb0 +\n", "1)\n1*\nb01 +\n", 1);
    let text = text.replacen("#92\n1#\n", "#92\n1#\nb0 +\n", 1);
    let text = text.replacen(
        "1/\nb10001000100010001000100010001 1\nb0 2\n",
        "1/\nb10001000100010001000100010001 1\nb01 2\n",
        1,
    );
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let mut errors = unknown_rows();
    errors.extend([
        row(40, 50, &["b_exokay", "bresp is EXOKAY", "error"]),
        row(150, 160, &["r_exokay", "rresp is EXOKAY", "error"]),
        row(180, 190, &["r_resp", "rresp is SLVERR", "warning"]),
        row(
            210,
            220,
            &["aw_dropped", "awvalid fell before awready", "error"],
        ),
        row(
            230,
            240,
            &["b_early", "response 3 before write 3's AW and W", "error"],
        ),
    ]);
    assert_eq!(table(stream(&ws, "s.errors")), errors);
}

/// axi4.vcd with bresp 2'b01 at the first B (back to 0 once it is answered) and rresp 2'b01 at the
/// first read's first R beat: EXOKAY answers an exclusive access, and no lock asked for one.
#[test]
fn axi4_exokay_non_exclusive_is_an_error() {
    let dir = Dir::new("axi4-exokay");
    let path = dir.0.join("exokay.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let text = text.replacen("b10 /\n", "b01 /\n", 1);
    let text = text.replacen("b110101000000 7\n", "b110101000000 7\nb01 9\n", 1);
    let text = text.replacen("b110100110000 7\n", "b110100110000 7\nb0 9\n", 1);
    assert!(text.contains("b01 /") && text.contains("b01 9"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(
                50,
                60,
                &[
                    "b_exokay",
                    "bresp is EXOKAY to a non-exclusive write",
                    "error"
                ]
            ),
            row(
                90,
                100,
                &[
                    "r_exokay",
                    "rresp is EXOKAY to a non-exclusive read",
                    "error"
                ]
            ),
            row(
                140,
                150,
                &["ar_dropped", "arvalid fell before arready", "error"]
            ),
        ]
    );
    // the responses are still the transactions' own: both joins are whole
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
    assert_eq!(table(stream(&ws, "m.rd")).len(), 2);
    assert!(stream(&ws, "m.wr.unmatched").records.is_empty());
    assert!(stream(&ws, "m.rd.unmatched").records.is_empty());
}

/// The same responses with `awlock` and `arlock` driven 1: both accesses are exclusive, so an
/// EXOKAY is the answer they asked for and the errors are the fixture's own.
#[test]
fn axi4_exokay_exclusive_is_silent() {
    let dir = Dir::new("axi4-exclusive");
    let path = dir.0.join("exclusive.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let text = text.replacen(
        "$var wire 1 : m_axi_rlast $end\n",
        "$var wire 1 : m_axi_rlast $end\n$var wire 1 A m_axi_awlock $end\n\
         $var wire 1 B m_axi_arlock $end\n",
        1,
    );
    let text = text.replacen("b0 9\n0:\n#5\n", "b0 9\n0:\n1A\n1B\n#5\n", 1);
    let text = text.replacen("b10 /\n", "b01 /\n", 1);
    let text = text.replacen("b110101000000 7\n", "b110101000000 7\nb01 9\n", 1);
    let text = text.replacen("b110100110000 7\n", "b110100110000 7\nb0 9\n", 1);
    assert!(text.contains("m_axi_awlock") && text.contains("1A"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [row(
            140,
            150,
            &["ar_dropped", "arvalid fell before arready", "error"]
        )]
    );
    // the accesses still join, so the silence is the lock's and not a decode that stopped
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
    assert_eq!(table(stream(&ws, "m.rd")).len(), 2);
}

/// The EXOKAY fixture with `arid` and `rid` renamed off the prefix: a port without IDs pairs its
/// reads in order (spec §16), and the first read's EXOKAY beat is still `r_exokay`. Its interleaved
/// beats frame differently without IDs, so `r_last` rows join the errors: only the exokay rows are
/// this test's.
#[test]
fn axi4_exokay_without_ids_is_an_error() {
    let dir = Dir::new("axi4-exokay-no-ids");
    let path = dir.0.join("exokay.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let text = text.replacen("b10 /\n", "b01 /\n", 1);
    let text = text.replacen("b110101000000 7\n", "b110101000000 7\nb01 9\n", 1);
    let text = text.replacen("b110100110000 7\n", "b110100110000 7\nb0 9\n", 1);
    let text = text
        .replace("m_axi_arid", "u_arid")
        .replace("m_axi_rid", "u_rid");
    assert!(text.contains("b01 9") && !text.contains("m_axi_arid") && !text.contains("m_axi_rid"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let exokay: Vec<_> = table(stream(&ws, "m.errors"))
        .into_iter()
        .filter(|r| r.2[0].ends_with("_exokay"))
        .collect();
    assert_eq!(
        exokay,
        [
            row(
                50,
                60,
                &[
                    "b_exokay",
                    "bresp is EXOKAY to a non-exclusive write",
                    "error"
                ]
            ),
            row(
                90,
                100,
                &[
                    "r_exokay",
                    "rresp is EXOKAY to a non-exclusive read",
                    "error"
                ]
            ),
        ]
    );
}

/// A port with a reset (`aresetn`, active low): a valid in it, and in the cycle it is released
/// in, is an error, and no handshake is recorded; out of it the handshakes are as ever.
#[test]
fn axilite_reset_takes_no_records() {
    let mut ws = open("axilite_reset.vcd");
    ws.add_decode(decode("s", "axilite", "clk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "s.errors")),
        [
            row(5, 15, &["aw_reset", "awvalid in reset", "error"]),
            row(
                15,
                25,
                &[
                    "aw_reset",
                    "awvalid in the cycle reset is released",
                    "error"
                ]
            )
        ]
    );
    // only the handshake after it counts: addr and the protection the port doesn't give
    assert_eq!(table(stream(&ws, "s.aw")), [row(25, 35, &["160", "0"])]);
    // read as active high, the same signal holds the other cycles: the reset is from #15 on
    let mut ws = open("axilite_reset.vcd");
    ws.add_decode(decode(
        "s",
        "axilite",
        "clk",
        &[],
        &[("prefix", "s_axi_"), ("reset_active", "high")],
    ))
    .unwrap();
    assert_eq!(
        table(stream(&ws, "s.errors")),
        [
            row(15, 25, &["aw_reset", "awvalid in reset", "error"]),
            row(25, 35, &["aw_reset", "awvalid in reset", "error"])
        ]
    );
    // the same signal holds the other cycles, so the handshake is the early one
    assert_eq!(table(stream(&ws, "s.aw")), [row(5, 15, &["160", "0"])]);
    // a polarity it can't read is refused
    let mut ws = open("axilite_reset.vcd");
    let err = ws
        .add_decode(decode(
            "s",
            "axilite",
            "clk",
            &[],
            &[("prefix", "s_axi_"), ("reset_active", "on")],
        ))
        .unwrap_err();
    assert_eq!(err, "s: reset_active: low or high, not on");
}

/// A port with a reset (`aresetn`, active low): a valid in it, and in the cycle it is released
/// in, is an error, and no handshake is recorded; the next cycle's is. `reset_active=high` reads
/// the same signal the other way, and a polarity it cannot read is refused.
#[test]
fn axi4_reset_takes_no_records() {
    let dir = Dir::new("axi4-reset");
    let path = dir.0.join("reset.vcd");
    // aresetn is low for two cycles, awvalid high from the second; the write that records is
    // the cycle after the release, to 0x200
    let text = "$timescale 1ns $end\n\
$scope module top $end\n\
$var wire 1 ! aclk $end\n\
$var wire 1 \" m_axi_aresetn $end\n\
$var wire 1 # m_axi_awvalid $end\n\
$var wire 32 $ m_axi_awaddr [31:0] $end\n\
$var wire 1 % m_axi_awready $end\n\
$var wire 1 & m_axi_wvalid $end\n\
$var wire 32 ' m_axi_wdata [31:0] $end\n\
$var wire 1 ( m_axi_wready $end\n\
$var wire 1 ) m_axi_wlast $end\n\
$var wire 1 * m_axi_bvalid $end\n\
$var wire 1 , m_axi_bready $end\n\
$upscope $end\n\
$enddefinitions $end\n\
#0\n1!\n0\"\n0#\nb100000000 $\n1%\n0&\nb0 '\n1(\n1)\n0*\n1,\n\
#5\n0!\n\
#10\n1!\n\
#12\n1#\n\
#15\n0!\n\
#20\n1!\n\
#22\n1\"\n\
#25\n0!\n\
#30\n1!\n\
#35\n0!\nb1000000000 $\n\
#40\n1!\n";
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(10, 20, &["aw_reset", "awvalid in reset", "error"]),
            row(
                20,
                30,
                &[
                    "aw_reset",
                    "awvalid in the cycle reset is released",
                    "error"
                ]
            ),
        ]
    );
    // neither handshake the reset took is there: the one after it is
    assert_eq!(
        table(stream(&ws, "m.aw")),
        [row(
            30,
            40,
            &["512", "0", "2", "1", "0", "0", "0", "0", "0"]
        )]
    );
    // read as active high, the reset is the other half: the first handshake records, the two
    // after 20ns do not
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode(
        "m",
        "axi4",
        "aclk",
        &[],
        &[("prefix", "m_axi_"), ("reset_active", "high")],
    ))
    .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(20, 30, &["aw_reset", "awvalid in reset", "error"]),
            row(30, 40, &["aw_reset", "awvalid in reset", "error"]),
        ]
    );
    assert_eq!(
        table(stream(&ws, "m.aw")),
        [row(
            10,
            20,
            &["256", "0", "2", "1", "0", "0", "0", "0", "0"]
        )]
    );
    // a polarity it can't read is refused
    let mut ws = Workspace::open(&path).unwrap();
    let err = ws
        .add_decode(decode(
            "m",
            "axi4",
            "aclk",
            &[],
            &[("prefix", "m_axi_"), ("reset_active", "on")],
        ))
        .unwrap_err();
    assert_eq!(err, "m: reset_active: low or high, not on");
}

/// `timeout=`: the write answered after 20ns is no response under 15ns, and its B, at 110ns, no
/// request: late, not early, so `errors` does not say it.
#[test]
fn axilite_timeout() {
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode(
        "s",
        "axilite",
        "aclk",
        &[],
        &[("prefix", "s_axi_"), ("timeout", "15ns")],
    ))
    .unwrap();
    assert_eq!(
        table(stream(&ws, "s.wr")),
        [row(30, 50, &["10", "16", "0", "286331153", "15", "0"])]
    );
    let kinds: Vec<(u64, String)> = table(stream(&ws, "s.wr.unmatched"))
        .into_iter()
        .map(|r| (r.0, r.2[0].clone()))
        .collect();
    // the write answered after 20ns, and its late B; the stray one at 230ns is `b_early` in
    // errors alone
    assert_eq!(
        kinds,
        [
            (60, "no response".to_string()),
            (110, "no request".to_string())
        ]
    );
    assert!(
        !table(stream(&ws, "s.errors"))
            .iter()
            .any(|r| r.0 == 110 && r.2[0] == "b_early")
    );
    // long enough: as without
    let mut long = open("axilite.vcd");
    long.add_decode(decode(
        "s",
        "axilite",
        "aclk",
        &[],
        &[("prefix", "s_axi_"), ("timeout", "1us")],
    ))
    .unwrap();
    assert_eq!(table(stream(&long, "s.wr")).len(), 2);
    // the session keeps it
    let mut back = open("axilite.vcd");
    assert_eq!(
        wtf_core::session::apply(&mut back, &wtf_core::session::to_toml(&ws)).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(table(stream(&back, "s.wr")).len(), 1);
    // a time it can't read, and a protocol without the param
    let err = ws
        .add_decode(decode(
            "t",
            "axilite",
            "aclk",
            &[],
            &[("prefix", "s_axi_"), ("timeout", "soon")],
        ))
        .unwrap_err();
    assert!(err.contains("join: timeout: "), "{err}");
    let err = ws
        .add_decode(decode(
            "x",
            "axis",
            "aclk",
            &[],
            &[("prefix", "s_axi_"), ("timeout", "1us")],
        ))
        .unwrap_err();
    assert!(err.contains("has no param timeout"), "{err}");
}

/// A response with no request to answer is `b_early`/`r_early` (A2.3), not the join's `no request`:
/// the response says what it is, and the write it does not answer keeps its record.
#[test]
fn axilite_b_early() {
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "s.errors"));
    assert_eq!(
        &errors[errors.len() - 1..],
        [row(
            230,
            240,
            &["b_early", "response 3 before write 3's AW and W", "error"]
        )]
    );
    // the early B answers nothing: both writes are still joined, and nothing is unmatched
    assert_eq!(table(stream(&ws, "s.wr")).len(), 2);
    assert!(stream(&ws, "s.wr.unmatched").records.is_empty());
    assert!(stream(&ws, "s.rd.unmatched").records.is_empty());
}

/// axilite.vcd with one more R, after the eighth read's: a response whose read has no AR is
/// `r_early`, and the eight reads stay as they were.
#[test]
fn axilite_r_early() {
    let dir = Dir::new("axi-r-early");
    let path = dir.0.join("early.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    let text = format!("{text}#452\n1/\n10\n#455\n0!\n#465\n1!\n#472\n0/\n00\n");
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "s.errors"));
    assert_eq!(
        &errors[errors.len() - 1..],
        [row(
            450,
            465,
            &["r_early", "response 9 before read 9's AR", "error"]
        )]
    );
    assert_eq!(table(stream(&ws, "s.rd")).len(), 8);
    assert!(stream(&ws, "s.rd.unmatched").records.is_empty());
}

/// axilite.vcd with bvalid and bready in the cycle of the first write's AW and W: the B comes
/// before its own request's handshakes, so it is `b_early`. The join calls it `no request` too, so
/// the next B answers that write: one response early, one pair shifted, nothing lost.
#[test]
fn axilite_b_early_same_cycle() {
    let dir = Dir::new("axi-b-early");
    let path = dir.0.join("same-cycle.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    let text = text
        .replacen("#32\n1\"\n1#\n", "#32\n1)\n1*\n1\"\n1#\n", 1)
        .replacen(
            "#42\n0\"\n0#\n0%\n0&\n1)\n1*\n",
            "#42\n0\"\n0#\n0%\n0&\n0)\n0*\n",
            1,
        )
        // no late B: the one early response, and no other
        .replacen("#232\n1)\n1*\n", "#232\n", 1)
        .replacen("#242\n0)\n0*\n", "#242\n", 1);
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "s.errors"));
    let early: Vec<_> = errors.iter().filter(|r| r.2[0] == "b_early").collect();
    assert_eq!(
        early,
        [&row(
            30,
            40,
            &["b_early", "response 1 before write 1's AW and W", "error"]
        )]
    );
    // the write is still joined, from its AW and W to the next B; its own request's W (the
    // second write's) waits unanswered
    assert_eq!(
        table(stream(&ws, "s.wr")),
        [row(30, 120, &["80", "16", "0", "286331153", "15", "0"])]
    );
    assert_eq!(
        table(stream(&ws, "s.wr.unmatched")),
        [row(60, 100, &["no response"])]
    );
}

/// An unmatched row says `no request` only for a late response: one with no request before it is
/// an `errors` row (`b_early`, `r_early`), said once.
#[test]
fn axilite_no_request_only_when_late() {
    let kinds = |ws: &Workspace, side: &str| {
        table(stream(ws, side))
            .into_iter()
            .map(|r| r.2[0].clone())
            .collect::<Vec<_>>()
    };
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert!(kinds(&ws, "s.wr.unmatched").is_empty());
    assert!(kinds(&ws, "s.rd.unmatched").is_empty());
    assert!(
        table(stream(&ws, "s.errors"))
            .iter()
            .any(|r| r.2[0] == "b_early")
    );
    // under a timeout the unanswered write is what is left
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode(
        "s",
        "axilite",
        "aclk",
        &[],
        &[("prefix", "s_axi_"), ("timeout", "15ns")],
    ))
    .unwrap();
    // one write and one read were answered too late: each request and its late response
    assert_eq!(kinds(&ws, "s.wr.unmatched"), ["no response", "no request"]);
    assert_eq!(kinds(&ws, "s.rd.unmatched"), ["no response", "no request"]);
}

/// axilite.vcd with a stray R at 110ns, before the first AR: it is `r_early` once, and the eight
/// reads after it pair as they did, none of them early.
#[test]
fn axilite_stray_response_once() {
    let dir = Dir::new("axi-stray");
    let path = dir.0.join("stray.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    let text = text
        .replacen("#112\n1*\n", "#112\n1*\n1/\n10\n", 1)
        .replacen("#122\n0)\n0*\n", "#122\n0)\n0*\n0/\n00\n", 1);
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let early: Vec<_> = table(stream(&ws, "s.errors"))
        .into_iter()
        .filter(|r| r.2[0] == "r_early")
        .collect();
    assert_eq!(
        early,
        [row(
            110,
            120,
            &["r_early", "response 1 before read 1's AR", "error"]
        )]
    );
    // errors and the join agree: every read pairs, and the stray R is not said twice
    let mut plain = open("axilite.vcd");
    plain
        .add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    assert_eq!(table(stream(&ws, "s.rd")), table(stream(&plain, "s.rd")));
    assert!(stream(&ws, "s.rd.unmatched").records.is_empty());
}

/// axilite.vcd with the second write's bvalid raised at 90ns, in the cycle of its AW handshake,
/// and held to the bready at 110ns: a BVALID before its write's handshakes is `b_early` (A2.3.2.1)
/// though its handshake comes after them, and the B still answers the write.
#[test]
fn axilite_bvalid_before_request() {
    let dir = Dir::new("axi-bvalid-early");
    let path = dir.0.join("bvalid.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axilite.vcd")).unwrap();
    let text = text.replacen("#92\n1#\n", "#92\n1#\n1)\n", 1).replacen(
        "#102\n0\"\n0#\n1)\n",
        "#102\n0\"\n0#\n",
        1,
    );
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let early: Vec<_> = table(stream(&ws, "s.errors"))
        .into_iter()
        .filter(|r| r.2[0] == "b_early")
        .collect();
    assert_eq!(
        early,
        [
            row(
                90,
                100,
                &["b_early", "response 2 before write 2's AW and W", "error"]
            ),
            row(
                230,
                240,
                &["b_early", "response 3 before write 3's AW and W", "error"]
            ),
        ]
    );
    // the B is the write's own: both writes pair, nothing is unmatched
    assert_eq!(table(stream(&ws, "s.wr")).len(), 2);
    assert!(stream(&ws, "s.wr.unmatched").records.is_empty());
}

/// The names of an instance checked without decoding (the decode form, spec §5): the same
/// mistakes `add_decode` reports, nothing made, no value read.
#[test]
fn check_decode_names_only() {
    let mut ws = open("axilite.vcd");
    let vars = ws.trace.vars.len();
    assert_eq!(
        ws.check_decode(&decode(
            "s",
            "axilite",
            "aclk",
            &[],
            &[("prefix", "s_axi_")]
        )),
        Ok(())
    );
    let r = ws
        .check_decode(&decode("t", "axilite", "", &[], &[("prefix", "m_axi_")]))
        .unwrap_err();
    let e = r.to_string();
    assert!(
        e.starts_with("t: awvalid: unknown signal: m_axi_awvalid") && e.ends_with("; no clock"),
        "{e}"
    );
    let on = r.fields();
    assert!(
        on.starts_with(&["awvalid", "awready"]) && on.contains(&"clock"),
        "{on:?}"
    );
    assert_eq!(
        ws.check_decode(&decode(
            "t",
            "axilite",
            "nope",
            &[],
            &[("prefix", "s_axi_")]
        ))
        .unwrap_err()
        .to_string(),
        "t: clock: unknown signal: nope"
    );
    assert!(
        ws.check_decode(&decode("t", "nope", "aclk", &[], &[]))
            .unwrap_err()
            .to_string()
            .starts_with("t: unknown protocol: nope")
    );
    // no signal at all: one reason names every role the protocol needs
    let on = ws
        .check_decode(&decode("t", "axilite", "aclk", &[], &[]))
        .unwrap_err()
        .fields()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert!(
        on.starts_with(&["awvalid".into(), "awready".into()]) && on.len() > 2,
        "{on:?}"
    );
    // a session protocol: its roles; a role it hasn't
    ws.add_protocol(Sampler {
        name: "w".into(),
        when: "valid && ready".into(),
        fields: vec![],
        roles: vec!["valid".into(), "ready".into()],
        wasm: None,
    })
    .unwrap();
    assert_eq!(
        ws.check_decode(&decode(
            "w1",
            "w",
            "aclk",
            &[("valid", "s_axi_wvalid"), ("ready", "s_axi_wready")],
            &[]
        )),
        Ok(())
    );
    let r = ws
        .check_decode(&decode(
            "w1",
            "w",
            "aclk",
            &[("valid", "s_axi_wvalid"), ("go", "s_axi_wready")],
            &[],
        ))
        .unwrap_err();
    let e = r.to_string();
    assert!(
        e.contains("no signal for ready") && e.contains("w has no role go"),
        "{e}"
    );
    assert_eq!(r.fields(), ["ready", "go"]);
    assert!(ws.streams().is_empty() && ws.decodes().is_empty() && ws.trace.vars.len() == vars);
    assert!(
        !ws.trace
            .vars
            .iter()
            .any(|v| v.path.contains("s_axi_") && ws.trace.loaded(ws.trace.by_path[&v.path])),
        "no value read"
    );
}

/// An on-stream instance is checked against its card (the decode form, spec §5), like a signal
/// protocol's: its streams exist, its roles and params are the card's, and `frame`'s params fit
/// the stream it names.
#[test]
fn check_decode_validates_on_stream_instances() {
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    // a stream it names that isn't decoded
    let r = ws
        .check_decode(&decode(
            "f",
            "frame",
            "",
            &[("input", "s.nope")],
            &[("gap", "1ns")],
        ))
        .unwrap_err();
    let e = r.to_string();
    assert!(e.contains("f: input: unknown stream: s.nope"), "{e}");
    let on = r.fields();
    assert!(on.contains(&"input"), "{on:?}");
    // over a decoded stream, a param that isn't the card's
    let r = ws
        .check_decode(&decode(
            "f",
            "frame",
            "",
            &[("input", "s.wr")],
            &[("gap", "1ns"), ("ids", "id")],
        ))
        .unwrap_err();
    let e = r.to_string();
    assert!(
        e.contains("frame: no parameter ids (last, gap, select, key, bytes, data)"),
        "{e}"
    );
    let on = r.fields();
    assert!(on.contains(&"protocol"), "{on:?}");
    // a role it hasn't is the same mistake as a signal protocol's
    let r = ws
        .check_decode(&decode(
            "f",
            "frame",
            "",
            &[("input", "s.wr"), ("request", "s.wr")],
            &[],
        ))
        .unwrap_err();
    assert!(
        r.to_string().contains("frame has no role request"),
        "{}",
        r.to_string()
    );
    // over a decoded stream, with params that fit, it checks clean
    assert_eq!(
        ws.check_decode(&decode(
            "f",
            "frame",
            "",
            &[("input", "s.wr")],
            &[("gap", "0")]
        )),
        Ok(())
    );
}

/// A protocol's expressions checked against its roles' signals without sampling (the protocol
/// form, spec §5): widths come from the signals and earlier fields, a name no role is an error
/// naming the role or field it is in, nothing is defined and no value read.
#[test]
fn check_protocol_names_only() {
    let mut ws = open("axilite.vcd");
    let vars = ws.trace.vars.len();
    let p = |when: &str, fields: &[(&str, &str)]| Sampler {
        name: "w".into(),
        when: when.into(),
        fields: pairs(fields),
        roles: vec!["valid".into(), "ready".into(), "data".into()],
        wasm: None,
    };
    let sigs = pairs(&[
        ("valid", "s_axi_wvalid"),
        ("ready", "s_axi_wready"),
        ("data", "s_axi_wdata"),
    ]);
    assert_eq!(
        ws.check_protocol(
            &p(
                "valid && ready && lo != 0",
                &[("lo", "data[7:0]"), ("hi", "lo[3:0] + 1")]
            ),
            &sigs
        ),
        Ok(())
    );
    let r = ws
        .check_protocol(
            &p("valid && go", &[("lo", "data[40:0]"), ("x", "aclk")]),
            &sigs,
        )
        .unwrap_err();
    let e = r.to_string();
    let parts: Vec<&str> = e.split("; ").collect();
    assert!(
        parts.len() == 3
            && parts[0].starts_with("lo: data[40:0]: data has no bits")
            && parts[1].starts_with("x: aclk is no role")
            && parts[2].starts_with("when: go is no role"),
        "{e}"
    );
    assert_eq!(r.fields(), ["lo", "x", "when"]);
    // a role without a signal, or with an unknown one
    let e = ws
        .check_protocol(&p("valid", &[]), &sigs[..1])
        .unwrap_err()
        .to_string();
    assert!(
        e.starts_with("ready: no signal") && e.contains("; data: no signal"),
        "{e}"
    );
    assert!(
        ws.check_protocol(
            &p("valid", &[]),
            &pairs(&[("valid", "nope"), ("ready", "aclk"), ("data", "aclk")])
        )
        .unwrap_err()
        .to_string()
        .starts_with("valid: unknown signal: nope")
    );
    let r = ws
        .check_protocol(
            &Sampler {
                name: "axis".into(),
                ..p("valid", &[])
            },
            &sigs,
        )
        .unwrap_err();
    assert_eq!(
        (r.to_string().as_str(), r.fields()),
        ("axis is a built-in protocol", vec!["name"])
    );
    assert!(ws.protocols().is_empty() && ws.trace.vars.len() == vars);
    assert!(
        !ws.trace
            .vars
            .iter()
            .any(|v| ws.trace.loaded(ws.trace.by_path[&v.path])),
        "no value read"
    );
}

/// The instance and the protocols signals play, from their names' endings alone (spec §5
/// "Decode form"): `s_axi_arvalid` never plays `rvalid`, a role two signals end in stays empty.
#[test]
fn plays_by_endings() {
    let ws = open("axilite.vcd");
    let vars = |names: &[&str]| -> Vec<usize> {
        names
            .iter()
            .map(|n| {
                ws.trace
                    .vars
                    .iter()
                    .position(|v| v.path == format!("top.{n}"))
                    .unwrap()
            })
            .collect()
    };
    let write = vars(&[
        "aclk",
        "s_axi_awvalid",
        "s_axi_awready",
        "s_axi_awaddr",
        "s_axi_wvalid",
        "s_axi_wready",
        "s_axi_wdata",
        "s_axi_arvalid",
    ]);
    let d = ws.plays("axilite", &write);
    assert_eq!(d.clock, "top.aclk");
    let roles: Vec<&str> = d.signals.iter().map(|(r, _)| r.as_str()).collect();
    assert_eq!(
        roles,
        [
            "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "arvalid"
        ]
    );
    assert_eq!(d.signals[3], ("wvalid".into(), "top.s_axi_wvalid".into()));
    // axis plays nothing here: tvalid isn't an ending of any of them
    assert_eq!(
        ws.playable(&write),
        [("axilite".to_string(), 7), ("axi4".to_string(), 7)]
    );
    // the ending is after `_` or `.`: s_axi_bready doesn't play ready
    assert!(
        ws.plays("axis", &vars(&["s_axi_bready"]))
            .signals
            .is_empty()
    );
    assert!(
        !ws.trace
            .vars
            .iter()
            .any(|v| ws.trace.loaded(ws.trace.by_path[&v.path])),
        "no value read"
    );
    // two signals ending in state (simple.vcd: top.cpu.state, top.dma.state): state stays empty
    let mut ws = Workspace::open(&fixture("vcd")).unwrap();
    let sigs: Vec<usize> = ["top.clk", "top.cpu.state", "top.dma.state", "top.cpu.busy"]
        .iter()
        .map(|p| ws.trace.vars.iter().position(|v| v.path == *p).unwrap())
        .collect();
    ws.add_protocol(Sampler {
        name: "hs".into(),
        when: "busy".into(),
        fields: vec![],
        roles: vec!["state".into(), "busy".into()],
        wasm: None,
    })
    .unwrap();
    let d = ws.plays("hs", &sigs);
    assert_eq!(
        (d.clock.as_str(), d.signals),
        ("top.clk", pairs(&[("busy", "top.cpu.busy")]))
    );
    assert_eq!(ws.playable(&sigs), [("hs".to_string(), 1)]);
    assert!(ws.plays("nope", &sigs).signals.is_empty());
    assert!(
        !ws.trace
            .vars
            .iter()
            .any(|v| ws.trace.loaded(ws.trace.by_path[&v.path])),
        "no value read"
    );
}

/// `:decode <name> off` (spec §16): the instance, its streams and their rows go, the vars after;
/// a decode another reads stays, naming it. Made again under its name, rows of streams it still
/// makes stay, those of streams it no longer makes go.
#[test]
fn remove_and_redecode() {
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let io = [("request", "s.aw"), ("response", "s.b")];
    ws.add_decode(decode("lat", "pair", "", &io, &[])).unwrap();
    let wr = ws.trace.by_path["s.wr"];
    ws.add_rows(&[wr, ws.trace.by_path["top.aclk"]]);
    assert_eq!(
        wtf_core::command::run(&mut ws, "decode s off"),
        Err("s is read by lat: remove it first".into())
    );
    assert_eq!(
        wtf_core::command::run(&mut ws, "decode nope off"),
        Err("no decode nope (decodes: s, lat)".into())
    );
    wtf_core::command::run(&mut ws, "decode lat off").unwrap();
    let cp = ws.checkpoint();
    assert_eq!(
        wtf_core::command::run(&mut ws, "decode s off"),
        Ok(Some(Msg::Info("s: removed".into())))
    );
    ws.record(cp);
    assert!(ws.decodes().is_empty() && ws.streams().is_empty());
    assert_eq!(ws.rows.len(), 1, "s.wr's row gone, aclk's kept");
    // the rows come back with undo
    ws.undo().unwrap();
    assert_eq!(ws.rows.len(), 2);
    ws.redo().unwrap();
    assert_eq!(
        wtf_core::command::run(&mut ws, "decode s off"),
        Err("no decode s: there are none yet".into())
    );
    // once nothing holds them, its vars go
    let mut fresh = open("axilite.vcd");
    fresh
        .add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    fresh.remove_decode("s").unwrap();
    fresh.drop_unused();
    assert_eq!(fresh.trace.vars.len(), fresh.trace.fixed);
    // made again: a protocol with other fields; the row of a field it no longer has goes
    let p = |name: &str, field: &str| Sampler {
        name: name.into(),
        when: "valid && ready".into(),
        fields: vec![(field.into(), "valid".into())],
        roles: vec!["valid".into(), "ready".into()],
        wasm: None,
    };
    ws.add_protocol(p("pa", "a")).unwrap();
    ws.add_protocol(p("pb", "b")).unwrap();
    let w = [("valid", "s_axi_wvalid"), ("ready", "s_axi_wready")];
    ws.add_decode(decode("x", "pa", "aclk", &w, &[])).unwrap();
    let (x, xa) = (ws.trace.by_path["x"], ws.trace.by_path["x.a"]);
    ws.add_rows(&[x, xa]);
    assert_eq!(
        ws.redecode(decode("x", "pb", "aclk", &w, &[])),
        Ok(String::new())
    );
    let paths: Vec<&str> = ws
        .rows
        .iter()
        .filter_map(|r| r.sig())
        .map(|s| ws.trace.vars[s.var].path.as_str())
        .collect();
    assert!(paths.contains(&"x") && !paths.contains(&"x.a"), "{paths:?}");
    assert_eq!(
        (
            ws.decodes().len(),
            ws.streams().len(),
            ws.streams()[0].schema[0].name.as_str()
        ),
        (1, 1, "b")
    );
    // a failing one leaves the old
    assert!(ws.redecode(decode("x", "pb", "nope", &w, &[])).is_err());
    assert_eq!(
        (ws.decodes()[0].clock.as_str(), ws.streams().len()),
        ("top.aclk", 1)
    );
    // not made yet: made
    assert_eq!(
        ws.redecode(decode("y", "pa", "aclk", &w, &[])),
        Ok(String::new())
    );
    assert_eq!(ws.decodes().len(), 2);
}

/// AXI4 in axi4.vcd: write 1 (two beats to 0x100) and write 2 (one beat to 0x200, SLVERR), whose B
/// responses come back in the other order; reads 3 and 4 (two beats each), whose R beats
/// interleave; and an AR valid dropped before its ready.
#[test]
fn axi4_bursts_by_id() {
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let names = |s: &str| {
        stream(&ws, s)
            .schema
            .iter()
            .map(|f| f.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names("m.wr"),
        [
            "delay",
            "addr",
            "aw_id",
            "aw_len",
            "size",
            "burst",
            "prot",
            "lock",
            "cache",
            "qos",
            "region",
            "beats",
            "wburst_len",
            "data",
            "complete",
            "resp_id",
            "resp"
        ]
    );
    // by ID: write 2's B comes first
    let wr = table(stream(&ws, "m.wr"));
    assert_eq!(
        wr.iter()
            .map(|r| (
                r.0,
                r.1,
                r.2[0].as_str(),
                r.2[2].as_str(),
                r.2[15].as_str(),
                r.2[16].as_str()
            ))
            .collect::<Vec<_>>(),
        [(10, 70, "40", "1", "1", "0"), (30, 60, "10", "2", "2", "2"),]
    );
    assert_eq!(wr[0].2[13], "a0 00 00 00 a1 00 00 00");
    assert_eq!(wr[1].2[12..14], ["2", "b0 00"], "wstrb 0b0011: two bytes");
    // interleaved R beats make one burst per ID; the latency is to the first beat
    assert_eq!(
        table(stream(&ws, "m.rburst"))
            .iter()
            .map(|r| (r.0, r.1, r.2[1].as_str()))
            .collect::<Vec<_>>(),
        [(90, 120, "4"), (100, 130, "3")]
    );
    let rd = table(stream(&ws, "m.rd"));
    assert_eq!(
        rd.iter()
            .map(|r| (
                r.0,
                r.1,
                r.2[0].as_str(),
                r.2[1].as_str(),
                r.2[2].as_str(),
                r.2[13].as_str()
            ))
            .collect::<Vec<_>>(),
        [
            (70, 130, "30", "768", "3", "30 0d 00 00 31 0d 00 00"),
            (80, 120, "10", "1024", "4", "40 0d 00 00 41 0d 00 00"),
        ]
    );
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(50, 60, &["b_resp", "bresp is SLVERR", "warning"]),
            row(
                140,
                150,
                &["ar_dropped", "arvalid fell before arready", "error"]
            )
        ]
    );
    assert!(
        stream(&ws, "m.wr.unmatched").records.is_empty()
            && stream(&ws, "m.rd.unmatched").records.is_empty()
    );
    // without IDs, in order: the B responses go to the wrong writes
    let mut plain = open("axi4.vcd");
    let roles = [
        "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "wlast", "bvalid", "bready",
        "bresp", "arvalid", "arready", "araddr", "rvalid", "rready", "rdata", "rlast", "aresetn",
    ];
    let signals: Vec<(&str, String)> = roles.iter().map(|r| (*r, format!("m_axi_{r}"))).collect();
    let signals: Vec<(&str, &str)> = signals.iter().map(|(r, s)| (*r, s.as_str())).collect();
    plain
        .add_decode(decode("m", "axi4", "aclk", &signals, &[]))
        .unwrap();
    let wr = table(stream(&plain, "m.wr"));
    assert_eq!(
        wr.iter()
            .map(|r| (r.2[1].as_str(), r.2.last().unwrap().as_str()))
            .collect::<Vec<_>>(),
        [("256", "2"), ("512", "0")]
    );
}

/// The required roles axi4 has, the payloads it adds when they are given, and the two sides each
/// enough on its own: a write-only or read-only port records what it has.
#[test]
fn axi4_required_roles() {
    let def = wtf_core::protocol::get("axi4").unwrap();
    let (required, optional) = def.roles();
    assert_eq!(
        required,
        [
            "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "bvalid", "bready",
            "arvalid", "arready", "araddr", "rvalid", "rready", "rdata", "aresetn",
        ]
    );
    assert_eq!(
        optional,
        [
            "wlast", "rlast", "awid", "awlen", "awsize", "awburst", "awprot", "awlock", "awcache",
            "awqos", "awregion", "wstrb", "bid", "bresp", "arid", "arlen", "arsize", "arburst",
            "arprot", "arlock", "arcache", "arqos", "arregion", "rid", "rresp",
        ]
    );
    assert_eq!(
        def.defaults(),
        [
            "awlen 00000000",
            "awsize log2(wdata/8)",
            "awburst 01",
            "awprot 000",
            "awlock 0",
            "awcache 0000",
            "awqos 0000",
            "awregion 0000",
            "wstrb bytes of wdata",
            "bresp 00",
            "arlen 00000000",
            "arsize log2(rdata/8)",
            "arburst 01",
            "arprot 000",
            "arlock 0",
            "arcache 0000",
            "arqos 0000",
            "arregion 0000",
            "rresp 00",
        ]
    );
    assert_eq!(def.params(), ["timeout", "reset_active"]);
    // the write side alone records its writes and has no reads at all
    let has = |ws: &Workspace, s: &str| ws.streams().iter().any(|x| x.name == s);
    let mut ws = open("axi4.vcd");
    let signals: Vec<(String, String)> = required[..8]
        .iter()
        .chain(&["aresetn"])
        .map(|r| ((*r).to_string(), format!("m_axi_{r}")))
        .collect();
    let signals: Vec<(&str, &str)> = signals
        .iter()
        .map(|(r, s)| (r.as_str(), s.as_str()))
        .collect();
    ws.add_decode(decode("m", "axi4", "aclk", &signals, &[]))
        .unwrap();
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
    for s in ["m.ar", "m.r", "m.rburst", "m.rd", "m.rd.unmatched"] {
        assert!(!has(&ws, s), "{s} is a read stream");
    }
    // the read side alone has no writes
    let mut ws = open("axi4.vcd");
    let signals: Vec<(String, String)> = required[8..]
        .iter()
        .map(|r| ((*r).to_string(), format!("m_axi_{r}")))
        .collect();
    let signals: Vec<(&str, &str)> = signals
        .iter()
        .map(|(r, s)| (r.as_str(), s.as_str()))
        .collect();
    ws.add_decode(decode("m", "axi4", "aclk", &signals, &[]))
        .unwrap();
    assert_eq!(table(stream(&ws, "m.rd")).len(), 2);
    for s in ["m.aw", "m.w", "m.b", "m.wburst", "m.wr", "m.wr.unmatched"] {
        assert!(!has(&ws, s), "{s} is a write stream");
    }
    // neither side whole is an error, and it lists what is missing
    let mut ws = open("axi4.vcd");
    let err = ws
        .add_decode(decode(
            "m",
            "axi4",
            "aclk",
            &[("awvalid", "m_axi_awvalid"), ("awready", "m_axi_awready")],
            &[],
        ))
        .unwrap_err();
    assert_eq!(
        err,
        "m: no signal for awaddr, wvalid, wready, wdata, bvalid, bready, aresetn (the file has: \
         decode m axi4 clock=top.aclk aresetn=top.m_axi_aresetn prefix=top.m_axi_)"
    );
}

/// axi4.vcd with only its required roles given: the payload roles the file lacks take the spec's
/// defaults (IHI 0022 Table B2.2, B2.3), and the sides pair in order, no ID given.
#[test]
fn axi4_defaults_for_absent_roles() {
    let mut ws = open("axi4.vcd");
    let (required, _) = wtf_core::protocol::get("axi4").unwrap().roles();
    let signals: Vec<(String, String)> = required
        .iter()
        .map(|r| ((*r).to_string(), format!("m_axi_{r}")))
        .collect();
    let signals: Vec<(&str, &str)> = signals
        .iter()
        .map(|(r, s)| (r.as_str(), s.as_str()))
        .collect();
    ws.add_decode(decode("m", "axi4", "aclk", &signals, &[]))
        .unwrap();
    let widths = |s: &str| {
        stream(&ws, s)
            .schema
            .iter()
            .map(|f| (f.name.clone(), f.width))
            .collect::<Vec<_>>()
    };
    // size is three bits: the log2 of wdata's bytes, 010 for a 32-bit wdata
    assert_eq!(
        widths("m.aw"),
        [
            ("addr".to_string(), Some(32)),
            ("len".to_string(), Some(8)),
            ("size".to_string(), Some(3)),
            ("burst".to_string(), Some(2)),
            ("prot".to_string(), Some(3)),
            ("lock".to_string(), Some(1)),
            ("cache".to_string(), Some(4)),
            ("qos".to_string(), Some(4)),
            ("region".to_string(), Some(4)),
        ]
    );
    // len 0 (one beat), size 010 (four bytes), burst 01 (INCR), prot 000, lock 0
    assert_eq!(
        table(stream(&ws, "m.aw")),
        [
            row(10, 20, &["256", "0", "2", "1", "0", "0", "0", "0", "0"]),
            row(30, 40, &["512", "0", "2", "1", "0", "0", "0", "0", "0"]),
        ]
    );
    // the file's wstrb 0b0011 is not bound: every byte of wdata is a data byte
    assert_eq!(
        table(stream(&ws, "m.w")),
        [
            row(20, 30, &["160", "15"]),
            row(30, 40, &["161", "15"]),
            row(40, 50, &["176", "15"]),
        ]
    );
    // both responses OKAY: the file's SLVERR is not bound
    assert_eq!(
        table(stream(&ws, "m.b")),
        [row(50, 60, &["0"]), row(60, 70, &["0"])]
    );
    assert_eq!(
        table(stream(&ws, "m.r")),
        [
            row(90, 100, &["3392", "0"]),
            row(100, 110, &["3376", "0"]),
            row(110, 120, &["3393", "0"]),
            row(120, 130, &["3377", "0"]),
        ]
    );
    // no ID either: the requests pair with the responses in order
    assert_eq!(
        table(stream(&ws, "m.wr"))
            .iter()
            .map(|r| (r.0, r.1))
            .collect::<Vec<_>>(),
        [(10, 60), (30, 70)]
    );
    assert_eq!(table(stream(&ws, "m.rd")).len(), 2);
}

/// axi4.vcd with awlock/awcache/awqos/awregion (and the read twins) declared and driven: lock 0,
/// cache/qos/region nonzero. They are payload fields of the request, so they reach `aw`/`ar` and
/// the paired writes/reads, and no rule of the channel reads them.
#[test]
fn axi4_request_fields_without_rules() {
    let dir = Dir::new("axi4-request-fields");
    let path = dir.0.join("fields.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let decl = "\
$var wire 1 A m_axi_awlock $end
\
$var wire 4 B m_axi_awcache $end
\
$var wire 4 C m_axi_awqos $end
\
$var wire 4 D m_axi_awregion $end
\
$var wire 1 E m_axi_arlock $end
\
$var wire 4 F m_axi_arcache $end
\
$var wire 4 G m_axi_arqos $end
\
$var wire 4 H m_axi_arregion $end
\
$upscope $end
$enddefinitions $end
";
    let text = text.replacen("$upscope $end\n$enddefinitions $end\n", decl, 1);
    // awlock 0 at every AW and AR: the exclusive rules of later phases never fire
    let values = "#0\n0A\nb0101 B\nb1010 C\nb0011 D\n0E\nb0110 F\nb1001 G\nb1100 H\n";
    let text = text.replacen("#0\n", values, 1);
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    // the request's own fields, in the spec's order: lock 0, then cache, qos and region
    assert_eq!(
        table(stream(&ws, "m.aw"))
            .iter()
            .map(|r| r.2[6..10].to_vec())
            .collect::<Vec<_>>(),
        [
            ["0", "5", "10", "3"].map(String::from).to_vec(),
            ["0", "5", "10", "3"].map(String::from).to_vec(),
        ]
    );
    assert_eq!(
        table(stream(&ws, "m.ar"))
            .iter()
            .map(|r| r.2[6..10].to_vec())
            .collect::<Vec<_>>(),
        [
            ["0", "6", "9", "12"].map(String::from).to_vec(),
            ["0", "6", "9", "12"].map(String::from).to_vec(),
        ]
    );
    // and they are carried into the paired transactions
    let wr = table(stream(&ws, "m.wr"));
    assert_eq!(wr[0].2[7..11], ["0", "5", "10", "3"]);
    let rd = table(stream(&ws, "m.rd"));
    assert_eq!(rd[0].2[7..11], ["0", "6", "9", "12"]);
    // no rule reads them: the errors are the plain trace's
    let mut plain = open("axi4.vcd");
    plain
        .add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        table(stream(&plain, "m.errors"))
    );
}

/// A3.4.1: without `wlast`/`rlast` a burst is AxLEN+1 beats, so axi4.vcd decodes as it does
/// with them, and the beats carry no `last`.
#[test]
fn axi4_last_optional_axlen_framing() {
    let roles = [
        "awvalid", "awready", "awaddr", "awid", "awlen", "wvalid", "wready", "wdata", "wstrb",
        "bvalid", "bready", "bid", "bresp", "arvalid", "arready", "araddr", "arid", "arlen",
        "rvalid", "rready", "rdata", "rid", "rresp", "aresetn",
    ];
    let signals: Vec<(&str, String)> = roles.iter().map(|r| (*r, format!("m_axi_{r}"))).collect();
    let signals: Vec<(&str, &str)> = signals.iter().map(|(r, s)| (*r, s.as_str())).collect();
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &signals, &[]))
        .unwrap();
    let mut last = open("axi4.vcd");
    last.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    for s in ["m.wburst", "m.rburst", "m.wr", "m.rd"] {
        assert_eq!(table(stream(&ws, s)), table(stream(&last, s)), "{s}");
    }
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
    assert_eq!(table(stream(&ws, "m.rd")).len(), 2);
    for s in ["m.w", "m.r"] {
        let has_last = |ws: &Workspace| stream(ws, s).schema.iter().any(|f| f.name == "last");
        assert!(!has_last(&ws), "{s} without LAST");
        assert!(has_last(&last), "{s} with LAST");
    }
}

/// axi4.vcd, read as text: each case moves one value change and loads its own copy.
fn axi4_text() -> String {
    std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap()
}

fn axi4_of(name: &str, text: &str) -> (Dir, Workspace) {
    let dir = Dir::new(name);
    let path = dir.0.join("t.vcd");
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    (dir, ws)
}

fn error_rows(ws: &Workspace, rule: &str) -> Vec<(u64, u64, Vec<String>)> {
    table(stream(ws, "m.errors"))
        .into_iter()
        .filter(|r| r.2[0] == rule)
        .collect()
}

fn axi4_roles(size: &str, burst: &str) -> String {
    let vars = "$var wire 3 ; m_axi_awsize [2:0] $end\n$var wire 2 < m_axi_awburst [1:0] $end\n";
    axi4_text()
        .replacen(
            "$var wire 8 & m_axi_awlen [7:0] $end\n",
            &format!("$var wire 8 & m_axi_awlen [7:0] $end\n{vars}"),
            1,
        )
        .replacen(
            "#12\n1\"\n1#\n",
            &format!("#12\n1\"\n1#\nb{size} ;\nb{burst} <\n"),
            1,
        )
}

/// The fixture's write 2 announces one beat (`AxLEN` 0) and its `wlast` is high there, so wtf has
/// nothing to report; `AxLEN` 2 makes three beats for write 1, whose `wlast` on the second is early
/// and whose burst then ends one beat short. An x or z `wlast` says nothing, and an `AxLEN` that
/// cannot be read leaves the beat it owns without a length.
#[test]
fn axi4_wlast_vs_len() {
    let text = axi4_text();
    let (_d, ws) = axi4_of("axi4-wlast", &text);
    assert!(error_rows(&ws, "w_last").is_empty());
    let len2 = text.replacen("b1 &\n", "b10 &\n", 1);
    assert!(len2.contains("b10 &\n"), "AxLEN changed");
    let (_d, ws) = axi4_of("axi4-wlast-len2", &len2);
    assert_eq!(
        error_rows(&ws, "w_last"),
        [row(30, 40, &["w_last", "wlast on beat 2 of 3", "error"])]
    );
    for last in ["x", "z"] {
        let text = len2.replacen("1+\n", &format!("{last}+\n"), 1);
        let (_d, ws) = axi4_of("axi4-wlast-unknown", &text);
        assert!(error_rows(&ws, "w_last").is_empty(), "wlast {last}");
    }
    let len0 = text.replacen("b1 &\n", "b0 &\n", 1);
    assert!(len0.contains("b0 &\n"), "AxLEN changed");
    let (_d, ws) = axi4_of("axi4-wlast-len0", &len0);
    assert_eq!(
        error_rows(&ws, "w_last"),
        [row(
            20,
            30,
            &["w_last", "missing wlast on beat 1 of 1", "error"]
        )]
    );
    let lenx = text.replacen("b1 &\n", "bx &\n", 1);
    let (_d, ws) = axi4_of("axi4-wlast-lenx", &lenx);
    assert!(error_rows(&ws, "w_last").is_empty());
    // no `wlast` role, no rule: the check reads the field out of the channel's schema
    let given = [
        ("awvalid", "m_axi_awvalid"),
        ("awready", "m_axi_awready"),
        ("awaddr", "m_axi_awaddr"),
        ("wvalid", "m_axi_wvalid"),
        ("wready", "m_axi_wready"),
        ("wdata", "m_axi_wdata"),
        ("bvalid", "m_axi_bvalid"),
        ("bready", "m_axi_bready"),
        ("arvalid", "m_axi_arvalid"),
        ("arready", "m_axi_arready"),
        ("araddr", "m_axi_araddr"),
        ("rvalid", "m_axi_rvalid"),
        ("rready", "m_axi_rready"),
        ("rdata", "m_axi_rdata"),
        ("aresetn", "m_axi_aresetn"),
    ];
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &given, &[]))
        .unwrap();
    assert!(error_rows(&ws, "w_last").is_empty());
    assert!(error_rows(&ws, "r_last").is_empty());
}

/// Read 4's first beat says last and read 3's last says not; each ID's beats count apart, so
/// neither read 3's first beat nor read 4's second is flagged.
#[test]
fn axi4_rlast_vs_len() {
    let mut text = axi4_text();
    let (_d, ws) = axi4_of("axi4-rlast", &text);
    assert!(error_rows(&ws, "r_last").is_empty());
    text = text.replacen("#92\n00\n01\n", "#92\n00\n01\n1:\n", 1);
    text = text.replacen("#102\n", "#102\n0:\n", 1);
    text = text.replacen("#122\n", "#122\n0:\n", 1);
    assert!(text.contains("#92\n00\n01\n1:\n") && text.contains("#102\n0:\n"));
    let (_d, ws) = axi4_of("axi4-rlast-moved", &text);
    assert_eq!(
        error_rows(&ws, "r_last"),
        [
            row(90, 100, &["r_last", "rlast on beat 1 of 2", "error"]),
            row(
                120,
                130,
                &["r_last", "missing rlast on beat 2 of 2", "error"]
            ),
        ]
    );
}

/// `wstrb` may only mark the byte lanes a write beat's address, size and burst give it (A3.2.1.1):
/// a narrow INCR burst's first beat takes the low lanes and its second the rest, a FIXED burst's
/// every beat the first's, and a WRAP burst counts its beats up in a box that may be smaller than
/// the bus.
#[test]
fn axi4_w_strb() {
    // write 1 of two one-byte beats from 0x100: lanes 0,1 while it says so, and 2,3 when it does
    let lanes = |start, end, n| {
        row(
            start,
            end,
            &[
                "w_strb",
                &format!("wstrb 1111 enables lanes 2,3 outside beat {n} of 2"),
                "error",
            ],
        )
    };

    // the plain fixture, whose beats strobe all four lanes of the full bus: nothing to report
    let (_d, ws) = axi4_of("axi4-strb-full", &axi4_roles("10", "01"));
    assert!(error_rows(&ws, "w_strb").is_empty());

    // two bytes a beat: write 1's first beat owns the low lanes and strobes all four
    let mut narrow = axi4_roles("1", "01");
    narrow = narrow.replacen("#32\n1\"\n1#\n", "#32\n1\"\n1#\nb1100 *\n", 1);
    let (_d, ws) = axi4_of("axi4-strb-narrow", &narrow);
    assert_eq!(error_rows(&ws, "w_strb"), [lanes(20, 30, 1)]);

    // FIXED repeats the address: both of write 1's beats own lanes 0,1 and strobe all four
    let (_d, ws) = axi4_of("axi4-strb-fixed", &axi4_roles("1", "00"));
    assert_eq!(
        error_rows(&ws, "w_strb"),
        [lanes(20, 30, 1), lanes(30, 40, 2)]
    );

    // WRAP of two one-byte beats from address 1: lane 1, then lane 0, in a box half the bus; write
    // 2 is INCR, so its length needs no wrap check
    let mut wrap = axi4_roles("0", "10");
    wrap = wrap.replacen("b100000000 $\n", "b1 $\n", 1);
    wrap = wrap.replacen("b1111 *\n", "b0010 *\n", 1);
    wrap = wrap.replacen("#32\n1\"\n1#\n", "#32\n1\"\n1#\nb01 <\nb0001 *\n", 1);
    wrap = wrap.replacen("b11 *\n", "b1 *\n", 1);
    let (_d, ws) = axi4_of("axi4-strb-wrap", &wrap);
    assert!(error_rows(&ws, "w_strb").is_empty());

    // an x strobe, address or size says nothing: no lane is judged (the port's own unknown rules
    // are other rows)
    for (what, text) in [
        ("wstrb", narrow.replacen("b1111 *\n", "bxxxx *\n", 1)),
        ("awaddr", narrow.replacen("b100000000 $\n", "bx $\n", 1)),
        ("awsize", narrow.replacen("b1 ;\n", "bx ;\n", 1)),
    ] {
        let (_d, ws) = axi4_of(&format!("axi4-strb-{what}"), &text);
        assert!(error_rows(&ws, "w_strb").is_empty(), "{what} is x");
    }

    // a beat wider than the bus is the size rule's row, and its lanes are not this rule's to judge
    let (_d, ws) = axi4_of("axi4-strb-wide", &axi4_roles("11", "01"));
    assert!(error_rows(&ws, "w_strb").is_empty());

    // a beat past every request's length belongs to no request: its strobe judges nothing (its
    // missing wlast is the LAST rule's row)
    let mut stray = axi4_roles("1", "01");
    stray = stray.replacen("b1 &\n", "b0 &\n", 1);
    stray = stray.replacen("b1111 *\n", "b0011 *\n", 1);
    stray = stray.replacen("#32\n1\"\n1#\n", "#32\n1\"\n1#\nb0011 *\n", 1);
    stray = stray.replacen("b11 *\n", "b1111 *\n", 1);
    let (_d, ws) = axi4_of("axi4-strb-stray", &stray);
    assert!(error_rows(&ws, "w_strb").is_empty());
}

/// axi4.vcd with awvalid x from 22ns to 32ns, sampled at the edge between write 1's AW and write
/// 2's: x is no value to read, so that cycle is an `aw_unknown` error and the handshakes are
/// what the plain decode has.
#[test]
fn axi4_unknown_valid() {
    let dir = Dir::new("axi4-unknown");
    let path = dir.0.join("unknown.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let text = text.replacen("#22\n0\"\n", "#22\nx\"\n", 1);
    assert!(text.contains("#22\nx\"\n"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let mut plain = open("axi4.vcd");
    plain
        .add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    // the one cycle that samples the x, then the plain decode's two rows
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(
                20,
                30,
                &["aw_unknown", "awvalid is x out of reset", "error"]
            ),
            row(50, 60, &["b_resp", "bresp is SLVERR", "warning"]),
            row(
                140,
                150,
                &["ar_dropped", "arvalid fell before arready", "error"]
            ),
        ]
    );
    for s in ["m.aw", "m.wr"] {
        assert_eq!(table(stream(&ws, s)), table(stream(&plain, s)), "{s}");
    }
}

/// axi4.vcd with write 2's SLVERR `bresp` x instead: the response says nothing, so it is neither
/// `b_resp` nor `b_exokay`, only `b_payload_unknown`; the write still pairs with its B.
#[test]
fn axi4_unknown_resp_is_no_response_rule() {
    let dir = Dir::new("axi4-unknown-resp");
    let path = dir.0.join("resp.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let text = text.replacen("b10 /\n", "bxx /\n", 1);
    assert!(text.contains("bxx /"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(
                50,
                60,
                &["b_payload_unknown", "bresp is x while bvalid", "error"]
            ),
            row(
                140,
                150,
                &["ar_dropped", "arvalid fell before arready", "error"]
            ),
        ]
    );
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
}

/// A `timeout` that is no time is refused when the decode is made, naming what it could not read.
#[test]
fn axi4_bad_timeout_is_refused() {
    let mut ws = open("axi4.vcd");
    let err = ws
        .add_decode(decode(
            "m",
            "axi4",
            "aclk",
            &[],
            &[("prefix", "m_axi_"), ("timeout", "soon")],
        ))
        .unwrap_err();
    assert!(err.contains("join: timeout: "), "{err}");
}

/// Each AXI4 stream says the streams it was made from (spec §16: `from`).
#[test]
fn axi4_records_from() {
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let inputs = |s: &str| stream(&ws, s).inputs.clone();
    assert_eq!(inputs("m.wburst"), ["m.w"]);
    assert_eq!(inputs("m.rburst"), ["m.r"]);
    assert_eq!(inputs("m.wr"), ["m.aw", "m.wburst", "m.b"]);
    assert_eq!(inputs("m.rd"), ["m.ar", "m.rburst"]);
    assert!(inputs("m.aw").is_empty() && inputs("m.errors").is_empty());
}

/// The rules a write and a read share, one fault each in axi4_errors.vcd, in the order the trace
/// makes them: `awvalid` in reset (10ns) and in the cycle reset is released (20ns), `awvalid`
/// dropped before `awready` (60ns), `wdata` changing while `wvalid` stalls (90ns), a B before its
/// write's AW and W (150ns), an R with no AR (200ns), EXOKAY to a write and a read that are not
/// locked (240ns, 270ns), SLVERR and DECERR (330ns, 360ns), `arvalid` x out of reset (380ns) and
/// `wdata` x while `wvalid` (400ns).
#[test]
fn axi4_shared_error_rules() {
    let mut ws = open("axi4_errors.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(10, 20, &["aw_reset", "awvalid in reset", "error"]),
            row(
                20,
                30,
                &[
                    "aw_reset",
                    "awvalid in the cycle reset is released",
                    "error"
                ]
            ),
            row(
                60,
                70,
                &["aw_dropped", "awvalid fell before awready", "error"]
            ),
            row(
                90,
                100,
                &["w_changed", "wdata changed while wvalid stalled", "error"]
            ),
            row(
                150,
                160,
                &["b_early", "response 1 before write 1's AW and W", "error"]
            ),
            row(
                200,
                210,
                &["r_early", "response 1 before read 1's AR", "error"]
            ),
            row(
                240,
                250,
                &[
                    "b_exokay",
                    "bresp is EXOKAY to a non-exclusive write",
                    "error"
                ]
            ),
            row(
                270,
                280,
                &[
                    "r_exokay",
                    "rresp is EXOKAY to a non-exclusive read",
                    "error"
                ]
            ),
            row(330, 340, &["b_resp", "bresp is SLVERR", "warning"]),
            row(360, 370, &["r_resp", "rresp is DECERR", "warning"]),
            row(
                380,
                390,
                &["ar_unknown", "arvalid is x out of reset", "error"]
            ),
            row(
                400,
                410,
                &["w_payload_unknown", "wdata is x while wvalid", "error"]
            ),
        ]
    );
}

/// The rules only a request and its beats break, one fault each in axi4_burst_errors.vcd, every
/// response OKAY so the shared rules stay quiet: a reserved `awburst` and `arburst` (20ns, 60ns),
/// WRAP with the wrong length and with an unaligned address (100ns, 160ns), FIXED past 16 beats
/// (210ns, 400ns), a burst across a 4KB boundary (600ns, 660ns), a size wider than the bus (730ns,
/// 770ns), a strobe outside its beat's lanes (820ns), an exclusive request not aligned to its total
/// size (860ns, 900ns), `wlast` on beat 2 of 4 (950ns) and the last `rlast` missing (1030ns). The
/// second burst of the write whose `wlast` comes early has no AW left: `no aw` is the join's.
#[test]
fn axi4_burst_request_strobe_exclusive_rules() {
    let mut ws = open("axi4_burst_errors.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.errors")),
        [
            row(20, 30, &["aw_burst", "awburst is reserved (0b11)", "error"]),
            row(60, 70, &["ar_burst", "arburst is reserved (0b11)", "error"]),
            row(
                100,
                110,
                &[
                    "aw_wrap",
                    "awlen 2 with WRAP: length must be 2, 4, 8 or 16",
                    "error"
                ]
            ),
            row(
                160,
                170,
                &[
                    "ar_wrap",
                    "araddr 0x1002 with WRAP: not aligned to 4 bytes",
                    "error"
                ]
            ),
            row(
                210,
                220,
                &["aw_fixed", "awlen 16 with FIXED: at most 16 beats", "error"]
            ),
            row(
                400,
                410,
                &["ar_fixed", "arlen 16 with FIXED: at most 16 beats", "error"]
            ),
            row(
                600,
                610,
                &[
                    "aw_4k",
                    "awaddr 0xff4, 4 beats of 4 bytes: crosses a 4KB boundary",
                    "error"
                ]
            ),
            row(
                660,
                670,
                &[
                    "ar_4k",
                    "araddr 0x1ff4, 4 beats of 4 bytes: crosses a 4KB boundary",
                    "error"
                ]
            ),
            row(
                730,
                740,
                &[
                    "aw_size",
                    "awsize 3 wider than the data bus (4 bytes)",
                    "error"
                ]
            ),
            row(
                770,
                780,
                &[
                    "ar_size",
                    "arsize 3 wider than the data bus (4 bytes)",
                    "error"
                ]
            ),
            row(
                820,
                830,
                &[
                    "w_strb",
                    "wstrb 1110 enables lane 1 outside beat 2 of 2",
                    "error"
                ]
            ),
            row(
                860,
                870,
                &[
                    "aw_exclusive",
                    "aw exclusive: address not aligned to 4 bytes",
                    "error"
                ]
            ),
            row(
                900,
                910,
                &[
                    "ar_exclusive",
                    "ar exclusive: address not aligned to 4 bytes",
                    "error"
                ]
            ),
            row(950, 960, &["w_last", "wlast on beat 2 of 4", "error"]),
            row(
                1030,
                1040,
                &["r_last", "missing rlast on beat 2 of 2", "error"]
            ),
        ]
    );
    assert_eq!(
        table(stream(&ws, "m.wr.unmatched")),
        [row(960, 980, &["no aw"])]
    );
    assert!(table(stream(&ws, "m.rd.unmatched")).is_empty());
}

/// SPI in spi.vcd (mode 0): a transfer of 0xA5 0x3C out and 0x5A 0xC3 in; a pulse of sclk with cs
/// high; then 12 bits, 0xF0 and 0b1010 out, whose second word cs cuts short.
#[test]
fn spi_words_and_transfers() {
    let pins = [
        ("sclk", "spi_clk"),
        ("cs", "spi_cs_n"),
        ("mosi", "spi_mosi"),
        ("miso", "spi_miso"),
    ];
    let mut ws = open("spi.vcd");
    ws.add_decode(decode("f", "spi", "", &pins, &[])).unwrap();
    let bits = table(stream(&ws, "f.bit"));
    // sampled at sclk's rise, each to the next, the last to sclk's fall; none while cs is high
    assert_eq!(
        (bits.len(), &bits[0], &bits[15]),
        (28, &row(110, 130, &["1", "0"]), &row(410, 420, &["0", "1"]))
    );
    assert_eq!(
        table(stream(&ws, "f.word")),
        [
            row(110, 270, &["165", "90", "1"]),
            row(270, 420, &["60", "195", "1"]),
            row(510, 670, &["240", "15", "1"]),
            row(670, 750, &["10", "5", "0"]),
        ]
    );
    assert_eq!(
        table(stream(&ws, "f.transfer")),
        [
            row(100, 430, &["2", "a5 3c", "5a c3", "1"]),
            row(500, 750, &["2", "f0 a", "0f 5", "1"])
        ]
    );
    // LSB first, and no miso
    let mut ws = open("spi.vcd");
    ws.add_decode(decode("f", "spi", "", &pins[..3], &[("order", "lsb")]))
        .unwrap();
    assert_eq!(stream(&ws, "f.word").schema.len(), 2);
    assert_eq!(
        table(stream(&ws, "f.transfer"))[1],
        row(500, 750, &["2", "0f 5", "1"])
    );
    // mistakes, all at once
    let err = ws
        .add_decode(decode(
            "g",
            "spi",
            "",
            &pins,
            &[("mode", "4"), ("bits", "0"), ("speed", "1")],
        ))
        .unwrap_err();
    assert_eq!(
        err,
        "g: spi has no param speed (params: prefix, mode, bits, order, cs_active)"
    );
    let err = ws
        .add_decode(decode(
            "g",
            "spi",
            "",
            &pins,
            &[("mode", "4"), ("bits", "0")],
        ))
        .unwrap_err();
    assert_eq!(err, "g: spi: mode=4: not 0, 1, 2 or 3; bits=0: not 1 - 64");
}

/// UART in uart.vcd at 1 Mbaud, even parity: 0x55 and 0xA3; 0x0F with a wrong parity bit; 0x81 with
/// a stop bit of 0; and a 100ns glitch that isn't a start bit.
#[test]
fn uart_bytes_and_errors() {
    let mut ws = open("uart.vcd");
    ws.add_decode(decode(
        "u",
        "uart",
        "",
        &[("line", "uart_tx")],
        &[("baud", "1000000"), ("parity", "even")],
    ))
    .unwrap();
    let want = [
        row(1000, 12000, &["85"]),
        row(13000, 24000, &["163"]),
        row(25000, 36000, &["15"]),
        row(37000, 48000, &["129"]),
    ];
    assert_eq!(table(stream(&ws, "u.byte")), want);
    let errors = stream(&ws, "u.errors");
    assert!(errors.violations);
    assert_eq!(
        table(errors),
        [
            row(
                25000,
                36000,
                &["parity", "parity bit 1, expected 0", "error"]
            ),
            row(
                37000,
                48000,
                &["framing", "stop bit 0, expected 1", "error"]
            )
        ]
    );
    // without parity the parity bit is taken for the stop bit: 0 for 0x55, 0xA3 and 0x81
    let mut ws = open("uart.vcd");
    ws.add_decode(decode(
        "u",
        "uart",
        "",
        &[("line", "uart_tx")],
        &[("baud", "1000000")],
    ))
    .unwrap();
    let errors = table(stream(&ws, "u.errors"));
    assert_eq!(
        errors.iter().map(|r| r.0).collect::<Vec<_>>(),
        [1000, 13000, 37000]
    );
    assert!(
        errors
            .iter()
            .all(|r| r.2 == ["framing", "stop bit 0, expected 1", "error"]),
        "{errors:?}"
    );
    let err = ws
        .add_decode(decode("v", "uart", "", &[("line", "uart_tx")], &[]))
        .unwrap_err();
    assert_eq!(
        err,
        "v: uart: needs baud=<a positive number of bits a second>"
    );
}

/// A port's reset is found as its clock is: in the scope when not under the prefix, active high
/// unless its name ends in `n`; none, or two, leave `aresetn=<rst>` to fill in.
#[test]
fn detects_the_reset() {
    let line = |resets: &[&str]| {
        let dir = Dir::new(&format!("reset-{}", resets.join("-")));
        let vcd = dir.0.join("r.vcd");
        let mut text =
            String::from("$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! clk $end\n");
        for (id, r) in "\"#$".chars().zip(["s_tvalid", "s_tready", "s_tdata"]) {
            text += &format!("$var wire 1 {id} {r} $end\n");
        }
        for (id, r) in "%&".chars().zip(resets) {
            text += &format!("$var wire 1 {id} {r} $end\n");
        }
        text += "$upscope $end\n$enddefinitions $end\n#0\n0!\n";
        std::fs::write(&vcd, text).unwrap();
        let t = wtf_core::trace::Trace::open(&vcd).unwrap();
        wtf_core::protocol::detect(&t)
            .iter()
            .map(|f| f.decode_line())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        line(&["rst_n"]),
        ["decode s axis clock=top.clk aresetn=top.rst_n prefix=top.s_"]
    );
    assert_eq!(
        line(&["rst"]),
        ["decode s axis clock=top.clk aresetn=top.rst reset_active=high prefix=top.s_"]
    );
    for low in ["rst_b", "rst_l", "nreset"] {
        assert_eq!(
            line(&[low]),
            [format!(
                "decode s axis clock=top.clk aresetn=top.{low} prefix=top.s_"
            )]
        );
    }
    assert_eq!(
        line(&[]),
        ["decode s axis clock=top.clk aresetn=<rst> prefix=top.s_"]
    );
    assert_eq!(
        line(&["rst", "sys_reset_n"]),
        ["decode s axis clock=top.clk aresetn=<rst> prefix=top.s_"]
    );
}

/// The interfaces built-in protocols decode, by their signals' names, with a decode line each.
#[test]
fn detects_interfaces() {
    let found = |f: &str| {
        let t = wtf_core::trace::Trace::open(
            format!("{}/../tests/data/{f}", env!("CARGO_MANIFEST_DIR")).as_ref(),
        )
        .unwrap();
        wtf_core::protocol::detect(&t)
            .iter()
            .map(|f| f.decode_line())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        found("axilite.vcd"),
        ["decode s_axi axilite clock=top.aclk aresetn=top.s_axi_aresetn prefix=top.s_axi_"]
    );
    assert_eq!(
        found("axis.vcd"),
        ["decode axis axis clock=top.clk aresetn=top.aresetn prefix=top."]
    );
    // UART has one role: `line` is never an interface
    assert!(found("uart.vcd").is_empty());
    // AXI4 over AXI-Lite, whose roles it has too
    assert_eq!(
        found("axi4.vcd")
            .iter()
            .filter(|l| l.contains(" axilite "))
            .count(),
        0
    );
    assert!(
        found("axi4.vcd").iter().any(|l| l.contains(" axi4 ")),
        "{:?}",
        found("axi4.vcd")
    );
    assert!(found("simple.vcd").is_empty());
}

/// One role is no interface: a lone `tvalid`, every other AXIS role at its default, is a stray
/// signal rather than a port.
#[test]
fn one_role_is_no_interface() {
    let dir = Dir::new("one-role");
    let vcd = dir.0.join("tvalid.vcd");
    let text = "$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! clk $end\n$var wire 1 \" tvalid $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\n0\"\n#5\n1!\n1\"\n";
    std::fs::write(&vcd, text).unwrap();
    let t = wtf_core::trace::Trace::open(&vcd).unwrap();
    assert!(wtf_core::protocol::detect(&t).is_empty());
}

/// A write-only or read-only AXI-Lite port: the side given is decoded, the other left out, and a
/// read-only one is found by its signals.
#[test]
fn axilite_one_side() {
    let mut ws = open("axilite.vcd");
    let write: Vec<(&str, String)> = [
        "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "bvalid", "bready", "aresetn",
    ]
    .iter()
    .map(|r| (*r, format!("s_axi_{r}")))
    .collect();
    let write: Vec<(&str, &str)> = write.iter().map(|(r, s)| (*r, s.as_str())).collect();
    ws.add_decode(decode("w", "axilite", "aclk", &write, &[]))
        .unwrap();
    assert_eq!(table(stream(&ws, "w.wr")).len(), 2);
    assert!(
        !ws.streams()
            .iter()
            .any(|s| s.name.starts_with("w.ar") || s.name.starts_with("w.rd"))
    );
    // half a side is still an error
    let err = ws
        .add_decode(decode("x", "axilite", "aclk", &write[..7], &[]))
        .unwrap_err();
    assert_eq!(
        err,
        "x: no signal for bready, aresetn (the file has: decode x axilite clock=top.aclk aresetn=top.s_axi_aresetn prefix=top.s_axi_)"
    );
    let dir = Dir::new("axilite-read");
    let vcd = dir.0.join("rd.vcd");
    let mut text =
        String::from("$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! aclk $end\n");
    for (id, (r, w)) in "\"#$%&'(".chars().zip([
        ("arvalid", 1),
        ("arready", 1),
        ("araddr", 8),
        ("rvalid", 1),
        ("rready", 1),
        ("rdata", 8),
        ("aresetn", 1),
    ]) {
        text += &format!("$var wire {w} {id} m_axi_{r} $end\n");
    }
    text += "$upscope $end\n$enddefinitions $end\n#0\n0!\n1(\n0\"\n1#\nb0 $\n0%\n1&\nb0 '\n#5\n1!\n#10\n0!\n1\"\nb100 $\n#15\n1!\n#20\n0!\n0\"\n1%\nb1010 '\n#25\n1!\n#30\n0!\n0%\n#35\n1!\n#40\n0!\n";
    std::fs::write(&vcd, text).unwrap();
    let t = wtf_core::trace::Trace::open(&vcd).unwrap();
    let found: Vec<String> = wtf_core::protocol::detect(&t)
        .iter()
        .map(|f| f.decode_line())
        .collect();
    assert_eq!(
        found,
        ["decode m_axi axilite clock=top.aclk aresetn=top.m_axi_aresetn prefix=top.m_axi_"]
    );
    let mut ws = Workspace::open(&vcd).unwrap();
    ws.add_decode(decode("m", "axilite", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "m.rd")),
        [row(5, 25, &["10", "4", "0", "10", "0"])]
    );
    assert!(!ws.streams().iter().any(|s| s.name == "m.wr"));
}

/// The roles of IHI 0022L Table B2.2 are required, `aresetn` too; the five it lets a slave leave
/// out are optional with the value the spec gives them.
#[test]
fn axilite_required_roles() {
    let def = wtf_core::protocol::get("axilite").unwrap();
    let (required, optional) = def.roles();
    assert_eq!(
        required,
        [
            "awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "bvalid", "bready",
            "arvalid", "arready", "araddr", "rvalid", "rready", "rdata", "aresetn"
        ]
    );
    assert_eq!(optional, ["awprot", "wstrb", "bresp", "arprot", "rresp"]);
    assert_eq!(
        def.defaults(),
        [
            "awprot 000",
            "wstrb bytes of wdata",
            "bresp 00",
            "arprot 000",
            "rresp 00"
        ]
    );
    assert_eq!(def.params(), ["timeout", "reset_active"]);
    // half a side is no port, even with the other side's optional roles there
    let write: Vec<(&str, &str)> = [
        ("awvalid", "s_axi_awvalid"),
        ("awready", "s_axi_awready"),
        ("awaddr", "s_axi_awaddr"),
        ("wvalid", "s_axi_wvalid"),
        ("wready", "s_axi_wready"),
        ("wdata", "s_axi_wdata"),
        ("bvalid", "s_axi_bvalid"),
    ]
    .to_vec();
    let mut ws = open("axilite.vcd");
    let err = ws
        .add_decode(decode("x", "axilite", "aclk", &write, &[]))
        .unwrap_err();
    assert_eq!(
        err,
        "x: no signal for bready, aresetn (the file has: decode x axilite clock=top.aclk aresetn=top.s_axi_aresetn prefix=top.s_axi_)"
    );
}

/// SPI's cs active high and UART's line idle low: the fixtures with that pin inverted decode as
/// the fixtures do.
#[test]
fn inverted_cs_and_line() {
    let dir = Dir::new("inverted");
    let inverted = |f: &str, id: char| {
        let text =
            std::fs::read_to_string(format!("{}/../tests/data/{f}", env!("CARGO_MANIFEST_DIR")))
                .unwrap();
        let flip = |l: &str| match (l.chars().next(), &l[1..]) {
            (Some('0'), rest) if rest == id.to_string() => format!("1{id}"),
            (Some('1'), rest) if rest == id.to_string() => format!("0{id}"),
            _ => l.to_string(),
        };
        let path = dir.0.join(f);
        std::fs::write(&path, text.lines().map(flip).collect::<Vec<_>>().join("\n")).unwrap();
        Workspace::open(&path).unwrap()
    };
    let pins = [
        ("sclk", "spi_clk"),
        ("cs", "spi_cs_n"),
        ("mosi", "spi_mosi"),
        ("miso", "spi_miso"),
    ];
    let mut ws = open("spi.vcd");
    ws.add_decode(decode("f", "spi", "", &pins, &[])).unwrap();
    let mut high = inverted("spi.vcd", '"');
    high.add_decode(decode("f", "spi", "", &pins, &[("cs_active", "high")]))
        .unwrap();
    assert_eq!(
        table(stream(&high, "f.transfer")),
        table(stream(&ws, "f.transfer"))
    );
    let line = [("line", "uart_tx")];
    let mut ws = open("uart.vcd");
    ws.add_decode(decode("u", "uart", "", &line, &[("baud", "1000000")]))
        .unwrap();
    let mut low = inverted("uart.vcd", '!');
    low.add_decode(decode(
        "u",
        "uart",
        "",
        &line,
        &[("baud", "1000000"), ("idle", "low")],
    ))
    .unwrap();
    assert_eq!(table(stream(&low, "u.byte")), table(stream(&ws, "u.byte")));
    let err = high
        .add_decode(decode("g", "spi", "", &pins, &[("cs_active", "up")]))
        .unwrap_err();
    assert_eq!(err, "g: spi: cs_active=up: not low or high");
    let err = low
        .add_decode(decode(
            "v",
            "uart",
            "",
            &line,
            &[("baud", "1000000"), ("idle", "up")],
        ))
        .unwrap_err();
    assert_eq!(err, "v: uart: idle=up: not high or low");
}

/// UART at 1 Mbaud, 2 stop bits: 0x55 whose second stop bit is 0, then a break (the line low
/// 20000ns - 35000ns), then 0x55 again, then 0x00.
#[test]
fn uart_break_and_stop_bits() {
    let dir = Dir::new("uart-break");
    let vcd = dir.0.join("b.vcd");
    // 0x55 LSB first: start 0, 1 0 1 0 1 0 1 0, stop 1, stop 0
    let frame = |at: u64, bits: &str| {
        bits.chars()
            .enumerate()
            .map(|(i, b)| format!("#{}\n{b}!\n", at + i as u64 * 1000))
            .collect::<String>()
    };
    let mut text = String::from(
        "$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! uart_tx $end\n$upscope $end\n$enddefinitions $end\n#0\n1!\n",
    );
    text += &frame(1000, "01010101010");
    text += "#12000\n1!\n#20000\n0!\n#35000\n1!\n";
    text += &frame(40000, "01010101011");
    text += "#60000\n1!\n";
    // 0x00 with its stop bits high: a byte, not a break
    text += &frame(62000, "00000000011");
    text += "#80000\n1!\n";
    std::fs::write(&vcd, text).unwrap();
    let mut ws = Workspace::open(&vcd).unwrap();
    ws.add_decode(decode(
        "u",
        "uart",
        "",
        &[("line", "uart_tx")],
        &[("baud", "1000000"), ("stop", "2")],
    ))
    .unwrap();
    assert_eq!(
        table(stream(&ws, "u.errors")),
        [
            row(1000, 12000, &["framing", "stop bit 0, expected 1", "error"]),
            row(
                20000,
                31000,
                &["break", "the line was low for a whole frame", "warning"]
            )
        ]
    );
    assert_eq!(
        table(stream(&ws, "u.byte"))
            .iter()
            .map(|r| (r.0, r.2[0].as_str()))
            .collect::<Vec<_>>(),
        [(1000, "85"), (40000, "85"), (62000, "0")]
    );
}

/// A write-only or read-only AXI4 port, by its roles given; and a port with only part of a side
/// isn't found.
#[test]
fn axi4_one_side() {
    let roles = |rs: &[&str]| {
        rs.iter()
            .map(|r| (r.to_string(), format!("m_axi_{r}")))
            .collect::<Vec<_>>()
    };
    let write = roles(&[
        "awvalid", "awready", "awaddr", "awid", "wvalid", "wready", "wdata", "wlast", "bvalid",
        "bready", "bid", "aresetn",
    ]);
    let read = roles(&[
        "arvalid", "arready", "araddr", "arid", "rvalid", "rready", "rdata", "rlast", "rid",
        "aresetn",
    ]);
    let names = |ws: &Workspace| {
        ws.streams()
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>()
    };
    let reads = ["m.ar", "m.r", "m.rburst", "m.rd", "m.rd.unmatched"];
    let writes = ["m.aw", "m.w", "m.b", "m.wburst", "m.wr", "m.wr.unmatched"];
    for (side, has, hasnt) in [(&write, "m.wr", &reads[..]), (&read, "m.rd", &writes[..])] {
        let mut ws = open("axi4.vcd");
        let given: Vec<(&str, &str)> = side.iter().map(|(r, s)| (r.as_str(), s.as_str())).collect();
        ws.add_decode(decode("m", "axi4", "aclk", &given, &[]))
            .unwrap();
        assert_eq!(table(stream(&ws, has)).len(), 2, "{:?}", names(&ws));
        assert!(
            !names(&ws).iter().any(|n| hasnt.contains(&n.as_str())),
            "{:?}",
            names(&ws)
        );
    }
    // awvalid without the rest of its side: neither protocol finds the port
    let dir = Dir::new("axi-half");
    let vcd = dir.0.join("half.vcd");
    let mut text =
        String::from("$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! aclk $end\n");
    for (id, (r, w)) in "\"#$%&'(".chars().zip([
        ("awvalid", 1),
        ("arvalid", 1),
        ("arready", 1),
        ("araddr", 8),
        ("rvalid", 1),
        ("rready", 1),
        ("rdata", 8),
    ]) {
        text += &format!("$var wire {w} {id} m_axi_{r} $end\n");
    }
    text += "$upscope $end\n$enddefinitions $end\n#0\n0!\n#10\n1!\n";
    std::fs::write(&vcd, text).unwrap();
    let t = wtf_core::trace::Trace::open(&vcd).unwrap();
    assert!(wtf_core::protocol::detect(&t).is_empty());
}

/// SPI's last bit: where sclk stays still after it, cs rising ends it.
#[test]
fn spi_last_bit_at_cs() {
    let dir = Dir::new("spi-still");
    let vcd = dir.0.join("s.vcd");
    // cs falls at 10, sclk rises at 20 with mosi 1, cs rises at 50, sclk falls at 60
    let text = "$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! sclk $end\n$var wire 1 \" cs $end\n$var wire 1 # mosi $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\n1\"\n1#\n#10\n0\"\n#20\n1!\n#50\n1\"\n#60\n0!\n#80\n";
    std::fs::write(&vcd, text).unwrap();
    let mut ws = Workspace::open(&vcd).unwrap();
    ws.add_decode(decode(
        "s",
        "spi",
        "",
        &[("sclk", "sclk"), ("cs", "cs"), ("mosi", "mosi")],
        &[("bits", "1")],
    ))
    .unwrap();
    assert_eq!(table(stream(&ws, "s.bit")), [row(20, 50, &["1"])]);
}

/// The three streams are one chain: a word from bits, a transfer from words, so only the transfer
/// is a result (spec §16 "Results").
#[test]
fn spi_streams_are_made_from_bits() {
    let pins = [
        ("sclk", "spi_clk"),
        ("cs", "spi_cs_n"),
        ("mosi", "spi_mosi"),
        ("miso", "spi_miso"),
    ];
    let mut ws = open("spi.vcd");
    let first = ws.add_decode(decode("f", "spi", "", &pins, &[])).unwrap();
    assert_eq!(first, var(&ws, "f.bit"));
    let inputs = |s: &str| stream(&ws, s).inputs.clone();
    assert!(inputs("f.bit").is_empty());
    assert_eq!(inputs("f.word"), ["f.bit"]);
    assert_eq!(inputs("f.transfer"), ["f.word"]);
    let names: Vec<String> = ws
        .results("f")
        .into_iter()
        .map(|i| ws.streams()[i].name.clone())
        .collect();
    assert_eq!(names, ["f.transfer"]);
}

/// A bit still open where the trace ends ends there, in the last cycle it was given.
#[test]
fn spi_edge_at_trace_end() {
    let dir = Dir::new("spi-end");
    let vcd = dir.0.join("e.vcd");
    // cs falls at 10, sclk rises at 20 with mosi 1, sclk falls at 30, where the trace ends
    let text = "$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! sclk $end\n$var wire 1 \" cs $end\n$var wire 1 # mosi $end\n$upscope $end\n$enddefinitions $end\n#0\n0!\n1\"\n1#\n#10\n0\"\n#20\n1!\n#30\n0!\n";
    std::fs::write(&vcd, text).unwrap();
    let mut ws = Workspace::open(&vcd).unwrap();
    ws.add_decode(decode(
        "s",
        "spi",
        "",
        &[("sclk", "sclk"), ("cs", "cs"), ("mosi", "mosi")],
        &[("bits", "1")],
    ))
    .unwrap();
    assert_eq!(table(stream(&ws, "s.bit")), [row(20, 31, &["1"])]);
}

/// A protocol without a clock is fed the changes of its own roles, not of every signal: a clock
/// and a signal it doesn't name change nothing.
#[test]
fn spi_ignores_other_signals() {
    let dir = Dir::new("spi-noisy");
    let pins = [("sclk", "sclk"), ("cs", "cs"), ("mosi", "mosi")];
    let build = |extra: bool| {
        // 4 bits out, 1 0 1 1 (0xB), sclk rising at 20, 40, 60 and 80, cs low from 10 to 100
        let mut ev: Vec<(u64, String)> = [
            (0, "0!"),
            (0, "1\""),
            (0, "0#"),
            (10, "0\""),
            (15, "1#"),
            (20, "1!"),
            (30, "0!"),
            (35, "0#"),
            (40, "1!"),
            (50, "0!"),
            (55, "1#"),
            (60, "1!"),
            (70, "0!"),
            (75, "1#"),
            (80, "1!"),
            (90, "0!"),
            (100, "1\""),
        ]
        .iter()
        .map(|&(t, l)| (t, l.to_string()))
        .collect();
        if extra {
            ev.extend(
                (0..=120)
                    .step_by(5)
                    .map(|t| (t, format!("{}$", (t / 5) % 2))),
            );
            ev.extend(
                (0..=120)
                    .step_by(7)
                    .map(|t| (t, format!("{}%", (t / 7) % 2))),
            );
        }
        ev.sort_by_key(|e| e.0);
        let mut text = String::from(
            "$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! sclk $end\n$var wire 1 \" cs $end\n$var wire 1 # mosi $end\n",
        );
        if extra {
            text += "$var wire 1 $ clk $end\n$var wire 1 % other $end\n";
        }
        text += "$upscope $end\n$enddefinitions $end\n";
        let mut at = None;
        for (t, line) in ev {
            if at != Some(t) {
                text += &format!("#{t}\n");
                at = Some(t);
            }
            text += &format!("{line}\n");
        }
        let path = dir.0.join(if extra { "noisy.vcd" } else { "plain.vcd" });
        std::fs::write(&path, text).unwrap();
        let mut ws = Workspace::open(&path).unwrap();
        ws.add_decode(decode("f", "spi", "", &pins, &[("bits", "4")]))
            .unwrap();
        table(stream(&ws, "f.word"))
    };
    let plain = build(false);
    assert_eq!(plain, [row(20, 90, &["11", "1"])]);
    assert_eq!(build(true), plain);
}

#[test]
fn decodes_of_a_signal() {
    // a signal's decodes and roles: the clock, one by the prefix, one given apart, a stream taken
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode(
        "s",
        "axilite",
        "aclk",
        &[("wdata", "s_axi_wdata")],
        &[("prefix", "s_axi_")],
    ))
    .unwrap();
    ws.add_decode(decode("t", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let of = |ws: &Workspace, path: &str| {
        let v = ws.trace.vars.iter().position(|v| v.path == path).unwrap();
        ws.decodes_of(v)
            .into_iter()
            .map(|(d, r)| format!("{d} {r}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(of(&ws, "top.aclk"), ["s clock", "t clock"]);
    assert_eq!(of(&ws, "top.s_axi_awvalid"), ["s awvalid", "t awvalid"]);
    assert_eq!(of(&ws, "top.s_axi_wdata"), ["s wdata", "t wdata"]);
    assert!(of(&ws, "s.aw").is_empty());
}

/// Spec §16 "Results" and "Records of a set": what nothing is made from is a result, and a set of
/// streams reads as one table.
#[test]
fn results_and_sets() {
    let mut ws = open("axilite.vcd");
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let names = |ws: &wtf_core::workspace::Workspace, v: Vec<usize>| {
        v.into_iter()
            .map(|i| ws.streams()[i].name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&ws, ws.results("s")),
        [
            "s.wr",
            "s.rd",
            "s.errors",
            "s.wr.unmatched",
            "s.rd.unmatched"
        ]
    );
    assert_eq!(
        names(&ws, ws.stream_set("s").unwrap()),
        names(&ws, ws.results("s"))
    );
    assert_eq!(
        names(&ws, ws.stream_set("s.rd s.w* s.rd").unwrap()),
        ["s.rd", "s.w", "s.wr"]
    );
    assert!(
        ws.stream_set("s.nope")
            .unwrap_err()
            .to_string()
            .starts_with("unknown stream: s.nope (streams: s.aw,")
    );
    // a frame on rd makes rd intermediate, across decodes
    ws.add_decode(decode(
        "f",
        "frame",
        "",
        &[("input", "s.rd")],
        &[("gap", "100ns")],
    ))
    .unwrap();
    assert!(!names(&ws, ws.results("s")).contains(&"s.rd".to_string()));
    // wr and rd share addr, prot, data, resp, delay; strb is wr's alone
    let set = wtf_core::stream::Set::new(vec![
        &ws.streams()[ws.stream_set("s.wr").unwrap()[0]],
        &ws.streams()[ws.stream_set("s.rd").unwrap()[0]],
    ]);
    assert_eq!(
        set.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["delay", "addr", "prot", "data", "strb", "resp"]
    );
    let (all, total) = set.select(0, u64::MAX, None, ws.trace.fs_per_tick).unwrap();
    assert_eq!((all.len(), total), (10, 10));
    assert!(
        all.windows(2)
            .all(|w| set.record(w[0]).start <= set.record(w[1]).start)
    );
    // a filter keeps the streams having every field it names
    let (strb, _) = set.select(0, u64::MAX, Some("strb == 3"), None).unwrap();
    assert_eq!(strb, [(0, 1)]);
    let (resp, _) = set.select(0, u64::MAX, Some("resp != 0"), None).unwrap();
    assert_eq!(resp.iter().map(|&(s, _)| s).collect::<Vec<_>>(), [1]);
    assert!(
        set.select(0, u64::MAX, Some("nope == 1"), None)
            .unwrap_err()
            .starts_with("no field nope in s.wr, s.rd (fields: delay, addr")
    );
    let groups = set.group_by(&all, &["resp".into()]).unwrap();
    assert_eq!(groups.iter().map(|g| g.count).sum::<usize>(), 10);
    // a name with two kinds is a column per stream
    let set = wtf_core::stream::Set::new(vec![
        &ws.streams()[ws.stream_set("s.errors").unwrap()[0]],
        &ws.streams()[ws.stream_set("s.wr.unmatched").unwrap()[0]],
    ]);
    assert_eq!(
        set.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["rule", "detail", "severity", "kind"]
    );
}

/// Spec §16: `records` names a stream, and a field it lacks lists those it has.
#[test]
fn set_errors_and_briefs() {
    let mut ws = open("axilite.vcd");
    assert_eq!(
        ws.stream_set("  ").unwrap_err().to_string(),
        "records of which stream? (a stream, a decode's name or a glob)"
    );
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")]))
        .unwrap();
    let one = wtf_core::stream::Set::new(vec![&ws.streams()[ws.stream_set("s.wr").unwrap()[0]]]);
    assert!(
        one.group_by(&[], &["nope".into()])
            .err()
            .unwrap()
            .starts_with("s.wr: no field nope (fields: ")
    );
    let two = wtf_core::stream::Set::new(vec![
        &ws.streams()[ws.stream_set("s.wr").unwrap()[0]],
        &ws.streams()[ws.stream_set("s.rd").unwrap()[0]],
    ]);
    assert!(
        two.group_by(&[], &["nope".into()])
            .err()
            .unwrap()
            .starts_with("no field nope (fields: ")
    );

    // `protocols <name>` in short: a built-in, a generic one, one of the session's, none
    let brief = |n: &str, ws: &Workspace| wtf_core::protocol::brief(n, ws.sources());
    assert!(
        brief("axilite", &ws)
            .unwrap()
            .starts_with("decode <name> axilite clock=<clk> prefix=<prefix>")
    );
    assert!(
        brief("frame", &ws)
            .unwrap()
            .starts_with("decode <name> frame ...\n\nrecords of a stream")
    );
    assert_eq!(brief("cmd", &ws), None);
    ws.add_protocol(Sampler {
        name: "cmd".into(),
        when: "valid && ready".into(),
        fields: vec![("op".into(), "cmd[3:0]".into())],
        roles: vec!["valid".into(), "ready".into(), "cmd".into()],
        wasm: None,
    })
    .unwrap();
    assert_eq!(
        brief("cmd", &ws).unwrap(),
        "decode <name> cmd clock=<clk> valid=<signal> ready=<signal> cmd=<signal>\n\nA record where valid && ready\nFields: op"
    );
}

/// Spec §16 "Records of a set": a name of an interface the file has but nothing decoded yet is an
/// error saying the line that decodes it, and a part of a decoded interface is a name unknown.
#[test]
fn undecoded_interface_names() {
    let mut ws = open("axilite.vcd");
    let line = "decode s_axi axilite clock=top.aclk aresetn=top.s_axi_aresetn prefix=top.s_axi_";
    let part = |ws: &Workspace, name: &str| match ws.stream_set(name).unwrap_err() {
        wtf_core::workspace::unknown::NoStream::NotDecoded {
            name: n,
            part,
            found,
        } => {
            assert_eq!(n, name);
            assert_eq!(
                found.iter().map(|f| f.decode_line()).collect::<Vec<_>>(),
                [line]
            );
            part
        }
        e => panic!("{e}"),
    };
    assert_eq!(part(&ws, "top.s_axi.wr"), "wr");
    assert_eq!(part(&ws, "s_axi.wr"), "wr");
    assert_eq!(part(&ws, "s_axi"), "");
    assert_eq!(part(&ws, "top.s_axi"), "");
    // as core says it: the line, and the stream it makes or its streams
    let said = |name: &str| ws.stream_set(name).unwrap_err().to_string();
    assert_eq!(
        said("s_axi.wr"),
        format!(
            "unknown stream: s_axi.wr: its interface isn't decoded yet; `{line}` makes s_axi.wr"
        )
    );
    assert_eq!(
        said("s_axi"),
        format!(
            "unknown stream: s_axi: its interface isn't decoded yet; `{line}` makes its streams"
        )
    );
    assert_eq!(
        said("nope"),
        "unknown stream: nope (none yet; make one with decode or protocol)"
    );
    // decoded, its streams are what it makes; a part it has not is a name unknown
    ws.add_decode(decode(
        "s_axi",
        "axilite",
        "top.aclk",
        &[],
        &[("prefix", "top.s_axi_")],
    ))
    .unwrap();
    assert!(
        matches!(ws.stream_set("s_axi.zz").unwrap_err(), wtf_core::workspace::unknown::NoStream::Unknown { name, .. } if name == "s_axi.zz")
    );
}

/// simple.vcd: a clocked search as a protocol, as `find` offers it (spec §15 `find`).
#[test]
fn protocol_of_a_search() {
    let mut ws = with(&[]);
    let shown = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    // `rose` and numbers aren't roles; two `state`s take their scope's name; slices are fields
    let (p, d) = ws
        .protocol_of(
            "ev",
            "s",
            "@clk rose(busy) && cpu.state == 2",
            &shown(&["dma.state", "data[7:4]", "data[3:0]"]),
        )
        .unwrap();
    assert_eq!(
        p.line(),
        "protocol ev roles=busy,cpu_state,dma_state,data when=rose(busy) && cpu_state == 2 dma_state=dma_state data_1=data[7:4] data_2=data[3:0]"
    );
    assert_eq!(
        d.line(),
        "decode s ev clock=clk busy=busy cpu_state=cpu.state dma_state=dma.state data=data"
    );
    // the lines read back as what they say
    assert_eq!(
        wtf_core::command::parse_decode(
            d.line().strip_prefix("decode ").unwrap(),
            wtf_core::protocol::Sources::default()
        )
        .unwrap(),
        d
    );
    wtf_core::command::run(&mut ws, &p.line()).unwrap();
    assert_eq!(ws.protocols().last(), Some(&p));
    wtf_core::command::run(&mut ws, &d.line()).unwrap();
    assert_eq!(wtf_core::command::quote("valid && b=1"), "\"valid && b=1\"");
    assert!(ws.protocol_of("ev", "s", "@clk nosuch", &[]).is_err());
    assert!(
        ws.protocol_of("ev", "s", "busy", &[])
            .unwrap_err()
            .contains("clocked")
    );
}

#[test]
fn free_names() {
    let mut ws = with(&[]);
    assert_eq!(ws.free_name("ev"), "ev");
    wtf_core::command::run(&mut ws, "protocol ev roles=a when=a").unwrap();
    assert_eq!(ws.free_name("ev"), "ev1");
}

/// The packets fixture's signals: every role but the reset and `tuser`.
const PACKETS: &[(&str, &str)] = &[
    ("tvalid", "tvalid"),
    ("tready", "tready"),
    ("tdata", "tdata"),
    ("tlast", "tlast"),
    ("tkeep", "tkeep"),
    ("tstrb", "tstrb"),
    ("tid", "tid"),
    ("tdest", "tdest"),
    ("aresetn", "aresetn"),
];

/// axis_packets.vcd: clk rises at 5, 15 ...; two packets by (tid, tdest), one of them left open,
/// a beat whose tkeep drops a byte tstrb keeps, then an x payload and a tvalid that falls.
#[test]
fn axis_frames_per_tid_tdest() {
    let mut ws = open("axis_packets.vcd");
    assert_eq!(
        ws.add_decode(decode("rx", "axis", "clk", PACKETS, &[]))
            .unwrap(),
        var(&ws, "rx.beat")
    );
    // beats, tid, tdest, len (bytes tkeep marks), pos (tstrb's), the bytes, complete
    let frames = table(stream(&ws, "rx.frame"));
    let want = [
        row(
            15,
            35,
            &["2", "1", "0", "8", "0", "a0 00 00 00 a1 00 00 00", "1"],
        ),
        row(35, 45, &["1", "2", "3", "3", "0", "00 00 00", "0"]),
        row(45, 55, &["1", "1", "0", "4", "0", "c0 00 00 00", "0"]),
    ];
    assert_eq!(frames, want);
    assert_eq!(
        stream(&ws, "rx.frame")
            .schema
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["beats", "tid", "tdest", "len", "pos", "data", "complete"]
    );
    // the beat's fields follow the spec's order, those given only
    assert_eq!(
        stream(&ws, "rx.beat")
            .schema
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["tdata", "tkeep", "tstrb", "tlast", "tid", "tdest"]
    );
    // one rule per packet, found as a signal
    ws.search("rx.frame.tid == 2", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 35);
}

/// axis_reset.vcd: aresetn low for the first cycles with tvalid up, then a beat after it is
/// released; `reset_active=high` reads the same signal upside down.
#[test]
fn axis_reset_takes_no_beats() {
    let mut ws = open("axis_reset.vcd");
    let reset = [
        ("tvalid", "tvalid"),
        ("tready", "tready"),
        ("tdata", "tdata"),
        ("aresetn", "aresetn"),
    ];
    ws.add_decode(decode("rx", "axis", "clk", &reset, &[]))
        .unwrap();
    // tvalid up in reset is an error, and the cycle it is released is still too early
    assert_eq!(
        table(stream(&ws, "rx.errors")),
        [
            row(5, 15, &["reset", "tvalid in reset", "error"]),
            row(
                15,
                25,
                &["reset", "tvalid in the cycle reset is released", "error"]
            )
        ]
    );
    assert_eq!(table(stream(&ws, "rx.beat")), [row(25, 35, &["160"])]);
    // read as active high, the same signal holds the other cycles: the first beat is out of it
    let mut ws = open("axis_reset.vcd");
    ws.add_decode(decode(
        "rx",
        "axis",
        "clk",
        &reset,
        &[("reset_active", "high")],
    ))
    .unwrap();
    // the same signal holds the other cycles, so the beat is the early one
    assert_eq!(
        table(stream(&ws, "rx.errors")),
        [
            row(15, 25, &["reset", "tvalid in reset", "error"]),
            row(25, 35, &["reset", "tvalid in reset", "error"])
        ]
    );
    assert_eq!(table(stream(&ws, "rx.beat")), [row(5, 15, &["160"])]);
    // a reset of x, as a testbench's before it drives it, says nothing of the tvalid under it and
    // takes no beat; the cycle it is released in is as ever
    let dir = Dir::new("axis-x-reset");
    let path = dir.0.join("x.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axis_reset.vcd")).unwrap();
    std::fs::write(&path, text.replacen("0%\n", "x%\n", 1)).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("rx", "axis", "clk", &reset, &[]))
        .unwrap();
    assert_eq!(
        table(stream(&ws, "rx.errors")),
        [row(
            15,
            25,
            &["reset", "tvalid in the cycle reset is released", "error"]
        )]
    );
    assert_eq!(table(stream(&ws, "rx.beat")), [row(25, 35, &["160"])]);
}

/// What `protocols uart` says: the one role, the params, the two streams, and the three rules its
/// `errors` carries — with their severity, which the step's `notes` line could not give.
#[test]
fn host_uart_describe() {
    let text =
        wtf_core::protocol::describe("uart", wtf_core::protocol::Sources::default()).unwrap();
    assert!(
        text.starts_with("uart: UART: bytes from an idle-high line (idle=low: inverted) at a baud rate, framing and parity errors, breaks\nroles: line (required)"),
        "{text}"
    );
    assert!(text.contains("\n  baud bits parity stop idle\n"), "{text}");
    assert!(text.contains("  <name>.byte: data\n"), "{text}");
    assert!(
        text.contains("  <name>.errors: rule detail severity (violations)"),
        "{text}"
    );
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with("        ") && l.contains(", spec "))
            .count(),
        3,
        "{text}"
    );
}

/// What `protocols axis` says: the roles, the defaults that make a short instance decode, and the
/// streams the trait makes.
#[test]
fn host_defaults_and_describe() {
    let text =
        wtf_core::protocol::describe("axis", wtf_core::protocol::Sources::default()).unwrap();
    assert!(
        text.starts_with("axis: AXI4-Stream: beats, packets by tlast and (tid, tdest), the handshake's errors\nroles: tvalid aresetn (required)"),
        "{text}"
    );
    // a signal not given reads a default, not an error: that is what makes a short instance decode
    assert!(text.contains("  when not given: tready 1, tdata 0, tkeep bytes of tdata, tstrb same as tkeep, tlast 1, tid 0, tdest 0, tuser 0"), "{text}");
    assert!(text.contains("\n  reset_active\n"), "{text}");
    assert!(
        text.contains("  <name>.errors: rule detail severity (violations)"),
        "{text}"
    );
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with("        ") && l.contains(", spec "))
            .count(),
        8,
        "{text}"
    );
}

/// A protocol from a file, as a later phase will make one: a card, and a factory the registry
/// calls for each instance.
struct Beats;

impl wtf_core::protocol::Protocol for Beats {
    fn desc(&self) -> &wtf_core::protocol::Desc {
        static DESC: std::sync::OnceLock<wtf_core::protocol::Desc> = std::sync::OnceLock::new();
        DESC.get_or_init(|| wtf_core::protocol::Desc {
            streams: vec![wtf_core::protocol::StreamDesc {
                name: "beat",
                fields: Vec::new(),
                violations: false,
                about: Vec::new(),
                from: &[],
            }],
        })
    }

    fn feed(
        &mut self,
        start: u64,
        end: u64,
        _roles: &wtf_core::protocol::Roles<'_>,
    ) -> Vec<wtf_core::protocol::Out> {
        vec![wtf_core::protocol::Out::Record {
            stream: "beat",
            start,
            end,
            fields: Vec::new(),
        }]
    }

    fn finish(&mut self) -> Vec<wtf_core::protocol::Out> {
        Vec::new()
    }
}

fn loaded(name: &'static str, about: &'static str) -> std::sync::Arc<wtf_core::protocol::Loaded> {
    use wtf_core::protocol;
    const ROLES: &[protocol::RoleDef] = &[protocol::RoleDef {
        name: "valid",
        required: true,
        default: None,
        about: "a beat this cycle",
    }];
    let make: protocol::Factory =
        std::sync::Arc::new(|_, _, _| Ok(Box::new(Beats) as Box<dyn protocol::Protocol>));
    let def = protocol::Def {
        about,
        example: "decode b clock=<clk> prefix=<prefix> valid=<signal>",
        clocked: true,
        sides: &[],
        roles: ROLES,
        params: &[],
        make: protocol::NO_MAKE,
    };
    std::sync::Arc::new(protocol::Loaded { name, def, make })
}

/// Spec §16: protocols loaded from files are the registry's third source — listed between the
/// on-streams and the session's, described and decoded as a built-in is, and their names kept.
#[test]
fn loaded_protocols() {
    use wtf_core::protocol;
    let mut ws = open("axilite.vcd");
    ws.push_loaded(loaded("beats", "one beat a cycle, from a file"));

    // listed between the on-streams protocols and the session's own
    let list = protocol::list(ws.sources());
    assert!(list.contains("\nloaded from files: decode <name> <protocol> ...\n  beats    one beat a cycle, from a file\nprotocols <name>"), "{list}");

    // described and briefed as a built-in is
    let text = protocol::describe("beats", ws.sources()).unwrap();
    assert!(
        text.starts_with("beats: one beat a cycle, from a file\nroles: valid (required)"),
        "{text}"
    );
    assert!(text.contains("  <name>.beat:"), "{text}");
    assert_eq!(
        protocol::brief("beats", ws.sources()).unwrap(),
        "decode <name> beats clock=<clk> prefix=<prefix>\n\none beat a cycle, from a file\nRoles: valid\nStreams: beat"
    );

    // the roles and the clock a `:decode` line gives it are its card's
    let any = ws.sources().get("beats").unwrap();
    assert_eq!(
        (any.roles(), any.params(), any.clocked()),
        ((vec!["valid"], vec![]), vec![], true)
    );

    // decoded on signals: its factory makes the instance
    ws.add_decode(decode("b", "beats", "aclk", &[], &[("prefix", "s_axi_aw")]))
        .unwrap();
    assert!(!stream(&ws, "b.beat").records.is_empty());

    // a loaded name is taken: the session may not define another, and `free_name` goes around it
    let cmd = Sampler {
        name: "beats".into(),
        when: "valid".into(),
        fields: Vec::new(),
        roles: vec!["valid".into()],
        wasm: None,
    };
    assert_eq!(
        ws.add_protocol(cmd).unwrap_err(),
        "beats is a protocol loaded from a file; give another name"
    );
    assert_eq!(ws.free_name("beats"), "beats1");

    // a built-in shadows a loaded one of its name
    ws.push_loaded(loaded("axis", "never reached"));
    assert!(
        protocol::describe("axis", ws.sources())
            .unwrap()
            .starts_with("axis: AXI4-Stream"),
        "a loaded axis must not win"
    );

    // `:e` reads the trace again; the loaded protocol is the process's, so the decode stays
    ws.reload().unwrap();
    assert!(!stream(&ws, "b.beat").records.is_empty());
}

/// Only tvalid, the clock and the reset: tready reads 1, tlast reads 1, the rest read 0.
#[test]
fn axis_tvalid_only_decodes() {
    let mut ws = open("axis.vcd");
    ws.add_decode(decode(
        "v",
        "axis",
        "clk",
        &[("tvalid", "tvalid"), ("aresetn", "aresetn")],
        &[],
    ))
    .unwrap();
    // no payload role given, so the beat carries no fields — but every tvalid cycle is one
    assert_eq!(table(stream(&ws, "v.beat"))[0], row(15, 25, &[]));
    // tlast reads 1: every beat ends its own packet, with no bytes and complete
    assert_eq!(
        stream(&ws, "v.frame")
            .schema
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["beats", "len", "complete"]
    );
    assert_eq!(
        table(stream(&ws, "v.frame"))[0],
        row(15, 25, &["1", "0", "1"])
    );
}

/// The error rules the packets fixture reaches: tkeep where tstrb is 1, an x payload while tvalid,
/// and a tvalid that falls. Twakeup's drop needs a stalled cycle with `twakeup` high, which the
/// packets fixture never sets: that rule, and the handshake-unknown errors, are pinned by
/// `axis_wakeup.vcd` below.
#[test]
fn axis_error_rules() {
    let mut ws = open("axis_packets.vcd");
    let signals: &[(&str, &str)] = &[
        ("tvalid", "tvalid"),
        ("tready", "tready"),
        ("tdata", "tdata"),
        ("tlast", "tlast"),
        ("tkeep", "tkeep"),
        ("tstrb", "tstrb"),
        ("tid", "tid"),
        ("tdest", "tdest"),
        ("aresetn", "aresetn"),
    ];
    ws.add_decode(decode("rx", "axis", "clk", signals, &[]))
        .unwrap();
    let errors: Vec<_> = table(stream(&ws, "rx.errors"))
        .into_iter()
        .filter(|(_, _, f)| f[2] == "error")
        .collect();
    assert_eq!(
        errors,
        [
            row(35, 45, &["keep", "tkeep 0, tstrb 1 on byte 0", "error"]),
            row(
                65,
                75,
                &["payload_unknown", "tdata is x while tvalid", "error"]
            ),
            row(75, 85, &["dropped", "tvalid fell before tready", "error"])
        ]
    );
    // twakeup's drop: a stall with `twakeup` high, then low while the transfer still waits
    let mut ws = open("axis_wakeup.vcd");
    ws.add_decode(decode(
        "rx",
        "axis",
        "clk",
        &[
            ("tvalid", "tvalid"),
            ("tready", "tready"),
            ("twakeup", "twakeup"),
            ("aresetn", "aresetn"),
        ],
        &[],
    ))
    .unwrap();
    let errors: Vec<_> = table(stream(&ws, "rx.errors"))
        .into_iter()
        .filter(|(_, _, f)| f[2] == "error")
        .collect();
    assert_eq!(
        errors,
        [
            row(
                15,
                25,
                &["wakeup", "twakeup fell while the transfer stalled", "error"]
            ),
            row(25, 35, &["dropped", "tvalid fell before tready", "error"]),
            row(35, 45, &["unknown", "tvalid is x out of reset", "error"]),
            row(45, 55, &["unknown", "tready is x out of reset", "error"]),
        ]
    );
}

/// The warning rule: twakeup in the same cycle as tvalid (`axis_wakeup.vcd`, which stalls without
/// ever transferring).
#[test]
fn axis_warning_rules() {
    let mut ws = open("axis_wakeup.vcd");
    ws.add_decode(decode(
        "rx",
        "axis",
        "clk",
        &[
            ("tvalid", "tvalid"),
            ("tready", "tready"),
            ("twakeup", "twakeup"),
            ("aresetn", "aresetn"),
        ],
        &[],
    ))
    .unwrap();
    let warnings: Vec<_> = table(stream(&ws, "rx.errors"))
        .into_iter()
        .filter(|(_, _, f)| f[2] == "warning")
        .collect();
    assert_eq!(
        warnings,
        [row(
            5,
            15,
            &[
                "wakeup_early",
                "twakeup in the same cycle as tvalid",
                "warning"
            ]
        ),]
    );
}

/// A protocol from a file takes its params from its card: `spin=` is one of `probe`'s, not a
/// signal of the instance (spec §16 "A protocol from a file").
#[test]
fn wasm_protocol_params() {
    let mut ws = open("simple.vcd");
    ws.push_loaded(common::loaded("probe", &["spin"]));
    wtf_core::command::run(&mut ws, "decode p probe clock=top.clk spin=3").unwrap();
    let d = ws.decodes().iter().find(|d| d.name == "p").unwrap();
    assert!(d.signals.is_empty(), "{:?}", d.signals);
    assert_eq!(d.params, vec![("spin".to_string(), "3".to_string())]);
}

/// A protocol from a file (spec §11): the file's name is the protocol's, the entry keeps nothing
/// of its own, a built-in's name is taken, and loading it again replaces it. A session naming one
/// that didn't load decodes nothing of it and says why.
#[test]
fn protocol_from_a_file() {
    use std::sync::Arc;
    use wtf_core::command::run;
    use wtf_core::protocol;
    let mut ws = open("axis.vcd");
    assert_eq!(
        run(&mut ws, "protocol foo wasm=p/bar.wasm"),
        Err("wasm p/bar.wasm holds bar, not foo: :protocol bar wasm=p/bar.wasm".into())
    );
    assert_eq!(
        run(&mut ws, "protocol bar wasm=p/bar.wasm when=tvalid"),
        Err(
            "a protocol from a file has no roles, when or fields of its own: :protocol bar wasm=p/bar.wasm"
                .into()
        )
    );
    ws.loader = Some(Arc::new(|path: &str| {
        let stem = std::path::Path::new(path).file_stem().unwrap();
        Ok(loaded(
            protocol::intern(stem.to_str().unwrap()),
            "a test's protocol",
        ))
    }));
    assert_eq!(
        run(&mut ws, "protocol axis wasm=p/axis.wasm"),
        Err("axis is a built-in protocol".into())
    );
    run(&mut ws, "protocol bar wasm=p/bar.wasm").unwrap();
    run(&mut ws, "protocol bar wasm=p/bar.wasm").unwrap();
    assert_eq!(ws.loaded().iter().filter(|l| l.name == "bar").count(), 1);

    let mut ws = open("axis.vcd");
    let session = "[[protocol]]\nname = \"baz\"\nwasm = \"p/baz.wasm\"\n\n\
        [[protocol]]\nname = \"qux\"\nwasm = \"p/qux.wasm\"\nroles = [\"a\"]\n\n\
        [[decode]]\nname = \"z\"\nprotocol = \"baz\"\nclock = \"clk\"\n";
    let warnings = wtf_core::session::apply(&mut ws, session).unwrap();
    assert_eq!(
        warnings,
        [
            "protocol baz: wasm p/baz.wasm is not loaded",
            "protocol qux: wasm p/qux.wasm keeps no roles, its card is the file's",
            "decode z: wasm p/baz.wasm is not loaded"
        ]
    );
    assert!(ws.streams().is_empty());
}

/// A protocol on streams: its roles are the streams it reads, its params its own, and its card
/// is its prose (spec §16).
#[test]
fn on_stream_protocol_card() {
    use wtf_core::protocol;
    let ws = open("reqresp.vcd");
    let any = ws.sources().get("pair").unwrap();
    assert_eq!(
        (any.roles(), any.params(), any.clocked()),
        (
            (vec!["request", "response"], vec![]),
            vec!["id", "req_id", "resp_id"],
            false
        )
    );
    let text = protocol::describe("pair", ws.sources()).unwrap();
    assert!(
        text.starts_with("pair: each response with its request, by order or by an id field, with the delay\nroles: request, response (streams)\n"),
        "{text}"
    );
}

/// axi4.vcd with a B for write 1 in the cycle of its last W beat: the burst has not ended, so the
/// response is `b_early` (A2.3.2), answers nothing, and both writes keep their own B.
#[test]
fn axi4_b_early_mid_burst() {
    let dir = Dir::new("axi4-b-early-mid-burst");
    let path = dir.0.join("early.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let text = text
        .replacen("#32\n1\"\n1#\n", "#32\n1\"\n1#\n1,\n1-\nb1 .\n", 1)
        .replacen("#42\n0\"\n0#\n", "#42\n0\"\n0#\n0,\n0-\n", 1);
    assert!(text.contains("#32\n1\"\n1#\n1,\n1-\nb1 .\n"));
    assert!(text.contains("#42\n0\"\n0#\n0,\n0-\n"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "m.errors"));
    let early: Vec<_> = errors.iter().filter(|r| r.2[0] == "b_early").collect();
    assert_eq!(
        early,
        [&row(
            30,
            40,
            &["b_early", "response 1 before write 1's AW and W", "error"]
        )]
    );
    // the early B answers nothing: both writes are still joined, and nothing is unmatched
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
    assert!(stream(&ws, "m.wr.unmatched").records.is_empty());
}

/// axi4.vcd as it is: write 2's B comes in the cycle right after its burst's last beat, which is
/// not early.
#[test]
fn axi4_b_after_last_beat() {
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "m.errors"));
    assert!(errors.iter().all(|r| r.2[0] != "b_early"), "{errors:?}");
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
    assert!(stream(&ws, "m.wr.unmatched").records.is_empty());
}

/// axi4.vcd's reads take two beats, their R beats interleaved by ID: a burst's first beat comes
/// after its AR, so no burst is early and both reads join.
#[test]
fn axi4_multi_beat_read_not_early() {
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "m.errors"));
    assert!(errors.iter().all(|r| r.2[0] != "r_early"), "{errors:?}");
    assert_eq!(table(stream(&ws, "m.rd")).len(), 2);
    assert!(stream(&ws, "m.rd.unmatched").records.is_empty());
}

/// axi4.vcd with bvalid and bready high in the cycle of write 1's AW handshake, before its W
/// beat: the B is early once (A2.3.2), and the two writes still join the B responses after it.
#[test]
fn axi4_b_early_same_cycle() {
    let dir = Dir::new("axi4-b-early-same-cycle");
    let path = dir.0.join("early.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    let text = text
        .replacen("#12\n1\"\n1#\n", "#12\n1\"\n1#\n1,\n1-\nb1 .\n", 1)
        .replacen("#22\n0\"\n0#\n", "#22\n0\"\n0#\n0,\n0-\n", 1);
    assert!(text.contains("#12\n1\"\n1#\n1,\n1-\nb1 .\n"));
    assert!(text.contains("#22\n0\"\n0#\n0,\n0-\n"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "m.errors"));
    let early: Vec<_> = errors.iter().filter(|r| r.2[0] == "b_early").collect();
    assert_eq!(
        early,
        [&row(
            10,
            20,
            &["b_early", "response 1 before write 1's AW and W", "error"]
        )]
    );
    assert_eq!(table(stream(&ws, "m.wr")).len(), 2);
    assert!(stream(&ws, "m.wr.unmatched").records.is_empty());
}

/// Under `timeout=15ns`, axi4.vcd's write 1 (its B 40ns after its AW and W) and read 3 wait too
/// long: each request is `no response` and each late response `timeout`, never `no request`, and
/// neither is said early.
#[test]
fn axi4_late_response_is_timeout() {
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode(
        "m",
        "axi4",
        "aclk",
        &[],
        &[("prefix", "m_axi_"), ("timeout", "15ns")],
    ))
    .unwrap();
    let kinds = |side: &str| -> Vec<(u64, String)> {
        table(stream(&ws, side))
            .into_iter()
            .map(|r| (r.0, r.2[0].clone()))
            .collect()
    };
    assert_eq!(
        kinds("m.wr.unmatched"),
        [(10, "no response".to_string()), (60, "timeout".to_string())]
    );
    assert_eq!(
        kinds("m.rd.unmatched"),
        [
            (70, "no response".to_string()),
            (100, "timeout".to_string())
        ]
    );
    assert_eq!(table(stream(&ws, "m.wr")).len(), 1);
    assert_eq!(table(stream(&ws, "m.rd")).len(), 1);
    let errors = table(stream(&ws, "m.errors"));
    assert!(
        errors
            .iter()
            .all(|r| r.2[0] != "b_early" && r.2[0] != "r_early"),
        "{errors:?}"
    );
}

/// A one-sided AXI4 port's VCD: every role of both sides declared, the driven channel's `valid`
/// and `ready` high throughout, its `addr`, `len`, `size`, `burst` and `lock` driven in the cycle
/// before each rising edge, and nothing else ever valid. Cycle k is `(10(k+1), 10(k+1)+10)`: the
/// trace's first rising edge is at 10ns, and nothing before it is in a cycle.
fn request_vcd(dir: &Dir, chan: &str, cycles: &[(u64, u64, u64, u64, u64)]) -> PathBuf {
    let roles = [
        ("awvalid", 1u32),
        ("awready", 1),
        ("awaddr", 32),
        ("awlen", 8),
        ("awsize", 3),
        ("awburst", 2),
        ("awlock", 1),
        ("wvalid", 1),
        ("wready", 1),
        ("wdata", 32),
        ("wlast", 1),
        ("bvalid", 1),
        ("bready", 1),
        ("arvalid", 1),
        ("arready", 1),
        ("araddr", 32),
        ("arlen", 8),
        ("arsize", 3),
        ("arburst", 2),
        ("arlock", 1),
        ("rvalid", 1),
        ("rready", 1),
        ("rdata", 32),
        ("rlast", 1),
        ("aresetn", 1),
    ];
    // the VCD identifier of each role: a printable character, `!` for `aclk` first
    let id = |role: &str| {
        let i = 1 + roles.iter().position(|(r, _)| *r == role).unwrap();
        char::from_u32(33 + i as u32).unwrap()
    };
    let mut text =
        String::from("$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! aclk $end\n");
    for (i, (role, width)) in roles.iter().enumerate() {
        let range = if *width > 1 {
            format!(" [{}:0]", width - 1)
        } else {
            String::new()
        };
        let id = char::from_u32(34 + i as u32).unwrap();
        text += &format!("$var wire {width} {id} m_axi_{role}{range} $end\n");
    }
    text += &format!(
        "$upscope $end\n$enddefinitions $end\n#0\n0!\n1{}\n#10\n1!\n",
        id("aresetn")
    );
    for f in ["valid", "ready"] {
        text += &format!("1{}\n", id(&format!("{chan}{f}")));
    }
    for (k, &(addr, len, size, burst, lock)) in cycles.iter().enumerate() {
        let t = 10 * (k as u64 + 1);
        text += &format!(
            "#{}\nb{addr:b} {}\nb{len:b} {}\nb{size:b} {}\nb{burst:b} {}\n{lock}{}\n\
             #{}\n0!\n#{}\n1!\n",
            t + 2,
            id(&format!("{chan}addr")),
            id(&format!("{chan}len")),
            id(&format!("{chan}size")),
            id(&format!("{chan}burst")),
            id(&format!("{chan}lock")),
            t + 5,
            t + 10,
        );
    }
    text += &format!("#{}\n0!\n", 10 * (cycles.len() as u64 + 1) + 5);
    let path = dir.0.join(format!("{chan}.vcd"));
    std::fs::write(&path, text).unwrap();
    path
}

/// The rows of `errors` after one handshake per entry of `cycles`.
fn request_errors(
    chan: &str,
    name: &str,
    cycles: &[(u64, u64, u64, u64, u64)],
) -> Vec<(u64, u64, Vec<String>)> {
    let dir = Dir::new(name);
    let path = request_vcd(&dir, chan, cycles);
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    table(stream(&ws, "m.errors"))
}

/// `AxBURST` `2'b11` is reserved (A3.1.4): one row per request, on `aw` and `ar` alike, and the
/// INCR after it fires nothing.
#[test]
fn axi4_request_burst_rule() {
    for chan in ["aw", "ar"] {
        assert_eq!(
            request_errors(
                chan,
                "axi4-burst-rule",
                &[(0x100, 0, 2, 3, 0), (0x100, 0, 2, 1, 0)]
            ),
            [row(
                10,
                20,
                &[
                    &format!("{chan}_burst"),
                    &format!("{chan}burst is reserved (0b11)"),
                    "error"
                ]
            )]
        );
    }
}

/// A WRAP burst is 2, 4, 8 or 16 beats and starts aligned to its transfer's size (A3.1.4): 3
/// beats, and 2 beats of 4 bytes from 0x302, fire; 0x300 is legal.
#[test]
fn axi4_request_wrap_rule() {
    assert_eq!(
        request_errors(
            "aw",
            "axi4-wrap-rule",
            &[
                (0x300, 2, 2, 2, 0),
                (0x302, 1, 2, 2, 0),
                (0x300, 1, 2, 2, 0)
            ],
        ),
        [
            row(
                10,
                20,
                &[
                    "aw_wrap",
                    "awlen 2 with WRAP: length must be 2, 4, 8 or 16",
                    "error"
                ]
            ),
            row(
                20,
                30,
                &[
                    "aw_wrap",
                    "awaddr 0x302 with WRAP: not aligned to 4 bytes",
                    "error"
                ]
            ),
        ]
    );
}

/// A FIXED burst is at most 16 beats (A3.1.2): 17 fires, 16 does not.
#[test]
fn axi4_request_fixed_rule() {
    assert_eq!(
        request_errors(
            "aw",
            "axi4-fixed-rule",
            &[(0x300, 16, 2, 0, 0), (0x300, 15, 2, 0, 0)]
        ),
        [row(
            10,
            20,
            &["aw_fixed", "awlen 16 with FIXED: at most 16 beats", "error"]
        )]
    );
}

/// An INCR burst may not cross a 4KB boundary (A3.1.2): 2 beats of 4 bytes from 0xffc reach into
/// the next page, from 0xff8 they stop at its edge. 4 beats of 4 bytes from the unaligned 0xff2
/// count from 0xff0 (A3.1.6), so they stop there too.
#[test]
fn axi4_request_4k_rule() {
    assert_eq!(
        request_errors(
            "aw",
            "axi4-4k-rule",
            &[
                (0xffc, 1, 2, 1, 0),
                (0xff8, 1, 2, 1, 0),
                (0xff2, 3, 2, 1, 0)
            ]
        ),
        [row(
            10,
            20,
            &[
                "aw_4k",
                "awaddr 0xffc, 2 beats of 4 bytes: crosses a 4KB boundary",
                "error"
            ]
        )]
    );
}

/// `AxSIZE` may not be wider than the data bus (A3.1.1): 8 bytes on a 32-bit `wdata` fires, on a
/// 32-bit `rdata` too, and 4 bytes does not.
#[test]
fn axi4_request_size_rule() {
    for chan in ["aw", "ar"] {
        assert_eq!(
            request_errors(
                chan,
                "axi4-size-rule",
                &[(0x300, 0, 3, 1, 0), (0x300, 0, 2, 1, 0)]
            ),
            [row(
                10,
                20,
                &[
                    &format!("{chan}_size"),
                    &format!("{chan}size 3 wider than the data bus (4 bytes)"),
                    "error"
                ]
            )]
        );
    }
}

/// An exclusive access (AxLOCK 1) is a power of two of bytes, up to 128, aligned to it, of at
/// most 16 beats (A6.3.3): 12 bytes, 8 bytes at 0x304 and 32 beats fire; 8 bytes at 0x300, and
/// the same request unexclusive, do not.
#[test]
fn axi4_request_exclusive_rule() {
    for chan in ["aw", "ar"] {
        assert_eq!(
            request_errors(
                chan,
                "axi4-exclusive-rule",
                &[
                    (0x300, 2, 2, 1, 1),
                    (0x304, 1, 2, 1, 1),
                    (0x300, 31, 2, 1, 1),
                    (0x300, 1, 2, 1, 0),
                    (0x300, 1, 2, 1, 1),
                ],
            ),
            [
                row(
                    10,
                    20,
                    &[
                        &format!("{chan}_exclusive"),
                        &format!("{chan} exclusive: 12 bytes is not a power of two up to 128"),
                        "error"
                    ]
                ),
                row(
                    20,
                    30,
                    &[
                        &format!("{chan}_exclusive"),
                        &format!("{chan} exclusive: address not aligned to 8 bytes"),
                        "error"
                    ]
                ),
                row(
                    30,
                    40,
                    &[
                        &format!("{chan}_exclusive"),
                        &format!("{chan} exclusive: 32 beats, at most 16"),
                        "error"
                    ]
                ),
            ]
        );
    }
}

/// A port that gives only `awaddr` fires no legality rule: phase 2's defaults (`INCR`, 4 bytes on
/// a 32-bit `wdata`, lock 0) are all legal.
#[test]
fn axi4_request_rules_fire_none_on_defaults() {
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let rules = [
        "aw_burst",
        "aw_wrap",
        "aw_fixed",
        "aw_4k",
        "aw_size",
        "aw_exclusive",
        "ar_burst",
        "ar_wrap",
        "ar_fixed",
        "ar_4k",
        "ar_size",
        "ar_exclusive",
    ];
    // the checks saw the requests: two write and two read handshakes
    assert_eq!(table(stream(&ws, "m.aw")).len(), 2);
    assert_eq!(table(stream(&ws, "m.ar")).len(), 2);
    let errors = table(stream(&ws, "m.errors"));
    assert!(
        errors.iter().all(|r| !rules.contains(&r.2[0].as_str())),
        "{errors:?}"
    );
}

/// An `awlen` of x at a handshake says nothing: `aw_payload_unknown`, and no legality rule.
#[test]
fn axi4_request_rules_skip_unknown_fields() {
    let dir = Dir::new("axi4-request-unknown");
    let path = dir.0.join("unknown.vcd");
    let text = std::fs::read_to_string(fixture("vcd").with_file_name("axi4.vcd")).unwrap();
    // write 1's AW drives awlen 1 at 12ns: make it x
    let text = text.replacen("b1 %\nb1 &\n#15", "b1 %\nbxxxxxxx1 &\n#15", 1);
    assert!(text.contains("bxxxxxxx1 &"));
    std::fs::write(&path, text).unwrap();
    let mut ws = Workspace::open(&path).unwrap();
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")]))
        .unwrap();
    let errors = table(stream(&ws, "m.errors"));
    let legality = [
        "aw_burst",
        "aw_wrap",
        "aw_fixed",
        "aw_4k",
        "aw_size",
        "aw_exclusive",
    ];
    // phase 1's rule says the signal is unknown; no legality rule reads a meaning into it
    assert!(
        errors.iter().any(|r| r.2[0] == "aw_payload_unknown"),
        "{errors:?}"
    );
    assert!(
        errors.iter().all(|r| !legality.contains(&r.2[0].as_str())),
        "{errors:?}"
    );
}
