# Changelog

What changed in each release. Commits follow [Conventional Commits](https://www.conventionalcommits.org/),
so `git log v0.1.0..` has the details.

## 0.3.0 (2026-11-04)

wtf opens to a start screen and works from a menu on Space. A picker finds signals and decodes
the interfaces wtf found in the trace, and `:records` opens a table that sorts, groups and
filters. An agent working on a trace shows in every window on it, step by step.

Breaking:

- Enter folds groups, buses, streams and composite rows, as Space is now the leader key. A click
  on the fold marker still folds.
- `wtf --attach` is gone. Open the trace with `wtf <trace>`: an agent working on it joins the
  window, and `:follow` follows it.
- `:protocol`, a session's `[[protocol]]` and the `workspace` tool's protocols take `roles` and no
  `clock`. The clock goes on the decode: `:decode req hs clock=clk valid=req_valid`.
- `:records` opens the records table. A `where` that keeps no records is no longer an error; the
  table says none match.
- The decode answers of `command` list the results, then the violations, then the streams they
  are made from.

New:

- A start screen when wtf opens without a trace, with the recent traces and the agents on this
  machine. A trace without a session opens to where to start and the interfaces wtf found.
- Space opens a menu of commands in groups, and which-key shows what can follow a key.
  `[keys]` and `[leader]` in the config move them.
- One picker (`Space f`) for signals, marks, records, commands, sessions, the agent's trail and
  the interfaces found, with a preview. `Tab` marks several signals, `Ctrl-s` changes the source.
- `Space d d` decodes one of the interfaces wtf found, and `Space d D` starts a `:decode` line.
- The records table: `s` sorts, `=` groups by the sorted field, `/` filters, `Tab` goes to the
  next set and `Enter` jumps to a record. `records` takes a decode name, a glob or several names.
- The command line completes every command's arguments and shows short docs in a card.
- Notifications fade out, and `:messages` lists them again.
- A new look: cards, Nerd Font icons (`--glyphs nerd`), a status line in segments, Name and
  Value headers, a value column of stable width and marks as tabs on the ruler.
- Marks have a name and a note (`:cursor 2 60ns name=late note=...`), and undo covers marks.
- An agent's queries and changes are a trail in the user's window (`:trail`). When no window is
  open, `present` keeps the evidence for the next one opened on the trace.
- A site with a quick start, recipes and every key: https://xtrollyj00.github.io/wtf

Fixed:

- `:group` on rows inside a group moves them out into the new group.
- `zoom` given two times points to `fit`.
- A record's field that really is z draws; only the time between records is blank (#28).
- Computed signals nothing uses are dropped, and `present` replaces the window's decodes.
- The signal panel keeps its place when computed signals are added (#35).
- A session from a newer wtf changes nothing.
- An agent's message never replaces the user's own on the status line.

## 0.2.0 (2026-10-31)

The mouse does what GTKWave and Surfer users reach for, the keys have one mode, and structs,
records and arrays are one row each.

Breaking:

- View and Edit mode are one mode. `i` and the `EDIT` badge are gone: `dd`, `J` / `K`, `r`,
  `V` and `u` work next to the time keys, and `Esc` ends a `V` range.
- `/` on the rows always searches values. To find a signal by name, open the panel with `a`,
  then `/`.
- `a`, or the `signals ▸` button at the bottom left, opens and closes the signal panel.
- `J` / `K` move a whole `V` range.
- Sessions write what is open under a row as `open = ["state"]`; `expanded = true` is no longer
  read.
- Every index in a path is `[i]` whatever the simulator wrote (`types.gen[0].p`, `grid[1][2]`).
  Names in either HDL's form are still found, and older sessions still load.

New:

- The mouse: click to select and put the cursor, the wheel to move, `Ctrl` + wheel to zoom.
  Drag a row by its name to move it, `Ctrl` + click to select a range, drag across the
  waveforms or the time axis to zoom to that range. Double click or drag a signal out of the
  panel to add it. Click a mark's number in the ruler to jump to it, drag it to move it.
- A right click opens a menu of what can be done there, each item with its key.
- A struct, record or array is one row that opens a level at a time, with its whole value;
  adding one takes all of its signals.
- Each field under a row, a struct's or a stream's, can have its own radix and colour (`r`,
  `:radix`, `:color`, the menu), kept in the session.
- VHDL's `grid(1, 2)` names an array element.

Fixed:

- wellen's panic on a GHW file with a multidimensional array is an open error, not a crash.
- `value_at` answers at the time asked, not at the end of the tick it falls in.
- `signals` says `(real)` or `(string)` instead of `(real or string)`.
- Zero shows as a bare `0` on a ps or fs trace, not `0ps`.
- A composite row's name no longer lands over its value.

The spec describes wtf as it is meant to work. The shortcuts taken so far are GitHub issues
labelled [`known-limit`](https://github.com/xtrollyj00/wtf/issues?q=is%3Aissue+label%3Aknown-limit).

## 0.1.0 (2026-10-28)

The first release.

- A Vim-like terminal viewer for VCD and FST: View and Edit modes, a searchable signal tree,
  groups, buses split into bits, radixes (hex, binary, decimal, signed, octal, ASCII, float,
  fixed point, FST enum names) and computed signals (`data[7:4]`, `{a, b}`).
- Search by value or expression, cycle by cycle with a clock (`/@clk fell(valid) && !past(ready)`).
- The cursor and eight marks, with the time between them on a ruler.
- Decoders for AXI-Stream, AXI4-Lite, AXI4, SPI and UART into records with latencies and protocol
  errors, and protocols of your own in a few lines.
- Sessions in a TOML file next to the trace; `:e` and a rerun of the simulation reload it.
- `wtf --mcp`: an MCP server through which an AI agent opens the trace, decodes it and shows its
  evidence in your window.
- Dark and light themes, the config file, `unicode`, `box` and `ascii` glyph sets chosen on the
  first start, `NO_COLOR`, the mouse.
- The dark theme fits the shades behind cards, the selected row and the cursor columns to the
  terminal's background, which wtf asks the terminal for; with no theme chosen, a light terminal
  gets the light theme.
- `--snapshot` renders a screen as text or, with `--ansi`, in colour, after `--keys`.
- Linux and macOS, x86_64 and arm64.
