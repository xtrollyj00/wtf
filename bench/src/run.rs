//! Times wtf on a trace written by `gen`, each against its target in spec §18. The core is timed
//! in this process; the first screen by running `wtf --snapshot`, which also draws.

use crate::generate::PERIOD;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use wtf_core::{command, query, workspace::Workspace};

pub fn main(args: &[String]) -> Result<(), String> {
    let (opts, trace) = super::options(args, &["wtf"])?;
    let wtf = match opts.into_iter().next() {
        Some((_, p)) => PathBuf::from(p),
        None => std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("wtf"),
    };
    if !wtf.exists() {
        return Err(format!("{}: no wtf binary; `cargo build --release` first, or give --wtf <path>", wtf.display()));
    }
    let trace = PathBuf::from(trace);
    let fst = trace.extension().is_some_and(|e| e == "fst");
    let bytes = std::fs::metadata(&trace).map_err(|e| format!("{}: {e}", trace.display()))?.len();

    let (open, ws) = time(|| Workspace::open(&trace));
    let mut ws = ws?;
    let rss_open = rss();
    let end = ws.trace.end();
    println!("{}: {:.2} GiB, {} signals, {} cycles", trace.display(), bytes as f64 / (1u64 << 30) as f64, ws.trace.vars.len(), end / PERIOD + 1);
    row("open, in the core", open, if fst { "1 s, first screen" } else { "2 s, first screen" });

    let (add, r) = time(|| command::run(&mut ws, "add top.hot"));
    r?;
    row("add a signal (top.hot)", add, "1 s");

    // top.hot counts cycles: its last value is the furthest a search can go
    let last = end / PERIOD;
    ws.sel = 0;
    let (search, r) = time(|| ws.search(&format!("0x{:x}", last & 0xFFFF_FFFF), true, 1));
    r?;
    if ws.view.cursor != last * PERIOD {
        return Err(format!("value search ended at {}, not {}", ws.view.cursor, last * PERIOD));
    }
    row(&format!("value search, {} transitions", last + 1), search, "1 s for 10M");

    let (clocked, found) = time(|| -> Result<u64, String> {
        let q = ws.query("@top.clk top.u0.s_axi_awvalid && top.u0.s_axi_awready")?;
        let s = query::Search { from: 0, to: None, forward: true, merge: true, runs: usize::MAX, matches: u64::MAX };
        Ok(query::find(&ws.trace, &q, &s).total)
    });
    row(&format!("clocked search, {} matches", found?), clocked, "none yet");
    let rss_loaded = rss();

    let session = std::env::temp_dir().join(format!("wtf-bench-{}.session.toml", std::process::id()));
    let first = snapshot(&wtf, &trace, &session, "time = \"0ns\"\nzoom = \"1ns\"")?;
    row("first screen, wtf --snapshot", first, if fst { "1 s" } else { "2 s" });
    let whole = snapshot(&wtf, &trace, &session, &format!("time = \"0ns\"\nto = \"{end}ns\""))?;
    row("whole trace on screen, wtf --snapshot", whole, "the first screen + 50 ms");
    std::fs::remove_file(&session).ok();

    let mb = |b: Option<u64>| b.map_or("n/a".into(), |b| format!("{} MB", b >> 20));
    println!("{:<44}{:>10}    target: proportional to the hierarchy", "memory after open (peak)", mb(rss_open));
    println!("{:<44}{:>10}", "memory after the searches (peak)", mb(rss_loaded));
    Ok(())
}

fn time<T>(f: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let r = f();
    (start.elapsed(), r)
}

fn row(what: &str, d: Duration, target: &str) {
    println!("{what:<44}{:>10.3?}    target: {target}", d);
}

/// One screen of top.hot and u0's write address, with this `[view]`; the wall time of it.
fn snapshot(wtf: &Path, trace: &Path, session: &Path, view: &str) -> Result<Duration, String> {
    let rows: String = ["top.clk", "top.hot", "top.u0.s_axi_awvalid", "top.u0.s_axi_awready", "top.u0.s_axi_awaddr", "top.u0.state"]
        .iter()
        .map(|s| format!("[[rows]]\nsignal = \"{s}\"\n\n"))
        .collect();
    std::fs::write(session, format!("version = 1\n\n[view]\n{view}\n\n{rows}")).map_err(|e| e.to_string())?;
    let (d, out) = time(|| std::process::Command::new(wtf).arg("--snapshot").arg("200x40").arg("-S").arg(session).arg(trace).output());
    let out = out.map_err(|e| format!("{}: {e}", wtf.display()))?;
    if !out.status.success() {
        return Err(format!("wtf --snapshot failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(d)
}

/// Peak resident memory of this process.
fn rss() -> Option<u64> {
    // shortcut: Linux only (/proc); n/a elsewhere until someone benchmarks on macOS.
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = s.lines().find_map(|l| l.strip_prefix("VmHWM:"))?.trim().strip_suffix("kB")?.trim().parse::<u64>().ok()?;
    Some(kb << 10)
}
