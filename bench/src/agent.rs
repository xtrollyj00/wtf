//! The agent benchmark (spec §17, milestone 10): debugging tasks a headless agent solves with
//! `wtf --mcp`, graded against known answers, with what each run cost: wtf calls, tool errors,
//! the size of wtf's answers, tokens, peak context, money and time.
//!
//! A task is bench/agent/tasks/<name>.toml: `trace` (from the repository root), `prompt` (`{trace}`
//! becomes its path), `answer` (the JSON shape asked for) and `expect` (the answer). A fix task is
//! a directory, bench/agent/tasks/<name>/, with a task.toml (`upstream`, `commit`, `[files]`,
//! `prompt`, `answer`, `expect`), a bug.patch, and the Makefile and testbench the agent debugs
//! with (see `Fix`); it passes when the answer is right and the agent's fix passes. Each run
//! appends a line to bench/agent/results.jsonl, its transcript (and review) go to
//! bench/agent/runs/; git ignores both.

use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage:
  wtf-bench agent run [--wtf <path>] [--model <model>] [--runs 1] [--budget 1.0|none] [--timeout 600] [--review] [--no-mcp] [--max-sims N] [<task>...]
                                       --no-mcp: fix tasks without wtf, as a baseline
                                       --max-sims: fix tasks get N `make sim` runs, then make refuses
  wtf-bench agent check [<task>...]    prove fix tasks without an agent
  wtf-bench agent report";

/// Asked after a run with --review, in the same session.
const REVIEW: &str = "Review your run, briefly: which wtf calls were wasted or failed and why, what \
cost the most tokens, and what wtf tool or answer would have saved you the most. Rank by cost.";

pub fn main(args: &[String]) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or("no repository root")?.to_path_buf();
    match args.first().map(String::as_str) {
        Some("run") => run(&root, &args[1..]),
        Some("check") => check(&root, &args[1..]),
        Some("report") => report(&root),
        _ => Err(USAGE.into()),
    }
}

struct Opts {
    wtf: Option<String>,
    model: Option<String>,
    runs: usize,
    budget: String,
    timeout: u64,
    review: bool,
    no_mcp: bool,
    max_sims: Option<u64>,
    tasks: Vec<String>,
}

fn opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { wtf: None, model: None, runs: 1, budget: "1.0".into(), timeout: 600, review: false, no_mcp: false, max_sims: None, tasks: vec![] };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().ok_or(format!("{a} needs a value\n{USAGE}"));
        match a.as_str() {
            "--wtf" => o.wtf = Some(val()?),
            "--model" => o.model = Some(val()?),
            "--runs" => o.runs = val()?.parse().map_err(|_| format!("--runs: a number\n{USAGE}"))?,
            "--budget" => o.budget = val()?,
            "--timeout" => o.timeout = val()?.parse().map_err(|_| format!("--timeout: seconds\n{USAGE}"))?,
            "--review" => o.review = true,
            "--no-mcp" => o.no_mcp = true,
            "--max-sims" => o.max_sims = Some(val()?.parse().map_err(|_| format!("--max-sims: a number\n{USAGE}"))?),
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n{USAGE}")),
            s => o.tasks.push(s.to_string()),
        }
    }
    Ok(o)
}

struct Task {
    name: String,
    prompt: String,
    expect: Value,
    ordered: Vec<String>,
    fix: Option<Fix>,
}

/// A fix task: files of an upstream design at a commit (`files`: where each goes in the work
/// directory, and its path upstream, a file or a directory), broken by the task's bug.patch; the
/// rest of the task directory (a Makefile, tb/) is copied next to them. `make sim` fails as
/// shipped; fixed is `make sim` and `make hidden` (a regression the agent never runs) passing
/// with the task's own Makefile and tb/ put back. UPSTREAM, given to make, is `deps`: the
/// checkout without the files under test, so neither the agent nor make sees the reference.
struct Fix {
    dir: PathBuf,
    upstream: String,
    commit: String,
    files: Vec<(String, String)>,
}

fn tasks(root: &Path, names: &[String]) -> Result<Vec<Task>, String> {
    let dir = root.join("bench/agent/tasks");
    let mut all: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter_map(|e| e.path().file_stem()?.to_str().map(String::from))
        .collect();
    all.sort();
    if let Some(bad) = names.iter().find(|n| !all.contains(n)) {
        return Err(format!("no task {bad} (tasks: {})", all.join(", ")));
    }
    let picked = if names.is_empty() { all } else { names.to_vec() };
    picked.into_iter().map(|name| task(&dir, root, name)).collect()
}

