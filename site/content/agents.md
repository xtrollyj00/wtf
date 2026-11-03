+++
title = "For agents"
description = "Let an AI coding agent debug your simulation traces with wtf --mcp: the benchmark, the setup, and how to watch it and take over."
+++

`wtf --mcp` lets an AI agent, such as Claude Code, open your traces and read them with the same
decoders, searches and records you use in the window. The agent answers from decoded values, and
when it has found something, it shows the evidence in your open window.

## What it measured

The benchmark gave Claude Haiku (`claude-haiku-5-5`) four failing RTL testbenches to debug and fix. Each task
ran five times with wtf and five times without it. Without wtf, the agent had the RTL, the
testbench, the simulation log and `make sim`, and worked with prints and reruns. With wtf, it also
had the MCP server.

{{ <bench caption="Each task five times with wtf and five without. The table below has every number." /> }}

| task | wtf | pass | fixed | evidence | cost (median) | wall (median) |
|---|---|---|---|---|---|---|
| eth-parser-headers | on | 5/5 | 5/5 | 1.00 | $0.029 | 134 s |
| | off | 2/5 | 5/5 | 0.60 | $0.058 | 270 s |
| eth-parser-stall | on | 5/5 | 5/5 | 1.00 | $0.014 | 76 s |
| | off | 5/5 | 5/5 | 1.00 | $0.024 | 136 s |
| head-trimmer-boundary | on | 5/5 | 5/5 | 1.00 | $0.012 | 85 s |
| | off | 3/5 | 5/5 | 0.80 | $0.024 | 203 s |
| axil2mi-stale-addr | on | 2/5 | 5/5 | 0.80 | $0.015 | 62 s |
| | off | 1/5 | 5/5 | 0.46 | $0.015 | 57 s |

