//! Streams (spec §16): the stateless sampler, and records by range and limit.

mod common;
use common::*;
use wtf_core::stream::{Field, Stream};
use wtf_core::trace::Val;
use wtf_core::workspace::Workspace;

/// handshake.vcd: clk rises at 5, 15 ...; valid / ready / data change at rising edges. Valid 15 - 45
/// with ready from 35 (data 0x11), valid 65 - 85 without ready (data 0x22, 0x23 from 75), valid and
/// ready 105 - 115 (data 0x33).
fn handshake() -> Workspace {
    Workspace::open(&fixture("vcd").with_file_name("handshake.vcd")).unwrap()
}

fn starts(s: &Stream) -> Vec<u64> {
    s.records.iter().map(|r| r.start).collect()
}

fn bits(s: &str) -> Option<Val> {
    Some(Val::Bits(s.into()))
}

#[test]
fn samples_a_record_per_cycle() {
    let mut ws = handshake();
    // a beat: from the cycle's starting edge to the edge that samples it, fields sampled with it
    let beat = ws.sample("rx.beat", "clk", "valid && ready", &[("tdata", "data"), ("low", "data[3:0]"), ("rdy", "ready")]).unwrap();
    assert_eq!(beat.name, "rx.beat");
    let field = |name: &str, width| Field { name: name.into(), width: Some(width), time: false, signed: false };
    assert_eq!(beat.schema, [field("tdata", 8), field("low", 4), field("rdy", 1)]);
    assert_eq!(beat.records.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>(), [(35, 45), (105, 115)]);
    assert_eq!(beat.records[0].fields, [bits("00010001"), bits("0001"), bits("1")]);
    assert_eq!(beat.records[1].fields[0], bits("00110011"));
    // each cycle is a record, the values sampled at its end: 0x22 changes to 0x23 at 75
    let valid = ws.sample("v", "clk", "valid", &[("d", "data")]).unwrap();
    assert_eq!(starts(&valid), [15, 25, 35, 65, 75, 105]);
    assert_eq!((&valid.records[3].fields[0], &valid.records[4].fields[0]), (&bits("00100010"), &bits("00100011")));
    // the predicate can look back: the cycle after a valid that dropped without its ready
    let dropped = ws.sample("err", "clk", "fell(valid) && !past(ready)", &[]).unwrap();
    assert_eq!(starts(&dropped), [85]);
    assert_eq!(dropped.records[0].fields, []);
}

#[test]
fn records_by_range_and_limit() {
    let mut ws = handshake();
    let s = ws.sample("v", "clk", "valid", &[]).unwrap();
    let page = |from, to, limit| {
        let p = s.records(from, to, limit);
        (p.records.iter().map(|r| r.start).collect::<Vec<_>>(), p.total)
    };
    // by start, both ends included
    assert_eq!(page(0, 200, 100), (vec![15, 25, 35, 65, 75, 105], 6));
    assert_eq!(page(35, 75, 100), (vec![35, 65, 75], 3));
    assert_eq!(page(36, 74, 100), (vec![65], 1));
    // a limit cuts the page, not the total
    assert_eq!(page(30, 200, 2), (vec![35, 65], 4));
    assert_eq!(page(0, 200, 0), (vec![], 6));
    assert_eq!(page(100, 200, usize::MAX), (vec![105], 1));
    // nothing there, or an empty range
    assert_eq!(page(120, 200, 5), (vec![], 0));
    assert_eq!(page(80, 20, 5), (vec![], 0));
}

#[test]
fn sampling_errors() {
    let mut ws = handshake();
    let err = |ws: &mut Workspace, clock: &str, when: &str, fields: &[(&str, &str)]| ws.sample("rx", clock, when, fields).err().unwrap();
    assert_eq!(err(&mut ws, "clk", "valid", &[("x", "nope")]), "rx.x: unknown signal: nope");
    assert_eq!(err(&mut ws, "nope", "valid", &[]), "unknown signal: nope");
    assert_eq!(err(&mut ws, "clk", "valid &&", &[]), "expected a signal at: ");
    // the sampler itself needs a clock
    let q = ws.query("valid").unwrap();
    assert_eq!(wtf_core::stream::sample(&ws.trace, "rx", &q, &[]).err().unwrap(), "rx: a stream samples at a clock (@clk ...)");
}

