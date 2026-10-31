# wtf specification

**wtf** — **W**aveform **T**erminal **F**rontend. Also what you say when the bus goes `X`.

Status: milestones 1–5 and 7–9 are built, 6 has a first measurement, 10 and 11 are planned (section 17).
The spec describes wtf as it is meant to work; shortcuts taken so far are the
[`known-limit` issues](https://github.com/xtrollyj00/wtf/issues?q=is%3Aissue+label%3Aknown-limit).
Implementation: Rust, a core crate and the interfaces (section 1).

One file per section; code cites them as `spec §N`. Read only the sections a change touches.

| § | File | Covers |
|---|---|---|
| 1 | this file | philosophy, architecture |
| 2, 3 | [02-modes.md](02-modes.md) | modes, Normal mode keys, mouse |
| 4 | [04-screen.md](04-screen.md) | screen layout, command line |
| 5, 6 | [05-edit.md](05-edit.md) | editing rows, signal panel, undo, groups |
| 7 | [07-commands.md](07-commands.md) | `:` commands |
| 8 | [08-search.md](08-search.md) | value search, expressions |
| 9 | [09-cursors.md](09-cursors.md) | cursor, marks, ruler |
| 10 | [10-radix.md](10-radix.md) | radixes |
| 11, 12, 13 | [11-sessions.md](11-sessions.md) | session files, command-line options, trace formats |
| 14 | [14-display.md](14-display.md) | time, zoom, waveforms, values |
| 15 | [15-mcp.md](15-mcp.md) | MCP server, workspaces, present, the link, attach |
| 16 | [16-decoders.md](16-decoders.md) | computed signals, streams, protocols, decoders, instances |
| 17 | [17-milestones.md](17-milestones.md) | build order |
| 18 | [18-limits.md](18-limits.md) | limits outside wtf, performance targets |
| 19 | [19-config.md](19-config.md) | config file: glyphs, themes, colours |

## 1. Philosophy

wtf browses a wave file the way Vim browses a text file.

- **Time is the line axis.** `:42` goes to line 42 in Vim; `:2000ns` goes to 2000 ns here.
  `gg` and `G` go to the start and end of the file, which is the start and end of the simulation.
- **Viewing is the default.** You open a file to look at it. The keys that change what is shown
  (`dd`, `J` / `K`, `r`) sit beside the ones that look, and `u` takes any change back.
- **Few keys, all useful.** Only the bindings that are used all the time. Anything rarer is a `:` command.
- **Ergonomics matter.** The common keys sit near the home row. Nothing frequent needs a stretch
  to the number row or a modifier.
- **The waveforms get the whole screen.** Other UI appears on demand and closes with `Esc`.

### Architecture

A core with two interfaces to it. The core (`core/`, the `wtf-core` crate) is a trace and what is
shown of it, and every operation on that: the workspace (rows, selection, cursors, zoom, undo),
the `:` commands, value searches and queries, and session files. It has no user interface and no
dependencies on one, so the compiler keeps it that way. Its operations return data and messages;
the interfaces decide how to show them.

- **Terminal UI** (`src/tui/`): modes, keys, the signal panel, the command line, the status line.
  It edits the core's workspace and owns only how it is edited: the window's own commands
  (`:w`, `:q`, `:help`) and the command completions.
- **MCP server** (`src/mcp.rs`): tools that parse their arguments, call the core and format its
  answers for agents.
- Shared by both: drawing a workspace (`src/draw.rs`, into a ratatui buffer, as text for the agent
  and `--snapshot`), and the link between sessions (`src/live.rs`).
