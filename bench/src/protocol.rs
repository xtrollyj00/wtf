//! Rust and WASM AXIS: one generated AXI4-Stream port decoded three ways — the built-in
//! `axilite`, the built-in `axis`, and `axis-wasm`, the same decoder as a WebAssembly component
//! (spec §16) — each on a server of its own, against the 3x bar. `bench/RESULTS.md` records it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::mcp::Server;

/// The bar: a protocol loaded from a file decodes within 3x its built-in (spec §16).
const BAR: f64 = 3.0;

pub fn main(args: &[String]) -> Result<(), String> {
    let (opts, trace) = super::options(args, &["wtf", "wasm"])?;
    let wasm = match opts.iter().find(|(n, _)| n == "wasm") {
        Some((_, p)) => PathBuf::from(p),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/data/wasm/axis-wasm.wasm"),
    };
    // `--wasm` is ours, not the binary's: only `--wtf` goes on to it
    let wtf = super::run::wtf_binary(opts.into_iter().filter(|(n, _)| n == "wtf").collect())?;
    if !wasm.is_file() {
        return Err(format!(
            "{}: no wasm protocol; build `wit/axis` first (its README) or give --wasm <path>",
            wasm.display()
        ));
    }
    let trace = std::fs::canonicalize(&trace).map_err(|e| format!("{trace}: {e}"))?;

    // a config directory of its own: the one protocol the wasm arm loads, and nothing else
    let tmp = Dir::new("protocol")?;
    let protocols = tmp.0.join("wtf").join("protocols");
    std::fs::create_dir_all(&protocols).map_err(|e| format!("{}: {e}", protocols.display()))?;
    std::fs::copy(&wasm, protocols.join("axis-wasm.wasm"))
        .map_err(|e| format!("{}: {e}", wasm.display()))?;
    let runtime = tmp.0.join("run");
    std::fs::create_dir_all(&runtime).map_err(|e| format!("{}: {e}", runtime.display()))?;

    println!("{}, Rust and WASM AXIS", trace.display());
    let mut times: Vec<(&str, Duration)> = Vec::new();
    for (name, line, config) in [
        (
            "axilite",
            "decode bus axilite clock=top.clk prefix=top.u0.s_axi_",
            false,
        ),
        (
            "axis",
            "decode rx axis clock=top.clk prefix=top.u0.s_axis_",
            false,
        ),
        (
            "axis-wasm",
            "decode rx axis-wasm clock=top.clk prefix=top.u0.s_axis_",
            true,
        ),
    ] {
        let mut s = Server::start(&wtf, &runtime, config.then_some(tmp.0.as_path()))?;
        let (d, r) = time(|| {
            s.tool("open", json!({"trace": trace.clone()}))?
                .map_err(|e| format!("{name}: open: {e}"))?;
            s.tool("command", json!({"line": line}))?
                .map_err(|e| format!("{name}: {line}: {e}"))?;
            Ok::<(), String>(())
        });
        r?;
        println!("{:<44}{:>10.3?}", format!("{name} (open + {line})"), d);
        times.push((name, d));
    }
    let (rust, wasm) = (times[1].1.as_secs_f64(), times[2].1.as_secs_f64());
    println!(
        "{:<44}{:>10.2}x    target: within {BAR}x",
        "wasm / rust",
        wasm / rust
    );
    Ok(())
}

/// A directory of its own, removed when the run ends, however it ends.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("wtf-bench-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Dir(path))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn time<T>(f: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let r = f();
    (start.elapsed(), r)
}
