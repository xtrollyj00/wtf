## 17. Milestones

Each milestone ends with something usable for day-to-day debugging.

1. **Look at a trace.** Open VCD and FST. View mode: `h` `l` `w` `b` `j` `k` `gg` `G` `z` `x`, counts.
   Name and value columns, status line, display rules (section 14). Commands `:<time>`, `:zoom`,
   `:fit`, `:add`, `:rm`, `:q`. `--snapshot`, so everything can be tested from the start.
2. **Edit mode.** Signal panel with tree and vim-style search, editing rows (`dd`, `J` / `K`, `V`),
   groups with the activity row, radix (`r`, `:radix`), undo.
3. **Sessions.** TOML session files: `:w`, auto-load, `-S`, `[+]`, `:wq`, `:q!`. Reload with `:e`.
4. **Value search.** `/`, `n` / `N`.
5. **Cursors.** Multiple cursors (`m`, `M`, `Tab`, `'N`) and the Δt ruler. Until then there is
   one cursor.
6. **Performance.** Measure the prototype, settle the targets in section 18, and add lazy loading
   and VCD indexing where the measurements call for them.
7. **AI-assisted debugging.** The agent investigates headless and shows its evidence live in your
   window: its own workspace, `present`, `user_view` and the link to the window (section 15).
8. **Sessions.** One MCP server for every trace, and the user can join the agent's work:
   `sessions`, `open`, `use`, the registry of sockets and `wtf --attach` (section 15).
9. **Decoders and computed signals** (section 16).
   - **First cut**: computed signals (slices, concatenation, `rose` / `fell` / `stable` /
     `past`), records and decoders, the protocols on streams `frame`, `pair` and `check`, sampled
     protocols, and AXIS (beat, frame, errors), end to end in the core, the TUI and the
     MCP server. AXI4, SPI and UART follow, one protocol each.
   - Also planned for agents, separately and smaller:
     - lists say how many there are (`20 of 157`) everywhere, not only `records`;
     - `view` lists each row's value at each cursor, the cheap check before `present`;
     - `info` lists the clocks it detects with their periods, and the runs of the trace (between
       resets);
     - run-length answers for repetitive values and "first 5, last 5" summaries;
     - `render` around an event (`420ns ± 10 cycles`) showing only the rows that change, and a
       clock too fast to draw labelled `clk (5ns)` instead of a solid block.
10. **Agent benchmark and agent support** (benchmark built, `wtf-bench agent`; support planned).
    Changes for agents are judged by measurement, not by how they look:
    - **Benchmark**: debugging tasks an agent solves with `wtf --mcp`, each a trace, a prompt and
      the expected answer. Real-world tasks (the AXI-Lite transaction table, handshake checks,
      write-to-command latency, counting reads on a register block) and synthetic traces with injected
      bugs (a dropped valid, a wrong address, a missing response), so the answers are known and
      the traces can live in the repo.
    - A runner starts a headless agent (`claude -p` with an MCP config) per task and records, per
      task and wtf version: correctness against the expected answer (graded by a script where the
      answer is a table or a number), tokens in and out, cost, tool calls, errors from tools, and
      wall time. Results are kept, so a change to a tool shows up as a difference.
    - Agents' own reviews of a run (what was missing, what cost the most) are kept with it, as the
      source of the next changes.
    - **As built** (`bench/src/agent.rs`): tasks are `bench/agent/tasks/<name>.toml` (a fixture, the
      prompt, the JSON shape of the answer, the expected answer). The agent gets only wtf's tools,
      in an empty directory without the user's settings, hooks or CLAUDE.md, under a time and money
      cap; it ends its answer with a JSON block, graded key by key (hex and decimal, case, `ns`
      and the order of unordered lists don't matter). Per run, appended to
      `bench/agent/results.jsonl`: pass and score, turns, wtf calls in all and per tool, tool
      errors, bytes of wtf's answers (all and the largest), tokens (input, output, cache read and
      write), the peak context, cost, wall and API time, whether it presented, the answer, and
      the wtf commit (dirty or not), model and Claude Code version. `report` averages them per
      task, commit and model; `--runs N` repeats each task, as one run varies; `--review` asks the
      agent afterwards what cost it the most. Transcripts and reviews go to `bench/agent/runs/`;
      both are outside git, as answers quote local paths.
    - **Fix tasks**: a component whose verification fails, debugged and fixed, as an engineer
      would. A task is a directory `bench/agent/tasks/<name>/`: files of an open-source design
      (`upstream` repository, `commit`, and `[files]`: each work path with its upstream path, a
      file or a whole directory, so the bug can hide in a subcomponent of a library),
      a `bug.patch` that breaks it, and a `Makefile` and testbench of the task's own (`make sim`:
      the visible test, writing `sim.log` and `trace.fst`; `make hidden`: a regression the agent
      never runs). The runner fetches the upstream at the commit into `target/upstream/`, builds
      the work directory and checks `make sim` fails; the agent works there with wtf, file reads
      and edits inside it, and `make sim`. Afterwards the task's Makefile and testbench are put
      back, and the run is `fixed` when `make sim` and `make hidden` pass; it passes when fixed
      and the answer (the faulty signal, when the failure showed) is right. `agent check` proves a
      task without an agent: the bug fails both, the upstream files pass both. Each run keeps
      the agent's change as a diff against the upstream. Upstreams are
      permissively licensed, so patches can live in the repo. The first, `head-trimmer-boundary`,
      is ndk-fpga's AXIS head trimmer (VHDL, cocotb on nvc; the cocotb packages in a Python venv
      on `PATH`).
    - **Agent support**: how to tell agents to use wtf well, judged by the benchmark: the server's
      instructions and tool descriptions first; then a skill (a debugging workflow: find
      transfers, check handshakes, pair requests, declare the workspace, present); then a Claude
      Code plugin bundling the MCP server registration and the skill, so one install sets up both.
11. **Looks** (planned). For people first: glyph sets for any terminal and font (`unicode`, `box`,
    `ascii`, section 14), the config file (section 19), `dark` / `light` themes and `NO_COLOR`,
    and a colour per signal (`:color`, in sessions and the agents' `workspace`). Which glyphs
    agents' `render` should use is decided after, by counting tokens (milestone 10's way).