fn task(dir: &Path, root: &Path, name: String) -> Result<Task, String> {
    let fix_dir = dir.join(&name);
    let path = if fix_dir.is_dir() { fix_dir.join("task.toml") } else { dir.join(format!("{name}.toml")) };
    let t: toml::Table = std::fs::read_to_string(&path).map_err(|e| e.to_string())?.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    let s = |k: &str| t.get(k).and_then(|v| v.as_str()).map(String::from).ok_or(format!("{}: needs {k}", path.display()));
    let (prompt, fix) = if fix_dir.is_dir() {
        let files: Vec<(String, String)> = t
            .get("files")
            .and_then(|v| v.as_table())
            .map(|f| f.iter().filter_map(|(to, from)| Some((to.clone(), from.as_str()?.to_string()))).collect())
            .unwrap_or_default();
        if files.is_empty() {
            return Err(format!("{}: needs files, a table of work path = upstream path", path.display()));
        }
        let fix = Fix { dir: fix_dir, upstream: s("upstream")?, commit: s("commit")?, files };
        (s("prompt")?, Some(fix))
    } else {
        let trace = root.join(s("trace")?);
        if !trace.exists() {
            return Err(format!("{}: no trace {}", path.display(), trace.display()));
        }
        (s("prompt")?.replace("{trace}", &trace.display().to_string()), None)
    };
    let prompt = format!("{}\nEnd your answer with a ```json block of exactly this shape, with the values you found:\n{}", prompt.trim(), s("answer")?);
    let expect = serde_json::from_str(&s("expect")?).map_err(|e| format!("{}: expect: {e}", path.display()))?;
    let ordered = t.get("ordered").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    Ok(Task { name, prompt, expect, ordered, fix })
}

fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let o = opts(args)?;
    let tasks = tasks(root, &o.tasks)?;
    if o.no_mcp && let Some(t) = tasks.iter().find(|t| t.fix.is_none()) {
        return Err(format!("{}: --no-mcp runs fix tasks only; without wtf an agent has no way to read the trace", t.name));
    }
    let wtf = match &o.wtf {
        Some(p) => PathBuf::from(p),
        None => root.join("target/release/wtf"),
    };
    let wtf = wtf.canonicalize().map_err(|_| format!("{}: no wtf binary; `cargo build --release` first, or give --wtf <path>", wtf.display()))?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let runs = root.join(format!("bench/agent/runs/{stamp}"));
    std::fs::create_dir_all(&runs).map_err(|e| e.to_string())?;
    let version = |cmd: &mut Command| cmd.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let commit = version(Command::new("git").current_dir(root).args(["rev-parse", "--short", "HEAD"]));
    let dirty = !version(Command::new("git").current_dir(root).args(["status", "--porcelain", "--untracked-files=no"])).is_empty();
    let claude = version(Command::new("claude").arg("--version"));
    let mut results = std::fs::OpenOptions::new().create(true).append(true).open(root.join("bench/agent/results.jsonl")).map_err(|e| e.to_string())?;
    println!("task\trun\tpass\tscore\tcalls\terrors\tanswers\ttokens\tpeak\tcost\twall");
    for t in &tasks {
        for i in 1..=o.runs {
            let upstream = t.fix.as_ref().map(|f| checkout(root, f).and_then(|full| Ok((deps(root, f, &t.name, &full)?, full)))).transpose()?;
            let mut row = once(&o, &wtf, t, &runs.join(format!("{}-{i}", t.name)), upstream.as_ref().map(|(d, u)| (d.as_path(), u.as_path())))?;
            let m = row.as_object_mut().expect("a row is an object");
            for (k, v) in [("stamp", json!(stamp)), ("task", json!(t.name)), ("run", json!(i)), ("wtf", json!(commit)), ("dirty", json!(dirty)), ("claude", json!(claude))] {
                m.insert(k.into(), v);
            }
            writeln!(results, "{row}").map_err(|e| e.to_string())?;
            let g = |k: &str| row.get(k).cloned().unwrap_or(Value::Null);
            println!(
                "{}\t{i}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t${:.3}\t{:.0}s{}",
                t.name, g("pass"), g("score"), g("calls"), g("tool_errors"), g("answer_bytes"), g("tokens"), g("peak_context"),
                g("cost_usd").as_f64().unwrap_or(0.0), g("wall_s").as_f64().unwrap_or(0.0),
                row.get("failure").map_or(String::new(), |f| format!("\t{f}"))
            );
        }
    }
    println!("transcripts in {}", runs.display());
    Ok(())
}