fn sampler(name: &str, when: &str, fields: &[(&str, &str)]) -> wtf_core::stream::Sampler {
    let fields = fields.iter().map(|&(f, s)| (f.to_string(), s.to_string())).collect();
    wtf_core::stream::Sampler { name: name.into(), clock: "clk".into(), when: when.into(), fields, roles: vec![] }
}

#[test]
fn streams_are_signals() {
    let mut ws = handshake();
    let rx = ws.add_protocol(sampler("rx.beat", "valid && ready", &[("tdata", "data")])).unwrap().unwrap();
    let tdata = var(&ws, "rx.beat.tdata");
    assert_eq!((var(&ws, "rx.beat"), ws.trace.vars[tdata].width, ws.trace.vars[rx].width), (rx, Some(8), None));
    // the stream: its record's number while it lasts; a field: its value, z between records
    let text = |s: &str| Some(Val::Text(s.into()));
    assert_eq!([30, 40, 50, 110].map(|t| ws.trace.value_at(rx, t)), [text(""), text("#0"), text(""), text("#1")]);
    assert_eq!([40, 50, 110].map(|t| ws.trace.value_at(tdata, t)), [bits("00010001"), bits("zzzzzzzz"), bits("00110011")]);
    let mut budget = 99;
    assert_eq!(wtf_core::query::changes(&mut ws.trace, (tdata, None), 0, 200, &mut budget), (vec![35, 45, 105, 115], false));
    // rows and searches name them as any signal
    wtf_core::command::run(&mut ws, "add rx.beat rx.beat.tdata").unwrap();
    assert_eq!(names(&ws), ["beat", "tdata"]);
    ws.search("rx.beat.tdata == 0x33", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 105);
    ws.view.jump(0);
    ws.search("@clk tdata == 0x11 && rx.beat == #0", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 35);
    // records back to back: the number changes, an unchanged field doesn't
    let v = ws.add_protocol(sampler("v", "valid", &[("d", "data")])).unwrap().unwrap();
    let mut budget = 99;
    assert_eq!(wtf_core::query::changes(&mut ws.trace, (v, None), 0, 60, &mut budget).0, [15, 25, 35, 45]);
    let d = var(&ws, "v.d");
    assert_eq!(wtf_core::query::changes(&mut ws.trace, (d, None), 0, 60, &mut budget).0, [15, 45]);
    // sampled again by name: replaced in place
    assert_eq!(ws.add_protocol(sampler("v", "valid && ready", &[("d", "data")])), Ok(Some(v)));
    assert_eq!((ws.streams.len(), ws.protocols.len(), ws.streams[1].records.len()), (2, 2, 2));
    // :e samples them again; the rows stay
    assert_eq!(ws.reload(), Ok(("reloaded".into(), false)));
    assert_eq!(names(&ws), ["beat", "tdata"]);
    assert_eq!(ws.trace.value_at(var(&ws, "rx.beat.tdata"), 110), bits("00110011"));
}

#[test]
fn stream_errors() {
    let mut ws = handshake();
    assert_eq!(ws.add_protocol(sampler("a b", "valid", &[])), Err("not a protocol name: a b".into()));
    assert_eq!(ws.add_protocol(sampler("rx", "valid", &[("d", "nope")])), Err("rx.d: unknown signal: nope".into()));
    // a field can't take a signal of the file, and then nothing is added
    let n = ws.trace.vars.len();
    assert_eq!(ws.add_protocol(sampler("top", "valid", &[("clk", "data")])), Err("top.clk is a signal of the file".into()));
    assert_eq!((ws.trace.vars.len(), ws.streams.len()), (n, 0));
}

