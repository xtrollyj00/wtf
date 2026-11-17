## 16. Computed signals and decoding

Signals computed from others, and protocols decoded into records, in the core with a stable
API, so the TUI, the MCP server and later protocols all build on one mechanism. It answers what agents otherwise work out by lining up
answers by hand: transfers, frames, request/response pairs with their delay, and protocol
checks that return only violations.

| file | covers |
| --- | --- |
| [axis.md](axis.md) | `axis`: beats, packets by (tid, tdest), the handshake's rules, the matrix against IHI 0051B |
| [axilite.md](axilite.md) | `axilite`: the five channels and their roles, signals and defaults, `aresetn`, the rules with their clauses, `wr` and `rd`, `timeout` |
| [axi4.md](axi4.md) | `axi4`: bursts by ID, `wburst`/`rburst`, `wr`/`rd` |
| [spi.md](spi.md) | `spi`: bits, words and transfers sampled by time |
| [uart.md](uart.md) | `uart`: bytes and errors sampled by time |
| [streams.md](streams.md) | `frame`, `pair`, `check`, `join`, and the `Decoder` trait |

- **Computed signals are vars.** Everything in wtf names signals by var (rows, search,
  `values`, `find`, `render`, the panel), so a computed signal is a var too, after the file's.
  It is evaluated once into a list of changes (materialized), queried like a file signal, and
  evaluated again on `:e`. One nothing uses any more (no row or undo step shows it, it is no named
  signal or stream field) is dropped after the key, command or `present` that left it unused, and
  made again when named again: a slice named once in a search, a stream's fields it no longer has
  after `:protocol` samples it again. `signals` doesn't list those an earlier query made. Defined by an expression, anywhere a signal is named and in the
  session:
  - bit slices `tdata[63:32]` and concatenation `{a, b, c}`; a part may be a sized SV number
    (`{insn[31], 1'b0}`), and an unsized one (`0`, `'1`) is an error;
  - any expression of section 8 without `past`, `rose`, `fell` or `stable` (which need a clock):
    arithmetic `pc + 4`, `signed(insn[31:20])`, and a comparison, a 1-bit value. The same holds
    for a sampled protocol's fields, sampled at its clock: `next = rvfi_pc_rdata + 4`. Its width
    is the largest its value can need (`pc + 4` on a 32-bit pc is 33 bits). One that can be
    negative (a `-`, a `signed`) holds two's complement, starts in `sdec`, is shown in decimal by
    `records` and reads as signed in expressions, so `imm = signed(insn[31:20])` stays -4 in
    `where imm < 0`. A field names its value in the fields after it and in `when`, as an
    instance's roles do: `f3=insn[14:12] ok=f3 == 0`. Its var is named by the expression with full paths and no spaces
    (`top.pc+4`), and a row shows that whole;
  - in search expressions too, `rose(x)`, `fell(x)`, `stable(x)` and `past(x)` (the value one
    clock cycle earlier, with `@clk`), so a protocol check is a search for its violations:
    `@clk fell(valid) && !past(ready)`. Unlike SVA's `$rose` / `$fell`, `rose` and `fell` need a
    known value before (U -> 0 in reset isn't a fall), so checks don't fire on reset.
- **Records and streams.** Decoding produces streams of records: a record has a start, an end
  and fields (`Record { start, end, fields }`), a stream has a name and a schema of field names
  and widths. A beat, a frame, a transaction and a violation are records.
- **Results.** A stream another stream is made from is intermediate (axilite's `aw`, `w`, `b`,
  `ar`, `r`, joined into `wr` and `rd`), across decodes too:
  a `frame` put on `bus.rd` makes `bus.rd` intermediate. A stream nothing is made from is a
  result, or violations. Each stream keeps the names of the streams it was made from, so this is
  known from each stream's declaration, never from the trace. A decode's results and violations are
  what a person reads: `decode` answers with them first (`bus: records bus.wr 2, bus.rd 8;
  violations bus.errors 2, ...; made from bus.aw 2, ...`), decoding from the interfaces picker adds
  them alone as rows, and a decode's name in `records` means them.
- **Records of a set.** `records` (the tool and `:records`) reads a set of streams: a stream
  (`bus.rd`), a decode's name (`bus`: its results, then its violations), a glob (`bus.*`), or
  several of these separated by spaces. A name of an interface the file has but nothing decoded
  yet is an error saying the line that decodes it. A stream's name comes first: a `pair` `lat`
  is its stream `lat`, and `lat lat.unmatched` its results and violations. A set of several is one table, its records in order of
  start with a `stream` column. Fields share a column when name and kind (width, sign, time or
  text) are the same, in order of first appearance; a name with two kinds is a column per stream,
  `<stream>.<field>`, so `data` as bytes never mixes with `data` as a number. A record of a
  stream without a column's field is empty there. `where` keeps the records of the streams that
  have every column it names (`strb != 0xF` on `bus` keeps writes only); a name no stream in the
  set has is an error listing the columns. `past(x)` is x of the stream's record before; `n` the
  record's number in its stream. `group_by` counts across the set (`resp` over writes and reads).
- **Terms.** Three words, used only in these senses:
  - A **protocol** is what can be decoded: it has roles (`tvalid`, `sclk`), params (`baud`) and
    the streams it makes. `protocols` lists them.
  - A **decoder** is a stateful stage inside a protocol, written in Rust: it reads records in time
    order, keeps state (a frame open until its last beat, a request until its response, bit
    times) and emits records.
  - An **instance** puts a protocol on chosen signals under a name (`:decode`, `[[decode]]`),
    making its streams: `rx` makes `rx.beat`, `rx.frame`, `rx.errors`.
- **Sampling and decoders, chained.** Sampling is stateless: at each edge of a clock where a
  predicate holds (`tvalid && tready`), one record with the fields sampled there (`tdata`,
  `tlast`). A decoder's input is a stream, sampled or another decoder's output, so protocols
  stack:
  ```
  AXIS:  tvalid tready tdata tlast  -> axis.beat, axis.frame (a packet per tid, tdest)
                                       axis.errors (a rule's name, what it saw, error or warning)
  AXI4-Lite: AW W B AR R            -> aw w b ar r -> wr (AW, W, B in order), rd (AR, R), with delay
                                       errors (dropped valid, payload changed while stalled, x or z,
                                       reset, resp not OKAY, early B or R)
  AXI4:  AW W B AR R beats          -> aw w b ar r -> wburst (W to wlast), rburst (R to rlast, per ID)
                                       -> wr (AW and its burst in order, B by ID), rd (AR, R burst by ID),
                                          with delay (latency to B / the first R beat); errors as AXI4-Lite
  SPI:   sclk cs mosi miso          -> bit (at sclk's sampling edge for mode 0 - 3, cs low)
                                       -> word (bits, msb or lsb first), transfer (cs low to high)
  UART:  the line and a baud rate   -> byte (each bit sampled in its middle), errors (framing, parity, break; rule, detail, severity)
  ```
  What each protocol's streams sample, its rules and its params are its own page: [AXIS](axis.md),
  [AXI4-Lite](axilite.md), [AXI4](axi4.md), [SPI](spi.md), [UART](uart.md); the protocols that read
  streams are [streams.md](streams.md).
- **Protocols come in three kinds:**
  - **Sampled protocols**, defined in the session without Rust (`[[protocol]]`, `:protocol`):
    roles, a predicate and fields as expressions of the roles. A definition names no signal and
    no clock, as a built-in's doesn't: an instance (`:decode`) puts it on signals at its clock.
    While an instance samples, a name that is no role, nor a field before it, nor the instance's
    clock is an error, so a protocol can't reach for a signal its instances don't give.
    Defining one again (`:protocol`) samples its instances again at once, with the instances
    made after them (which may stack on their streams), in order; if one fails, the protocol
    stays defined and the error says which failed.
    ```toml
    [[protocol]]
    name = "cmd"
    roles = ["valid", "ready", "cmd"]
    when = "valid && ready"
    fields = { op = "cmd[7:0]", addr = "cmd[39:8]" }

    [[decode]]
    name = "host"
    protocol = "cmd"
    clock = "top.clk"
    signals = { valid = "top.cmd_valid", ready = "top.cmd_ready", cmd = "top.cmd" }
    ```
  - **Built-in protocols** (`axis`, `axilite`, `axi4`, `spi`, `uart`), written in Rust: each a
    `Protocol` the host feeds its roles cycle by cycle, so a record of one packet, a beat, a frame
    or a violation is one cycle's decision.
    One file each, in that order: [axis.md](axis.md), [axilite.md](axilite.md), [axi4.md](axi4.md),
    [spi.md](spi.md), [uart.md](uart.md).
    A `Protocol` names its roles with a default each, so a signal not given is no error: `axis`
    reads `tready` 1, `tkeep` the bytes of `tdata`, `tstrb` the same as `tkeep` and `tlast` 1.
    It names its rules too, each with the spec clause it comes from and its severity (an error or
    a warning), so `errors` carries `rule`, `detail` and `severity`: what happened, and how bad.
    A new one is a new `Protocol`, and `protocols` describes it.
  - **Protocols on streams**, generic, whose input is streams of other instances: `frame`
    (records grouped by a last flag, a gap or a select, with `key` a frame per value of a field,
    side by side, as AXI4's interleaved read bursts by ID; with `data`, the frame's bytes as hex
    text, `tkeep`'s bytes only for AXIS), `pair` (a request stream and a response stream, by
    order or by an ID field, with the delay; unmatched ones are violations) and `check` (rules,
    emitting a violation record where one holds). Checks answer yes or no with counterexamples.
    [streams.md](streams.md) has them, and `join`.
- **Decoders** implement one trait, `Decoder { schema, feed(record) -> records }`, made by name
  (`frame`). `pair`, `join` (several request streams answered by one, in order, as
  AXI-Lite's AW and W by B, or by an ID, as AXI4's B by BID) and `check` work on whole streams
  instead, as functions. `axilite` and `axi4` take a `timeout` param (`timeout=1us`): a request
  still unanswered more than that after it completed is `no response` when the next response
  comes, so a lost B or R doesn't pair every later response with the request before its own.
  `axilite` takes `reset_active` too, `low` or `high`, for its optional `aresetn`.
- **Instances** are kept in the session, so they persist, and agents declare them like the rest
  of the workspace:
  ```toml
  [[decode]]
  name = "rx"
  protocol = "axis"
  clock = "top.clk"
  signals = { tvalid = "top.rx_tvalid", tready = "top.rx_tready", tdata = "top.rx_tdata", tlast = "top.rx_tlast" }
  ```
  This makes the streams `rx.beat`, `rx.frame` and `rx.errors`. Each stream is also a signal
  (`rx.frame`, its segments the records) with a signal per field (`rx.frame.len`), holding the
  field's value during each record.
  `:decode <name> off` removes an instance: its streams and the rows showing them go (an undo
  step for the rows; the signals go once nothing holds them). An instance another reads (its
  signals, clock or rules name a stream of it, or a stream of its is made from one) stays, the
  error naming the readers to remove first. `:decode` under a name taken replaces that instance;
  the decode form's edit (section 5 "Decodes") also drops the rows of streams it no longer makes
  and makes the instances that read it again.
- **TUI.** A stream row draws like a bus, a segment per record with a short label
  (`#3 64B`); `Enter` expands it into a row per field. Violation records draw in their
  severity's colour: an error red (`unknown`), a warning in `warning` (section 19); a stream
  without a `severity` field (`check`, a pair's `unmatched`) red. Field
  signals search like any other (`/len == 64`). `:decode <name> <protocol> <role>=<signal>...`
  adds an instance.
- **MCP.** Streams and fields are signal names for every tool; `records` lists a stream's records
  and `decode` declares instances in the workspace (section 15).
- Decoders need the trace's history from the start, so a stream is evaluated from the trace's
  start.
