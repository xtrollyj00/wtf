## 15. MCP server

`wtf --mcp` serves AI agents over MCP on stdin / stdout instead of opening the screen. It is
registered once (`claude mcp add wtf -- wtf --mcp`) and opens traces as the agent asks. Its tools
reuse wtf's own pieces:

| Tool | Does |
|---|---|
| `sessions` | the open sessions: the user's windows and agents' workspaces, each with its id and trace |
| `open(trace)` | open a trace in a new workspace of the agent's, which becomes the current one |
| `use(session)` | make another of this server's workspaces the current one |
| `info` | file, time range, timescale, number of signals, and the interfaces a built-in protocol decodes, each with its `decode` line (`open` answers with it too) |
| `signals(pattern)` | signals with their widths; `*` / `**` globs, or any path containing the text; array elements and generate instances of one width are one line, the indices as ranges (`mux_in.[0..15] (8 bits, 16 signals)`, `gen(0..4).x`) |
| `values(signals, from, to, radix, unit)` | the value at `from`, then each change up to `to`, as `time value` lines |
| `value_at(signals, time, radix, unit)` | values at one time |
| `find(expr, from, to, backward, count, show, count_only, unit)` | match times of a search expression (section 8), `@clk` included, as a table |
| `render(signals, from, to, width, unit)` | the signals drawn as text, as on the screen (`--snapshot`); without signals, the workspace |
| `workspace(scope, signals, protocols, decode, rows, cursors, range)` | set up the whole workspace in one call |
| `protocols(name)` | what can be decoded and how; for one protocol its roles, params and streams |
| `records(stream, from, to, where, group_by, count, count_only, unit)` | a stream's records as a table |
| `reload` | reopen the trace after the simulation was rerun, like `:e` |
| `command(line)` | run a `:` command on the agent's workspace (milestone 7) |
| `view(unit)` | describe the workspace: rows, cursors with Δt, visible range |
| `present` | show the workspace in the user's window, or write `<trace>.agent.session.toml` |
| `user_view(adopt)` | what the user's window shows; `adopt` copies it into the workspace |

- Signals are named by full path or by the end of a path if only one signal ends that way; `data[3]`
  is one bit. Names may be written as in VHDL or Verilog (section 8): any case, `gen(4)` or
  `gen[4]`, `data(7 downto 4)` or `data[7:4]`. Times take units; radixes are those of section 10.
- Agents pay for round trips and for every byte they read, and can't un-read an answer, so
  answers are dense and bounded: at most 1000 lines with a note (`values`: 1000 changes in all),
  and at most 24000 bytes, cut at a line with a note saying how much more there was.
- **A time unit per workspace**, so answers compare across calls and carry no femtosecond noise
  (a testbench's 1 fs skew made `2595.000001ns`). Once a search names a clock (`@clk`, also in a
  protocol or a decode), the unit is the largest that keeps the fastest such clock's shortest
  phase at 1 or more (`ns` for a 10 ns clock, and for a 200 MHz one with its 2.5 ns phases);
  `command` `unit ns` sets it, `unit auto` goes back to the clocks'; the window refuses `:unit`,
  as it picks the unit that fits. Times in the workspace's
  unit, or in the `unit` a call asks for, are rounded to a thousandth of it: `2595.000001ns` is
  `2595ns`, `39.895000001us` is `39895ns`, a half cycle stays `2.5ns`; `unit: "fs"` gives them
  exactly. Before there is a unit, all the times in one answer share one unit, the largest that
  keeps the smallest nonzero one at 1 or more, exactly (`80ns`, `2595.000001ns`).