#[test]
fn sessions_keep_streams() {
    let mut ws = handshake();
    ws.add_protocol(sampler("rx", "valid && ready", &[("tdata", "data"), ("low", "data[3:0]")])).unwrap();
    wtf_core::command::run(&mut ws, "add rx.low rx").unwrap();
    // fields in their order, before the rows that show them
    let toml = wtf_core::session::to_toml(&ws);
    assert!(toml.contains("[[protocol]]\nname = \"rx\"\nclock = \"top.clk\"\nwhen = \"valid && ready\"\nfields = { tdata = \"data\", low = \"data[3:0]\" }\n\n[[rows]]"), "{toml}");
    let mut other = handshake();
    assert_eq!(wtf_core::session::apply(&mut other, &toml), Ok(vec![]));
    assert_eq!((names(&other), wtf_core::session::to_toml(&other)), (names(&ws), toml));
    assert_eq!(other.streams[0].schema.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["tdata", "low"]);
    // bad entries are warnings
    let bad = "[[protocol]]\nname = \"e\"\nclock = \"clk\"\nwhen = \"nope\"\n[[protocol]]\nname = \"f\"\n[[protocol]]\nname = \"g\"\nclock = \"clk\"\nwhen = \"valid\"\nfields = { \"a b\" = \"data\" }\n";
    assert_eq!(wtf_core::session::apply(&mut other, bad), Ok(vec![
        "protocol e: unknown signal: nope".to_string(),
        "protocol: needs a name, a clock (or roles), a when, fields as a table and roles as a list".to_string(),
        "protocol g: g: not a field name: a b".to_string(),
    ]));
}

fn decode(name: &str, protocol: &str, clock: &str, signals: &[(&str, &str)]) -> wtf_core::stream::Decode {
    let signals = signals.iter().map(|&(r, s)| (r.to_string(), s.to_string())).collect();
    wtf_core::stream::Decode { name: name.into(), protocol: protocol.into(), clock: clock.into(), signals, params: vec![] }
}

/// `cmd`: a user-defined protocol, its signals given by each instance.
fn cmd() -> wtf_core::stream::Sampler {
    let fields = vec![("op".into(), "cmd[3:0]".into()), ("arg".into(), "cmd[7:4]".into())];
    wtf_core::stream::Sampler { name: "cmd".into(), clock: String::new(), when: "valid && ready".into(), fields, roles: vec!["valid".into(), "ready".into(), "cmd".into()] }
}

