//! Stateful decoders (spec §16): the registry, and frames built from a beat stream.

mod common;
use common::*;
use wtf_core::decode;
use wtf_core::stream::Stream;
use wtf_core::workspace::Workspace;

/// A beat per cycle valid is high in handshake.vcd: 15, 25, 35 (ready at 35), 65, 75, 105 (ready);
/// each to the next edge. d: data, 0x11 for the first three, then 0x22, 0x23, 0x33.
fn beats() -> (Workspace, Stream) {
    let mut ws = Workspace::open(&fixture("vcd").with_file_name("handshake.vcd")).unwrap();
    let s = ws.sample("v", "clk", "valid", &[("d", "data"), ("rdy", "ready")]).unwrap();
    (ws, s)
}

/// Frames by the parameters given: (start, end, then each field as a number).
fn frames(params: &[(&str, &str)]) -> Vec<(u64, u64, Vec<u64>)> {
    let (ws, s) = beats();
    let params: Vec<(String, String)> = params.iter().map(|&(k, v)| (k.into(), v.into())).collect();
    let d = decode::make("frame", &s.schema, &params, ws.trace.fs_per_tick).unwrap();
    let f = decode::run("v.frame", &s, d);
    assert_eq!(f.name, "v.frame");
    let num = |v: &Option<wtf_core::trace::Val>| match v {
        Some(wtf_core::trace::Val::Bits(b)) => u64::from_str_radix(b, 2).unwrap(),
        _ => panic!("{v:?}"),
    };
    f.records.iter().map(|r| (r.start, r.end, r.fields.iter().map(num).collect())).collect()
}

#[test]
fn frames_from_beats() {
    // by a last flag: beats, complete
    assert_eq!(frames(&[("last", "rdy")]), [(15, 45, vec![3, 1]), (65, 115, vec![3, 1])]);
    // by a gap: back to back, and the frame the trace cut short isn't complete
    assert_eq!(frames(&[("gap", "0")]), [(15, 45, vec![3, 1]), (65, 85, vec![2, 1]), (105, 115, vec![1, 0])]);
    // a gap of 20ns is allowed, so all of them are one
    assert_eq!(frames(&[("gap", "20ns")]), [(15, 115, vec![6, 0])]);
    assert_eq!(frames(&[("gap", "19ns")]), [(15, 45, vec![3, 1]), (65, 85, vec![2, 1]), (105, 115, vec![1, 0])]);
    // by select: a frame per run of one value, which is a field of the frame
    assert_eq!(frames(&[("select", "d")]), [(15, 45, vec![3, 0x11, 1]), (65, 75, vec![1, 0x22, 1]), (75, 85, vec![1, 0x23, 1]), (105, 115, vec![1, 0x33, 0])]);
    // together: any of them ends a frame
    assert_eq!(frames(&[("last", "rdy"), ("gap", "0")]), [(15, 45, vec![3, 1]), (65, 85, vec![2, 1]), (105, 115, vec![1, 1])]);
}

#[test]
fn frame_schema_and_errors() {
    let (ws, s) = beats();
    let p = |ps: &[(&str, &str)]| ps.iter().map(|&(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
    let make = |ps: &[(&str, &str)]| decode::make("frame", &s.schema, &p(ps), ws.trace.fs_per_tick);
    let names = |ps: &[(&str, &str)]| make(ps).ok().unwrap().schema().iter().map(|f| (f.name.clone(), f.width)).collect::<Vec<_>>();
    assert_eq!(names(&[("last", "rdy")]), [("beats".into(), Some(32)), ("complete".into(), Some(1))]);
    assert_eq!(names(&[("select", "d")]), [("beats".into(), Some(32)), ("d".into(), Some(8)), ("complete".into(), Some(1))]);
    let err = |ps: &[(&str, &str)]| make(ps).err().unwrap();
    assert_eq!(err(&[]), "frame: needs last, gap or select");
    assert_eq!(err(&[("last", "tlast"), ("gap", "-1"), ("gap", "soon"), ("size", "4")]), "frame: no field tlast (fields: d, rdy); frame: gap: bad time: -1; frame: gap: bad time: soon; frame: no parameter size (last, gap, select, key, bytes, data)");
    assert_eq!(decode::make("axi9", &s.schema, &[], None).err().unwrap(), "unknown decoder: axi9 (decoders: frame, spi, uart)");
    // an empty stream makes no frames
    let empty = Stream { name: "e".into(), schema: s.schema.clone(), records: vec![], violations: false };
    assert!(decode::run("f", &empty, make(&[("gap", "0")]).unwrap()).records.is_empty());
}

/// `join`: request k is the k-th record of each request stream; a response answers the oldest
/// complete request started before it.
#[test]
fn join_in_order() {
    use wtf_core::stream::{Field, Record};
    use wtf_core::trace::Val;
    let stream = |name: &str, starts: &[u64]| Stream {
        name: name.into(),
        schema: vec![Field { name: name.into(), width: Some(8), time: false }],
        records: starts.iter().map(|&t| Record { start: t, end: t + 5, fields: vec![Some(Val::Bits(format!("{t:08b}")))] }).collect(),
        violations: false,
    };
    // a at 10, 50, 70; b at 20, 60: requests complete at 20 and 60, a's third has no b
    let (a, b, resp) = (stream("a", &[10, 50, 70]), stream("b", &[20, 60]), stream("r", &[5, 30]));
    let (done, unmatched) = decode::join("t", &[("a", &a), ("b", &b)], &resp, None).unwrap();
    assert_eq!(done.schema.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["delay", "a", "b", "r"]);
    let starts: Vec<(u64, u64)> = done.records.iter().map(|r| (r.start, r.end)).collect();
    assert_eq!((starts, &done.records[0].fields[0]), (vec![(10, 35)], &Some(Val::Bits(format!("{:064b}", 10)))));
    let kinds: Vec<(u64, String)> = unmatched.records.iter().map(|r| (r.start, format!("{:?}", r.fields[0]))).collect();
    let text = |k: &str| format!("{:?}", Some(Val::Text(k.into())));
    assert_eq!(kinds, [(5, text("no request")), (50, text("no response")), (70, text("no b"))]);
    assert!(unmatched.violations && unmatched.name == "t.unmatched");
}
