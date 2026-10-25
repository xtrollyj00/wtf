//! Protocols written in Rust and the generic stateful decoders, put on a fixture's signals
//! (spec §16).

mod common;
use common::*;
use wtf_core::stream::{Decode, Sampler, Stream};
use wtf_core::trace::Val;
use wtf_core::workspace::Workspace;

fn open(name: &str) -> Workspace {
    Workspace::open(&fixture("vcd").with_file_name(name)).unwrap()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|&(a, b)| (a.into(), b.into())).collect()
}

fn decode(name: &str, protocol: &str, clock: &str, signals: &[(&str, &str)], params: &[(&str, &str)]) -> Decode {
    Decode { name: name.into(), protocol: protocol.into(), clock: clock.into(), signals: pairs(signals), params: pairs(params) }
}

const AXIS: &[(&str, &str)] = &[("tvalid", "tvalid"), ("tready", "tready"), ("tdata", "tdata"), ("tlast", "tlast"), ("tkeep", "tkeep")];

fn stream<'a>(ws: &'a Workspace, name: &str) -> &'a Stream {
    ws.streams.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("no stream {name}"))
}

/// Each record: start, end, and each field as a number, or its text.
fn table(s: &Stream) -> Vec<(u64, u64, Vec<String>)> {
    let show = |v: &Option<Val>| match v {
        Some(Val::Bits(b)) => u64::from_str_radix(b, 2).map_or(b.clone(), |n| n.to_string()),
        Some(Val::Text(t)) => t.clone(),
        None => "-".into(),
    };
    s.records.iter().map(|r| (r.start, r.end, r.fields.iter().map(show).collect())).collect()
}

fn row(start: u64, end: u64, fields: &[&str]) -> (u64, u64, Vec<String>) {
    (start, end, fields.iter().map(|f| f.to_string()).collect())
}

/// axis.vcd: clk rises at 5, 15 ...; cycle k is driven at 5 + 10k. Frame A: beats 0xA0, 0xA1 (after
/// a stall that holds it), 0xA2 (tkeep 0b0011), 10 bytes; B: 0xB0; C: 0xC0, 0xC1; then 0xD0 is
/// dropped before its ready (105ns), 0xE0 changes to 0xE1 while stalled (125ns), and E: 0xE1.
#[test]
fn axis_beats_and_frames() {
    let mut ws = open("axis.vcd");
    let rx = ws.add_decode(decode("rx", "axis", "clk", AXIS, &[])).unwrap();
    assert_eq!(rx, var(&ws, "rx.beat"));
    let beat = stream(&ws, "rx.beat");
    assert_eq!(beat.schema.iter().map(|f| (f.name.as_str(), f.width)).collect::<Vec<_>>(), [("tdata", Some(32)), ("tlast", Some(1)), ("tkeep", Some(4))]);
    let data: Vec<(u64, String)> = table(beat).into_iter().map(|(s, _, f)| (s, format!("{:X}", f[0].parse::<u64>().unwrap()))).collect();
    let want = [(15, "A0"), (35, "A1"), (45, "A2"), (55, "B0"), (65, "C0"), (75, "C1"), (135, "E1")];
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
    ws.add_decode(decode("a", "axis", "clk", &AXIS[..4], &[])).unwrap();
    assert_eq!(table(stream(&ws, "a.frame"))[0], row(15, 55, &["3", "12", "a0 00 00 00 a1 00 00 00 a2 00 00 00", "1"]));
    ws.add_decode(decode("b", "axis", "clk", &AXIS[..3], &[])).unwrap();
    assert!(ws.streams.iter().all(|s| s.name != "b.frame"));
    assert_eq!(stream(&ws, "b.beat").schema.len(), 1);
}