#[test]
fn protocols_put_on_signals() {
    let mut ws = handshake();
    // a protocol with roles is a type: no stream until an instance puts it on signals
    assert_eq!(ws.add_protocol(cmd()), Ok(None));
    assert!(ws.streams.is_empty());
    let c0 = ws.add_decode(decode("c0", "cmd", "clk", &[("valid", "top.valid"), ("ready", "ready"), ("cmd", "data")])).unwrap();
    let c0s = &ws.streams[0];
    assert_eq!((var(&ws, "c0"), c0s.name.as_str(), c0s.schema.iter().map(|f| (f.name.as_str(), f.width)).collect::<Vec<_>>()), (c0, "c0", vec![("op", Some(4)), ("arg", Some(4))]));
    // data 0x11 in the transfer at 35, 0x33 at 105
    let rec: Vec<_> = c0s.records.iter().map(|r| (r.start, r.fields.clone())).collect();
    assert_eq!(rec, [(35, vec![bits("0001"), bits("0001")]), (105, vec![bits("0011"), bits("0011")])]);
    ws.search("c0.op == 3", true, 1).unwrap();
    assert_eq!(ws.view.cursor, 105);
    // another instance of a protocol whose clock it has: roles in `past` and functions too
    let mut drop = cmd();
    (drop.name, drop.clock, drop.when, drop.fields, drop.roles) = ("drop".into(), "clk".into(), "fell(v) && !past(r)".into(), vec![], vec!["v".into(), "r".into()]);
    ws.add_protocol(drop).unwrap();
    ws.add_decode(decode("err", "drop", "", &[("v", "valid"), ("r", "ready")])).unwrap();
    assert_eq!(starts(&ws.streams[1]), [85]);
    // roles played by signals of each other's names: `ready` is top.valid, 1 at 15ns where
    // top.ready is 0
    let mut both = cmd();
    (both.name, both.clock, both.when, both.fields, both.roles) = ("both".into(), "clk".into(), "valid || ready".into(), vec![("b".into(), "ready".into())], vec!["valid".into(), "ready".into()]);
    ws.add_protocol(both).unwrap();
    ws.add_decode(decode("sw", "both", "", &[("valid", "ready"), ("ready", "valid")])).unwrap();
    let sw = ws.streams.last().unwrap();
    assert_eq!((sw.records[0].start, &sw.records[0].fields[0]), (15, &bits("1")));
    // the roles are gone after it: `cmd` is no signal
    assert_eq!(ws.resolve("cmd"), Err("unknown signal: cmd".into()));
    // sessions keep protocols and instances; :e samples them again
    wtf_core::command::run(&mut ws, "add c0.op").unwrap();
    let toml = wtf_core::session::to_toml(&ws);
    assert!(toml.contains("[[protocol]]\nname = \"cmd\"\nroles = [\"valid\", \"ready\", \"cmd\"]\nwhen = \"valid && ready\"\nfields = { op = \"cmd[3:0]\", arg = \"cmd[7:4]\" }\n"), "{toml}");
    assert!(toml.contains("[[decode]]\nname = \"c0\"\nprotocol = \"cmd\"\nclock = \"top.clk\"\nsignals = { valid = \"top.valid\", ready = \"top.ready\", cmd = \"top.data\" }\n"), "{toml}");
    assert!(toml.contains("[[decode]]\nname = \"err\"\nprotocol = \"drop\"\nsignals = { v = \"top.valid\", r = \"top.ready\" }\n"), "{toml}");
    let mut other = handshake();
    assert_eq!(wtf_core::session::apply(&mut other, &toml), Ok(vec![]));
    assert_eq!((names(&other), wtf_core::session::to_toml(&other)), (names(&ws), toml));
    assert_eq!(ws.reload(), Ok(("reloaded".into(), false)));
    assert_eq!((ws.streams.len(), starts(&ws.streams[1])), (3, vec![85]));
    // a type defined again samples its instances again, at once
    let mut wider = cmd();
    wider.fields = vec![("all".into(), "cmd".into())];
    assert_eq!(ws.add_protocol(wider), Ok(None));
    assert_eq!(ws.streams[0].schema.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["all"]);
    let mut bad = cmd();
    bad.when = "nope".into();
    assert!(ws.add_protocol(bad).unwrap_err().starts_with("cmd defined, but instances failed: c0: "));
}

#[test]
fn instance_errors_at_once() {
    let mut ws = handshake();
    ws.add_protocol(cmd()).unwrap();
    assert_eq!(ws.add_decode(decode("x", "nope", "clk", &[])), Err("x: unknown protocol: nope (protocols: axis, axilite, axi4, spi, uart, frame, pair, check, cmd; `protocols <name>` describes one)".into()));
    let bad = decode("x", "cmd", "", &[("valid", "valid"), ("cmd", "nope"), ("last", "valid")]);
    assert_eq!(ws.add_decode(bad), Err("x: no signal for ready; cmd: unknown signal: nope; cmd has no role last; no clock (the protocol has none either)".into()));
    // a protocol without roles has nothing to put on signals; roles must be names
    ws.add_protocol(sampler("plain", "valid", &[])).unwrap();
    assert_eq!(ws.add_decode(decode("y", "plain", "", &[])), Err("y: protocol plain has no roles to put on signals".into()));
    let mut odd = cmd();
    odd.roles.push("a b".into());
    assert_eq!(ws.add_protocol(odd), Err("cmd: not a role name: a b".into()));
    assert_eq!((ws.decodes.len(), ws.streams.len()), (0, 1));
    // in a session they are warnings
    let warn = wtf_core::session::apply(&mut ws, "[[decode]]\nname = \"z\"\nprotocol = \"cmd\"\nclock = \"clk\"\nsignals = { valid = \"valid\" }\n[[decode]]\nname = \"w\"\n");
    assert_eq!(warn, Ok(vec!["decode z: no signal for ready, cmd".to_string(), "decode: needs a name, a protocol, signals and params as tables".to_string()]));
}
