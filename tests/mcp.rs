//! `wtf --mcp`: the real binary, spoken to over stdin / stdout as an MCP client would. The user's
//! windows are played by a fake speaking the link's wire format (spec §15), and each test has its
//! own `XDG_RUNTIME_DIR`, so it sees only its own sessions.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Server {
    fn start(runtime: &Path) -> Self {
        let mut child = wtf(runtime).arg("--mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let (stdin, stdout) = (child.stdin.take(), BufReader::new(child.stdout.take().unwrap()));
        let mut s = Server { child, stdin, stdout, id: 0 };
        let init = s.call("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}));
        assert_eq!(init["serverInfo"]["name"], "wtf");
        writeln!(s.stdin.as_ref().unwrap(), r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
        s
    }

    /// One request; its `result`.
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let req = json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params});
        writeln!(self.stdin.as_ref().unwrap(), "{req}").unwrap();
        let mut line = String::new();
        loop {
            line.clear();
            assert!(self.stdout.read_line(&mut line).unwrap() > 0, "server closed");
            let msg: Value = serde_json::from_str(&line).unwrap();
            if msg["id"] == self.id {
                return msg["result"].clone();
            }
        }
    }

    /// A tool's text answer, or its error.
    fn tool(&mut self, name: &str, args: Value) -> Result<String, String> {
        let r = self.call("tools/call", json!({"name": name, "arguments": args}));
        let content = r["content"].as_array().unwrap();
        assert_eq!(content.len(), 1, "{r}");
        let text = content[0]["text"].as_str().unwrap().to_string();
        if r["isError"] == true { Err(text) } else { Ok(text) }
    }

    fn ok(&mut self, name: &str, args: Value) -> String {
        self.tool(name, args).unwrap_or_else(|e| panic!("{name}: {e}"))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn wtf(runtime: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_wtf"));
    c.env("XDG_RUNTIME_DIR", runtime);
    // the user's config must not change what the tests see (spec §19)
    c.env("XDG_CONFIG_HOME", runtime);
    // nor read or write the user's history (spec §4 "Start screen")
    c.env("XDG_STATE_HOME", runtime);
    c
}

/// A fresh directory of this test's own, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("wtf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn simple() -> PathBuf {
    std::fs::canonicalize("tests/data/simple.vcd").unwrap()
}

/// The user's wtf window `id` on `trace`: answers `GET` with `session`, and passes on what the
/// agent `PUT`s.
fn fake_window(runtime: &Path, id: &str, trace: &Path, session: &str) -> mpsc::Receiver<String> {
    let listener = UnixListener::bind(runtime.join(format!("wtf-{id}.sock"))).unwrap();
    // as a real window announces it: links resolved (live::window)
    let info = format!("window\n{}", std::fs::canonicalize(trace).unwrap().display());
    let get = format!("rows (> selected):\n> clk\n---\n{session}");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            r.read_line(&mut head).unwrap();
            let answer = match head.trim_end() {
                "INFO" => info.clone(),
                "GET" => get.clone(),
                put => {
                    let n: usize = put.strip_prefix("PUT ").unwrap().parse().unwrap();
                    let mut buf = vec![0; n];
                    r.read_exact(&mut buf).unwrap();
                    let _ = tx.send(String::from_utf8(buf).unwrap());
                    "agent: presented".into()
                }
            };
            write!(stream, "OK {}\n{answer}", answer.len()).unwrap();
        }
    });
    rx
}