- The core keeps times as ticks and never picks a unit: the interfaces pass the unit and the
  precision to its one formatter. So a duration a decoder works out (a pair's `delay`) is a time
  field, ticks, shown in the answer's unit, and compared with a time (`where: "delay == 40ns"`); a
  value with a time unit is a time wherever a search takes values.
- A signal's radix can be given with its name, `awaddr:dec`, wherever tools take signals.
- Rows in `workspace` can also take a colour (section 14), after the radix or alone:
  `awaddr:hex:red`, `wvalid:red`, to point the user at the guilty signal when presented.
- `find` answers as a tab-separated table with a header. With a clock, consecutive matching
  cycles are one row: `from`, `last`, `cycles`. `show` adds a column per signal with its value on
  each match, as the clock edge samples it (one row per cycle), so `@clk awvalid && awready` with
  `show: [awaddr, wdata]` is a table of the writes. `to` ends the search, e.g. at a reset.
  `count_only` gives the number of matches and runs, first and last.
- `records` lists a stream's records (section 16) as a table, `n` (the record's number, as the
  stream's value `#n` shows it), `start`, `end` and a column per field, headed with how many are shown of all in the range (`20 of 157`). With more
  than one record, long values are shortened, a frame's data to its first 8 bytes and its size
  (`f7 88 0c 5c 25 8f 30 3a ... (580 bytes)`), other text to 40 characters, and the header says
  so; one record shows them whole. `from` /
  `to` pick records by start, `count_only` counts them. A stream of an interface `open` listed
  but not decoded yet (`top.s_axi.wr`, `s_axi.wr`) is an error that gives its `decode` line.
  `where` keeps the records a search
  expression over the fields holds for, the fields named bare (`addr != 0x1C && resp != 0`,
  `rule == dropped`; `past(x)` is x of the record before), evaluated on each record, so never on
  the gaps between records where field signals are `z`. `n` is the record's number unless a field
  has that name (`n == 138`, a testbench's packet #138), and a field compares with another field
  too, as unsigned numbers or text (`req_addr != resp_addr`: a `pair`'s two sides, as a
  scoreboard checks them). The header counts the matches too
  (`3 of 3 matching, of 157`). `group_by` names fields and answers one line per distinct value of
  them, in order of first appearance, with how many records have it and the first and last start
  (`128 × addr=0x1C data=0x3 (2.975us .. 69.015us)`), so 128 identical polls are one line. Streams are declared in `workspace` with
  `protocols`: `{"name": "rx", "clock": "aclk", "when": "tvalid && tready", "fields": ["tdata =
  tdata", "op = cmd[7:0]"]}` (with `roles`, a type), `decode` puts types on signals: `{"name":
  "c0", "protocol": "cmd", "clock": "clk", "signals": ["valid = cmd_valid", "cmd = cmd"]}`, and
  `signals` names computed signals: `["op = cmd[7:0]"]`.
- `workspace` declares the whole workspace in JSON, replacing it as one undo step, so the agent
  writes the state it wants instead of editing toward it:
  `{"scope": "dut", "rows": ["s_axi_aclk", {"group": "write", "rows": ["s_axi_awaddr:hex",
  "s_axi_awvalid"]}], "cursors": {"1": "80ns", "2": "390ns"}, "range": ["60ns",
  "480ns"]}`. It is the session (section 11) in another shape, and goes through the same checks.
  Every unknown or ambiguous name, bad radix and bad time is reported at once and nothing
  changes, so one retry fixes them. It answers as `view` does.
- `command` answers with the rows it added and removed, so a pattern that caught too much shows.
  Each command is one undo step, as in the window.
- A name ambiguous in the trace means the one directly in the workspace's scope (`command` `scope
  dut`, or `scope` in `workspace`, kept for every later call and in the session), then the
  signal the workspace shows, if it shows just one of them, then the only one under the scope,
  then the shallowest, if just one is (`clk`, passed down to every instance, is the top's);
  otherwise the error lists the candidates. A pattern that matches nothing hints that it
  matches the whole path (`**.*fifo*`).
- Every other tool works on the current workspace; until `open`, they say to open a trace first.
- Session files aren't loaded; the agent starts from an empty workspace (or adopts the user's view).
- `command` refuses `:w`, `:q`, `:e` and `:help`, which belong to the user's window.
- `protocols` is the agent's documentation, generated from the registry so it can't drift: the
  list of protocols (built in, on streams, the workspace's own), and for one its roles, params, the
  streams it makes with their fields (worked out from its steps), the values of its violation
  fields, what each stream is, and an example `command` line. What a stream is comes from its
  step where it can: a sampled stream's condition and field signals (`a record per cycle where
  awvalid && awready; addr = awaddr`), a check's rules with their expressions; a line written
  with the protocol says the rest (`writes: AW and W in either order, then B; delay from the later
  of AW and W to B`). An unknown protocol's error names them all.
- Agents are pushed to decode: a decoder's records and checks are written and tested once, while
  searches an agent writes differ from run to run and go wrong. So `info` names the interfaces
  found by their signals (every required role of a built-in protocol a signal `<prefix><role>`;
  AXI4 over AXI-Lite, whose roles it has too; the clock `<prefix>aclk`, or the one clock-like
  signal in the scope), the server's instructions put decoding before `find`, and `find`'s
  description and a comparison of two signals point to the decoders.
- `command` decodes without touching the rows (`decode`, `protocol`, `signal`). `decode` answers
  with the streams it made and their records, violations apart (`bus: records bus.aw 2, ...;
  violations bus.errors 2, bus.wr.unmatched 1, ...`), then where to read more; the window shows
  that line on its status line.

### Workspaces, present and the link

- **Workspace**: `wtf --mcp` keeps its own rows, groups, radixes and cursors, invisible to the
  user. A `command(line)` tool runs wtf's `:` commands on it (`:add`, `:group`, `:rename`,
  `:radix`, `:color`, `:<time>`, `:zoom`, `:fit`, the cursor commands `:cursor <n> <time>` and
  `:cursor <n> off`). `render` without signals draws the workspace, so the agent can check what it
  prepared; `view` describes it (rows, cursors with Δt, visible range).
- **Present**: `present` shows the workspace in the user's wtf window for the same trace, if one
  is open: its layout and cursors replace the window's, as one undo step (`u` restores the
  user's view), and the status line says `agent: presented …`. Without a window it writes a
  session file and says how to open it (`wtf -S <file> <trace>`), so headless work still ends
  in something to look at.
- **User's view**: `user_view` reads the open window's rows, cursors, selected signal and
  visible range, so the agent can start from what the user is looking at.
- **Link**: each wtf window listens on a local socket (see Registry); `wtf --mcp` looks for one with the same trace path. Two requests travel over it,
  both as session TOML (section 11): "send your session" and "take this session". Several
  windows on one trace: the most recently started one.
- The agent's changes never save the user's session file by themselves; the user `:w`s if they
  want to keep them.
- `wtf --mcp` takes no trace. The agent lists what is open with `sessions`, opens traces with
  `open` (several at once, switching with `use`), and works on the current workspace.
- **Registry**: every session listens on a socket `wtf-<id>.sock` in `$XDG_RUNTIME_DIR`, or
  without it in `wtf-<uid>` in the temp directory, made so only the user can enter it and refused
  (with an error saying so) if someone else owns it or can enter it: a window's id is its process id, a workspace's is the server's process id and
  a number (`4123-1`). A third request, "who are you", answers the kind (window or agent) and
  the trace's full path without waiting for the window's main loop, so listing is instant.
  Listing removes sockets nobody listens on (left by killed processes).
- **Attach**: `wtf --attach <id>` opens a window on the session's trace with its usual session
  file, then shows the session's rows and cursors on top, as `present` does (`u` restores the
  usual view). It is a copy: from then on it is an ordinary window, the agent updates it with
  `present`, and `:w` saves to the trace's session file. `wtf --attach` alone lists the sessions.
- Workspaces live as long as the server: when the agent's client exits they are gone (`present`
  first to keep one).
