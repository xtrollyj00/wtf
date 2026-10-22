## 18. Deferred

To be decided once prototypes exist:

- Mouse support.
- Subgroups (nesting), creating groups automatically from file scopes.
- Remote traces (client/server), for traces on a machine reached over ssh, where drawing remotely
  lags. Until then: mount the directory with sshfs and run wtf locally, or use mosh.
  Sketch: `wtf -r host:path/run.fst` starts `ssh host wtf --serve path/run.fst` and talks to it over
  stdin / stdout (no ports, no daemon). The seam is `Trace`: the rest of wtf only asks it for values
  and changes, thousands of times per frame, so those can't be remote calls. The client fetches the
  hierarchy once and each signal's changes when it is added, and answers queries locally. Open
  points: a wire format for signal data, version checks between client and server, fetching huge
  signals by time window, and `:e` and errors over the link.
- Cursors: naming cursors, more than 9.
- Attach as a live mirror: the window follows the agent's workspace as it changes, instead of
  copying it once (milestone 8).
- User config file (TOML, e.g. `~/.config/wtf/config.toml`): colors, plain terminal output, default radix, key remapping?
- Performance: targets and the rules that follow from them. Draft below; milestone 6 has a first
  measurement (clocked searches), the targets are not settled yet.

### Known limits

Shortcuts taken so far, to fix when they get in the way.

- A named signal can't be removed, only defined again (`:signal`); removing vars would renumber
  them.
- A named signal that uses one defined after it fails on `:e`, as they are made in order.
- Every slice or concatenation named, even once in a search, stays a var until the trace is
  reopened, and is listed in the panel and by `signals`.
- The signal panel is rebuilt when computed vars appear, losing its highlighted item and Space
  marks.
- A stream that can't be sampled again on `:e` is reported and dropped, not kept to retry on the
  next `:e` (named signals are kept).
- Sampling a stream again with fewer fields leaves the old field vars, with their old values.
- `records` shows every field in hex; no per-field radix (`tdata:dec`).
- Defining a protocol type again doesn't sample its instances again until `:e`.
- Applying a session (also `present`) adds its signals, protocols and instances to the ones there;
  none are removed.
- A pair's `delay` is a time (ticks, `delay == 40ns`), but there is no `>` to find slow ones; as a
  row, its field signal shows the ticks in the row's radix, not as a time.
- `check` rules are violations written directly (`valid && !ready`), one record per cycle; a rule
  over records (a frame too long) needs a decoder.
- Streams are worked out over the whole trace at once (spec §16 "Known limit").
- A stream row's label is `#n` with `len` (`64B`) and the text fields (delay, rule, kind); there is
  no per-protocol label.
- `:decode` has no quoting: a value runs to the next `key=` word, so a value can't contain one
  (`a==1` is fine, `x=1` is taken as a key).
- A frame's `data` is text (`de ad be ef`), held for every frame: big for long frames, and searched
  only whole (`==`), not for a part.
- The `e` box doesn't scroll: a value longer than the screen ends in `… n more lines`.
- `axilite` needs both the write and the read channels; a write-only or read-only port can't use it.
- `join` pairs strictly in order (AXI-Lite has no IDs): after a lost response, the transactions
  after it are still paired, one off.
- An instance of `frame` / `pair` / `check` on a stream made by a later instance fails on `:e`, as
  instances are made in order.
- A time rounded to a thousandth of the workspace's unit can be just before the edge it stands for (`2595ns` for an
  edge at 2595.000001ns), so `value_at` there reads the value before the edge; `find` `show` and
  `records` sample correctly. A per-call `unit: "fs"` gives the exact time.
- The workspace's time unit isn't saved in the session, nor shown in the window.
- SPI's chip select is active low, and UART's line idle high; there is no param to invert them.
- SPI's bit records last from one sampling edge to the next, so the last bit of a transfer ends at
  cs rising, however long after.
- UART takes only the first stop bit into account; a break (the line held low) is a framing error.
- A slice or concatenation SPI reads its pins through (`{cs, sclk, mosi, miso}`) stays a var, as
  every computed signal named.

### Performance (draft)

#### Size targets

wtf must stay comfortable with traces up to **10 GB** and hierarchies of
**hundreds of thousands of signals**.

| Action | Target |
|---|---|
| open an FST trace, first screen shown | under 1 s |
| open a VCD trace, first screen shown | under 2 s; values become available while the file is indexed in the background |
| add a signal, its waveform shown (FST) | under 1 s |
| any key in View or Edit mode, screen redrawn | under 50 ms, at any zoom |
| signal panel search, per typed character | under 50 ms with 500k signals |
| value search (`/`) on a signal with 10M transitions | under 1 s |
| memory after opening, before adding signals | proportional to the hierarchy, never to the file size |

#### Rules that follow from the targets

- **Lazy loading**: opening a file reads only the hierarchy. A signal's values are read when it is
  added, and memory grows with the shown signals, not with the file.
- **VCD indexing**: VCD has no index, so a background pass over the file builds one. The UI stays
  usable during the pass, shows its progress on the status line, and signals fill in as their data
  becomes available.
- **Drawing cost depends on the screen, not the data**: drawing a row looks up only the columns
  on screen (e.g. by binary search over transitions), so zooming out to the whole simulation is as
  fast as any other zoom.