/// One run of a task: the agent in an empty directory, with only wtf's tools (no shell, no file
/// reads, so it can't read the trace itself), none of the user's settings, hooks or CLAUDE.md.
/// A fix task's agent works in the task's work directory instead, and may also read and edit
/// files there (Claude Code's default: reads and, with acceptEdits, edits inside the working
/// directory only) and run `make sim`; `upstream` is (deps, the checkout): make gets deps, the
/// checkout is only read here, for the work files and the diff.
fn once(o: &Opts, wtf: &Path, t: &Task, out: &Path, upstream: Option<(&Path, &Path)>) -> Result<Value, String> {
    let tmp = std::env::temp_dir().join(format!("wtf-agent-{}-{}", std::process::id(), out.file_name().unwrap_or_default().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("run")).map_err(|e| e.to_string())?;
    let rt = tmp.join("run").display().to_string();
    let mcp = json!({"mcpServers": {"wtf": {"type": "stdio", "command": wtf, "args": ["--mcp"], "env": {"XDG_RUNTIME_DIR": rt, "XDG_CONFIG_HOME": rt}}}});
    let config = tmp.join("mcp.json");
    std::fs::write(&config, mcp.to_string()).map_err(|e| e.to_string())?;
    let work = tmp.join("work");
    let fix = t.fix.as_ref().zip(upstream);
    if let Some((f, (deps, up))) = fix {
        prepare(f, up, &work, false)?;
        if make(&work, deps, "sim")? {
            return Err(format!("{}: `make sim` passes with the bug; `wtf-bench agent check {}`", t.name, t.name));
        }
    }
    let sims = tmp.join("sims");
    if fix.is_some() {
        shim(&tmp.join("bin"), &sims, o.max_sims)?;
    }
    let claude = |prompt: &str, resume: Option<&str>| {
        let mut c = Command::new("claude");
        c.args(["-p", prompt, "--output-format", "stream-json", "--verbose", "--strict-mcp-config", "--setting-sources", ""]);
        match fix {
            Some((_, (deps, _))) => c.current_dir(&work).env("UPSTREAM", deps).env("PATH", path_with(&tmp.join("bin"))).args(["--tools", "Read,Edit,Grep,Bash", "--permission-mode", "acceptEdits", "--allowedTools", "mcp__wtf", "Bash(make sim:*)"]),
            None => c.current_dir(&tmp).args(["--tools", "", "--allowedTools", "mcp__wtf"]),
        };
        if !o.no_mcp {
            c.arg("--mcp-config").arg(&config);
        }
        if o.budget != "none" {
            c.args(["--max-budget-usd", &o.budget]);
        }
        if let Some(m) = &o.model {
            c.args(["--model", m]);
        }
        if let Some(id) = resume {
            c.args(["--resume", id]);
        }
        // a review resumes the session, so only then is it kept
        if !o.review {
            c.arg("--no-session-persistence");
        }
        c
    };
    let start = Instant::now();
    let prompt = match o.max_sims.filter(|_| fix.is_some()) {
        Some(n) => format!("You can run `make sim` at most {n} times.\n{}", t.prompt),
        None => t.prompt.clone(),
    };
    let (lines, failure) = stream(claude(&prompt, None), Duration::from_secs(o.timeout))?;
    let wall = start.elapsed().as_secs_f64();
    std::fs::write(out.with_extension("jsonl"), lines.join("\n")).map_err(|e| e.to_string())?;
    let events: Vec<Value> = lines.iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let mut row = metrics(&events);
    let m = row.as_object_mut().expect("metrics are an object");
    m.insert("wall_s".into(), json!((wall * 10.0).round() / 10.0));
    if o.no_mcp {
        m.insert("mcp".into(), json!("off"));
    }
    if fix.is_some() {
        // the runs make let through, from the shim's count of asks
        let asked: u64 = std::fs::read_to_string(&sims).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        m.insert("sims".into(), json!(o.max_sims.map_or(asked, |n| asked.min(n))));
        m.insert("max_sims".into(), json!(o.max_sims));
    }
    let answer = m.get("answer").and_then(Value::as_str).unwrap_or("").to_string();
    let (score, mut pass, keys) = grade(&answer, &t.expect, &t.ordered);
    if let Some((f, (deps, up))) = fix {
        // the agent's RTL with the task's own Makefile and testbench
        copy_task(f, &work)?;
        let fixed = make(&work, deps, "sim")? && make(&work, deps, "hidden")?;
        let _ = std::fs::write(out.with_extension("diff"), changes(f, up, &work));
        m.insert("fixed".into(), json!(fixed));
        pass &= fixed;
    }
    m.insert("score".into(), json!(score));
    m.insert("pass".into(), json!(pass));
    m.insert("keys".into(), keys);
    if let Some(f) = failure {
        m.insert("failure".into(), json!(f));
    }
    if o.review
        && let Some(id) = m.get("session").and_then(Value::as_str).map(String::from)
    {
        let (lines, _) = stream(claude(REVIEW, Some(&id)), Duration::from_secs(o.timeout))?;
        let review: Vec<Value> = lines.iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let text = review.iter().rev().find_map(|e| e.get("result").and_then(Value::as_str)).unwrap_or("(no review)");
        std::fs::write(out.with_extension("review.md"), text).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(row)
}

/// The upstream checkout at the task's commit, in target/upstream/ (fetched once).
fn checkout(root: &Path, f: &Fix) -> Result<PathBuf, String> {
    let dir = root.join("target/upstream").join(&f.commit);
    if f.files.iter().all(|(_, from)| dir.join(from).exists()) {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for args in [vec!["init", "-q"], vec!["fetch", "-q", "--depth", "1", &f.upstream, &f.commit], vec!["checkout", "-q", "FETCH_HEAD"]] {
        let ok = Command::new("git").current_dir(&dir).args(&args).status().map_err(|e| format!("git: {e}"))?.success();
        if !ok {
            return Err(format!("git {} failed in {}", args.join(" "), dir.display()));
        }
    }
    Ok(dir)
}

/// A `make` first on the agent's PATH, in `bin` (outside the work directory, out of the agent's
/// reach): it counts `make sim` in `count` and, past `max`, refuses with a message to answer.
fn shim(bin: &Path, count: &Path, max: Option<u64>) -> Result<(), String> {
    let make = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join("make"))
        .find(|p| p.is_file())
        .ok_or("no make on PATH")?;
    let refuse = match max {
        Some(n) => format!("  if [ $n -gt {n} ]; then echo \"make sim: the budget of {n} runs is spent; finish with what you have\" >&2; exit 2; fi\n"),
        None => String::new(),
    };
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *\" sim \"*)\n  n=$(( $(cat '{c}' 2>/dev/null || echo 0) + 1 )); echo $n > '{c}'\n{refuse};;\nesac\nexec '{m}' \"$@\"\n",
        c = count.display(),
        m = make.display()
    );
    std::fs::create_dir_all(bin).map_err(|e| e.to_string())?;
    let path = bin.join("make");
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

/// PATH with `dir` first.
fn path_with(dir: &Path) -> std::ffi::OsString {
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&rest))).unwrap_or(rest)
}

