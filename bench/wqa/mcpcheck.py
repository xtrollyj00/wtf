"""Every signal path a question names resolves in wtf (calls `wtf --mcp` over stdio)."""
import json, re, subprocess, sys, os, tempfile

wtf, vcds, qs = sys.argv[1], sys.argv[2], sys.argv[3]
rt = tempfile.mkdtemp()
p = subprocess.Popen([wtf, "--mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env={**os.environ, "XDG_RUNTIME_DIR": rt, "XDG_CONFIG_HOME": rt})
n = 0
def call(method, params=None, note=False):
    global n
    n += 1
    msg = {"jsonrpc": "2.0", "method": method, **({"params": params} if params is not None else {})}
    if not note: msg["id"] = n
    p.stdin.write(json.dumps(msg) + "\n"); p.stdin.flush()
    if note: return None
    while True:
        r = json.loads(p.stdout.readline())
        if r.get("id") == n: return r
def tool(name, args):
    r = call("tools/call", {"name": name, "arguments": args})
    res = r.get("result", {})
    return res.get("isError", False), " ".join(c.get("text", "") for c in res.get("content", []))
call("initialize", {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "check", "version": "0"}})
call("notifications/initialized", {}, note=True)
opened = None
for line in open(qs):
    q = json.loads(line)
    if q["trace"] != opened:
        err, out = tool("open", {"trace": f"{vcds}/{q['trace']}"})
        print("open", q["trace"], "ERR " + out if err else "ok"); opened = q["trace"]
    names = sorted(set(re.findall(r"\b(?:testbench|TOP|tb_top)\.[\w.\[\]]+\w\]?", q["question"])))
    err, out = tool("values", {"signals": names, "from": re.search(r"t=(\d+\w+)", q["question"]).group(1)})
    print(f"  {q['id']:24} {'ERR' if err else 'ok '} {out[:300] if err else ''}")
p.terminate()