#[test]
fn axis_errors_are_the_injected_ones() {
    let mut ws = open("axis.vcd");
    ws.add_decode(decode("rx", "axis", "clk", AXIS, &[])).unwrap();
    assert_eq!(table(stream(&ws, "rx.errors")), [row(105, 115, &["dropped"]), row(125, 135, &["changed"])]);
    // without tkeep / tlast the payload is what there is
    ws.add_decode(decode("b", "axis", "clk", &AXIS[..3], &[])).unwrap();
    assert_eq!(table(stream(&ws, "b.errors")).len(), 2);
    ws.search("rx.errors.rule == changed", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 125);
}

#[test]
fn generic_check() {
    let mut ws = open("axis.vcd");
    // rules in order at one time; each a record per cycle it holds
    let rules = pairs(&[("stall", "tvalid && !tready"), ("short", "tvalid && tlast && tkeep != 0xF")]);
    let s = ws.check("chk", "clk", &rules).unwrap();
    assert_eq!(table(&s), [row(25, 35, &["stall"]), row(45, 55, &["short"]), row(95, 105, &["stall"]), row(115, 125, &["stall"]), row(125, 135, &["stall"])]);
    assert_eq!(ws.check("chk", "clk", &pairs(&[("bad", "nope")])).err().unwrap(), "bad: unknown signal: nope");
    // a signal compares with a signal: `x != past(x)` is `!stable(x)`
    let mut by = |rule: &str| table(&ws.check("chk", "clk", &pairs(&[("changed", rule)])).unwrap());
    let stalled = "past(tvalid) && !past(tready)";
    assert_eq!(by(&format!("{stalled} && tdata != past(tdata)")), by(&format!("{stalled} && !stable(tdata)")));
    assert!(!by(&format!("{stalled} && tdata != past(tdata)")).is_empty());
    assert!(by("tvalid && tdata == tdata").len() == by("tvalid").len());
    assert_eq!(ws.check("chk", "clk", &pairs(&[("bad", "tdata == 0xZQ")])).err().unwrap(), "bad: tdata: not a value: 0xZQ");
    // as an instance, its params the rules
    let short = [("short", "tvalid && tlast && tkeep != 0xF")];
    ws.add_decode(decode("c", "check", "clk", &[], &short)).unwrap();
    assert_eq!(table(stream(&ws, "c")), [row(45, 55, &["short"])]);
    assert_eq!(ws.add_decode(decode("c", "check", "", &[("x", "tvalid")], &[])).err().unwrap(), "c: no clock; check: no rules (params = { rule = \"expression\" }); check takes no signals");
    assert!(wtf_core::session::to_toml(&ws).contains("[[decode]]\nname = \"c\"\nprotocol = \"check\"\nclock = \"top.clk\"\nparams = { short = \"tvalid && tlast && tkeep != 0xF\" }\n"));
}

#[test]
fn builtin_instance_errors() {
    let mut ws = open("axis.vcd");
    let err = |ws: &mut Workspace, d| ws.add_decode(d).err().unwrap();
    assert_eq!(err(&mut ws, decode("rx", "axis", "", &[("tvalid", "tvalid"), ("tuser", "tdata"), ("tdata", "nope")], &[("x", "1")])),
        "rx: no signal for tready; axis has no role tuser; tdata: unknown signal: nope; axis has no param x (params: prefix); no clock");
    // its names are taken
    let p = Sampler { name: "axis".into(), clock: "clk".into(), when: "tvalid".into(), fields: vec![], roles: vec![] };
    assert_eq!(ws.add_protocol(p), Err("axis is a built-in protocol".into()));
    assert!(ws.streams.is_empty() && ws.decodes.is_empty());
}

/// reqresp.vcd: requests (req_valid, req_id) 1 at 15ns, 2 at 25, 3 at 45; responses (resp_valid,
/// resp_id) 2 at 35, 1 at 55, 0 at 85.
fn reqresp() -> Workspace {
    let mut ws = open("reqresp.vcd");
    for (name, v, id) in [("req", "req_valid", "req_id"), ("resp", "resp_valid", "resp_id")] {
        let fields = pairs(&[("id", id)]);
        ws.add_protocol(Sampler { name: name.into(), clock: "clk".into(), when: v.into(), fields, roles: vec![] }).unwrap();
    }
    ws
}