/// The checkout as make sees it, in target/upstream/<commit>-<task>/, rebuilt each time:
/// hardlinks, without .git and without the HDL sources under the task's `files`, so the correct
/// files can't be read or restored from it; the rest (a cocotb model beside them) stays.
fn deps(root: &Path, f: &Fix, task: &str, full: &Path) -> Result<PathBuf, String> {
    let dir = root.join("target/upstream").join(format!("{}-{task}", f.commit));
    let _ = std::fs::remove_dir_all(&dir);
    let ok = Command::new("cp").arg("-al").arg(full).arg(&dir).status().map_err(|e| format!("cp: {e}"))?.success();
    if !ok {
        return Err(format!("linking {} to {} failed", full.display(), dir.display()));
    }
    std::fs::remove_dir_all(dir.join(".git")).map_err(|e| format!("{}: {e}", dir.join(".git").display()))?;
    for (_, from) in &f.files {
        let ok = Command::new("find")
            .arg(dir.join(from))
            .args(["-type", "f", "(", "-name", "*.vhd", "-o", "-name", "*.vhdl", "-o", "-name", "*.v", "-o", "-name", "*.sv", ")", "-delete"])
            .status()
            .map_err(|e| format!("find: {e}"))?
            .success();
        if !ok {
            return Err(format!("removing the HDL under {from} from {} failed", dir.display()));
        }
    }
    Ok(dir)
}