- **Long operations never block**: anything that can take longer than the redraw target
  (indexing, loading a signal, value search) runs in the background and can be cancelled with `Esc`.
- `:e` (reload) follows the same rules as opening.

#### Measuring

The targets are checked against generated reference traces (a 10 GB FST, a 10 GB VCD, a hierarchy
with 500k signals), so results are comparable between versions and implementations.

`bench/` (`wtf-bench`) writes them and times wtf on them. `gen --size 10G --format fst|vcd <dir>`
writes `bench.fst` / `.vcd`: a 10 ns clock, a reset, `top.hot` (a 32-bit count of the cycles, a
transition every cycle) and `--modules` (200) AXI4-Lite slaves `top.u<i>` with random
transactions, run until the file has the size asked for; the same arguments write the same trace.
`--modules 26316 --size 100M` is the 500k-signal hierarchy. `run <trace>` times, against the
targets: opening in the core, adding `top.hot`, a value search for its last value, a clocked
search over the whole trace, the first screen and the whole trace by `wtf --snapshot`, and peak
memory (Linux only).

#### First results

wtf 0.1.0 on an AMD Ryzen 9 9950X3D (32 threads), 122 GB of memory, NVMe; 2026-10-22. The traces:
`gen --size 10G`, 3803 signals (10 GiB: 24.4M cycles in VCD, 122.8M in FST), and
`--modules 26300 --size 200M`, 499,703 signals (FST).

| | 10 GiB VCD | 10 GiB FST | 500k signals | target |
|---|---|---|---|---|
| open, in the core | 2.6 s | 0.9 s | 0.1 s | |
| first screen, `wtf --snapshot` | 3.8 s | 12.7 s | 0.2 s (was 8.9 s) | 2 s VCD, 1 s FST |
| whole trace on screen | 3.9 s | 12.6 s | | the first screen + 50 ms |
| add `top.hot` | 0.6 s | 7.7 s | | 1 s |
| value search to `top.hot`'s end | 1.3 s (24M; was 5.4 s) | 6.5 s (123M; was 28.4 s) | | 1 s for 10M |
| clocked search, whole trace | 2.2 s (was 12.9 s) | 14.2 s (was 71.5 s) | | |
| peak memory after open | 13.5 GB | 3.8 GB | 134 MB | proportional to the hierarchy |
| peak memory after the searches | 13.5 GB | 8.9 GB | 147 MB | |

What they say:

- A VCD is read whole when it is opened (wellen), so its memory grows with the file: 13.5 GB for
  10 GiB. This is the miss that background indexing (above) is for.
- Drawing is not the cost: the whole trace on screen takes as long as the first screen.
- An FST's first screen is slow because the signals on it are loaded whole, over the whole trace:
  `top.hot` alone (123M changes) takes 7.7 s.
- Value and clocked searches went at about 2.3 s per 10M changes and 0.55 s per million cycles in
  both formats: each step to the next change searched the trace again (four binary searches) and
  read the value twice. Walking a signal's changes in order, they take 0.55 s per 10M changes and
  0.14 s per million cycles.
- With 500k signals the window took 8.9 s while the core opened in 0.1 s: building the signal
  panel's tree looked each scope up among its siblings one by one, quadratic in a scope's children
  (26,300 modules under `top`). Looked up by name now, it takes 0.2 s.

#### The MCP server

`wtf-bench mcp <trace>` plays an agent's session over `wtf --mcp` and times each call, with the
size of its answer. On the same machine and 10 GiB traces (2026-10-22):

| call | VCD | FST | answer |
|---|---|---|---|
| `open` | 2.6 s | 0.7 s | 99 B |
| `find @clk awvalid && awready`, `count_only` | 2.2 s (was 2.6 s) | 14.3 s (was 17.2 s) | 84 B |
| the same, first 20 with `show` addr and data | 0.14 s (was 0.34 s) | 2.1 s (was 3.8 s) | 583 B |
| `values` of `top.hot` and awaddr over 1 µs | 0.61 s | 7.2 s | 1.9 KB |
| `render`, 4 signals over 200 ns | 0.2 ms | 0.2 ms | 2.2 KB |
| `command decode bus axilite` on one port | 7.0 s (was 19.6 s) | 46.9 s (was 108 s) | 4 B |
| `records` `where`, `group_by`, `count_only` | 19 ms at most | 89 ms at most | 0.1 - 0.9 KB |
| server memory (peak) | 13.5 GB | 11.5 GB (was 12.7 GB) | |

- Answers stay small; what costs an agent is time.
- Decoding a protocol takes longest, long enough for a client to give up waiting. It was the
  clocked sampling decoders share with the clocked search, which evaluated each query on every
  cycle (an index of values per signal and sample, a hash lookup per signal and row, a `String`
  per value). It is evaluated once per stretch of cycles in which its signals don't change now,
  so it follows their changes, not the cycles. What is left grows with the cycles: walking the
  clock's changes for its edges, and loading the signals (the 19 an AXI4-Lite port reads).
- In an FST, the first call that reads a signal waits for all of it to load: `values` over 1 µs
  takes 7.2 s for `top.hot`'s 123M changes, as the first screen does.
