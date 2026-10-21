//! Queries (spec §15): matches as runs of clock cycles, limits, sampling, changes.

mod common;
use common::*;
use wtf_core::query::{self, Run, Search};

fn search(from: u64, forward: bool) -> Search {
    Search { from, to: None, forward, merge: true, runs: usize::MAX, matches: u64::MAX }
}

fn runs(found: &query::Found) -> Vec<(u64, u64, u64)> {
    found.runs.iter().map(|r| (r.first, r.last, r.cycles)).collect()
}

#[test]
fn runs_of_cycles() {
    let mut ws = open();
    // clk rises at 5, 15 ... 75; busy is high 20ns - 40ns
    let q = ws.query("@clk !busy").unwrap();
    let f = query::find(&ws.trace, &q, &search(0, true));
    assert_eq!((runs(&f), f.total, f.more), (vec![(5, 5, 1), (35, 65, 4)], 5, false));
    // backward: the runs come latest first, `first` the later end
    let f = query::find(&ws.trace, &q, &search(81, false));
    assert_eq!(runs(&f), [(65, 35, 4), (5, 5, 1)]);
    assert_eq!(f.runs[0].span(), (35, 65));
    // without merging, each cycle
    let f = query::find(&ws.trace, &q, &Search { merge: false, ..search(0, true) });
    assert_eq!(f.runs.iter().map(|r| r.first).collect::<Vec<_>>(), [5, 35, 45, 55, 65]);
    // without a clock, nothing to merge: where it becomes true
    let q = ws.query("busy").unwrap();
    assert_eq!(runs(&query::find(&ws.trace, &q, &search(0, true))), [(20, 20, 1)]);
}

#[test]
fn limits() {
    let mut ws = open();
    let q = ws.query("@clk !busy").unwrap();
    let f = query::find(&ws.trace, &q, &Search { runs: 1, ..search(0, true) });
    assert_eq!((runs(&f), f.more), (vec![(5, 5, 1)], true));
    // the last run still takes its cycles before the limit stops the search
    let f = query::find(&ws.trace, &q, &Search { runs: 2, ..search(0, true) });
    assert_eq!((runs(&f), f.more), (vec![(5, 5, 1), (35, 65, 4)], false));
    let f = query::find(&ws.trace, &q, &Search { matches: 3, ..search(0, true) });
    assert_eq!((f.total, f.more), (3, true));
    // a window of time
    let f = query::find(&ws.trace, &q, &Search { to: Some(50), ..search(30, true) });
    assert_eq!(runs(&f), [(35, 45, 2)]);
    let f = query::find(&ws.trace, &q, &Search { to: Some(50), ..search(70, false) });
    assert_eq!(runs(&f), [(65, 55, 2)]);
    let f = query::find(&ws.trace, &q, &Search { to: Some(4), ..search(0, true) });
    assert!(f.runs.is_empty() && !f.more);
}

#[test]
fn samples_where_the_clock_does() {
    let mut ws = open();
    // a cycle from 15ns ends at the edge at 25ns, which samples what the signals hold just before
    let q = ws.query("@clk busy").unwrap();
    assert_eq!(query::sampled(&ws.trace, &q, 15), 24);
    // no edge after the last cycle start: the start itself
    assert_eq!(query::sampled(&ws.trace, &q, 75), 75);
    let q = ws.query("busy").unwrap();
    assert_eq!(query::sampled(&ws.trace, &q, 20), 20);
}

#[test]
fn changes_within_a_budget() {
    let mut ws = open();
    let data = (var(&ws, "top.cpu.data"), None);
    let clk = var(&ws, "top.clk");
    let mut budget = 100;
    assert_eq!(query::changes(&mut ws.trace, data, 15, 45, &mut budget), (vec![20, 30, 40, 41, 42, 43, 44], false));
    assert_eq!(budget, 93);
    // the budget is shared, and ends a list early
    let mut budget = 3;
    assert_eq!(query::changes(&mut ws.trace, data, 0, 80, &mut budget), (vec![10, 20, 30], true));
    assert_eq!(query::changes(&mut ws.trace, (clk, None), 0, 80, &mut budget), (vec![], true));
    // one bit of the bus: X, then 0x43 (0), 0x96 (1), 0x3A (0), z, 0x40 (0)
    let mut budget = 100;
    assert_eq!(query::changes(&mut ws.trace, (data.0, Some(7)), 0, 80, &mut budget).0, [10, 20, 40, 50, 60]);
}

#[test]
fn changes_loads_the_signal() {
    // a freshly opened trace has loaded no values: changes loads what it is asked about
    let mut ws = open();
    let data = (var(&ws, "top.cpu.data"), None);
    let mut budget = 100;
    assert_eq!(query::changes(&mut ws.trace, data, 15, 45, &mut budget), (vec![20, 30, 40, 41, 42, 43, 44], false));
}

