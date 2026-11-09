"""Run WaveformQA-style questions through `claude -p`, one arm at a time, and score them.

  run.py ARM QUESTIONS VCDS OUT [--model haiku] [--budget 0.3] [--jobs 4] [--wtf PATH] [--ids a,b]

ARM: wtf (only wtf's MCP tools) or shell (Bash and Read, no wtf). OUT gets one JSON line per
question: the answer given, right or not, cost, turns, tool calls and wtf errors."""

import argparse
import concurrent.futures
import json
import os
import re
import subprocess
import tempfile
import time

PROMPT = """The file {path} is a VCD waveform from a simulation of a RISC-V core. Answer this question about it:

{question}

Times are in the units written (2575320ps is 2575320 ps). Write values in hex (0x1f), counts and times as plain numbers, lists comma-separated, signals by their full path. {how} End your reply with one line: ANSWER: <answer>"""

HOW = {"wtf": "Read the waveform with the wtf MCP tools.", "shell": "Read the waveform with the shell."}


def number(tok):
    t = tok.strip().strip("`'\".").lower()
    t = re.sub(r"\s*(ps|ns|us|ms|s|mhz|hz)$", "", t)
    try:
        if t.startswith("0x"):
            return int(t, 16)
        if t.startswith("'h"):
            return int(t[2:], 16)
        if t.startswith("0b") or t.startswith("'b"):
            return int(t[2:], 2)
        return float(t) if "." in t or "e" in t else int(t)
    except ValueError:
        return None


def right(kind, want, got):
    if got is None:
        return False
    if kind == "value":
        return number(got) is not None and number(got) == number(want)
    if kind == "number":
        g, w = number(got), float(want)
        return g is not None and abs(g - w) <= max(abs(w) * 0.01, 1e-12)
    if kind == "word":
        g = got.strip().lower()
        g = "same" if "same" in g else g.split()[0].strip(".,") if g else g
        return g == want
    if kind == "list":
        g = [number(x) for x in got.split(",")]
        return g == [number(x) for x in want.split(",")]
    if kind == "names":
        g = [x.strip().strip("`'\".") for x in got.split(",")]
        w = [x.strip() for x in want.split(",")]
        return len(g) == len(w) and all(a == b or b.endswith("." + a) for a, b in zip(g, w))
    raise ValueError(kind)


def run(q, a, vcds, wtf):
    tmp = tempfile.mkdtemp(prefix=f"wqa-{a.arm}-")
    trace = os.path.join(tmp, q["trace"])
    os.symlink(os.path.join(vcds, q["trace"]), trace)
    prompt = PROMPT.format(path=trace, question=q["question"], how=HOW[a.arm])
    cmd = ["claude", "-p", prompt, "--output-format", "stream-json", "--verbose", "--strict-mcp-config", "--setting-sources", "",
           "--no-session-persistence", "--model", a.model, "--max-budget-usd", str(a.budget)]
    env = dict(os.environ)
    if a.arm == "wtf":
        rt = os.path.join(tmp, "rt")
        os.mkdir(rt)
        cfg = os.path.join(tmp, "mcp.json")
        json.dump({"mcpServers": {"wtf": {"type": "stdio", "command": wtf, "args": ["--mcp"], "env": {"XDG_RUNTIME_DIR": rt, "XDG_CONFIG_HOME": rt}}}}, open(cfg, "w"))
        cmd += ["--mcp-config", cfg, "--tools", "", "--allowedTools", "mcp__wtf"]
        env["ENABLE_TOOL_SEARCH"] = "false"
    else:
        cmd += ["--tools", "Bash,Read", "--allowedTools", "Bash", "Read"]
    start = time.time()
    try:
        p = subprocess.run(cmd, cwd=tmp, env=env, capture_output=True, text=True, timeout=a.timeout)
        out, timeout = p.stdout, False
    except subprocess.TimeoutExpired as e:
        out, timeout = (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), True
    wall = time.time() - start
    open(os.path.join(a.logs, f"{q['id']}.jsonl"), "w").write(out)
    result, calls, errors, ids = None, [], [], {}
    for line in out.splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("type") == "result":
            result = d
        m = d.get("message")
        for c in (m.get("content") or []) if isinstance(m, dict) else []:
            if not isinstance(c, dict):
                continue
            if c.get("type") == "tool_use":
                ids[c["id"]] = c["name"]
                calls.append(c["name"].removeprefix("mcp__wtf__"))
            if c.get("type") == "tool_result" and c.get("is_error") and ids.get(c.get("tool_use_id"), "").startswith("mcp__wtf__"):
                t = c.get("content")
                t = t if isinstance(t, str) else " ".join(x.get("text", "") for x in t or [] if isinstance(x, dict))
                errors.append(t[:200])
    text = (result or {}).get("result") or ""
    m = re.findall(r"ANSWER:\s*(.+)", text)
    got = m[-1].strip() if m else None
    return {"id": q["id"], "category": q["category"], "subcategory": q["subcategory"], "trace": q["trace"], "arm": a.arm, "model": a.model,
            "want": q["answer"], "got": got, "right": right(q["kind"], q["answer"], got), "timeout": timeout,
            "cost": (result or {}).get("total_cost_usd", 0), "turns": (result or {}).get("num_turns"), "wall": round(wall),
            "calls": calls, "wtf_errors": errors, "end": (result or {}).get("subtype")}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("arm", choices=["wtf", "shell"])
    ap.add_argument("questions")
    ap.add_argument("vcds")
    ap.add_argument("out")
    ap.add_argument("--model", default="haiku")
    ap.add_argument("--budget", type=float, default=0.3)
    ap.add_argument("--timeout", type=int, default=600)
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--wtf", default=os.path.abspath("target/release/wtf"))
    ap.add_argument("--ids")
    a = ap.parse_args()
    qs = [json.loads(l) for l in open(a.questions)]
    if a.ids:
        qs = [q for q in qs if q["id"] in a.ids.split(",")]
    a.logs = a.out + ".logs"
    os.makedirs(a.logs, exist_ok=True)
    vcds = os.path.abspath(a.vcds)
    with open(a.out, "a") as f, concurrent.futures.ThreadPoolExecutor(a.jobs) as ex:
        for row in ex.map(lambda q: run(q, a, vcds, a.wtf), qs):
            f.write(json.dumps(row) + "\n")
            f.flush()
            print(f"{row['id']:26} {'RIGHT' if row['right'] else 'wrong'} ${row['cost']:.3f} {row['wall']}s calls={len(row['calls'])} errs={len(row['wtf_errors'])} got={row['got']!r} want={row['want']!r}", flush=True)


if __name__ == "__main__":
    main()
