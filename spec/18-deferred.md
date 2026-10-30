## 18. Deferred

To be decided once prototypes exist:

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
- More in the config file (section 19): default radix, key remapping; picking the theme from the
  terminal's background (OSC 11).
- Performance: targets and the rules that follow from them. Draft below; milestone 6 has a first
  measurement (clocked searches), the targets are not settled yet.

### Known limits

Shortcuts taken so far, to fix when they get in the way.

- A named signal can't be removed, only defined again (`:signal`); removing vars would renumber
  them.
- Every slice or concatenation named, even once in a search, stays a var until the trace is
  reopened, and is listed in the panel and by `signals`.
- The signal panel is rebuilt when computed vars appear, losing its highlighted item and Space
  marks.
- Sampling a stream again with fewer fields leaves the old field vars, holding no records.
- Arithmetic (section 8) takes operands up to 64 bits and results up to 127 (an `i128`); a
  wider one is an error suggesting a slice.
- In `c ? a : b`, any unknown bit the condition reads makes the result x, even where SV's `&&` /
  `||` would still know the answer (`0 && x` is 0 in SV).
- Applying a session (also `present`) adds its signals, protocols and instances to the ones there;
  none are removed.
- `check` rules are violations written directly (`valid && !ready`), one record per cycle; a rule
  over records (a frame too long) needs a decoder.
- Streams are worked out over the whole trace at once (spec §16 "Known limit").
- A stream row's label is `#n` with `len` (`64B`) and the text fields (delay, rule, kind); there is
  no per-protocol label.
- A frame's `data` is text (`de ad be ef`), held for every frame: big for long frames, and searched
  only whole (`==`), not for a part.
- `join` pairs strictly in order (AXI-Lite has no IDs): after a lost response, the transactions
  after it are still paired, one off.
- The status line's `match 3 of 157` counts matches only up to 100 000 (`100000+`), so `n` stays
  quick on huge traces; past that the total isn't known.
- A name that matches only in the other HDL's form (`HEADERS_VLD`, `gen(4)` for `gen[4]`, section
  8) is looked up by putting every path in that form, each time: O(signals) per such name.
- A slice or concatenation SPI reads its pins through (`{cs, sclk, mosi, miso}`) stays a var, as
  every computed signal named.
- Composite types are shown as the simulator dumps them (fixtures `types-*`, made by
  `tests/data/types.sh`; pinned in `core/tests/types.rs`):
  - a struct, record, union or array dumped as a scope is one row with a whole value (section
    6, "Composite rows"), worked out from its signals at each lookup, O(signals in it); with a
    text field its value is text, so it compares only whole (`==`); a union's value joins its
    members, each the same bits (`word` is `0xC3C3`);
  - a packed struct dumped as one var (Verilator without `--trace-structs`) has no field names
    (`pkt.state`) and no enum names, and `x[1]` of a flat packed 2-D vector is bit 1, not
    element 1;
  - a VCD from Verilator writes a struct as a module scope, so its fields are shown by their leaf
    (`hi`, not `pkt.pair.hi`), listed scopes first, and adding it takes only its direct fields;
    it is no composite row, nor is a packed struct dumped as one var;
  - a module or instance named as a signal (`value_at top.cpu`) is an error listing its signals;
    finding the scopes a name ends with goes through every path, O(signals), only on such an
    error;
  - wellen 0.25.7 can't read a GHW file (GHDL) with a multidimensional array ("not yet
    implemented"): opening it is an error, though the panic's message still goes to stderr (the
    panic hook is process-wide, so it isn't swapped around the read); GHDL's VCD has no records
    or arrays at all;
  - wellen 0.25.7 misnames the elements of a Verilator FST's 2-D unpacked array whose indices are
    equal (`grid[1][1]` becomes `types.grid[1].grid[1].`): an upstream bug.

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

#### Composite rows

`gen --size 1G --format fst --mem 1024` adds `top.mem[0..1023]`, 32 bits each, one element written
a cycle (12.0M cycles), shown as one row (section 6). `wtf-bench run` on the same machine
(2026-10-30):

| | 1 GiB FST | target |
|---|---|---|
| `add top.mem` (loads its 1024 signals) | 0.75 s | 1 s |
| its value at the end (32 Kbit) | 0.5 ms | |
| `top.mem` folded, whole trace, `wtf --snapshot` | 0.98 s | the first screen (1.18 s here) + 50 ms |
| `top.mem` open one level, the same | 0.98 s | as above |

A composite's value is never stored: each value, change and changes-in-a-column is worked out
from its signals when asked (O(signals in it) a lookup), so the cost is loading its signals, as
for any rows.

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

#### Agents with and without wtf

`wtf-bench agent run --no-mcp` runs the fix tasks without wtf: the agent has the RTL, the
testbench, `sim.log` and `make sim`, and debugs with prints and reruns. Haiku (claude-haiku-5-5),
5 runs per arm, `--max-sims 3`, `--jobs 5`, wtf d0ebd30 (2026-10-25); `pass` is fixed and every
answer key right, `evidence` the share of keys right, costs are medians:

| task | wtf | pass | fixed | evidence | `make sim` | denials | tokens | cost | wall |
|---|---|---|---|---|---|---|---|---|---|
| eth-parser-headers | on | 5/5 | 5/5 | 1.00 | 1 | 1.4 | 0.68M | $0.029 | 134 s |
| | off | 2/5 | 5/5 | 0.60 | 1 | 8.6 | 1.73M | $0.058 | 270 s |
| eth-parser-stall | on | 5/5 | 5/5 | 1.00 | 1 | 1.2 | 0.40M | $0.014 | 76 s |
| | off | 5/5 | 5/5 | 1.00 | 2 | 5.8 | 0.76M | $0.024 | 136 s |
| head-trimmer-boundary | on | 5/5 | 5/5 | 1.00 | 1 | 1.2 | 0.30M | $0.012 | 85 s |
| | off | 3/5 | 5/5 | 0.80 | 3 | 8.0 | 0.63M | $0.024 | 203 s |
| axil2mi-stale-addr | on | 2/5 | 5/5 | 0.80 | 1 | 0.6 | 0.37M | $0.015 | 62 s |
| | off | 1/5 | 5/5 | 0.46 | 2 | 5.0 | 0.38M | $0.015 | 57 s |
| ibex-checksum (wtf 916c847, 2026-10-26) | on | 3/5 | 4/5 | 0.75 | 1 | 4.0 | 2.02M | $0.133 | 277 s |
| | off | 1/5 | 5/5 | 0.40 | 2 | 7.8 | 3.46M | $0.139 | 242 s |

`axil2mi-stale-addr` is the hard one: an AXI4-Lite to MI converter whose write address is sampled
outside its handshake, so a write queued behind another goes one word up; the test only reads the
registers back at the end, 3.5 - 12.8 us after the bad writes, and never says when they were made.

`ibex-checksum` was meant to be harder: Ibex (maxperf, 3 stages), 10k lines of RTL, whose store
decoding lacks `rf_ren_b_o`, so an adjacent `lw rX; sw rX` may store the load's address when the
store is granted in the load's response cycle; the test only sees a wrong checksum 10 000 cycles
later. It is still found by reading: without wtf all 5 runs fixed it, grepping `rf_ren_*` across
the decoders (OP and BRANCH set it, STORE not), and a first version (`rf_rd_b_hz & ~lsu_we`,
beside `rf_rd_a_hz`) was fixed by both arms in one run each. wtf again gets the evidence (0.75
against 0.40: the first bad write and how many); its miss on the fix was a run that hit the
600 s timeout. One run with wtf counted 12 writes, the 6 bad ones and their copies, from a diff
against a passing run.

- Every run fixed its bug, with wtf or without: these bugs are found from the log and the code,
  even axil2mi's (260 lines of state machine give the cause away). The tasks measure what wtf
  adds beyond the fix, not whether the fix needs it; that needs harder tasks.
- wtf gets the evidence: 17 of 20 runs had every key right with it, 11 of 20 without. Without it,
  the times were mostly `unknown`: the agents could not read `trace.fst`.
- With wtf an agent ran `make sim` once, to confirm its fix. It took 0.4 - 0.6 times the time and
  0.5 - 0.6 times the cost on eth-parser-headers, eth-parser-stall and head-trimmer; on axil2mi
  about the same.
- The axil2mi misses with wtf named a later bad write (1064ns), the register 0x24 write for the
  first, or 0x24's one clock early (9248ns). No run used `pair`, which with `records where
  req_addr != resp_addr` lists the 19 bad writes in one call. A batch an hour earlier, with
  nearly the same tool docs, gave 5/5 on axil2mi: batches of 5 vary by that much.
- The questions now say which cycle they mean in signals (the edge that starts the cycle in which
  valid and ready are high), which lifted the runs without wtf from 4 to 11 of 20 against ef0ae14.
- Without wtf the agents tried to read `trace.fst` some other way (`fst2vcd`, Python readers),
  most of their sandbox denials.
- Agents decoded AXI-Lite and AXIS (`decode`, `records ... where addr == 0x24`) but read the MI
  side and internal state with `find` and `show`; none used `protocol` or `pair`.
- Earlier runs (wtf d89ab69, 41f64da) gave the same picture. They showed a big answer costs on
  every later turn, as the context is read again: two answers cut at 24 KB (`signals` of a scope,
  `records` of frames with their data) cost more than the calls, and 41f64da made them 4.8 and
  11 KB. eth-parser-stall then asked "when the parser stopped taking RX data", a cycle's start or
  its ending edge 5 ns later; agents with wtf gave the start, as `find @clk` reports it, those
  without the edge, as cocotb prints it, so wtf scored 3/5 against 4/5 for the wording; it now
  asks for the cycle's start, and the grader takes a signal by its last name.