/// The task's files (all but task.toml and bug.patch) copied into `work`, over what is there.
fn copy_task(f: &Fix, work: &Path) -> Result<(), String> {
    for e in std::fs::read_dir(&f.dir).map_err(|e| e.to_string())?.flatten() {
        if ["task.toml", "bug.patch"].contains(&e.file_name().to_str().unwrap_or("")) {
            continue;
        }
        let to = work.join(e.file_name());
        let _ = std::fs::remove_dir_all(&to).or_else(|_| std::fs::remove_file(&to));
        let ok = Command::new("cp").arg("-r").arg(e.path()).arg(&to).status().map_err(|e| format!("cp: {e}"))?.success();
        if !ok {
            return Err(format!("copying {} failed", e.path().display()));
        }
    }
    Ok(())
}

/// A fresh work directory: the task's files, the upstream file, with the bug unless `fixed`.
fn prepare(f: &Fix, upstream: &Path, work: &Path, fixed: bool) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    copy_task(f, work)?;
    for (to, from) in &f.files {
        let to = work.join(to);
        std::fs::create_dir_all(to.parent().unwrap_or(work)).map_err(|e| e.to_string())?;
        let ok = Command::new("cp").arg("-r").arg(upstream.join(from)).arg(&to).status().map_err(|e| format!("cp: {e}"))?.success();
        if !ok {
            return Err(format!("copying {from} from the upstream checkout failed"));
        }
    }
    if !fixed {
        let ok = Command::new("git").current_dir(work).arg("apply").arg(f.dir.join("bug.patch")).status().map_err(|e| format!("git: {e}"))?.success();
        if !ok {
            return Err(format!("{}: bug.patch doesn't apply", f.dir.display()));
        }
    }
    Ok(())
}

/// What the agent changed in the upstream files, as a diff.
fn changes(f: &Fix, upstream: &Path, work: &Path) -> String {
    f.files
        .iter()
        .filter_map(|(to, from)| Command::new("git").args(["diff", "--no-index", "--"]).arg(upstream.join(from)).arg(work.join(to)).output().ok())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .collect()
}

/// `make <target>` in the work directory; whether it passed (its output is in the target's logs).
fn make(work: &Path, upstream: &Path, target: &str) -> Result<bool, String> {
    let status = Command::new("make")
        .current_dir(work)
        .arg(target)
        .arg(format!("UPSTREAM={}", upstream.display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("make: {e}"))?;
    Ok(status.success())
}

/// Prove a fix task without an agent: with the bug `make sim` and `make hidden` fail, with the
/// upstream file they pass.
fn check(root: &Path, names: &[String]) -> Result<(), String> {
    let tasks: Vec<Task> = tasks(root, names)?.into_iter().filter(|t| t.fix.is_some()).collect();
    if tasks.is_empty() {
        return Err("no fix tasks to check".into());
    }
    for t in &tasks {
        let f = t.fix.as_ref().expect("filtered");
        let upstream = checkout(root, f)?;
        let deps = deps(root, f, &t.name, &upstream)?;
        let work = std::env::temp_dir().join(format!("wtf-check-{}-{}", std::process::id(), t.name));
        for fixed in [false, true] {
            prepare(f, &upstream, &work, fixed)?;
            for target in ["sim", "hidden"] {
                let start = Instant::now();
                let passed = make(&work, &deps, target)?;
                let verdict = if passed == fixed { "ok" } else { "WRONG" };
                println!("{}\t{}\t{target}\t{}\t{verdict}\t{:.0}s", t.name, if fixed { "upstream" } else { "bug" }, if passed { "passes" } else { "fails" }, start.elapsed().as_secs_f64());
                if passed != fixed {
                    return Err(format!("{}: {target} {} with {}; its logs are in {}", t.name, if passed { "passes" } else { "fails" }, if fixed { "the upstream file" } else { "the bug" }, work.display()));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(())
}

/// The command's stream-json lines; killed after the timeout, which is then the failure.
fn stream(mut cmd: Command, timeout: Duration) -> Result<(Vec<String>, Option<String>), String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("claude: {e}"))?;
    let stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || BufReader::new(stdout).lines().map_while(Result::ok).collect::<Vec<_>>());
    let start = Instant::now();
    let mut failure = None;
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            failure = Some(format!("timeout after {}s", timeout.as_secs()));
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let lines = reader.join().map_err(|_| "reading claude's output failed")?;
    if failure.is_none() && !status.success() && lines.is_empty() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut e, &mut err);
        }
        return Err(format!("claude failed: {}", err.trim()));
    }
    Ok((lines, failure))
}