#[test]
fn pairs_by_id_and_by_order() {
    let mut ws = reqresp();
    let io = [("request", "req"), ("response", "resp")];
    // by id: delay, req_id, resp_id; the unmatched are violations
    ws.add_decode(decode("p", "pair", "", &io, &[("id", "id")])).unwrap();
    assert_eq!(table(stream(&ws, "p")), [row(15, 65, &["40", "1", "1"]), row(25, 45, &["10", "2", "2"])]);
    assert_eq!(table(stream(&ws, "p.unmatched")), [row(45, 55, &["no response"]), row(85, 95, &["no request"])]);
    assert_eq!(stream(&ws, "p").schema.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["delay", "req_id", "resp_id"]);
    // by order: the oldest request waiting
    ws.add_decode(decode("o", "pair", "", &io, &[])).unwrap();
    assert_eq!(table(stream(&ws, "o")), [row(15, 45, &["20", "1", "2"]), row(25, 65, &["30", "2", "1"]), row(45, 95, &["40", "3", "0"])]);
    assert!(stream(&ws, "o.unmatched").records.is_empty());
    // as signals: overlapping pairs show the latest started one still open
    let p = var(&ws, "p");
    assert_eq!([20, 30, 50, 70].map(|t| ws.trace.value_at(p, t)), ["#0", "#1", "#0", ""].map(|v| Some(Val::Text(v.into()))));
    // and kept by sessions and :e
    ws.search("p.delay == 10ns", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 25);
    let toml = wtf_core::session::to_toml(&ws);
    assert!(toml.contains("[[decode]]\nname = \"p\"\nprotocol = \"pair\"\nsignals = { request = \"req\", response = \"resp\" }\nparams = { id = \"id\" }\n"), "{toml}");
    let mut other = open("reqresp.vcd");
    assert_eq!(wtf_core::session::apply(&mut other, &toml), Ok(vec![]));
    assert_eq!(table(stream(&other, "p.unmatched")).len(), 2);
    assert_eq!(ws.reload(), Ok(("reloaded".into(), false)));
    assert_eq!(table(stream(&ws, "o")).len(), 3);
}

#[test]
fn chained_frame_and_their_errors() {
    let mut ws = reqresp();
    // frame on a stream from a session protocol: requests back to back
    ws.add_decode(decode("burst", "frame", "", &[("input", "req")], &[("gap", "0")])).unwrap();
    assert_eq!(table(stream(&ws, "burst")), [row(15, 35, &["2", "1"]), row(45, 55, &["1", "0"])]);
    let err = |ws: &mut Workspace, d| ws.add_decode(d).err().unwrap();
    assert_eq!(err(&mut ws, decode("f", "frame", "", &[("in", "req")], &[("gap", "0")])), "f: no stream for input; frame has no role in");
    assert_eq!(err(&mut ws, decode("f", "frame", "", &[("input", "nope")], &[])), "f: input: unknown stream: nope");
    assert_eq!(err(&mut ws, decode("f", "frame", "", &[("input", "req")], &[("last", "x")])), "f: frame: no field x (fields: id)");
    let io = [("request", "req"), ("response", "resp")];
    assert_eq!(err(&mut ws, decode("q", "pair", "", &io, &[("req_id", "id"), ("by", "x")])), "q: pair: no parameter by (id, req_id, resp_id); pair: req_id and resp_id go together");
    assert_eq!(err(&mut ws, decode("q", "pair", "", &io, &[("id", "nope")])), "q: pair: req has no field nope; pair: resp has no field nope");
}