#[test]
fn tools_list() {
    let dir = Dir::new("list");
    let mut s = Server::start(&dir.0);
    let list = s.call("tools/list", json!({}));
    let mut names: Vec<&str> = list["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["command", "find", "help", "info", "open", "present", "protocols", "records", "reload", "render", "sessions", "signals", "use", "user_view", "value_at", "values", "view", "workspace"]);
    // clients reject tools whose input schema isn't an object
    for t in list["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(!t["description"].as_str().unwrap().is_empty(), "{t}");
    }
    // the tool definitions ride along on every turn of an agent: they grow only on purpose (spec
    // §15); raise a bound in the same commit as the text, saying why
    let size = list.to_string().len();
    assert!(size <= 13_400, "the tool definitions are {size} bytes, past the 13400 allowed");
    for t in list["tools"].as_array().unwrap() {
        let d = t["description"].as_str().unwrap();
        assert!(d.chars().count() <= 330, "{}: a description of {} characters, past 330: move the rest to its topic", t["name"], d.chars().count());
        for (arg, p) in t["inputSchema"]["properties"].as_object().into_iter().flatten() {
            let d = p["description"].as_str().unwrap_or("");
            assert!(d.chars().count() <= 260, "{}.{arg}: {} characters, past 260", t["name"], d.chars().count());
        }
    }
}

/// `help` is the reference the descriptions point to: the topics, then each one, before a trace
/// is open too; every topic a description names exists.
#[test]
fn help_topics() {
    let dir = Dir::new("help");
    let mut s = Server::start(&dir.0);
    let list = s.ok("help", json!({}));
    assert!(list.starts_with("help <topic>:\nnames: "), "{list}");
    assert!(s.ok("help", json!({"topic": "Records"})).contains("`n == 138`"));
    let err = s.tool("help", json!({"topic": "nope"})).unwrap_err();
    assert!(err.starts_with("no help topic nope; topics:\nnames: "), "{err}");
    let tools = s.call("tools/list", json!({})).to_string();
    for t in tools.split("`help ").skip(1) {
        let topic = t.split('`').next().unwrap();
        assert!(s.tool("help", json!({"topic": topic})).is_ok(), "no topic {topic}");
    }
}

/// An agent's `open` goes into the history, and `wtf` without a trace shows it in Recent, an
/// agent's half day in its colour (spec §4 "Start screen").
#[test]
fn opens_are_recent() {
    let dir = Dir::new("recent");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": simple()}));
    let history = std::fs::read_to_string(dir.0.join("wtf/history")).unwrap();
    assert!(history.ends_with(&format!("\tagent\t{}\n", simple().display())), "{history}");
    let out = wtf(&dir.0).args(["--snapshot", "100x32"]).output().unwrap();
    let screen = String::from_utf8(out.stdout).unwrap();
    assert!(screen.contains("simple.vcd") && screen.contains("now          1"), "{screen}");
    // the agent's workspace, to join
    assert!(screen.contains("Join ") && screen.contains("1 agent on this machine"), "{screen}");
}

#[test]
fn reading_tools() {
    let dir = Dir::new("read");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": simple()}));
    assert_eq!(s.ok("info", json!({})), "simple.vcd\n0ns - 80ns\n1 tick = 1ns\n6 signals");
    // a regex found anywhere in the path, in any case; a scope it names is a line before its signals
    assert_eq!(s.ok("signals", json!({"pattern": "/^top\\.cpu\\./"})), "top.cpu.state (3 bits)\ntop.cpu.data (8 bits)\ntop.cpu.busy (1 bit)");
    assert_eq!(s.ok("signals", json!({"pattern": "/cpu/"})), "top.cpu (scope of 3 signals)\ntop.cpu.state (3 bits)\ntop.cpu.data (8 bits)\ntop.cpu.busy (1 bit)");
    assert_eq!(s.ok("signals", json!({"pattern": "/cpu$/"})), "top.cpu (scope of 3 signals)");
    assert_eq!(s.ok("signals", json!({"pattern": "/STATE/"})), "top.cpu.state (3 bits)\ntop.dma.state (3 bits)");
    assert_eq!(s.ok("signals", json!({"pattern": "/.*/"})), s.ok("signals", json!({})));
    assert_eq!(s.ok("signals", json!({"pattern": "/nope/"})), "no match: /nope/");
    // nothing matches: the nearest scopes and signals
    assert_eq!(s.ok("signals", json!({"pattern": "/bsy/"})), "no match: /bsy/ (nearest: top.cpu.busy)");
    assert_eq!(s.ok("signals", json!({"pattern": "/cpux/"})), "no match: /cpux/ (nearest: top.cpu)");
    // a pattern is a regex in slashes, as everywhere an agent writes one; a glob, or a regex
    // without them, is an error giving the form meant
    assert_eq!(s.tool("signals", json!({"pattern": "*sta*"})), Err("*sta* is a glob; a pattern is a regex in slashes: /sta/".into()));
    assert_eq!(s.tool("signals", json!({"pattern": "**.{busy,data}"})), Err("**.{busy,data} is a glob; a pattern is a regex in slashes: /(busy|data)/".into()));
    assert_eq!(s.tool("signals", json!({"pattern": "/*sta*/"})), Err("not a regex: /*sta*/ (repetition operator missing expression); `/sta/` for that glob".into()));
    assert_eq!(s.tool("signals", json!({"pattern": "^top\\.cpu"})), Err("^top\\.cpu is no pattern: a pattern is a regex in slashes, /^top\\.cpu/".into()));
    // a slice a query made isn't listed afterwards (spec §16)
    s.ok("find", json!({"expr": "cpu.data[3:0] == 3", "count_only": true}));
    assert_eq!(s.ok("signals", json!({"pattern": "/data/"})), "top.cpu.data (8 bits)");
    // array elements and generate instances of one width are one line, the indices as ranges
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/vhdl.fst").unwrap()}));
    assert_eq!(s.ok("signals", json!({"pattern": "/vhdl/"})), "vhdl (scope of 5 signals)\nvhdl.clk (1 bit)\nvhdl.arr (array, 8 bits, 2 signals)\nvhdl.gen[0..1].x (1 bit, 2 signals)");
    assert_eq!(s.ok("signals", json!({"pattern": "/arr\\[/"})), "vhdl.arr[0..1] (4 bits, 2 signals)");
    // a regex is matched against paths in one form, either HDL's written as `canon` writes it:
    // lowercase, indices in brackets (the file has `gen_stages(0)`, `headers_vld`)
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/hdl.vcd").unwrap()}));
    assert_eq!(s.ok("signals", json!({"pattern": "/gen_stages\\[0\\]/"})), "dut.gen_stages[0] (scope of 1 signal)\ndut.gen_stages[0].x (1 bit)");
    assert_eq!(s.ok("signals", json!({"pattern": "/HEADERS_VLD/"})), "dut.headers_vld (1 bit)");
    assert_eq!(s.ok("signals", json!({"pattern": "/gen_stages\\(0\\)/"})), "no match: /gen_stages\\(0\\)/ (nearest: dut.gen_stages)");
    // without a width: a real, or a string (nvc dumps an enum as one, its type not kept)
    let reals = dir.0.join("real.vcd");
    std::fs::write(&reals, "$timescale 1ns $end\n$scope module top $end\n$var real 64 r level $end\n$upscope $end\n$enddefinitions $end\n#0\nr1.5 r\n").unwrap();
    s.ok("open", json!({"trace": reals}));
    assert_eq!(s.ok("signals", json!({"pattern": "/level/"})), "top.level (real)");
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/types-nvc.fst").unwrap()}));
    assert_eq!(s.ok("signals", json!({"pattern": "/types.pkt.state/"})), "types.pkt.state (string)");
    // a file wellen can't read is an error, not a crash or a hang
    let ghw = std::fs::canonicalize("tests/data/types-ghdl.ghw").unwrap();
    assert!(s.tool("open", json!({"trace": ghw})).unwrap_err().ends_with("wellen can't read it: not yet implemented: support multi-dimensional arrays!"));
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/vhdl.fst").unwrap()}));
    // 1 fs ticks: the time as asked, not the span's end (20.000499ns)
    assert_eq!(s.ok("value_at", json!({"signals": ["clk"], "time": "20ns"})), "at 20ns:\nclk = 0 (1 earlier in 20ns)");
    s.ok("open", json!({"trace": simple()}));
    assert_eq!(s.ok("values", json!({"signals": ["cpu.data"], "from": "15ns", "to": "40ns", "radix": "dec"})), "cpu.data:\n  15ns 67\n  20ns 150\n  30ns 225\n  40ns 58\n");
    assert_eq!(s.ok("value_at", json!({"signals": ["clk", "data[7]", "busy"], "time": "25ns"})), "at 25ns:\nclk = 1\ndata[7] = 1\nbusy = 1");
    assert_eq!(s.ok("find", json!({"expr": "@clk busy"})), "from\tlast\tcycles\n15ns\t25ns\t2");
    assert_eq!(s.ok("find", json!({"expr": "data == 0x40", "backward": true})), "time\n60ns");
    assert_eq!(s.ok("find", json!({"expr": "data == 0x99"})), "no match");
    // slices and concatenations are signals too
    assert_eq!(s.ok("values", json!({"signals": ["cpu.data[7:4]"], "from": "15ns", "to": "30ns", "radix": "dec"})), "cpu.data[7:4]:\n  15ns 4\n  20ns 9\n  30ns 14\n");
    assert_eq!(s.ok("find", json!({"expr": "{busy, data[0]} == 2"})), "time\n20ns");
    insta::assert_snapshot!("render_signals", s.ok("render", json!({"signals": ["clk", "data", "busy"], "from": "0ns", "to": "40ns", "width": 60})));
    assert_eq!(s.ok("reload", json!({})), "reloaded simple.vcd: now 0 - 80ns");
}

/// Composite types (tests/data/types-verilator-structs.fst, as in core/tests/types.rs): a struct's
/// fields are signals under its name, an enum field shows its names; the struct is a signal of
/// its whole value, one row in the workspace.
#[test]
fn struct_fields() {
    let dir = Dir::new("structs");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/types-verilator-structs.fst").unwrap()}));
    // a struct or array is one line, what is in it listed a level at a time as the pattern names it
    assert_eq!(s.ok("signals", json!({"pattern": "/types\\.pkt\\./"})), "types.pkt.valid (1 bit)\ntypes.pkt.state (2 bits)\ntypes.pkt.pair (struct, 8 bits, 2 signals)");
    assert_eq!(s.ok("signals", json!({"pattern": "/pkts/"})), "types.pkts (array, 22 bits, 8 signals)");
    assert_eq!(s.ok("signals", json!({"pattern": "/pkts\\[/"})), "types.pkts[0..1] (struct, 11 bits, 4 signals, 2 structs)");
    assert_eq!(s.ok("signals", json!({"pattern": "/pkts.*pair\\./"})), "types.pkts[0..1].pair.hi (4 bits, 2 signals)\ntypes.pkts[0..1].pair.lo (4 bits, 2 signals)");
    assert_eq!(s.ok("value_at", json!({"signals": ["pkt.state", "pkts(1).pair.lo"], "time": "10ns"})), "at 10ns:\npkt.state = BUSY\npkts(1).pair.lo = 0x7");
    assert_eq!(
        s.ok("value_at", json!({"signals": ["pkt"], "time": "10ns"})),
        "at 10ns:\npkt = 0x5A5\npkt.valid = 1\npkt.state = BUSY\npkt.pair.hi = 0xA\npkt.pair.lo = 0x5"
    );
    assert_eq!(s.ok("find", json!({"expr": "pkt == 0x5A5"})), "time\n10ns");
    // one row each, folded or with levels open, drawn as the window draws them
    let bad = s.tool("workspace", json!({"rows": [{"signal": "grid", "open": ["grid", "mem[1]"]}]})).unwrap_err();
    assert_eq!(bad, "grid has no mem[1] to open; its levels are named from grid on");
    let view = s.ok("workspace", json!({"rows": ["pkt", {"signal": "pkts", "open": ["pkts", "pkts[1]"]}], "range": ["0ns", "20ns"]}));
    assert!(view.contains("pkts  types.pkts  hex  (open: pkts, pkts[1])"), "{view}");
    insta::assert_snapshot!("render_composites", s.ok("render", json!({"width": 60})));
    // a field's radix and colour, written as a row's; named as levels are
    let bad = s.tool("workspace", json!({"rows": [{"signal": "pkt", "fields": ["pair.hi:dec"]}]})).unwrap_err();
    assert_eq!(bad, "pkt has no field pair.hi; fields are named from pkt on");
    let view = s.ok("workspace", json!({"rows": [{"signal": "pkt", "open": ["pkt", "pkt.pair"], "fields": ["pkt.pair.hi:dec:red", "pkt.state:green"]}]}));
    assert!(view.contains("pkt  types.pkt  hex  (open: pkt, pkt.pair)  (fields: pkt.pair.hi:dec:red, pkt.state:enum:green)"), "{view}");
    let r = s.ok("render", json!({"width": 60}));
    assert!(r.contains("    hi   │ 0     │ ┃──── 0 ─────🮧─────────── 10 ─"), "the field in dec, lo in hex: {r}");
}

/// Enum names from the file (spec §10): tests/data/enum.fst, as in core/tests/enums.rs.
#[test]
fn enum_names() {
    let dir = Dir::new("enum");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/enum.fst").unwrap()}));
    // names by default, hex without a name; another radix on request
    assert_eq!(
        s.ok("values", json!({"signals": ["state", "state:hex"], "from": "20ns", "to": "50ns"})),
        "state:\n  20ns WRITE\n  30ns DONE\n  40ns 0x5\n  50ns 0xX\nstate:\n  20ns 0x2\n  30ns 0x3\n  40ns 0x5\n  50ns 0xX\n"
    );
    // the cycle whose ending edge (25ns) samples WRITE
    assert_eq!(s.ok("find", json!({"expr": "@clk state == write", "show": ["state", "count"]})), "time\tstate\tcount\n15ns\tWRITE\t0x2\nas records (`help decode`): `protocol ev roles=state,count when=state == write state=state count=count` then `decode s ev clock=clk state=state count=count`");
    assert_eq!(s.tool("values", json!({"signals": ["count:enum"], "from": "0ns", "to": "10ns"})), Err("enum: the file names no values of count".into()));
}

#[test]
fn find_runs_values_and_counts() {
    let dir = Dir::new("find");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": simple()}));
    // clk rises at 5, 15, 25 ...; busy is high 20ns - 40ns, so the cycles from 15ns and 25ns sample it
    assert_eq!(s.ok("find", json!({"expr": "@clk !busy"})), "from\tlast\tcycles\n5ns\t5ns\t1\n35ns\t65ns\t4");
    assert_eq!(s.ok("find", json!({"expr": "@clk !busy", "backward": true})), "from\tlast\tcycles\n35ns\t65ns\t4\n5ns\t5ns\t1");
    assert_eq!(s.ok("find", json!({"expr": "@clk !busy", "count": 1})), "from\tlast\tcycles\n5ns\t5ns\t1\n(more may follow; search on from the last time)");
    assert_eq!(s.ok("find", json!({"expr": "@clk !busy", "count_only": true})), "5 matches in 2 runs of consecutive cycles, first 5ns, last 65ns");
    assert_eq!(s.ok("find", json!({"expr": "@clk busy", "count_only": true})), "2 matches in 1 run of consecutive cycles, first 15ns, last 25ns");
    // values as the ending edge samples them: data changes at 20ns and 30ns, just after the cycles start
    assert_eq!(s.ok("find", json!({"expr": "@clk busy", "show": ["data", "cpu.state"]})), "time\tdata\tcpu.state\n15ns\t0x96\t0x2\n25ns\t0xE1\t0x2\nas records (`help decode`): `protocol ev roles=busy,data,state when=busy data=data state=state` then `decode s ev clock=clk busy=busy data=data state=cpu.state`");
    // two signals of one last name: each field takes its scope, a slice of one a number
    let mut line = |show: Value| s.ok("find", json!({"expr": "@clk busy", "show": show, "count": 1})).lines().last().unwrap().to_string();
    let two = line(json!(["cpu.state", "dma.state"]));
    assert!(two.ends_with("`protocol ev roles=busy,cpu_state,dma_state when=busy cpu_state=cpu_state dma_state=dma_state` then `decode s ev clock=clk busy=busy cpu_state=cpu.state dma_state=dma.state`"), "{two}");
    let slices = line(json!(["data[7:4]", "data[3:0]"]));
    assert!(slices.ends_with("`protocol ev roles=busy,data when=busy data_0=data[7:4] data_1=data[3:0]` then `decode s ev clock=clk busy=busy data=data`"), "{slices}");
    // without a clock, at the match
    assert_eq!(s.ok("find", json!({"expr": "data == 0x40", "show": ["data:dec", "busy"]})), "time\tdata\tbusy\n60ns\t64\t0");
    // a window of the trace, and a unit of the agent's
    assert_eq!(s.ok("find", json!({"expr": "@clk !busy", "from": "30ns", "to": "50ns", "unit": "us"})), "from\tlast\tcycles\n0.035us\t0.045us\t2");
    assert_eq!(s.tool("find", json!({"expr": "busy", "unit": "parsec"})), Err("bad time unit: parsec (s, ms, us, ns, ps, fs)".into()));
    // a bad time says the form, with the fix where there is one (spec §15 times)
    let at = |s: &mut Server, t: &str| s.tool("value_at", json!({"signals": ["data"], "time": t})).unwrap_err();
    assert_eq!(at(&mut s, "\"20ns\""), "bad time: \"20ns\": without quotes, 20ns");
    assert_eq!(at(&mut s, "None"), "bad time: None (a number and a unit, 20ns or 1.5us; a plain number is file ticks)");
    assert_eq!(at(&mut s, "20 ns"), "bad time: a space before the unit: 20ns");
    assert_eq!(at(&mut s, "20nsec"), "bad time unit: nsec (s, ms, us, ns, ps, fs)");
    assert_eq!(s.tool("find", json!({"expr": "busy", "show": ["state"]})), Err("ambiguous: state (2 signals: top.cpu.state, top.dma.state)".into()));
}

#[test]
fn agent_commands() {
    let dir = Dir::new("cmds");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": simple()}));
    let mut cmd = |l: &str| s.tool("command", json!({"line": l}));
    // the answer says which rows changed, so a pattern that caught too much shows
    assert_eq!(cmd("add top.cpu"), Ok("added 4: group cpu, top.cpu.state, top.cpu.data, top.cpu.busy".into()));
    assert_eq!(cmd("rm /^top\\.cpu\\..*s/"), Ok("removed 2: top.cpu.state, top.cpu.busy".into()));
    // several at once; the ones that fail are named
    assert_eq!(cmd("add top.clk top.rst"), Ok("added 2: top.clk, top.rst".into()));
    assert_eq!(cmd("add top.cpu.busy nope"), Ok("added 1: top.cpu.busy\nno match: nope".into()));
    assert_eq!(cmd("add nope top.nada"), Err("no match: nope; no match: top.nada".into()));
    // with a group's header selected, new rows go after the group, outside it
    // (top.clk, top.rst and top.cpu.busy went below a row of the group, so they joined it)
    cmd("select cpu").unwrap();
    cmd("add top.dma.state").unwrap();
    let rows = |s: &mut Server| s.ok("view", json!({})).split("cursors").next().unwrap().to_string();
    assert_eq!(rows(&mut s), "rows (> selected):\n  ▾ cpu\n    data  top.cpu.data  hex\n    clk  top.clk\n    rst  top.rst\n    busy  top.cpu.busy\n> state  top.dma.state  hex\n");
    let mut cmd = |l: &str| s.tool("command", json!({"line": l}));
    cmd("select data").unwrap();
    cmd("ungroup").unwrap();
    assert_eq!(cmd("ungroup"), Err("not in a group".into()));
    assert_eq!(rows(&mut s), "rows (> selected):\n> data  top.cpu.data  hex\n  clk  top.clk\n  rst  top.rst\n  busy  top.cpu.busy\n  state  top.dma.state  hex\n");
    // a name ambiguous in the trace, but only one of them shown
    assert_eq!(s.ok("value_at", json!({"signals": ["state"], "time": "70ns"})), "at 70ns:\nstate = 0x1");
    // undo steps, one per command
    let mut cmd = |l: &str| s.tool("command", json!({"line": l}));
    assert_eq!(cmd("undo"), Ok("added 1: group cpu".into()));
    assert_eq!(cmd("undo"), Ok("removed 1: top.dma.state".into()));
    assert_eq!(cmd("redo"), Ok("added 1: top.dma.state".into()));
    assert_eq!(cmd("rm /.*/"), Ok("removed 6: group cpu, top.cpu.data, top.clk, top.rst, top.cpu.busy, top.dma.state".into()));
    // cursors: 2 - 9 are marks; `cursor n` alone jumps the cursor (1) there, and moving on leaves the mark
    cmd("add top.clk").unwrap();
    cmd("cursor 2 30ns").unwrap();
    cmd("cursor 2").unwrap();
    cmd("45ns").unwrap();
    assert_eq!(cmd("cursor 3"), Err("no cursor 3".into()));
    let view = s.ok("view", json!({}));
    assert!(view.contains("cursors, in time:\n  2: 30ns\n  1: 45ns, +15ns after 2\n"), "{view}");
}

#[test]
fn workspace_in_one_call() {
    let dir = Dir::new("decl");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": simple()}));
    let decl = json!({
        "scope": "top",
        "rows": ["clk", {"group": "cpu", "rows": ["cpu.data:dec", "cpu.busy"]}, "dma.state:bin"],
        "cursors": {"1": "15ns", "2": "35ns"},
        "range": ["10ns", "50ns"],
    });
    // (zoom goes in steps: 40ns over the workspace's 80 columns shows 10ns - 49ns plus the last column)
    let view = s.ok("workspace", decl.clone());
    assert_eq!(view, "rows (> selected):\n> clk  top.clk\n  ▾ cpu\n    data  top.cpu.data  dec\n    busy  top.cpu.busy\n  state  top.dma.state  bin\ncursors, in time:\n  1: 15ns\n  2: 35ns, +20ns after 1\nvisible: 10ns - 49ns\n");
    // the same declaration again changes nothing; one undo goes back to the empty workspace
    assert_eq!(s.ok("workspace", decl), view);
    assert_eq!(s.tool("command", json!({"line": "undo"})), Ok("removed 5: top.clk, group cpu, top.cpu.data, top.cpu.busy, top.dma.state".into()));
    // every mistake at once, and nothing changes
    let bad = s.tool("workspace", json!({"rows": ["state", "top.clk:float", "nope", "data[3]"], "cursors": {"12": "1ns", "2": "soon"}, "range": ["50ns", "10ns"]}));
    assert_eq!(bad, Err("ambiguous: state (2 signals: top.cpu.state, top.dma.state)\nfloat doesn't fit top.clk (1 bits)\nunknown signal: nope\ndata[3]: a bit can't be a row; add its bus\ncursors: no cursor 12 (1-9)\nsoon: bad time: soon (a number and a unit, 20ns or 1.5us; a plain number is file ticks)\nrange: the end must be after the start".into()));
    assert!(s.ok("view", json!({})).starts_with("rows (> selected):\n  (none)\n"));
    // view in a unit of the agent's
    s.ok("workspace", json!({"rows": ["top.clk"], "cursors": {"2": "35ns"}}));
    assert!(s.ok("view", json!({"unit": "ps"})).contains("  2: 35000ps, +35000ps after 1\nvisible: 0ps - "));
    // a mark with a name and a note (spec §9); the cursor takes a time only
    let view = s.ok("workspace", json!({"rows": ["top.clk"], "cursors": {"2": {"time": "35ns", "name": "first bad write", "note": "AW before W"}, "3": {"time": "45ns", "name": "end"}}}));
    assert!(view.contains("  2: 35ns, +35ns after 1, \"first bad write\": AW before W\n  3: 45ns, +10ns after 2, \"end\"\n"), "{view}");
    let bad = s.tool("workspace", json!({"rows": ["top.clk"], "cursors": {"1": {"time": "5ns", "name": "x"}}}));
    assert_eq!(bad, Err("cursors: 1 is the cursor and moves, so it takes a time only; name a mark (2-9)".into()));
    // a collapsed group
    let view = s.ok("workspace", json!({"scope": "top", "rows": [{"group": "cpu", "rows": ["cpu.data", "cpu.busy"], "collapsed": true}, "clk"]}));
    // (the description lists a collapsed group's signals too, under its header)
    assert!(view.starts_with("rows (> selected):\n> ▸ cpu (2)  (collapsed)\n    data  top.cpu.data  hex\n    busy  top.cpu.busy\n  clk  top.clk\n"), "{view}");
    // a colour last, after a radix or alone; a slice's `:` is no colour's
    let view = s.ok("workspace", json!({"scope": "top", "rows": ["cpu.data:dec:red", "clk:green", "cpu.data[7:4]"]}));
    assert!(view.starts_with("rows (> selected):\n> data  top.cpu.data  dec  red\n  clk  top.clk  green\n  data[7:4]  top.cpu.data[7:4]  hex\n"), "{view}");
    assert_eq!(s.tool("command", json!({"line": "color off"})), Ok("done".into()));
    // named computed signals, declared with the rows that show them; bad ones reported with the rest
    let view = s.ok("workspace", json!({"signals": ["hi = cpu.data[7:4]", "pair = {busy, data[0]}"], "rows": ["hi:dec", "pair"]}));
    assert!(view.starts_with("rows (> selected):\n> hi  hi  dec\n  pair  pair  hex\n"), "{view}");
    assert_eq!(s.ok("find", json!({"expr": "pair == 2"})), "time\n20ns");
    let bad = s.tool("workspace", json!({"signals": ["x = nope", "y"], "rows": ["x"]}));
    assert_eq!(bad, Err("signal x: unknown signal: nope\nsignals: y: not `name = expr`\nunknown signal: x".into()));
    // decoding made with `command` before stays: the declaration adds to it (spec §15)
    s.ok("command", json!({"line": "protocol b roles=busy,data when=busy d=data"}));
    s.ok("command", json!({"line": "decode beat b clock=top.clk busy=top.cpu.busy data=top.cpu.data"}));
    s.ok("workspace", json!({"rows": ["top.clk"]}));
    assert!(s.ok("records", json!({"stream": "beat", "count_only": true})).starts_with("2 records"), "the decode survived");
}

#[test]
fn render_range_stays_with_the_workspace() {
    let dir = Dir::new("range");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": simple()}));
    s.ok("workspace", json!({"rows": ["top.clk", "top.cpu.data"]}));
    let shown = s.ok("render", json!({"from": "20ns", "to": "60ns", "width": 80}));
    // zoom steps can show more than asked, never less: the next step in would end at 53ns
    assert!(shown.starts_with("20ns - 80ns:"), "{shown}");
    // drawn again without a range, and described: the same stretch
    assert_eq!(s.ok("render", json!({"width": 80})), shown);
    let heading = shown.lines().next().unwrap().trim_end_matches(':');
    assert!(s.ok("view", json!({})).ends_with(&format!("visible: {heading}\n")), "{heading}");
}

#[test]
fn stale_sockets_are_removed() {
    let dir = Dir::new("stale");
    // a window that was killed: its socket file is left behind, with nothing listening on it
    let sock = dir.0.join("wtf-99.sock");
    drop(UnixListener::bind(&sock).unwrap());
    assert!(sock.exists());
    let mut s = Server::start(&dir.0);
    assert_eq!(s.ok("sessions", json!({})), "no sessions: the user has no wtf window open; `open` a trace");
    assert!(!sock.exists(), "listing removes it");
}

#[test]
fn errors() {
    let dir = Dir::new("errors");
    let mut s = Server::start(&dir.0);
    let none = "no trace is open: `open` a VCD, FST or GHW waveform (`sessions` lists the user's windows and their traces)";
    assert_eq!(s.tool("info", json!({})), Err(none.into()));
    assert_eq!(s.tool("command", json!({"line": "add clk"})), Err(none.into()));
    assert!(s.tool("open", json!({"trace": dir.0.join("missing.vcd")})).unwrap_err().contains("missing.vcd"));
    s.ok("open", json!({"trace": simple()}));
    assert_eq!(s.tool("value_at", json!({"signals": ["nope"], "time": "1ns"})), Err("unknown signal: nope".into()));
    assert_eq!(s.tool("values", json!({"signals": ["data"], "from": "0ns", "to": "9ns", "radix": "float"})), Err("float doesn't fit data (8 bits)".into()));
    assert_eq!(s.tool("values", json!({"signals": ["data"], "from": "0ns", "to": "9ns", "radix": "hexx"})), Err("unknown radix: hexx".into()));
    assert!(s.tool("find", json!({"expr": "data =="})).is_err());
    assert_eq!(s.tool("render", json!({"signals": ["clk"], "from": "40ns", "to": "10ns"})), Err("`to` must be after `from`".into()));
    // a range written backwards, likely a unit slip: the trace's range to compare with
    let back = s.tool("values", json!({"signals": ["data"], "from": "40ns", "to": "10ns"})).unwrap_err();
    assert!(back.starts_with("`to` 10ns is before `from` 40ns; the trace runs 0"), "{back}");
    assert!(s.tool("find", json!({"expr": "clk", "from": "40ns", "to": "10ns"})).unwrap_err().starts_with("`to` 10ns is before"));
    assert!(s.tool("find", json!({"expr": "clk", "from": "40ns", "to": "10ns", "backward": true})).is_ok());
    assert_eq!(s.tool("find", json!({"expr": "@clk rising"})).unwrap_err(), "rising: `@clk` already samples at the clock's rising edges; an edge of a signal is rose(x), SVA's $rose without the $");
    assert!(s.tool("render", json!({})).unwrap_err().starts_with("the workspace has no signals yet"));
    assert!(s.tool("present", json!({})).unwrap_err().starts_with("the workspace has no signals yet"));
    assert!(s.tool("command", json!({"line": ":w"})).unwrap_err().starts_with(":w belongs to the user's window"));
    assert_eq!(s.tool("command", json!({"line": "add nope"})), Err("no match: nope".into()));
    assert_eq!(s.tool("user_view", json!({})), Err("no wtf window is open on this trace".into()));
}

/// A trace big enough to hit the 1000-line caps: 1100 wires and a clock with 2100 edges.
fn big_trace(dir: &Path) -> PathBuf {
    let mut v = String::from("$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! clk $end\n");
    for i in 0..1100 {
        v += &format!("$var wire 1 w{i} w{i} $end\n");
    }
    v += "$upscope $end\n$enddefinitions $end\n";
    for t in 0..=2100 {
        v += &format!("#{t}\n{}!\n", t % 2);
    }
    let path = dir.join("big.vcd");
    std::fs::write(&path, v).unwrap();
    path
}

/// A long `signals` answer folds to a line per scope below the deepest level that fits, but never
/// above the scope its matches share, so asking for one scope lists it (spec §15).
#[test]
fn signals_fold() {
    let dir = Dir::new("fold");
    let mut v = String::from("$timescale 1ns $end\n$scope module top $end\n$var wire 1 ! clk $end\n");
    for c in 0..4 {
        v += &format!("$scope module core{c} $end\n$var wire 1 r{c} rst $end\n");
        for l in 0..60 {
            v += &format!("$scope module lane{l} $end\n");
            for (k, name) in ["valid", "ready", "data"].iter().enumerate() {
                v += &format!("$var wire 8 s{c}_{l}_{k} {name} $end\n");
            }
            v += "$upscope $end\n";
        }
        v += "$upscope $end\n";
    }
    v += "$upscope $end\n$enddefinitions $end\n#0\n0!\n";
    let path = dir.0.join("deep.vcd");
    std::fs::write(&path, v).unwrap();
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": path}));
    let note = "folded to fit: a scope's line counts its matches; ask for one by its path";
    assert_eq!(
        s.ok("signals", json!({})),
        format!("top (scope of 725 signals)\ntop.clk (1 bit)\ntop.core0 (scope, 181 matching signals)\ntop.core1 (scope, 181 matching signals)\ntop.core2 (scope, 181 matching signals)\ntop.core3 (scope, 181 matching signals)\n{note}, `^top\\.core0\\.`")
    );
    let core = s.ok("signals", json!({"pattern": "/^top\\.core1\\./"}));
    assert_eq!(core.lines().count(), 62, "{core}");
    assert!(core.starts_with("top.core1.rst (1 bit)\ntop.core1.lane0 (scope, 3 matching signals)\n"), "{core}");
    assert!(core.ends_with(&format!("{note}, `^top\\.core1\\.lane0\\.`")), "{core}");
    assert_eq!(s.ok("signals", json!({"pattern": "/^top\\.core1\\.lane7\\./"})), "top.core1.lane7.valid (8 bits)\ntop.core1.lane7.ready (8 bits)\ntop.core1.lane7.data (8 bits)");
    // what fits isn't folded: a scope per core, a lane's signals
    assert_eq!(s.ok("signals", json!({"pattern": "/lane7$/"})).lines().count(), 4);
    assert_eq!(s.ok("signals", json!({"pattern": "/core2.*lane7\\./"})).lines().count(), 3);
    // scopes only, folded: a line counts the scopes under it
    let lanes = s.ok("signals", json!({"pattern": "/lane\\d+$/"}));
    assert!(lanes.starts_with("top.core0 (scope, 60 matching scopes)\n"), "{lanes}");
}

#[test]
fn answers_are_capped() {
    let dir = Dir::new("caps");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": big_trace(&dir.0)}));
    let all = s.ok("signals", json!({}));
    assert_eq!(all.lines().count(), 1001);
    assert!(all.starts_with("top (scope of 1101 signals)\n"), "{}", &all[..60]);
    assert!(all.ends_with("\n... 102 more; narrow the pattern"), "{}", &all[all.len() - 60..]);
    let clk = s.ok("values", json!({"signals": ["clk"], "from": "0ns", "to": "2100ns"}));
    // the name, the value at `from`, 1000 changes, the note
    assert_eq!(clk.lines().count(), 1003);
    assert!(clk.ends_with("  1000ns 0\n  ... more changes until 2100ns; narrow the range, or `find` the events\n"), "{}", &clk[clk.len() - 80..]);
    // the cap counts the whole answer, not each signal
    let two = s.ok("values", json!({"signals": ["clk", "w0"], "from": "0ns", "to": "2100ns"}));
    assert!(two.contains("narrow the range") && !two.contains("w0:"), "{}", &two[two.len() - 80..]);
    let found = s.ok("find", json!({"expr": "clk", "count": 5000}));
    assert_eq!(found.lines().count(), 1002);
    assert!(found.ends_with("1999ns\n(more may follow; search on from the last time)"), "{}", &found[found.len() - 80..]);
    assert_eq!(s.ok("find", json!({"expr": "clk", "count_only": true})), "1050 matches, first 1ns, last 2099ns");
    // a level that holds for the whole trace: one run, however many cycles
    assert_eq!(s.ok("find", json!({"expr": "@clk !w5"})), "from\tlast\tcycles\n1ns\t2097ns\t1049");
    assert_eq!(s.ok("find", json!({"expr": "clk", "from": "10ns", "count": 2})), "time\n11ns\n13ns\n(more may follow; search on from the last time)");
    // every answer has a byte budget
    let wide = s.ok("values", json!({"signals": ["clk", "w1", "w2", "w3"], "from": "0ns", "to": "2100ns", "radix": "q1.0"}));
    assert!(wide.len() < 25_000, "{}", wide.len());
    let mut many: Vec<String> = (0..1100).map(|i| format!("w{i}")).collect();
    many.push("clk".into());
    let cut = s.ok("values", json!({"signals": many, "from": "0ns", "to": "2100ns"}));
    assert!(cut.len() < 25_000 && cut.contains("... cut at 24000 bytes, ") && cut.ends_with("more lines; ask for less (a shorter range, fewer signals, `count`)"), "{}", &cut[cut.len() - 120..]);
}

#[test]
fn workspaces_and_joining() {
    let dir = Dir::new("ws");
    let mut s = Server::start(&dir.0);
    assert_eq!(s.ok("sessions", json!({})), "no sessions: the user has no wtf window open; `open` a trace");
    let pid = s.child.id();
    let fst = std::fs::canonicalize("tests/data/simple.fst").unwrap();
    let joined = "(the user's wtf windows on this trace see your steps; :follow there shows the workspace)";
    assert!(s.ok("open", json!({"trace": simple()})).starts_with(&format!("workspace {pid}-1 {joined}\nsimple.vcd\n")));
    assert!(s.ok("open", json!({"trace": fst})).starts_with(&format!("workspace {pid}-2 {joined}\nsimple.fst\n")));
    let _window = fake_window(&dir.0, "77", &simple(), "version = 1\n");
    let mut listed: Vec<String> = s.ok("sessions", json!({})).lines().map(String::from).collect();
    listed.sort();
    let mut expected = [
        format!("77  window  {}", simple().display()),
        format!("{pid}-1  agent  {}  (yours)", simple().display()),
        format!("{pid}-2  agent  {}  (yours, current)", fst.display()),
    ];
    expected.sort();
    assert_eq!(listed, expected);
    // each workspace keeps its own rows
    s.ok("command", json!({"line": "add top.cpu.busy"}));
    assert!(s.ok("use", json!({"session": format!("{pid}-1")})).starts_with("simple.vcd\n"));
    assert_eq!(s.tool("use", json!({"session": "77"})), Err("77 is not one of your workspaces; see `sessions`".into()));
    s.ok("command", json!({"line": "add top.cpu.data"}));
    s.ok("command", json!({"line": "cursor 2 30ns"}));
    insta::assert_snapshot!("render_workspace", s.ok("render", json!({"width": 60})));

    // a window on the trace follows the agent working on it (spec §15 "Following")
    let shot = |trace: &Path, keys: &str| String::from_utf8(wtf(&dir.0).arg(trace).args(["--snapshot", "120", "--keys", keys]).output().unwrap().stdout).unwrap();
    let followed = shot(&simple(), ":follow<CR>");
    assert!(followed.contains("data │ 0xXX") && followed.contains("1─────────── 30ns ────────────2") && followed.contains(&format!("following {pid}-1")), "{followed}");
    assert!(shot(&fst, ":follow<CR>").contains("busy"));
    assert!(shot(&simple(), ":follow<CR>:follow<CR>").contains(&format!("following {pid}-1")));
    let err = shot(&simple(), &format!(":follow {pid}-2<CR>"));
    assert!(err.contains(&format!("no agent {pid}-2 on this trace")) && err.contains(&format!("{pid}-1)")), "{err}");

    // an agent's workspace can be read over the link, not written
    let mut link = UnixStream::connect(dir.0.join(format!("wtf-{pid}-1.sock"))).unwrap();
    write!(link, "PUT 3\nabc").unwrap();
    let mut answer = String::new();
    link.read_to_string(&mut answer).unwrap();
    assert_eq!(answer, "ERR an agent's workspace takes no sessions\n");

    // the server's sockets go with it
    drop(s);
    assert!(shot(&simple(), ":follow<CR>").contains("no agent is working on this trace"));
}

#[test]
fn trail_reaches_the_window() {
    // spec §15 "Trail": queries and changes are steps; present hands over those since the last
    let dir = Dir::new("trail");
    let mut s = Server::start(&dir.0);
    let trace = dir.0.join("simple.vcd");
    std::fs::copy("tests/data/simple.vcd", &trace).unwrap();
    s.ok("open", json!({"trace": trace}));
    s.ok("command", json!({"line": "add top.cpu.data"}));
    s.ok("find", json!({"expr": "top.cpu.busy", "count_only": true}));
    s.ok("help", json!({"topic": "times"}));
    let _ = s.tool("value_at", json!({"signals": ["nope"], "time": "10ns"}));
    // the agent's own declaration keeps the steps before it
    s.ok("workspace", json!({"rows": ["top.cpu.data"]}));
    // a window reads the whole trail over the link (spec §15 "Joining"); present carries none
    let window = fake_window(&dir.0, "5", &trace, "version = 1\n");
    s.ok("present", json!({}));
    assert!(!window.recv().unwrap().contains("[[trail]]"));
    let ws = UnixStream::connect(dir.0.join(format!("wtf-{}-1.sock", s.child.id()))).unwrap();
    let mut ws = BufReader::new(ws);
    writeln!(ws.get_mut(), "GET").unwrap();
    let mut toml = String::new();
    ws.read_to_string(&mut toml).unwrap();
    let steps: Vec<&str> = toml.split("\n[[trail]]\n").skip(1).collect();
    assert_eq!(steps.len(), 4, "{toml}");
    assert_eq!(steps[0], "tool = \"command\"\nask = \"add top.cpu.data\"\nanswer = \"added 1: top.cpu.data\"\n");
    assert!(steps[1].starts_with("tool = \"find\"\nask = \"top.cpu.busy\"\nanswer = \"1 matches, first 20ns") && steps[1].ends_with("time = \"20ns\"\n"), "{}", steps[1]);
    assert!(steps[2].starts_with("tool = \"value_at\"\nask = \"nope at 10ns\"\nanswer = \"error: "), "{}", steps[2]);
    // the agent's own declaration keeps the steps before it
    assert!(steps[3].starts_with("tool = \"workspace\"\nask = \"1 rows, 0 cursors\"\nanswer = \"declared\"\n"), "{}", steps[3]);
    // a real window following the agent lists them
    let shot = String::from_utf8(wtf(&dir.0).arg(&trace).args(["--snapshot", "100x14", "--keys", ":follow<CR>:trail<CR>"]).output().unwrap().stdout).unwrap();
    assert!(shot.contains("4 steps") && shot.contains("find top.cpu.busy  ·  1 matches, first 20ns"), "{shot}");
    // every window on the trace gets a present (spec §15 "Present")
    let second = fake_window(&dir.0, "6", &trace, "version = 1\n");
    assert_eq!(s.ok("present", json!({})), "shown in the user's 2 windows on this trace: agent: presented; agent: presented");
    assert!(window.recv().is_ok() && second.recv().is_ok());
}

#[test]
fn present_and_user_view() {
    let dir = Dir::new("present");
    let mut s = Server::start(&dir.0);
    // a trace of the test's own, so the headless session file lands in its directory
    let trace = dir.0.join("simple.vcd");
    std::fs::copy("tests/data/simple.vcd", &trace).unwrap();
    s.ok("open", json!({"trace": trace}));
    s.ok("command", json!({"line": "add top.cpu.data"}));

    // no window: written to a session file, with the steps (spec §15 "Present")
    s.ok("command", json!({"line": "cursor 2 20ns name=first busy note=busy rises"}));
    let r = s.ok("present", json!({}));
    let file = dir.0.join("simple.vcd.agent.session.toml");
    assert_eq!(r, format!("no wtf window is open on this trace; written to {}. Tell the user: `wtf {}` shows it (their own view is one `u` away)", file.display(), trace.display()));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(r#"signal = "top.cpu.data""#) && text.contains("[[trail]]\ntool = \"command\"\nask = \"cursor 2 20ns name=first busy note=busy rises\""), "{text}");

    // the user opens the trace: the agent's file is newer than theirs (none), so it shows
    let shot = |keys: &str| String::from_utf8(wtf(&dir.0).arg(&trace).args(["--snapshot", "120", "--keys", keys]).output().unwrap().stdout).unwrap();
    let opened = shot("");
    assert!(opened.contains("2 first busy") && opened.contains("agent: presented 1 signals, 2 cursors; :trail lists its 2 steps"), "{opened}");
    assert!(shot(":trail<CR>").contains("2 steps"));
    // after the user's own :w the file is only offered; :agent shows it
    std::fs::write(dir.0.join("simple.vcd.session.toml"), "version = 1\n").unwrap();
    let offered = shot("");
    assert!(offered.contains("an agent presented this trace before your") && offered.contains(":agent shows it") && !offered.contains("first busy"), "{offered}");
    assert!(shot(":agent<CR>").contains("2 first busy"));
    std::fs::remove_file(dir.0.join("simple.vcd.session.toml")).unwrap();
    // one without steps says none; one that doesn't read is a warning
    std::fs::write(&file, "version = 1\n[[rows]]\nsignal = \"top.clk\"\n").unwrap();
    let opened = shot("");
    assert!(opened.contains("agent: presented 1 signals, 1 cursors; u gives your own view"), "{opened}");
    std::fs::write(&file, "rows = [").unwrap();
    // a card cuts a long message; :messages has all of it
    let wide = String::from_utf8(wtf(&dir.0).arg(&trace).args(["--snapshot", "400x12", "--keys", ":messages<CR>"]).output().unwrap().stdout).unwrap();
    assert!(wide.contains("simple.vcd.agent.session.toml: session: "), "{wide}");
    std::fs::remove_file(&file).unwrap();

    // a window on another trace doesn't count; one on this trace gets the workspace
    let _other = fake_window(&dir.0, "1", &simple(), "version = 1\n");
    assert!(s.ok("present", json!({})).starts_with("no wtf window is open"));
    // (top.gone: the user's session names a signal the trace no longer has)
    let users = "version = 1\n\n[view]\nzoom = \"1ns\"\ntime = \"10ns\"\n\n[cursors]\n1 = \"10ns\"\n2 = \"50ns\"\n\n[[rows]]\nsignal = \"top.clk\"\n\n[[rows]]\nsignal = \"top.gone\"\n";
    let presented = fake_window(&dir.0, "2", &trace, users);
    assert_eq!(s.ok("present", json!({})), "shown in the user's window: agent: presented");
    let toml = presented.recv().unwrap();
    assert!(toml.contains(r#"signal = "top.cpu.data""#) && toml.contains("\nto = \"79ns\"\n"), "{toml}");

    // the user's view, then adopted into the workspace
    assert_eq!(s.ok("user_view", json!({})), "rows (> selected):\n> clk");
    assert_eq!(s.ok("user_view", json!({"adopt": true})), "rows (> selected):\n> clk\n(adopted into the workspace; agent: presented 1 signals, 2 cursors; not in trace: top.gone)");
    let view = s.ok("view", json!({}));
    assert!(view.contains("> clk  top.clk") && view.contains("2: 50ns, +40ns after 1") && !view.contains("data"), "{view}");
}

#[test]
fn records_of_a_declared_stream() {
    let dir = Dir::new("records");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/handshake.vcd").unwrap()}));
    assert_eq!(s.tool("records", json!({"stream": "rx"})), Err("unknown stream: rx (none yet; make one with `command`: `decode <name> <protocol> ...`)".into()));
    // valid 15 - 45 (data 0x11), 65 - 85 (0x22, 0x23 from 75), 105 - 115 (0x33); ready 35 - 45, 105 - 115
    let decl = json!({
        "protocols": [
            {"name": "beat", "roles": ["valid", "ready", "data"], "when": "valid && ready", "fields": ["tdata = data", "low = data[3:0]"]},
            {"name": "any", "roles": ["valid", "data"], "when": "valid", "fields": ["d = data"]},
        ],
        "decode": [
            {"name": "rx", "protocol": "beat", "clock": "clk", "signals": ["valid = valid", "ready = ready", "data = data"]},
            {"name": "v", "protocol": "any", "clock": "clk", "signals": ["valid = valid", "data = data"]},
        ],
        "rows": ["rx", "rx.tdata"],
    });
    assert!(s.ok("workspace", decl).starts_with("rows (> selected):\n> rx  rx\n  rx.tdata  rx.tdata  hex\n"));
    assert_eq!(s.ok("records", json!({"stream": "rx"})), "2 of 2\nn\tstart\tend\ttdata\tlow\n0\t35ns\t45ns\t0x11\t0x1\n1\t105ns\t115ns\t0x33\t0x3");
    // pages, ranges and counts
    assert_eq!(s.ok("records", json!({"stream": "v", "count": 2})), "2 of 6\nn\tstart\tend\td\n0\t15ns\t25ns\t0x11\n1\t25ns\t35ns\t0x11");
    assert_eq!(s.ok("records", json!({"stream": "v", "from": "60ns", "to": "80ns"})), "2 of 2\nn\tstart\tend\td\n3\t65ns\t75ns\t0x22\n4\t75ns\t85ns\t0x23");
    assert_eq!(s.ok("records", json!({"stream": "v", "count_only": true})), "6 records, first 15ns, last 105ns");
    assert_eq!(s.ok("records", json!({"stream": "v", "from": "120ns"})), "no records");
    assert_eq!(s.tool("records", json!({"stream": "w"})), Err("unknown stream: w (streams: rx, v; or a decode's name for its results)".into()));
    // the stream's signals work as any: a search on a field
    assert_eq!(s.ok("find", json!({"expr": "rx.tdata == 0x33"})), "time\n105ns");
    // a protocol with roles, put on signals by an instance
    let decl = json!({
        "protocols": [{"name": "cmd", "roles": ["valid", "ready", "cmd"], "when": "valid && ready", "fields": ["op = cmd[3:0]"]}],
        "decode": [{"name": "c0", "protocol": "cmd", "clock": "clk", "signals": ["valid = valid", "ready = ready", "cmd = data"]}],
        "rows": ["c0.op"],
    });
    assert!(s.ok("workspace", decl).starts_with("rows (> selected):\n> c0.op  c0.op  hex\n"));
    assert_eq!(s.ok("records", json!({"stream": "c0"})), "2 of 2\nn\tstart\tend\top\n0\t35ns\t45ns\t0x1\n1\t105ns\t115ns\t0x3");
    let bad = s.tool("workspace", json!({"decode": [{"name": "c1", "protocol": "cmd", "signals": ["valid = valid", "x"]}], "rows": []}));
    assert_eq!(bad, Err("decode c1: x: not `name = signal`\ndecode c1: no signal for ready, cmd; no clock".into()));
    // mistakes come back with the rest, and nothing changes; a protocol names its roles only
    let decl = json!({
        "protocols": [{"name": "e", "roles": ["v"], "when": "v", "fields": ["x = nope", "y"]}],
        "decode": [{"name": "e0", "protocol": "e", "clock": "clk", "signals": ["v = valid"]}],
        "rows": ["nope2"],
    });
    let bad = s.tool("workspace", decl);
    assert_eq!(bad, Err("protocol e: y: not `name = signal`\ndecode e0: e0.x: nope is no role: e names only its roles (v) and fields\nunknown signal: nope2".into()));
}

/// A protocol is a definition on roles: it needs them, and its decode gives the signals and clock.
#[test]
fn protocol_needs_roles_and_no_clock() {
    let dir = Dir::new("protocol-roles");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/handshake.vcd").unwrap()}));
    let bad = s.tool("workspace", json!({"protocols": [{"name": "ret", "roles": [], "when": "valid && ready", "fields": ["d = data"]}], "rows": []}));
    assert!(bad.unwrap_err().starts_with("protocol ret: ret: needs roles"));
    let bad = s.tool("command", json!({"line": "protocol ret roles=v clock=clk when=v"})).unwrap_err();
    assert!(bad.starts_with("a protocol names no signals, its clock neither: :decode gives it"), "{bad}");
}

#[test]
fn axis_frames_and_errors() {
    let dir = Dir::new("axis");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/axis.vcd").unwrap()}));
    let decl = json!({
        "decode": [{"name": "rx", "protocol": "axis", "clock": "clk", "signals": ["tvalid = tvalid", "tready = tready", "tdata = tdata", "tlast = tlast", "tkeep = tkeep"]}],
        "rows": ["rx.frame", "rx.frame.len:dec", "rx.errors"],
    });
    assert!(s.ok("workspace", decl).starts_with("rows (> selected):\n> rx.frame  rx.frame\n  rx.frame.len  rx.frame.len  dec\n  rx.errors  rx.errors\n"));
    // a frame's data past 8 bytes is its count in a table, whole in one record
    let short = "(long values shortened; one record, `count: 1` with `from`, shows them whole)";
    assert_eq!(s.ok("records", json!({"stream": "rx.frame"})), format!("4 of 4 {short}\nn\tstart\tend\tbeats\tlen\tdata\tcomplete\n0\t15ns\t55ns\t0x3\t0xA\ta0 00 00 00 a1 00 00 00 ... (10 bytes)\t1\n1\t55ns\t65ns\t0x1\t0x4\tb0 00 00 00\t1\n2\t65ns\t85ns\t0x2\t0x8\tc0 00 00 00 c1 00 00 00\t1\n3\t135ns\t145ns\t0x1\t0x4\te1 00 00 00\t1"));
    assert_eq!(s.ok("records", json!({"stream": "rx.frame", "count": 1})), "1 of 4\nn\tstart\tend\tbeats\tlen\tdata\tcomplete\n0\t15ns\t55ns\t0x3\t0xA\ta0 00 00 00 a1 00 00 00 a2 00\t1");
    assert_eq!(s.ok("records", json!({"stream": "rx.errors"})), "2 of 2\nn\tstart\tend\trule\n0\t105ns\t115ns\tdropped\n1\t125ns\t135ns\tchanged");
    assert_eq!(s.ok("find", json!({"expr": "rx.errors.rule == changed"})), "time\n125ns");
    // the workspace draws each record with its label
    assert!(s.ok("render", json!({"width": 100})).contains("#0 10B"));
}

/// An agent decodes with `command`, without a workspace declaration: AXI-Lite transactions, its
/// errors, and a protocol of its own.
#[test]
fn decode_with_command() {
    let dir = Dir::new("axilite");
    let mut s = Server::start(&dir.0);
    // opening names the interfaces a protocol decodes, with the line that decodes each: it works as given
    let opened = s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/axilite.vcd").unwrap()}));
    let line = "decode s_axi axilite clock=top.aclk prefix=top.s_axi_";
    assert!(opened.ends_with(&format!("interfaces a built-in protocol decodes (decode them: exact records, handshake checks built in):\n  top.s_axi_* axilite: {line}")), "{opened}");
    // a stream of an interface not decoded yet: the error gives its line
    let undecoded = format!("its interface isn't decoded yet; with `command`, `{line}` makes s_axi.wr");
    assert_eq!(s.tool("records", json!({"stream": "top.s_axi.wr"})), Err(format!("unknown stream: top.s_axi.wr: {undecoded}")));
    assert_eq!(s.tool("records", json!({"stream": "s_axi.wr"})), Err(format!("unknown stream: s_axi.wr: {undecoded}")));
    assert_eq!(s.tool("records", json!({"stream": "s_axi"})), Err(format!("unknown stream: s_axi: its interface isn't decoded yet; with `command`, `{line}` makes its streams (`protocols axilite`)")));
    assert!(s.ok("command", json!({"line": line})).starts_with("s_axi: records s_axi.wr 2, s_axi.rd 8;"));
    s.ok("command", json!({"line": "add top.aclk"}));
    // decode says what it made and where the violations are, and where to read more
    assert_eq!(
        s.ok("command", json!({"line": "decode bus axilite clock=aclk prefix=s_axi_"})),
        "bus: records bus.wr 2, bus.rd 8; violations bus.errors 2, bus.wr.unmatched 1, bus.rd.unmatched 0; made from bus.aw 2, bus.w 2, bus.b 3, bus.ar 8, bus.r 8\n`records <stream>` lists one, `records <name>` its records and violations in order; `protocols axilite` says what each stream and field is; `pair` matches a stream with another (`help decode`)"
    );
    assert!(s.ok("view", json!({})).contains("aclk"), "the rows stay");
    // a decode's name: its results and violations in order of start, a column per field, empty
    // where a stream has none; a filter keeps the streams having its fields
    assert_eq!(
        s.ok("records", json!({"stream": "bus", "count": 3})),
        "3 of 13\nstream\tn\tstart\tend\tdelay\taddr\tdata\tstrb\tresp\trule\tkind\nbus.wr\t0\t30ns\t50ns\t10ns\t0x10\t0x11111111\t0xF\t0x0\t\t\nbus.wr\t1\t60ns\t120ns\t20ns\t0x14\t0x22222222\t0x3\t0x0\t\t\nbus.rd\t0\t130ns\t160ns\t20ns\t0x10\t0x11111111\t\t0x0\t\t"
    );
    assert_eq!(s.ok("records", json!({"stream": "bus", "where": "strb != 0xF", "count_only": true})), "1 matching records, of 13, first 60ns, last 60ns");
    assert_eq!(s.ok("records", json!({"stream": "bus.wr bus.rd", "group_by": ["resp"]})), "2 of 2 distinct, of 10 records\n9 × resp=0x0 (30ns .. 410ns)\n1 × resp=0x2 (170ns .. 170ns)");
    // a set by pattern, a regex in slashes as everywhere; a glob is an error giving it
    assert_eq!(s.ok("records", json!({"stream": "/^bus\\.(wr|rd)$/", "group_by": ["resp"]})), "2 of 2 distinct, of 10 records\n9 × resp=0x0 (30ns .. 410ns)\n1 × resp=0x2 (170ns .. 170ns)");
    assert_eq!(s.tool("records", json!({"stream": "bus.*"})), Err("bus.* is a glob; a pattern is a regex in slashes: /bus\\./".into()));
    assert_eq!(s.ok("records", json!({"stream": "bus.wr"})), "2 of 2\nn\tstart\tend\tdelay\taddr\tdata\tstrb\tresp\n0\t30ns\t50ns\t10ns\t0x10\t0x11111111\t0xF\t0x0\n1\t60ns\t120ns\t20ns\t0x14\t0x22222222\t0x3\t0x0");
    assert_eq!(s.ok("records", json!({"stream": "bus.errors"})), "2 of 2\nn\tstart\tend\trule\n0\t180ns\t190ns\tr_resp\n1\t210ns\t220ns\taw_dropped");
    // a radix for every number field (not the delay, a time), or one field's
    let wr = |radix: &str| json!({"stream": "bus.wr", "radix": radix, "count": 1});
    assert_eq!(s.ok("records", wr("dec")), "1 of 2\nn\tstart\tend\tdelay\taddr\tdata\tstrb\tresp\n0\t30ns\t50ns\t10ns\t16\t286331153\t15\t0");
    assert_eq!(s.ok("records", wr("addr:dec, strb:bin")), "1 of 2\nn\tstart\tend\tdelay\taddr\tdata\tstrb\tresp\n0\t30ns\t50ns\t10ns\t16\t0x11111111\t0b1111\t0x0");
    assert_eq!(s.tool("records", wr("delay:dec")), Err("delay isn't a number: a time or text takes no radix".into()));
    assert_eq!(s.tool("records", wr("adr:dec")), Err("bus.wr: no field adr (fields: delay, addr, data, strb, resp)".into()));
    // `where` picks records by their fields, on each record only (never `z` between them)
    let rd = |w: &str, count_only| json!({"stream": "bus.rd", "where": w, "count_only": count_only});
    assert_eq!(s.ok("records", rd("resp != 0", false)), "1 of 1 matching, of 8\nn\tstart\tend\tdelay\taddr\tdata\tresp\n1\t170ns\t190ns\t10ns\t0x20\t0xDEADBEEF\t0x2");
    assert_eq!(s.ok("records", rd("resp != 0", true)), "1 matching records, of 8, first 170ns, last 170ns");
    assert_eq!(s.ok("records", rd("addr == 0x99", false)), "no matching records, of 8");
    assert_eq!(s.ok("records", json!({"stream": "bus.errors", "where": "rule == aw_dropped && past(rule) == r_resp"})), "1 of 1 matching, of 2\nn\tstart\tend\trule\n1\t210ns\t220ns\taw_dropped");
    assert_eq!(s.tool("records", rd("adr == 1", false)), Err("bus.rd: no field adr (fields: delay, addr, data, resp, and n, the record's number)".into()));
    // a delay is a time: in the unit asked for, and compared with one
    assert_eq!(s.ok("value_at", json!({"signals": ["bus.wr.delay", "bus.wr.addr"], "time": "70ns"})), "at 70ns:\nbus.wr.delay = 20ns\nbus.wr.addr = 0x14");
    assert_eq!(s.ok("values", json!({"signals": ["bus.wr.delay"], "from": "30ns", "to": "70ns"})), "bus.wr.delay:\n  30ns 10ns\n  50ns 0xZZZZZZZZZZZZZZZZ\n  60ns 20ns\n");
    let wr = json!({"stream": "bus.wr", "where": "delay == 20ns", "unit": "ps"});
    assert_eq!(s.ok("records", wr), "1 of 1 matching, of 2\nn\tstart\tend\tdelay\taddr\tdata\tstrb\tresp\n1\t60000ps\t120000ps\t20000ps\t0x14\t0x22222222\t0x3\t0x0");
    // ordered, as unsigned numbers; a time with a time
    assert_eq!(s.ok("records", json!({"stream": "bus.wr", "where": "delay > 10ns", "count_only": true})), "1 matching records, of 2, first 60ns, last 60ns");
    assert_eq!(s.ok("records", rd("addr >= 0x1C && addr < 0x20", true)), "5 matching records, of 8, first 260ns, last 380ns");
    // `group_by`: the six polls after 260ns as one line per distinct poll
    let polls = |by: Value, count_only| json!({"stream": "bus.rd", "from": "260ns", "group_by": by, "count_only": count_only});
    assert_eq!(s.ok("records", polls(json!(["addr", "data"]), false)), "3 of 3 distinct, of 6 records\n4 × addr=0x1C data=0x3 (260ns .. 350ns)\n1 × addr=0x1C data=0x7 (380ns .. 380ns)\n1 × addr=0x18 data=0x0 (410ns .. 410ns)");
    assert_eq!(s.ok("records", polls(json!(["addr"]), true)), "2 distinct, of 6 records");
    let polls_dec = json!({"stream": "bus.rd", "from": "260ns", "group_by": ["addr", "data"], "radix": "dec", "count": 1});
    assert_eq!(s.ok("records", polls_dec), "1 of 3 distinct, of 6 records\n4 × addr=28 data=3 (260ns .. 350ns)");
    let polled = json!({"stream": "bus.rd", "group_by": ["addr"], "where": "addr == 0x1C"});
    assert_eq!(s.ok("records", polled), "1 of 1 distinct, of 5 matching records, of 8\n5 × addr=0x1C (260ns .. 380ns)");
    assert_eq!(s.ok("find", json!({"expr": "bus.rd.resp == 2", "show": ["bus.rd.addr"]})), "time\tbus.rd.addr\n170ns\t0x20");
    // an empty check is its answer
    assert_eq!(s.ok("records", json!({"stream": "bus.rd.unmatched"})), "no records: no violations");
    s.ok("command", json!({"line": "protocol beat roles=valid,ready,data when=valid && ready data=data"}));
    s.ok("command", json!({"line": "decode rd beat clock=aclk valid=s_axi_rvalid ready=s_axi_rready data=s_axi_rdata"}));
    assert_eq!(s.ok("records", json!({"stream": "rd", "count_only": true})), "8 records, first 150ns, last 420ns");
    assert!(s.tool("command", json!({"line": "protocol x roles=data data=data"})).unwrap_err().starts_with("usage: :protocol"));
}

/// `protocols` lists what can be decoded and describes each, before a trace is open too.
#[test]
fn protocols_describe_themselves() {
    let dir = Dir::new("protocols");
    let mut s = Server::start(&dir.0);
    let all = s.ok("protocols", json!({}));
    for line in ["  axilite  AXI4-Lite: write and read", "  frame    records of a stream", "  check    a violation record"] {
        assert!(all.contains(line), "{all}");
    }
    let axil = s.ok("protocols", json!({"name": "axilite"}));
    for line in [
        "  awprot wstrb bresp arprot rresp (optional)",
        "  <name>.wr: delay addr prot data strb resp\n",
        "  <name>.wr.unmatched: kind (violations: kind = no request | no response | no aw | no w)",
        "  <name>.rd.unmatched: kind (violations: kind = no request | no response)",
        // what each stream is: what it samples, its rules' expressions, a note for the rest
        "  <name>.aw: addr prot\n      a record per cycle where awvalid && awready; addr = awaddr, prot = awprot\n",
        "        aw_dropped: fell(awvalid) && !past(awready)\n",
        "        r_resp: rvalid && rready && rresp != 0\n",
        "      writes: AW and W in either order, then B; delay from the later of AW and W to B\n",
        "      a B without a write (no request), a write never answered (no response)",
        "example: decode bus axilite clock=aclk prefix=s_axi_",
    ] {
        assert!(axil.contains(line), "{line}\n{axil}");
    }
    let axi4 = s.ok("protocols", json!({"name": "axi4"}));
    for line in ["  <name>.rburst: beats id data complete\n", "  <name>.rd: delay addr ar_id len size burst prot beats resp_id data complete\n", "example: decode mem axi4 clock=aclk prefix=m_axi_"] {
        assert!(axi4.contains(line), "{line}\n{axi4}");
    }
    // the fields come from the decoders themselves
    assert!(s.ok("protocols", json!({"name": "axis"})).contains("  <name>.frame: beats len data complete\n"));
    assert_eq!(s.tool("protocols", json!({"name": "axi"})), Err("unknown protocol: axi (protocols: axis, axilite, axi4, spi, uart, frame, pair, check; `protocols <name>` describes one)".into()));
    // and the workspace's own, with how to put them on signals
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/handshake.vcd").unwrap()}));
    s.ok("command", json!({"line": "protocol hs roles=v,r when=v && r"}));
    let all = s.ok("protocols", json!({}));
    assert!(all.contains("in this session (protocol <name> ...):\n  hs       roles v, r: a record where v && r at the instance's clock"), "{all}");
    assert!(s.ok("protocols", json!({"name": "hs"})).ends_with("example: decode x hs clock=<clk> v=<signal> r=<signal>"));
    let err = s.tool("command", json!({"line": "decode x hz v=valid"})).unwrap_err();
    assert!(err.contains("unknown protocol: hz (protocols: axis, axilite, axi4, spi, uart, frame, pair, check, hs;"), "{err}");
}

/// A trace in fs whose clock and signals lag by a femtosecond or two, as a real capture's: once a clock
/// is searched, answers are in its unit, whole numbers of it; a finer unit gives them exactly.
/// `records` `where` on a `pair`: its two sides compared field with field, as a scoreboard does,
/// and a record picked by its number. reqresp.vcd paired by order: (req_id, resp_id) (1, 2),
/// (2, 1), (3, 0).
#[test]
fn records_compare_fields_and_pick_by_number() {
    let dir = Dir::new("reqresp");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/reqresp.vcd").unwrap()}));
    s.ok("command", json!({"line": "protocol hs roles=valid,id when=valid id=id"}));
    s.ok("command", json!({"line": "decode req hs clock=clk valid=req_valid id=req_id"}));
    s.ok("command", json!({"line": "decode resp hs clock=clk valid=resp_valid id=resp_id"}));
    s.ok("command", json!({"line": "decode o pair request=req response=resp"}));
    let o = |w: &str| json!({"stream": "o", "where": w});
    assert_eq!(s.ok("records", o("req_id > resp_id")), "2 of 2 matching, of 3\nn\tstart\tend\tdelay\treq_id\tresp_id\n1\t25ns\t65ns\t30ns\t0x2\t0x1\n2\t45ns\t95ns\t40ns\t0x3\t0x0");
    assert_eq!(s.ok("records", o("req_id == resp_id")), "no matching records, of 3");
    assert_eq!(s.ok("records", o("n == 0 || past(resp_id) == req_id")), "2 of 2 matching, of 3\nn\tstart\tend\tdelay\treq_id\tresp_id\n0\t15ns\t45ns\t20ns\t0x1\t0x2\n1\t25ns\t65ns\t30ns\t0x2\t0x1");
    // the number counts from the stream's start, not from `from`
    assert_eq!(s.ok("records", json!({"stream": "o", "from": "40ns", "where": "n == 2", "count_only": true})), "1 matching records, of 1, first 45ns, last 45ns");
    // a signal compares with a signal; a table of events offers its `protocol` and `decode` lines, which work
    let table = s.ok("find", json!({"expr": "@clk req_valid && req_id != resp_id", "show": ["req_id", "resp_id"]}));
    assert_eq!(table, "time\treq_id\tresp_id\n15ns\t0x1\t0x0\n25ns\t0x2\t0x0\n45ns\t0x3\t0x2\nas records (`help decode`): `protocol ev roles=req_valid,req_id,resp_id when=req_valid && req_id != resp_id req_id=req_id resp_id=resp_id` then `decode s ev clock=clk req_valid=req_valid req_id=req_id resp_id=resp_id`");
    let last = table.lines().last().unwrap();
    for line in [last.split('`').nth(3).unwrap(), last.split('`').nth(5).unwrap()] {
        s.ok("command", json!({"line": line}));
    }
    assert_eq!(s.ok("records", json!({"stream": "s", "count_only": true})), "3 records, first 15ns, last 45ns");
    // `@rising(clk)` names the clock inside
    assert_eq!(s.tool("find", json!({"expr": "@rising(clk) req_valid"})), Err("@ takes the clock signal itself, as its rising edges: @clk".into()));
}

/// Arithmetic in fields and `where` (spec §8, §16): exact, a negative field signed, and errors
/// that show the fix.
#[test]
fn arithmetic_in_fields_and_where() {
    let dir = Dir::new("arith");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/reqresp.vcd").unwrap()}));
    s.ok("command", json!({"line": "protocol pp roles=valid,req,resp when=valid id=req next=req + 1 back=resp - req"}));
    s.ok("command", json!({"line": "decode p pp clock=clk valid=req_valid req=req_id resp=resp_id"}));
    let mut p = |w: &str| s.ok("records", json!({"stream": "p", "where": w}));
    let all = "n\tstart\tend\tid\tnext\tback\n0\t15ns\t25ns\t0x1\t0x2\t-1\n1\t25ns\t35ns\t0x2\t0x3\t-2\n2\t45ns\t55ns\t0x3\t0x4\t-1";
    assert_eq!(p("next == id + 1"), format!("3 of 3 matching, of 3\n{all}"));
    // a negative field stays signed, and compares exactly with an unsigned one
    assert_eq!(p("back < -1"), "1 of 1 matching, of 3\nn\tstart\tend\tid\tnext\tback\n1\t25ns\t35ns\t0x2\t0x3\t-2");
    assert_eq!(p("back < id && (id & 1) == 1 && id << 2 == 12"), "1 of 1 matching, of 3\nn\tstart\tend\tid\tnext\tback\n2\t45ns\t55ns\t0x3\t0x4\t-1");
    // a field names its value for the fields after it and for `when`
    s.ok("command", json!({"line": "protocol qq roles=valid,req when=valid && odd == 1'b1 id=req odd=id[0:0] twice=id * 2"}));
    s.ok("command", json!({"line": "decode q qq clock=clk valid=req_valid req=req_id"}));
    assert_eq!(s.ok("records", json!({"stream": "q"})), "2 of 2\nn\tstart\tend\tid\todd\ttwice\n0\t15ns\t25ns\t0x1\t1\t0x2\n1\t45ns\t55ns\t0x3\t1\t0x6");
    s.ok("command", json!({"line": "protocol r roles=valid when=valid x=req_id"}));
    assert_eq!(s.tool("command", json!({"line": "decode r0 r clock=clk valid=req_valid"})), Err("r0: r0.x: req_id is no role: r names only its roles (valid) and fields".into()));
    // a field's bits, and ! as an operand
    assert_eq!(s.ok("records", json!({"stream": "p", "where": "id[1] == 1 && id[0:0] == !1", "count_only": true})), "1 matching records, of 3, first 25ns, last 25ns");
    assert_eq!(s.tool("records", json!({"stream": "p", "where": "id & 1 == 1"})), Err("id & 1 == 1: & and == need parentheses: (id & 1) == 1 or id & (1 == 1)".into()));
    assert_eq!(s.tool("find", json!({"expr": "~(req_id + 1) == 0"})), Err("~(req_id + 1): ~ inverts bits at a width, so give one with a slice: ~(req_id + 1)[1:0]".into()));
}

#[test]
fn times_in_the_clocks_unit() {
    let dir = Dir::new("skew");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/skew.vcd").unwrap()}));
    let data = json!({"signals": ["data"], "from": "0ns", "to": "70ns"});
    assert_eq!(s.ok("values", data.clone()), "data:\n  0ns 0x0\n  20.000002ns 0x5\n  30.000002ns 0x6\n");
    assert_eq!(s.ok("find", json!({"expr": "@clk valid && ready", "show": ["data"]})), "time\tdata\n20ns\t0x5\n30ns\t0x6\nas records (`help decode`): `protocol ev roles=valid,ready,data when=valid && ready data=data` then `decode s ev clock=clk valid=valid ready=ready data=data`");
    assert_eq!(s.ok("values", data.clone()), "data:\n  0ns 0x0\n  20ns 0x5\n  30ns 0x6\n");
    s.ok("command", json!({"line": "protocol beat roles=valid,ready,data when=valid && ready d=data"}));
    s.ok("command", json!({"line": "decode rx beat clock=clk valid=valid ready=ready data=data"}));
    assert_eq!(s.ok("records", json!({"stream": "rx"})), "2 of 2\nn\tstart\tend\td\n0\t20ns\t30ns\t0x5\n1\t30ns\t40ns\t0x6");
    // a rounded time stands for every tick that prints as it: read after them, a change among them said
    assert_eq!(s.ok("value_at", json!({"signals": ["data", "ready"], "time": "20ns"})), "at 20ns:\ndata = 0x5 (0x0 earlier in 20ns)\nready = 1");
    assert_eq!(s.ok("find", json!({"expr": "@clk valid && ready", "from": "20ns"})), "from\tlast\tcycles\n30ns\t30ns\t1");
    // backward: before every tick the time stands for
    let back = |from: &str| json!({"expr": "@clk valid && ready", "from": from, "backward": true});
    assert_eq!(s.ok("find", back("20ns")), "no match");
    assert_eq!(s.ok("find", back("30ns")), "from\tlast\tcycles\n20ns\t20ns\t1");
    assert_eq!(s.ok("records", json!({"stream": "rx", "from": "30ns", "to": "30ns", "count_only": true})), "1 records, first 30ns, last 30ns");
    // exact in a finer unit, or with the unit set
    assert_eq!(s.ok("find", json!({"expr": "@clk valid", "unit": "fs"})), "from\tlast\tcycles\n20000001fs\t30000001fs\t2");
    s.ok("command", json!({"line": "unit ps"}));
    assert_eq!(s.ok("value_at", json!({"signals": ["data"], "time": "20.000002ns"})), "at 20000.002ps:\ndata = 0x5");
    assert_eq!(s.tool("command", json!({"line": "unit min"})), Err("usage: :unit <s|ms|us|ns|ps|fs|auto>".into()));
    s.ok("command", json!({"line": "unit auto"}));
    assert_eq!(s.ok("values", data), "data:\n  0ns 0x0\n  20ns 0x5\n  30ns 0x6\n");
}

/// A default scope: a leaf ambiguous in the trace (`state`, in cpu and dma) names the one under
/// it, in every tool, and the session keeps it.
#[test]
fn names_under_the_scope() {
    let dir = Dir::new("scope");
    let mut s = Server::start(&dir.0);
    let trace = dir.0.join("simple.vcd");
    std::fs::copy("tests/data/simple.vcd", &trace).unwrap();
    s.ok("open", json!({"trace": trace}));
    let at = json!({"signals": ["state"], "time": "0ns"});
    assert!(s.tool("value_at", at.clone()).unwrap_err().starts_with("ambiguous: state (2 signals"));
    assert_eq!(s.tool("command", json!({"line": "scope top.cpux"})), Err("no scope top.cpux".into()));
    s.ok("command", json!({"line": "scope top.dma"}));
    assert!(s.ok("value_at", at.clone()).ends_with("\nstate = 0x0"));
    assert_eq!(s.ok("find", json!({"expr": "state != 0", "count_only": true})), s.ok("find", json!({"expr": "top.dma.state != 0", "count_only": true})));
    assert_eq!(s.ok("command", json!({"line": "add state"})), "added 1: top.dma.state");
    // the scope's signals resolve in full too, and other scopes by their path
    assert_eq!(s.ok("command", json!({"line": "add top.cpu.state"})), "added 1: top.cpu.state");
    s.ok("present", json!({}));
    let file = std::fs::read_to_string(dir.0.join("simple.vcd.agent.session.toml")).unwrap();
    assert!(file.contains("scope = \"top.dma\"\n"), "{file}");
    // workspace sets it for later calls; a failed declaration changes nothing
    assert!(s.tool("workspace", json!({"scope": "top.cpu", "rows": ["nope"]})).is_err());
    assert!(s.ok("command", json!({"line": "rm state"})).contains("removed 1: top.dma.state"));
    s.ok("workspace", json!({"scope": "top.cpu", "rows": ["state", "data"]}));
    assert!(s.ok("value_at", json!({"signals": ["state"], "time": "0ns"})).contains("state = "));
    assert!(s.ok("view", json!({})).contains("top.cpu.state"));
    // cleared, the session has none; `state` is then the one the workspace shows
    s.ok("command", json!({"line": "scope"}));
    s.ok("present", json!({}));
    assert!(!std::fs::read_to_string(dir.0.join("simple.vcd.agent.session.toml")).unwrap().contains("\nscope = "));
    assert!(s.ok("value_at", at).contains("state = "));
}

/// SPI and UART, decoded by an agent with `command`: they sample by time, so they need no clock.
#[test]
fn spi_and_uart_with_command() {
    let dir = Dir::new("serial");
    let mut s = Server::start(&dir.0);
    let uart = s.ok("protocols", json!({"name": "uart"}));
    for line in ["roles: line (required)", "  baud bits parity stop idle\n", "  <name>.byte: data\n", "  <name>.errors: rule (violations)\n", "example: decode console uart line=uart_tx baud=115200"] {
        assert!(uart.contains(line), "{line}\n{uart}");
    }
    assert!(!uart.contains("levels"), "the line's changes are no stream of the instance's");
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/spi.vcd").unwrap()}));
    s.ok("command", json!({"line": "decode flash spi sclk=spi_clk cs=spi_cs_n mosi=spi_mosi miso=spi_miso mode=0"}));
    assert_eq!(s.ok("records", json!({"stream": "flash.transfer"})), "2 of 2\nn\tstart\tend\twords\tmosi\tmiso\tcomplete\n0\t100ns\t430ns\t0x2\ta5 3c\t5a c3\t1\n1\t500ns\t750ns\t0x2\tf0 a\t0f 5\t1");
    assert_eq!(s.ok("records", json!({"stream": "flash.word", "where": "complete == 0"})), "1 of 1 matching, of 4\nn\tstart\tend\tmosi\tmiso\tcomplete\n3\t670ns\t750ns\t0xA\t0x5\t0");
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/uart.vcd").unwrap()}));
    assert_eq!(s.tool("command", json!({"line": "decode console uart line=uart_tx"})), Err("console: uart: needs baud=<a positive number of bits a second>".into()));
    s.ok("command", json!({"line": "decode console uart line=uart_tx baud=1000000 parity=even"}));
    assert_eq!(s.ok("records", json!({"stream": "console.byte"})), "4 of 4\nn\tstart\tend\tdata\n0\t1us\t12us\t0x55\n1\t13us\t24us\t0xA3\n2\t25us\t36us\t0xF\n3\t37us\t48us\t0x81");
    assert_eq!(s.ok("records", json!({"stream": "console.errors"})), "2 of 2\nn\tstart\tend\trule\n0\t25us\t36us\tparity\n1\t37us\t48us\tframing");
}

/// A help topic as Markdown: what Markdown would read as markup outside `code` escaped, and an
/// example line on its own (indented after a `:`, no code in it) as code.
fn markdown(text: &str) -> String {
    let mut out = Vec::new();
    let (mut prev, mut code) = ("", false);
    for line in text.lines() {
        if ["Form", "Recipes", "Details"].contains(&line) {
            // a topic's sections
            out.push(format!("**{line}**\n"));
        } else if prev.ends_with(':') && line.starts_with("  ") && !line.contains('`') {
            out.push(format!("\n```\n{}\n```\n", line.trim()));
        } else {
            // a code span can run on to the next line
            let parts: Vec<String> = line.split('`').enumerate().map(|(k, p)| match (k % 2 == 1) != code {
                false => p.chars().map(|c| if "\\*_<>[]".contains(c) { format!("\\{c}") } else { c.to_string() }).collect(),
                true => p.to_string(),
            }).collect();
            code ^= parts.len().is_multiple_of(2);
            out.push(parts.join("`"));
        }
        prev = line;
    }
    out.join("\n")
}

/// The site's agent tools page (`site/content/tools.md`) and reference (`reference.md`), written
/// from what the server tells an agent: each tool's description and arguments, and the `help`
/// topics. `UPDATE_SITE=1` rewrites them.
#[test]
fn site_tools() {
    let dir = Dir::new("site");
    let mut s = Server::start(&dir.0);
    let cell = |v: &Value| v.as_str().unwrap_or("").replace('\n', " ").replace('|', "\\|");
    let mut out = String::from(
        "+++\ntitle = \"Agent tools\"\ndescription = \"The tools wtf --mcp gives an agent.\"\n+++\n\n\
         <!-- Written by `UPDATE_SITE=1 cargo test --test mcp site_tools` from what wtf --mcp tells an agent; don't edit. -->\n\n\
         What `wtf --mcp` tells an agent, word for word: each tool's description and arguments. See [For agents](@/agents.md) for setting it up.\n\n## Tools\n\n",
    );
    let list = s.call("tools/list", json!({}));
    for t in list["tools"].as_array().unwrap() {
        out.push_str(&format!("### `{}`\n\n{}\n\n", t["name"].as_str().unwrap(), t["description"].as_str().unwrap()));
        let props = t["inputSchema"]["properties"].as_object();
        if let Some(props) = props.filter(|p| !p.is_empty()) {
            let required: Vec<&str> = t["inputSchema"]["required"].as_array().map_or(vec![], |r| r.iter().filter_map(|v| v.as_str()).collect());
            out.push_str("| Argument | Means |\n|---|---|\n");
            for (name, p) in props {
                let need = if required.contains(&name.as_str()) { " (required)" } else { "" };
                out.push_str(&format!("| `{name}`{need} | {} |\n", cell(&p["description"])));
            }
            out.push('\n');
        }
    }
    out.push_str("The `help` topics they point to are the [reference](@/reference.md).\n");
    // the help topics: the same texts the window shows with `:help <topic>`
    let mut reference = String::from(
        "+++\ntitle = \"Reference\"\ndescription = \"Names, patterns, times, radixes, search expressions, decoders, records and commands, as wtf explains them.\"\n+++\n\n\
         <!-- Written by `UPDATE_SITE=1 cargo test --test mcp site_tools` from wtf's help topics; don't edit. -->\n\n\
         One topic per concept, word for word as wtf explains it: `:help <topic>` shows one in the \
         window, and an agent reads it with the `help` tool. The [tour](@/get-started.md) and the \
         [recipes](@/recipes.md) show them at work.\n\n",
    );
    let topics = s.ok("help", json!({}));
    for line in topics.lines().skip(1) {
        // a topic's first line says what it covers
        let Some((topic, _)) = line.split_once(": ") else { continue };
        reference.push_str(&format!("## {topic}\n\n{}\n\n", markdown(&s.ok("help", json!({"topic": topic})))));
    }
    for (name, page) in [("tools", out), ("reference", reference)] {
        let page = page.trim_end().to_string() + "\n";
        // Zola runs content through Tera
        assert!(!page.contains("{{") && !page.contains("{%") && !page.contains("{#"), "{name}: Tera syntax in a text: wrap it in raw");
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("site/content/{name}.md"));
        if std::env::var_os("UPDATE_SITE").is_some() {
            std::fs::write(&path, &page).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == page, "site/content/{name}.md is out of date: UPDATE_SITE=1 cargo test --test mcp site_tools");
    }
}

/// Signals alike down to their scope's name take a number in the roles `find` suggests (spec §15).
#[test]
fn find_roles_alike_get_numbers() {
    let dir = Dir::new("roles-alike");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/hdl.vcd").unwrap()}));
    let out = s.ok("find", json!({"expr": "@clk data", "show": ["dut.gen_stages(0).x", "dut.gen_stages(1).x"]}));
    assert!(out.contains("roles=data,gen_stages_x_1,gen_stages_x_2"), "{out}");
}

/// `protocols` and `records` errors name the stream wanted and what there is (spec §16).
#[test]
fn records_errors() {
    let dir = Dir::new("records-errors");
    let mut s = Server::start(&dir.0);
    s.ok("open", json!({"trace": std::fs::canonicalize("tests/data/axilite.vcd").unwrap()}));
    s.ok("command", json!({"line": "decode bus axilite clock=aclk prefix=s_axi_"}));
    assert!(s.tool("records", json!({"stream": "bus.wr", "group_by": ["nope"]})).unwrap_err().starts_with("bus.wr: no field nope (fields: "));
    assert_eq!(s.tool("records", json!({"stream": " "})), Err("records of which stream? (a stream, a decode's name or a glob)".into()));
}

/// `--ansi` draws colours, with or without a trace; the start screen takes `--keys` (spec §4).
#[test]
fn snapshot_ansi_and_start_keys() {
    let dir = Dir::new("ansi");
    let out = |args: &[&str]| String::from_utf8(wtf(&dir.0).args(args).output().unwrap().stdout).unwrap();
    assert!(out(&["--snapshot", "100x32", "--ansi"]).contains("\x1b["));
    let trace = simple();
    assert!(out(&[trace.to_str().unwrap(), "--snapshot", "100x20", "--ansi"]).contains("\x1b["));
    let typing = out(&["--snapshot", "100x32", "--keys", "ox.vcd"]);
    assert!(typing.contains("x.vcd"), "{typing}");
}

/// `--snapshot` takes no screen smaller than the window's smallest (spec §4 "Too small", §11);
/// at that size, every mode and the start screen draw what fits.
#[test]
fn snapshot_smallest_screen() {
    let dir = Dir::new("small");
    let trace = std::fs::canonicalize("tests/data/reqresp.vcd").unwrap();
    for size in ["39", "80x3", "39x10", "40x9"] {
        let out = wtf(&dir.0).arg(&trace).args(["--snapshot", size]).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success() && err.contains(&format!("{size} is too small; the smallest screen is 40x10")), "{size}: {err}");
    }
    for size in ["40x10", "41x10", "200x10", "40x30"] {
        for keys in ["", "?", ":", "<Space>", "a", "e", ":records<CR>"] {
            for with_trace in [true, false] {
                let mut cmd = wtf(&dir.0);
                if with_trace {
                    cmd.arg(&trace);
                }
                cmd.args(["--snapshot", size]);
                if !keys.is_empty() {
                    cmd.args(["--keys", keys]);
                }
                let out = cmd.output().unwrap();
                assert!(out.status.success(), "{size} {keys:?} trace={with_trace}: {}", String::from_utf8_lossy(&out.stderr));
                let height: usize = size.split('x').nth(1).unwrap().parse().unwrap();
                assert_eq!(String::from_utf8(out.stdout).unwrap().lines().count(), height, "{size} {keys:?}");
            }
        }
    }
}

/// A trace with an interface a built-in protocol decodes says so once (spec §4).
#[test]
fn found_interfaces_are_offered() {
    let dir = Dir::new("found");
    let trace = std::fs::canonicalize("tests/data/axilite.vcd").unwrap();
    let shot = wtf(&dir.0).arg(trace).args(["--snapshot", "140x12"]).output().unwrap();
    let shot = String::from_utf8(shot.stdout).unwrap();
    assert!(shot.contains("axilite at top.s_axi_") && shot.contains("decodes"), "{shot}");
}