/// What a run cost, from its stream-json events.
fn metrics(events: &[Value]) -> Value {
    let mut calls = Map::new();
    let (mut n, mut errors, mut bytes, mut largest, mut peak, mut other, mut denials) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut seen = std::collections::HashSet::new();
    let mut wtf_ids = std::collections::HashSet::new();
    let mut presented = false;
    let content = |e: &Value| e.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
    for e in events {
        match e.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                // one event per content block, each with its message's usage: count a message once
                let id = e.pointer("/message/id").and_then(Value::as_str).unwrap_or("").to_string();
                if seen.insert(id) {
                    let u = |k: &str| e.pointer(&format!("/message/usage/{k}")).and_then(Value::as_u64).unwrap_or(0);
                    peak = peak.max(u("input_tokens") + u("cache_read_input_tokens") + u("cache_creation_input_tokens"));
                }
                for b in content(e).iter().filter(|b| b["type"] == "tool_use") {
                    let name = b["name"].as_str().unwrap_or("?");
                    match name.strip_prefix("mcp__wtf__") {
                        Some(tool) => {
                            n += 1;
                            *calls.entry(tool).or_insert(json!(0)) = json!(calls.get(tool).and_then(Value::as_u64).unwrap_or(0) + 1);
                            presented |= tool == "present";
                            wtf_ids.insert(b["id"].as_str().unwrap_or("").to_string());
                        }
                        None => other += 1,
                    }
                }
            }
            Some("user") => {
                for b in content(e).iter().filter(|b| b["type"] == "tool_result") {
                    let text = match &b["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(a) => a.iter().filter_map(|c| c["text"].as_str()).collect(),
                        _ => String::new(),
                    };
                    if !wtf_ids.contains(b["tool_use_id"].as_str().unwrap_or("")) {
                        // the sandbox saying no, apart from the agent's own failures
                        let denied = ["approval", "permission", "was blocked"].iter().any(|w| text.contains(w));
                        denials += (b["is_error"] == true && denied) as u64;
                        continue;
                    }
                    let size = text.len() as u64;
                    bytes += size;
                    largest = largest.max(size);
                    errors += b["is_error"].as_bool().unwrap_or(false) as u64;
                }
            }
            _ => {}
        }
    }
    let init = events.iter().find(|e| e["type"] == "system" && e["subtype"] == "init");
    let result = events.iter().rev().find(|e| e["type"] == "result").cloned().unwrap_or(Value::Null);
    let u = |k: &str| result.pointer(&format!("/usage/{k}")).and_then(Value::as_u64).unwrap_or(0);
    let (input, output, read, write) = (u("input_tokens"), u("output_tokens"), u("cache_read_input_tokens"), u("cache_creation_input_tokens"));
    let wtf_status = init.and_then(|i| i["mcp_servers"].as_array()).and_then(|s| s.iter().find(|s| s["name"] == "wtf")).and_then(|s| s["status"].as_str()).unwrap_or("?");
    json!({
        "model": init.and_then(|i| i["model"].as_str()).unwrap_or("?"),
        "session": result["session_id"],
        "mcp": wtf_status,
        "outcome": result["subtype"],
        "turns": result["num_turns"],
        "calls": n,
        "calls_by_tool": calls,
        "tool_errors": errors,
        "other_tools": other,
        "denials": denials,
        "answer_bytes": bytes,
        "largest_answer": largest,
        "presented": presented,
        "tokens": input + output + read + write,
        "input_tokens": input,
        "output_tokens": output,
        "cache_read": read,
        "cache_write": write,
        "peak_context": peak,
        "cost_usd": result["total_cost_usd"],
        "api_s": result["duration_api_ms"].as_f64().map(|ms| (ms / 100.0).round() / 10.0),
        "answer": result["result"],
    })
}

/// The answer's last ```json block against the expected one, key by key: (share right, all
/// right, each key's verdict). Hex and decimal numbers are the same number, case and a `ns`
/// suffix don't matter, and lists are compared as sets unless the task orders them.
fn grade(answer: &str, expect: &Value, ordered: &[String]) -> (f64, bool, Value) {
    let block = answer.rsplit_once("```json").and_then(|(_, b)| b.split_once("```")).map(|(b, _)| b);
    let got: Option<Value> = block.and_then(|b| serde_json::from_str(b).ok());
    let Some(exp) = expect.as_object() else { return (0.0, false, json!({})) };
    let keys: Map<String, Value> = exp
        .iter()
        .map(|(k, v)| {
            let sort = !ordered.contains(k);
            let ok = got.as_ref().and_then(|g| g.get(k)).is_some_and(|g| canon(g, sort) == canon(v, sort));
            (k.clone(), json!(ok))
        })
        .collect();
    let right = keys.values().filter(|v| v.as_bool() == Some(true)).count();
    let score = (right as f64 / exp.len().max(1) as f64 * 100.0).round() / 100.0;
    (score, right == exp.len(), Value::Object(keys))
}

