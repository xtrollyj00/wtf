//! Times `wtf --mcp`'s tools on a trace written by `gen`: an agent's session played over stdin /
//! stdout, as `tests/mcp.rs` speaks to the server. Each call's wall time and the size of its
//! answer, which is what an agent pays for in tokens.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

pub fn main(args: &[String]) -> Result<(), String> {
    let (opts, trace) = super::options(args, &["wtf"])?;
    let wtf = super::run::wtf_binary(opts)?;
    let trace = std::fs::canonicalize(&trace).map_err(|e| format!("{trace}: {e}"))?;
    // a runtime directory of its own, so the user's `sessions` don't list this server
    let runtime = std::env::temp_dir().join(format!("wtf-bench-mcp-{}", std::process::id()));
    std::fs::create_dir_all(&runtime).map_err(|e| e.to_string())?;
    let mut s = Server::start(&wtf, &runtime)?;
    println!("{}, wtf --mcp", trace.display());
    println!("{:<56}{:>10}{:>10}", "call", "time", "answer");

    let txn = "@top.clk top.u0.s_axi_awvalid && top.u0.s_axi_awready";
    let calls: Vec<(String, &str, Value)> = vec![
        ("open".into(), "open", json!({"trace": trace})),
        ("info".into(), "info", json!({})),
        ("signals top.u0.*".into(), "signals", json!({"pattern": "top.u0.*"})),
        ("find @clk awvalid && awready, count_only".into(), "find", json!({"expr": txn, "count_only": true})),
        ("find @clk awvalid && awready, show addr data".into(), "find", json!({"expr": txn, "show": ["top.u0.s_axi_awaddr", "top.u0.s_axi_wdata"]})),
        ("values hot, awaddr over 1 us".into(), "values", json!({"signals": ["top.hot", "top.u0.s_axi_awaddr"], "from": "100us", "to": "101us"})),
        ("render 4 signals over 200 ns".into(), "render", json!({"signals": ["top.clk", "top.u0.s_axi_awvalid", "top.u0.s_axi_awready", "top.u0.s_axi_awaddr"], "from": "100us", "to": "100.2us"})),
        ("command decode bus axilite (top.u0)".into(), "command", json!({"line": "decode bus axilite clock=top.clk prefix=top.u0.s_axi_"})),
        ("records bus.rd where resp != 0".into(), "records", json!({"stream": "bus.rd", "where": "resp != 0"})),
        ("records bus.rd group_by resp".into(), "records", json!({"stream": "bus.rd", "group_by": ["resp"]})),
        ("records bus.wr count_only".into(), "records", json!({"stream": "bus.wr", "count_only": true})),
    ];
    for (what, tool, a) in calls {
        let start = Instant::now();
        let r = s.tool(tool, a)?;
        let d = start.elapsed();
        let (size, note) = match &r {
            Ok(text) => (text.len(), String::new()),
            Err(e) => (e.len(), format!("  error: {}", e.lines().next().unwrap_or(""))),
        };
        println!("{what:<56}{:>10.3?}{:>8} B{note}", d, size);
    }
    let peak = std::fs::read_to_string(format!("/proc/{}/status", s.child.id())).ok().and_then(|st| {
        // shortcut: Linux only (/proc), as `run`'s memory.
        st.lines().find_map(|l| l.strip_prefix("VmHWM:")).and_then(|v| v.trim().strip_suffix("kB")?.trim().parse::<u64>().ok())
    });
    println!("{:<56}{:>10}", "server memory (peak)", peak.map_or("n/a".into(), |kb| format!("{} MB", kb >> 10)));
    drop(s);
    std::fs::remove_dir_all(&runtime).ok();
    Ok(())
}

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Server {
    fn start(wtf: &PathBuf, runtime: &PathBuf) -> Result<Self, String> {
        let mut child = Command::new(wtf)
            .arg("--mcp")
            .env("XDG_RUNTIME_DIR", runtime)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{}: {e}", wtf.display()))?;
        let (stdin, stdout) = (child.stdin.take(), BufReader::new(child.stdout.take().unwrap()));
        let mut s = Server { child, stdin, stdout, id: 0 };
        s.call("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "wtf-bench", "version": "0"}}))?;
        s.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        Ok(s)
    }

    fn send(&mut self, msg: &Value) -> Result<(), String> {
        writeln!(self.stdin.as_ref().unwrap(), "{msg}").map_err(|e| format!("wtf --mcp: {e}"))
    }

    /// One request; its `result`.
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params}))?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("wtf --mcp closed".into());
            }
            let msg: Value = serde_json::from_str(&line).map_err(|e| format!("wtf --mcp: {e}: {line}"))?;
            if msg["id"] == self.id {
                return Ok(msg["result"].clone());
            }
        }
    }

    /// A tool's text answer, or its error; Err outside for the server failing.
    fn tool(&mut self, name: &str, args: Value) -> Result<Result<String, String>, String> {
        let r = self.call("tools/call", json!({"name": name, "arguments": args}))?;
        let text = r["content"][0]["text"].as_str().ok_or(format!("{name}: no text in {r}"))?.to_string();
        Ok(if r["isError"] == true { Err(text) } else { Ok(text) })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}