/// A short name unique only thanks to the rows shown is kept by its full path, so the session,
/// which makes instances before rows, makes it again.
#[test]
fn instance_names_signals_in_full() {
    let mut ws = open("simple.vcd");
    let add = |ws: &mut Workspace, p: Sampler| ws.add_protocol(p).unwrap();
    add(&mut ws, Sampler { name: "busy".into(), clock: String::new(), when: "b".into(), fields: pairs(&[("s", "s")]), roles: vec!["b".into(), "s".into()] });
    let state = ws.var_named("top.cpu.state").unwrap();
    ws.add_rows(&[state]);
    assert!(ws.resolve("state").is_ok() && ws.resolve("dma.state").is_ok());
    ws.add_decode(decode("cpu", "busy", "clk", &[("b", "busy"), ("s", "state")], &[])).unwrap();
    assert_eq!(ws.decodes[0].signals, pairs(&[("b", "top.cpu.busy"), ("s", "top.cpu.state")]));
    assert_eq!(ws.decodes[0].clock, "top.clk");
    let text = wtf_core::session::to_toml(&ws);
    let mut back = open("simple.vcd");
    assert_eq!(wtf_core::session::apply(&mut back, &text).unwrap(), Vec::<String>::new(), "{text}");
    assert_eq!(table(stream(&back, "cpu")), table(stream(&ws, "cpu")));
}

/// AXI-Lite in axilite.vcd: two writes (the second's W before its AW), two reads (the second
/// SLVERR), an AW valid dropped before its ready, a B without a request, every valid X in reset,
/// then six polling reads from 260ns.
#[test]
fn axilite_transactions_and_errors() {
    let mut ws = open("axilite.vcd");
    // the prefix plays every role; kept by full path, as the clock
    ws.add_decode(decode("s", "axilite", "aclk", &[], &[("prefix", "s_axi_")])).unwrap();
    assert_eq!((ws.decodes[0].clock.as_str(), &ws.decodes[0].params), ("top.aclk", &pairs(&[("prefix", "top.s_axi_")])));
    let wr = stream(&ws, "s.wr");
    assert_eq!(wr.schema.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["delay", "addr", "data", "strb", "resp"]);
    let want = [row(30, 50, &["10", "16", "286331153", "15", "0"]), row(60, 120, &["20", "20", "572662306", "3", "0"])];
    assert_eq!(table(wr), want);
    let want = [row(130, 160, &["20", "16", "286331153", "0"]), row(170, 190, &["10", "32", "3735928559", "2"])];
    let rd = table(stream(&ws, "s.rd"));
    assert_eq!((&rd[..2], rd.len()), (&want[..], 8));
    // only the injected ones: X -> 0 in reset is no dropped valid
    assert_eq!(table(stream(&ws, "s.errors")), [row(180, 190, &["r_resp"]), row(210, 220, &["aw_dropped"])]);
    assert_eq!(table(stream(&ws, "s.wr.unmatched")), [row(230, 240, &["no request"])]);
    assert!(stream(&ws, "s.rd.unmatched").records.is_empty());
    // the session makes it again
    let text = wtf_core::session::to_toml(&ws);
    let mut back = open("axilite.vcd");
    assert_eq!(wtf_core::session::apply(&mut back, &text).unwrap(), Vec::<String>::new(), "{text}");
    assert_eq!(table(stream(&back, "s.wr")), table(stream(&ws, "s.wr")));
    // a role missing under the prefix is reported
    let err = ws.add_decode(decode("t", "axilite", "aclk", &[], &[("prefix", "m_axi_")])).unwrap_err();
    assert!(err.starts_with("t: awvalid: unknown signal: m_axi_awvalid"), "{err}");
}