fn canon(v: &Value, sort: bool) -> Value {
    match v {
        Value::String(s) => {
            let s = s.trim().to_lowercase();
            let s = s.strip_suffix("ns").unwrap_or(&s).trim();
            let n = match s.strip_prefix("0x") {
                Some(h) => u64::from_str_radix(h, 16).ok(),
                None => s.parse().ok(),
            };
            n.map_or(Value::String(s.to_string()), Value::from)
        }
        Value::Number(n) => n.as_u64().map_or(v.clone(), Value::from),
        Value::Array(a) => {
            let mut a: Vec<Value> = a.iter().map(|x| canon(x, sort)).collect();
            if sort {
                a.sort_by_key(|x| x.to_string());
            }
            Value::Array(a)
        }
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (canon(&json!(k), false).to_string().trim_matches('"').to_string(), canon(x, sort))).collect()),
        other => other.clone(),
    }
}

/// The results so far, averaged per task, wtf commit and model, oldest first.
fn report(root: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(root.join("bench/agent/results.jsonl")).map_err(|_| "no results yet: `wtf-bench agent run` first")?;
    let rows: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    type Key = (String, String, String, &'static str, String); // task, wtf commit, model, mcp, sim budget
    let mut groups: Vec<(Key, Vec<&Value>)> = Vec::new();
    for r in &rows {
        let s = |k: &str| r[k].as_str().unwrap_or("?").to_string();
        let budget = r["max_sims"].as_u64().map_or("-".into(), |n| n.to_string());
        let key = (s("task"), format!("{}{}", s("wtf"), if r["dirty"] == true { "+" } else { "" }), s("model"), if r["mcp"] == "off" { "off" } else { "on" }, budget);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(r),
            None => groups.push((key, vec![r])),
        }
    }
    groups.sort_by(|a, b| a.0.0.cmp(&b.0.0));
    // fixed: the hidden test passed (fix tasks); evidence: the answer's keys right; costs are medians
    println!("task\twtf\tmodel\tmcp\tbudget\truns\tpass\tfixed\tevidence\tcalls\terrors\tdenials\tother\tsims\tanswers\ttokens\tpeak\tcost\twall");
    for ((task, wtf, model, mcp, budget), g) in groups {
        // rows from before a metric existed don't have it: `-`
        let mean = |k: &str| {
            let v: Vec<f64> = g.iter().filter_map(|r| r[k].as_f64()).collect();
            if v.is_empty() { f64::NAN } else { v.iter().sum::<f64>() / v.len() as f64 }
        };
        let median = |k: &str| {
            let mut v: Vec<f64> = g.iter().filter_map(|r| r[k].as_f64()).collect();
            v.sort_by(f64::total_cmp);
            match v.len() {
                0 => f64::NAN,
                n if n % 2 == 1 => v[n / 2],
                n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
            }
        };
        let count = |k: &str| g.iter().filter(|r| r[k] == true).count();
        let fixed = if g.iter().any(|r| r.get("fixed").is_some()) { format!("{}/{}", count("fixed"), g.len()) } else { "-".into() };
        let f = |x: f64, decimals: usize| if x.is_nan() { "-".to_string() } else { format!("{x:.decimals$}") };
        println!(
            "{task}\t{wtf}\t{model}\t{mcp}\t{budget}\t{}\t{}/{}\t{fixed}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t${}\t{}s",
            g.len(), count("pass"), g.len(), f(mean("score"), 2), f(mean("calls"), 1), f(mean("tool_errors"), 1), f(mean("denials"), 1),
            f(mean("other_tools"), 1), f(median("sims"), 0), f(median("answer_bytes"), 0), f(median("tokens"), 0), f(median("peak_context"), 0),
            f(median("cost_usd"), 3), f(median("wall_s"), 0)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_every_task() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the repository");
        let all = tasks(root, &[]).expect("tasks load");
        let fix = all.iter().find(|t| t.name == "head-trimmer-boundary").and_then(|t| t.fix.as_ref()).expect("a fix task");
        assert!(fix.dir.join("bug.patch").exists());
        assert!(all.iter().any(|t| t.name == "axis-frames" && t.fix.is_none()));
    }

    #[test]
    fn grades_by_key() {
        let expect = json!({"writes": [{"addr": "0x10", "data": "0x11111111"}], "frames": [10, 4], "n": 1, "lat": {"1": 40}});
        let ordered = vec!["frames".to_string()];
        // hex case, decimal for hex, a `ns` suffix, list order where unordered: all the same
        let answer = "Found them.\n```json\n{\"writes\": [{\"data\": \"0X11111111\", \"addr\": 16}], \"frames\": [10, 4], \"n\": \"1\", \"lat\": {\"1\": \"40ns\"}}\n```";
        assert_eq!(grade(answer, &expect, &ordered), (1.0, true, json!({"writes": true, "frames": true, "n": true, "lat": true})));
        // an ordered list out of order, a missing key; the last block counts
        let answer = "```json\n{}\n```\n```json\n{\"writes\": [{\"addr\": \"0x10\", \"data\": \"0x11111111\"}], \"frames\": [4, 10], \"n\": 1}\n```";
        assert_eq!(grade(answer, &expect, &ordered), (0.5, false, json!({"writes": true, "frames": false, "n": true, "lat": false})));
        assert_eq!(grade("no json here", &expect, &ordered).0, 0.0);
    }

    #[test]
    fn make_shim_counts_and_refuses() {
        let dir = std::env::temp_dir().join(format!("wtf-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");
        std::fs::write(dir.join("Makefile"), "sim:\n\t@true\nbuild:\n\t@true\n").expect("a Makefile");
        shim(&dir.join("bin"), &dir.join("sims"), Some(1)).expect("the shim");
        let make = |target: &str| Command::new(dir.join("bin/make")).current_dir(&dir).arg(target).output().expect("make runs");
        assert!(make("sim").status.success());
        assert!(make("build").status.success(), "only sim counts");
        let refused = make("sim");
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("budget of 1 runs is spent"));
        assert_eq!(std::fs::read_to_string(dir.join("sims")).expect("the count").trim(), "2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn metrics_from_events() {
        let events: Vec<Value> = [
            json!({"type": "system", "subtype": "init", "model": "m", "mcp_servers": [{"name": "wtf", "status": "connected"}]}),
            json!({"type": "assistant", "message": {"id": "a", "usage": {"input_tokens": 10, "cache_read_input_tokens": 100}, "content": [{"type": "tool_use", "id": "t1", "name": "mcp__wtf__find"}]}}),
            json!({"type": "assistant", "message": {"id": "a", "usage": {"input_tokens": 10, "cache_read_input_tokens": 100}, "content": [{"type": "tool_use", "id": "t2", "name": "mcp__wtf__present"}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "abcd"}, {"type": "tool_result", "tool_use_id": "t2", "content": [{"type": "text", "text": "xy"}], "is_error": true}]}}),
            json!({"type": "assistant", "message": {"id": "c", "usage": {"input_tokens": 5}, "content": [{"type": "tool_use", "id": "t3", "name": "Bash"}, {"type": "tool_use", "id": "t4", "name": "Bash"}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t3", "content": "cp in '/tmp/x' needs approval", "is_error": true}, {"type": "tool_result", "tool_use_id": "t4", "content": "Exit code 2", "is_error": true}]}}),
            json!({"type": "assistant", "message": {"id": "b", "usage": {"input_tokens": 5, "cache_read_input_tokens": 200}, "content": [{"type": "text", "text": "done"}]}}),
            json!({"type": "result", "subtype": "success", "num_turns": 2, "total_cost_usd": 0.01, "duration_api_ms": 1500, "result": "done", "session_id": "s",
                   "usage": {"input_tokens": 15, "output_tokens": 7, "cache_read_input_tokens": 300, "cache_creation_input_tokens": 0}}),
        ]
        .into();
        let m = metrics(&events);
        assert_eq!((m["calls"].clone(), m["calls_by_tool"].clone(), m["tool_errors"].clone()), (json!(2), json!({"find": 1, "present": 1}), json!(1)));
        assert_eq!((m["answer_bytes"].clone(), m["largest_answer"].clone(), m["presented"].clone()), (json!(6), json!(4), json!(true)));
        assert_eq!((m["other_tools"].clone(), m["denials"].clone()), (json!(2), json!(1)));
        assert_eq!((m["tokens"].clone(), m["peak_context"].clone(), m["mcp"].clone(), m["api_s"].clone()), (json!(322), json!(205), json!("connected"), json!(1.5)));
    }
}