`pass` counts the runs that fixed the bug and had every answer key right. `fixed` counts the runs
that fixed the bug. `evidence` is the share of answer keys right. The table leaves out `make sim`
runs, denied commands and tokens, which are in
[`bench/RESULTS.md`](https://github.com/xtrollyj00/wtf/blob/main/bench/RESULTS.md).

Every run fixed its bug, with wtf or without. These bugs can be found from the log and the code,
so the benchmark measures what wtf adds beyond the fix. On the four tasks, 17 of 20 runs with wtf
had every answer key right, and 11 of 20 without it. Without wtf, the agents could not read the
`trace.fst` files, so most of their times were `unknown`. With wtf they gave the times.

A fifth task, `ibex-checksum`, is harder: a store decoding bug in a 10k-line Ibex core, seen only as
a wrong checksum 10 000 cycles later. With wtf, 3 of 5 runs had every answer key right against
1 of 5 without (evidence 0.75 against 0.40), but 4 of 5 fixed the bug against 5 of 5: one run hit
the 600 s timeout. Median cost was $0.133 with wtf and $0.139 without, wall time 277 s and 242 s.
These runs used wtf at `916c847` (2026-10-26); the four above, `d0ebd30` (2026-10-25).

To run the benchmark yourself, from a checkout:

```sh
cargo build --release && cargo run -p wtf-bench --release -- agent run
```

It costs money, because it runs `claude -p` on each task.

## Why the answers are exact

The agent decodes the bus first. `decode bus axilite` turns each AXI4-Lite transaction into a
record with its start and end, its address, data and response, and a check for each protocol
rule. An agent writing its own search has to get the handshake right itself; the decoder has
it right already, and wtf's tests check it.

Then the agent reads the records with `records`, filtered by `where`. The answer lists each
failing read with its time and values, taken from the trace.

## A session on the demo trace

The example below uses `docs/demo.vcd` from a clone of the repository, copied to `sim/axi_tb.vcd`.
It is the sequence the agent ran for the question "the second read fails, why?". The first line of each call is the tool and
its arguments, and the lines under it are the answer.

```
open {"trace": "sim/axi_tb.vcd"}
workspace <id> (the user's wtf windows on this trace see your steps; :follow there shows the workspace)
axi_tb.vcd
0ns - 240ns
1 tick = 1ns
19 signals
interfaces a built-in protocol decodes (decode them: exact records, handshake checks built in):
  top.s_axi_* axilite: decode s_axi axilite clock=top.aclk prefix=top.s_axi_

command {"line": "decode s_axi axilite clock=top.aclk prefix=top.s_axi_"}
s_axi: records s_axi.wr 1, s_axi.rd 2; violations s_axi.errors 1, s_axi.wr.unmatched 0, s_axi.rd.unmatched 0; made from s_axi.aw 1, s_axi.w 1, s_axi.b 1, s_axi.ar 2, s_axi.r 2

records {"stream": "s_axi"}
4 of 4
stream         n start end   delay addr data       strb resp rule   kind
s_axi.wr       0 55ns  95ns  20ns  0x10 0xCAFEF00D 0xF  0x0
s_axi.rd       0 115ns 145ns 20ns  0x10 0xCAFEF00D      0x0
s_axi.rd       1 175ns 205ns 20ns  0x40 0x0             0x2
s_axi.errors   0 195ns 205ns                                 r_resp
```

The second read, at 175ns, is the one at address `0x40`, and its response is `0x2`. The
`s_axi.errors` record at 195ns names the `r_resp` rule.

The answer lines above are the server's own, with the hint line after `decode` left out.

## Set it up

wtf has to be on your `PATH` ([Get started](@/get-started.md) has the install). For Claude Code:

```sh
claude mcp add wtf -- wtf --mcp
```

Any other client that speaks MCP over stdin and stdout takes the same command, usually in a JSON
file:

```json
{ "mcpServers": { "wtf": { "command": "wtf", "args": ["--mcp"] } } }
```

The server takes no trace at start. The agent opens the traces it needs, and it finds the wtf
windows you have open with `sessions`.

## Ask

Name the trace and what went wrong: "the second read in `sim/axi_tb.vcd` fails, why?". If you
have the trace open, you can say "look at what I'm looking at" instead. The agent reads your
window's rows, cursors and visible range, and starts from there.

## Watch it work

Your window on the same trace shows `agent 4123-1 joined (:follow shows its workspace)`. Each step
the agent takes then shows on the status line, for example
`agent: find @clk wr && addr == 0x40  ·  3 matches, first 120ns, last 180ns`.
A step does not move your view.

| Keys | Does |
|---|---|
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>t</kbd> | the agent's steps, oldest first; <kbd>Enter</kbd> puts the cursor at a step's time |
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>f</kbd> | follow: the window shows the agent's workspace and each change it makes |
| <kbd>Space</kbd> <kbd>a</kbd> <kbd>F</kbd> | stop following, and get your own view back |

## Take it from there

When the agent has its answer, it prepares the rows, the marks and the decodes that show it, and
presents them. Your window takes that layout in place of yours, as one change. <kbd>u</kbd> gives
your own view back, and <kbd>Ctrl-r</kbd> gives the agent's again. The decodes it made come
along, so `:records` and <kbd>Tab</kbd> in the records table work on them as they do on your own.

The agent works in its own workspace, and nothing is saved until you `:w`. Your view moves in two
cases: when the agent presents its layout, and while you follow it with `:follow`; `:follow off`
brings back your own view from before. The agent reads your window with `user_view`, which returns
its rows, cursors and visible range and does not change it.

If no window was open, for example after a simulation that failed in CI, `present` writes the
layout next to the trace, as `<trace>.agent.session.toml`. `wtf <trace>` shows it when you open
the trace, and `:trail` lists the steps that led to it. If you saved your own session after that,
the agent's layout is only offered: <kbd>Space</kbd> <kbd>a</kbd> <kbd>s</kbd> shows it.

`open` takes any path the agent gives it, so the agent can open any trace its process can read,
not only the ones you named. It cannot run `:w`, `:q`, `:e` or `:help` on your window. A workspace
ends when the agent's client exits, so `present` a layout first if you want to keep it.

## What the agent sees

[Agent tools](@/tools.md) has each tool the server offers and the help it reads, word for word,
written from wtf itself. The [reference](@/reference.md) is the agent's reference for names, times,
expressions and decoding.