/// AXI4 in axi4.vcd: write 1 (two beats to 0x100) and write 2 (one beat to 0x200, SLVERR), whose B
/// responses come back in the other order; reads 3 and 4 (two beats each), whose R beats
/// interleave; and an AR valid dropped before its ready.
#[test]
fn axi4_bursts_by_id() {
    let mut ws = open("axi4.vcd");
    ws.add_decode(decode("m", "axi4", "aclk", &[], &[("prefix", "m_axi_")])).unwrap();
    let names = |s: &str| stream(&ws, s).schema.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    assert_eq!(names("m.wr"), ["delay", "addr", "aw_id", "aw_len", "beats", "wburst_len", "data", "complete", "resp_id", "resp"]);
    // by ID: write 2's B comes first
    let wr = table(stream(&ws, "m.wr"));
    assert_eq!(wr.iter().map(|r| (r.0, r.1, r.2[0].as_str(), r.2[2].as_str(), r.2[8].as_str(), r.2[9].as_str())).collect::<Vec<_>>(), [
        (10, 70, "40", "1", "1", "0"),
        (30, 60, "10", "2", "2", "2"),
    ]);
    assert_eq!(wr[0].2[6], "a0 00 00 00 a1 00 00 00");
    assert_eq!(wr[1].2[5..7], ["2", "b0 00"], "wstrb 0b0011: two bytes");
    // interleaved R beats make one burst per ID; the latency is to the first beat
    assert_eq!(table(stream(&ws, "m.rburst")).iter().map(|r| (r.0, r.1, r.2[1].as_str())).collect::<Vec<_>>(), [(90, 120, "4"), (100, 130, "3")]);
    let rd = table(stream(&ws, "m.rd"));
    assert_eq!(rd.iter().map(|r| (r.0, r.1, r.2[0].as_str(), r.2[1].as_str(), r.2[2].as_str(), r.2[6].as_str())).collect::<Vec<_>>(), [
        (70, 130, "30", "768", "3", "30 0d 00 00 31 0d 00 00"),
        (80, 120, "10", "1024", "4", "40 0d 00 00 41 0d 00 00"),
    ]);
    assert_eq!(table(stream(&ws, "m.errors")), [row(50, 60, &["b_resp"]), row(140, 150, &["ar_dropped"])]);
    assert!(stream(&ws, "m.wr.unmatched").records.is_empty() && stream(&ws, "m.rd.unmatched").records.is_empty());
    // without IDs, in order: the B responses go to the wrong writes
    let mut plain = open("axi4.vcd");
    let roles = ["awvalid", "awready", "awaddr", "wvalid", "wready", "wdata", "wlast", "bvalid", "bready", "bresp", "arvalid", "arready", "araddr", "rvalid", "rready", "rdata", "rlast"];
    let signals: Vec<(&str, String)> = roles.iter().map(|r| (*r, format!("m_axi_{r}"))).collect();
    let signals: Vec<(&str, &str)> = signals.iter().map(|(r, s)| (*r, s.as_str())).collect();
    plain.add_decode(decode("m", "axi4", "aclk", &signals, &[])).unwrap();
    let wr = table(stream(&plain, "m.wr"));
    assert_eq!(wr.iter().map(|r| (r.2[1].as_str(), r.2.last().unwrap().as_str())).collect::<Vec<_>>(), [("256", "2"), ("512", "0")]);
}