#[test]
fn run_span() {
    assert_eq!(Run { first: 9, last: 3, cycles: 2 }.span(), (3, 9));
}

#[test]
fn across_time() {
    // handshake.vcd: clk rises at 5, 15 ...; valid / ready / data change at rising edges.
    // A transfer 15 - 45 (ready from 35); valid 65 - 85 dropped without ready, data 0x22 -> 0x23 at
    // 75 while stalled; a transfer 105 - 115.
    let mut ws = wtf_core::workspace::Workspace::open(&fixture("vcd").with_file_name("handshake.vcd")).unwrap();
    let mut at = |expr: &str, forward: bool| {
        let q = ws.query(expr).unwrap();
        let from = if forward { 0 } else { 200 };
        let f = query::find(&ws.trace, &q, &Search { merge: false, ..search(from, forward) });
        f.runs.iter().map(|r| r.first).collect::<Vec<_>>()
    };
    // the AXI rule: valid only falls after a handshake
    assert_eq!(at("@clk fell(valid) && !past(ready)", true), [85]);
    assert_eq!(at("@clk fell(valid) && !past(ready)", false), [85]);
    // the payload stays while stalled
    assert_eq!(at("@clk past(valid) && !past(ready) && !stable(data)", true), [75]);
    assert_eq!(at("@clk rose(valid)", true), [15, 65, 105]);
    assert_eq!(at("@clk fell(valid)", true), [45, 85, 115]);
    assert_eq!(at("@clk past(past(data)) == 0x22 && data == 0x23", true), [85]);
    assert_eq!(at("@clk past(data) == 0x22", true), [75]);
    assert_eq!(at("@clk past(past(data)) == 0x22", true), [85]);
    // without a clock: one tick earlier, so rose / fell are the edges
    assert_eq!(at("rose(valid)", true), [15, 65, 105]);
    assert_eq!(at("fell(valid)", true), [45, 85, 115]);
    assert_eq!(at("fell(valid)", false), [115, 85, 45]);
    assert_eq!(at("!stable(data)", true), [15, 65, 75, 105]);
    // past(x) changes a tick after x: !past(valid) becomes true a tick after valid falls
    assert_eq!(at("!past(valid) && !valid", true), [46, 86, 116]);
    // unfinished calls
    assert!(ws.query("rose(").is_err());
    assert_eq!(ws.query("@clk fell(valid").err().unwrap(), "missing )");
}

/// The clocked search works over a table of samples; it finds what searching edge by edge
/// (`search::find_query`, still the window's `/` without a clock) finds, either way, from any time.
#[test]
fn table_matches_edge_by_edge() {
    let traces: [(&str, &[&str]); 2] = [
        ("axilite.vcd", &[
            "@aclk s_axi_rvalid && s_axi_rready",
            "@aclk fell(s_axi_awvalid) && !past(s_axi_awready)",
            "@aclk s_axi_arvalid && !stable(s_axi_araddr)",
            "@aclk past(past(s_axi_bvalid)) || s_axi_rdata == 0x3",
            "@aclk !s_axi_awvalid",
        ]),
        ("axi4.vcd", &["@aclk m_axi_rvalid && m_axi_rid == 4", "@aclk rose(m_axi_wlast)"]),
    ];
    for (file, exprs) in traces {
        let mut ws = wtf_core::workspace::Workspace::open(&fixture(file).with_file_name(file)).unwrap();
        let end = ws.trace.end();
        for e in exprs {
            let q = ws.query(e).unwrap();
            for forward in [true, false] {
                for from in [0, 37, 132, 200, end + 1] {
                    // edge by edge, as before
                    let mut want = Vec::new();
                    let mut t = from;
                    while let Some(x) = wtf_core::search::find_query(&ws.trace, &q, t, forward) {
                        want.push(x);
                        t = x;
                    }
                    let s = Search { from, to: None, forward, merge: false, runs: usize::MAX, matches: u64::MAX };
                    let got: Vec<u64> = query::find(&ws.trace, &q, &s).runs.iter().map(|r| r.first).collect();
                    assert_eq!(got, want, "{file} {e} from {from} forward {forward}");
                    // and `to` ends it there, either way
                    let to = if forward { 150 } else { 60 };
                    let bounded: Vec<u64> = query::find(&ws.trace, &q, &Search { to: Some(to), ..s }).runs.iter().map(|r| r.first).collect();
                    let within: Vec<u64> = want.iter().copied().filter(|&x| if forward { x <= to } else { x >= to }).collect();
                    assert_eq!(bounded, within, "{file} {e} from {from} to {to} forward {forward}");
                }
            }
        }
    }
}
