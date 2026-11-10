+++
title = "Agent tools"
description = "The tools wtf --mcp gives an agent."
+++

<!-- Written by `UPDATE_SITE=1 cargo test --test mcp site_tools` from what wtf --mcp tells an agent; don't edit. -->

What `wtf --mcp` tells an agent, word for word: each tool's description and arguments. See [For agents](@/agents.md) for setting it up.

## Tools

### `command`

Run one of wtf's `:` commands on the workspace: `decode`, `protocol`, rows, cursors (`help commands`). Answers with the rows it added or removed; `undo` takes a step back.

| Argument | Means |
|---|---|
| `line` (required) | A `:` command without the colon: `decode ...`, `add top.cpu /_valid$/`, `cursor 2 80ns` (`help commands`). |

### `find`

When a search expression holds, as a table of times; `show` adds values, `count_only` counts. With `@<clock> ` first, a match is a cycle whose ending edge samples it true (`@clk x`: the cycles x is high), reported at the cycle's start; without, where it becomes true (`rose(x)`: x's edges). A bus: decode it (`help decode`).

| Argument | Means |
|---|---|
| `backward` | Search toward the start: the latest matches first. |
| `count` | How many lines to list; default 20. |
| `count_only` | Only count the matches; no list. |
| `expr` (required) | A search expression as in SystemVerilog: `==` `!=` `<` `&&` `\|\|` `!`; only `rose(x)` `fell(x)` `stable(x)` `past(x)` (SVA's, no `$`); `@<clock> ` first samples each cycle: `@top.clk valid && ready`, `rose(irq) && !past(busy)` (`help expressions`). |
| `from` | Search after (or with `backward`, before) this time; default the start. |
| `show` | Signals whose values to list on each match: a table of the transfers. |
| `to` | Stop at this time (with `backward`, down to it); default the end. |
| `unit` | The unit of the answer's times, `ns`, `us` (`help times`). |

### `help`

The reference for one concept: its form, recipes and details. Topics: names, patterns, times, radix, expressions, decode, records, workspace, commands; without a topic, lists them. Call it when an argument's doc or an error isn't enough.

| Argument | Means |
|---|---|
| `topic` | A topic; none lists them. |

### `info`

The open waveform: file, time range, signals, and the interfaces a built-in protocol decodes, each with its `decode` line.

### `open`

Open a waveform (VCD, FST or GHW file) in a new workspace, the current one from then on. Answers with `info`, which lists the interfaces to decode.

| Argument | Means |
|---|---|
| `trace` (required) | Path of a VCD, FST or GHW file, best absolute. |

### `present`

Show your workspace to the user as evidence: it replaces the view of every wtf window on this trace (theirs is one `u` away), or with none open writes `<trace>.agent.session.toml` for them to open.

### `protocols`

What can be decoded; with a name, its roles, params, streams and fields (`help decode`).

| Argument | Means |
|---|---|
| `name` | A protocol to describe (`axilite`, `frame`, ...); none lists them all. |

### `records`

Records of a decoded stream, or of several in start order (a decode's name: its results), as a table: n, start (the rising edge that starts the record's first clock cycle), end (the edge that samples its last) and the fields; `where`, `group_by`, `count_only` (`help records`).

| Argument | Means |
|---|---|
| `count` | How many records to list; default 20. |
| `count_only` | Only count the records; no list. |
| `from` | Records starting at or after this time; default the start. |
| `group_by` | Fields to group by: a line per distinct value, with its count. |
| `radix` | hex, dec, ... for every field (`help radix`); `len:dec` for one, several with commas. |
| `stream` (required) | A stream, e.g. `bus.wr`; a decode's name for its results, `bus`; a pattern, `/^bus\./`; or several, space-separated (`help records`). |
| `to` | Records starting at or before this time; default the end. |
| `unit` | The unit of the answer's times, `ns`, `us` (`help times`). |
| `where` | Only the records this holds for: `resp != 0`, `n == 138`, `req_addr != resp_addr` (`help records`). |

### `reload`

Reopen the trace file, after the simulation was run again.

### `render`

Draw signals over a time range as text, as wtf's screen does; without signals, the workspace.

| Argument | Means |
|---|---|
| `from` | Start of the range; default the start, or the workspace's range. |
| `signals` | Signals drawn top to bottom; none: the workspace. |
| `to` | End of the range; default the end, or the workspace's range. |
| `unit` | The unit of the range in the heading, `ns`, `us` (`help times`). |
| `width` | Width in characters; default 120. |

### `sessions`

The open sessions: the user's wtf windows and agents' workspaces, with ids and traces, yours marked. `use` switches between yours; `open` starts one.

### `signals`

Search the waveform's design, scopes (instances) and signals, by a regex in slashes found anywhere in the path, as grep does (`/uart/`, `/_valid$/`); none lists it all, folded to fit.

| Argument | Means |
|---|---|
| `pattern` | A regex in slashes, found anywhere in the path in any case, indices in brackets (`/s_axi_.*valid/`, `/^top\.cpu\./`, `/gen\[0\]/`); none lists all. |

### `use`

Make another of this server's workspaces (ids from `sessions`) the current one, which the other tools then work on; nothing is closed. Answers with `info`.

| Argument | Means |
|---|---|
| `session` (required) | The id of one of this server's workspaces, from `sessions`. |

### `user_view`

What the user is looking at in their wtf window on this trace, to start from what they see; `adopt` replaces the workspace's rows, cursors and range with it.

| Argument | Means |
|---|---|
| `adopt` | Also copy the user's rows, cursors and visible range into the workspace, to build on them. |

### `value_at`

Values of signals at one time; a struct or array whole, then each signal in it. For the changes over a range use `values`; for when something happens, `find`.

| Argument | Means |
|---|---|
| `radix` | hex, bin, dec, ... (`help radix`); `data:dec` for one signal. |
| `signals` (required) | Signals by path or path end (`help names`); `signals` finds them. |
| `time` (required) | The time, `120ns` (`help times`). |
| `unit` | The unit of the answer's times, `ns`, `us` (`help times`). |

### `values`

Values of signals over a time range: the value at `from`, then each change, as `time value` lines.

| Argument | Means |
|---|---|
| `from` (required) | Start time with its unit, any unit: `120ns`, `3.4ms` (`help times`). |
| `radix` | hex, bin, dec, ... (`help radix`); `data:dec` for one signal. |
| `signals` (required) | Signals by path or path end (`help names`); `signals` finds them. |
| `to` (required) | End time, inclusive. |
| `unit` | The unit of the answer's times (`help times`). |

### `view`

Describe the workspace in words: rows, cursors with the time between them, visible range. Drawn: `render`; the user's window: `user_view`.

| Argument | Means |
|---|---|
| `unit` | The unit of the answer's times, `ns`, `us` (`help times`). |

### `workspace`

Set up the whole workspace in one call: rows, cursors, range, decodes (`help workspace`). Answers as `view` does.

| Argument | Means |
|---|---|
| `cursors` | Cursors by number, `{"1": "80ns", "2": "390ns"}`: 1 the cursor, 2 - 9 marks; a mark may be `{"time": "390ns", "name": "first bad write", "note": "..."}`. |
| `decode` | Decodes, as `decode` makes them (`help decode`). |
| `protocols` | Sampled protocols, as `protocol` makes them (`help decode`). |
| `range` | The visible range, `["60ns", "480ns"]`; default the whole trace. |
| `rows` (required) | The rows, top to bottom: `"name:radix:colour"`, a group or a struct's open levels (`help workspace`). |
| `scope` | The scope names resolve under first (`help names`); not given: it stays. |
| `signals` | Computed signals, `["op = cmd[7:0]"]`. |

The `help` topics they point to are the [reference](@/reference.md).