/// SPI in spi.vcd (mode 0): a transfer of 0xA5 0x3C out and 0x5A 0xC3 in; a pulse of sclk with cs
/// high; then 12 bits, 0xF0 and 0b1010 out, whose second word cs cuts short.
#[test]
fn spi_words_and_transfers() {
    let pins = [("sclk", "spi_clk"), ("cs", "spi_cs_n"), ("mosi", "spi_mosi"), ("miso", "spi_miso")];
    let mut ws = open("spi.vcd");
    ws.add_decode(decode("f", "spi", "", &pins, &[])).unwrap();
    let bits = table(stream(&ws, "f.bit"));
    // sampled at sclk's rise, each to the next; none while cs is high
    assert_eq!((bits.len(), &bits[0], &bits[15]), (28, &row(110, 130, &["1", "0"]), &row(410, 430, &["0", "1"])));
    assert_eq!(table(stream(&ws, "f.word")), [
        row(110, 270, &["165", "90", "1"]),
        row(270, 430, &["60", "195", "1"]),
        row(510, 670, &["240", "15", "1"]),
        row(670, 750, &["10", "5", "0"]),
    ]);
    assert_eq!(table(stream(&ws, "f.transfer")), [row(100, 430, &["2", "a5 3c", "5a c3", "1"]), row(500, 750, &["2", "f0 a", "0f 5", "1"])]);
    // LSB first, and no miso
    let mut ws = open("spi.vcd");
    ws.add_decode(decode("f", "spi", "", &pins[..3], &[("order", "lsb")])).unwrap();
    assert_eq!(stream(&ws, "f.word").schema.len(), 2);
    assert_eq!(table(stream(&ws, "f.transfer"))[1], row(500, 750, &["2", "0f 5", "1"]));
    // mistakes, all at once
    let err = ws.add_decode(decode("g", "spi", "", &pins, &[("mode", "4"), ("bits", "0"), ("speed", "1")])).unwrap_err();
    assert_eq!(err, "g: spi has no param speed (params: prefix, mode, bits, order)");
    let err = ws.add_decode(decode("g", "spi", "", &pins, &[("mode", "4"), ("bits", "0")])).unwrap_err();
    assert_eq!(err, "g: spi: mode=4: not 0, 1, 2 or 3; bits=0: not 1 - 64");
}

/// UART in uart.vcd at 1 Mbaud, even parity: 0x55 and 0xA3; 0x0F with a wrong parity bit; 0x81 with
/// a stop bit of 0; and a 100ns glitch that isn't a start bit.
#[test]
fn uart_bytes_and_errors() {
    let mut ws = open("uart.vcd");
    ws.add_decode(decode("u", "uart", "", &[("line", "uart_tx")], &[("baud", "1000000"), ("parity", "even")])).unwrap();
    let want = [row(1000, 12000, &["85"]), row(13000, 24000, &["163"]), row(25000, 36000, &["15"]), row(37000, 48000, &["129"])];
    assert_eq!(table(stream(&ws, "u.byte")), want);
    let errors = stream(&ws, "u.errors");
    assert!(errors.violations);
    assert_eq!(table(errors), [row(25000, 36000, &["parity"]), row(37000, 48000, &["framing"])]);
    // without parity the parity bit is taken for the stop bit: 0 for 0x55, 0xA3 and 0x81
    let mut ws = open("uart.vcd");
    ws.add_decode(decode("u", "uart", "", &[("line", "uart_tx")], &[("baud", "1000000")])).unwrap();
    assert_eq!(table(stream(&ws, "u.errors")).iter().map(|r| r.0).collect::<Vec<_>>(), [1000, 13000, 37000]);
    let err = ws.add_decode(decode("v", "uart", "", &[("line", "uart_tx")], &[])).unwrap_err();
    assert_eq!(err, "v: uart: needs baud=<a positive number of bits a second>");
}

/// The interfaces built-in protocols decode, by their signals' names, with a decode line each.
#[test]
fn detects_interfaces() {
    let found = |f: &str| {
        let t = wtf_core::trace::Trace::open(format!("{}/../tests/data/{f}", env!("CARGO_MANIFEST_DIR")).as_ref()).unwrap();
        wtf_core::protocol::detect(&t).iter().map(|f| f.decode_line()).collect::<Vec<_>>()
    };
    assert_eq!(found("axilite.vcd"), ["decode s_axi axilite clock=top.aclk prefix=top.s_axi_"]);
    assert_eq!(found("axis.vcd"), ["decode axis axis clock=top.clk prefix=top."]);
    // AXI4 over AXI-Lite, whose roles it has too
    assert_eq!(found("axi4.vcd").iter().filter(|l| l.contains(" axilite ")).count(), 0);
    assert!(found("axi4.vcd").iter().any(|l| l.contains(" axi4 ")), "{:?}", found("axi4.vcd"));
    assert!(found("simple.vcd").is_empty());
}
