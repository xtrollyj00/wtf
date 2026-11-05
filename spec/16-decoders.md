## 16. Computed signals and decoding

Signals computed from others, and protocols decoded into records, in the core with a stable
API, so the TUI, the MCP server and later protocols all build on one mechanism. It answers what agents otherwise work out by lining up
answers by hand: transfers, frames, request/response pairs with their delay, and protocol
checks that return only violations.

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
  `ar`, `r`, joined into `wr` and `rd`; axis's `beat`, framed into `frame`), across decodes too:
  a `frame` put on `bus.rd` makes `bus.rd` intermediate. A stream nothing is made from is a
  result, or violations. Each stream keeps the names of the streams it was made from, so this is
  known from the steps, never from the trace. A decode's results and violations are what a person
  reads: `decode` answers with them first (`bus: records bus.wr 2, bus.rd 8; violations
  bus.errors 2, ...; made from bus.aw 2, ...`), decoding from the interfaces picker adds them
  alone as rows, and a decode's name in `records` means them.
- **Records of a set.** `records` (the tool and `:records`) reads a set of streams: a stream
  (`bus.rd`), a decode's name (`bus`: its results, then its violations), a glob (`bus.*`), or
  several of these separated by spaces. A stream's name comes first: a `pair` `lat` is its
  stream `lat`, and `lat lat.unmatched` its results and violations. A set of several is one table, its records in order of
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
  AXIS:  tvalid tready tdata tlast  -> axis.beat -> axis.frame
                                       axis.errors (valid dropped, payload changed while stalled)
  AXI4-Lite: AW W B AR R            -> aw w b ar r -> wr (AW, W, B in order), rd (AR, R), with delay
                                       errors (dropped valid, payload changed while stalled, resp != OKAY)
  AXI4:  AW W B AR R beats          -> aw w b ar r -> wburst (W to wlast), rburst (R to rlast, per ID)
                                       -> wr (AW and its burst in order, B by ID), rd (AR, R burst by ID),
                                          with delay (latency to B / the first R beat); errors as AXI4-Lite
  SPI:   sclk cs mosi miso          -> bit (at sclk's sampling edge for mode 0 - 3, cs low)
                                       -> word (bits, msb or lsb first), transfer (cs low to high)
  UART:  the line and a baud rate   -> byte (each bit sampled in its middle), errors (framing, parity, break)
  ```
  An AXI-Lite or AXI4 port that only writes or only reads is decoded too: a side none of whose
  roles is given (explicitly or under the prefix) is left out, and the instance makes only the
  other side's streams. A side given in part is still an error, naming the missing roles; where
  the file has that protocol's ports, the error ends with the line that decodes them (`no signal
  for tvalid (the file has: decode rx axis clock=top.clk prefix=top.)`).
  SPI and UART sample by time, not at a clock: their decoder reads the changes of the pins (for
  SPI, of `{cs, sclk, mosi, miso}`) as records and keeps the edges and bit times itself, so they
  take no `clock`. Their settings are params of the instance: SPI's `mode`, `bits`, `order`,
  `cs_active` (`low`, the default, or `high`); UART's `baud`, `bits`, `parity` (`none`, `even`,
  `odd`), `stop`, `idle` (`high`, the default, or `low`, which reads the line inverted as RS-232
  levels do). An SPI bit lasts to the next sampling edge, the last of a transfer to sclk's next
  edge. UART checks every stop bit; the line low for a whole frame is a `break`, with no byte.
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
  - **Built-in protocols** (`axis`, `axilite`, `axi4`, `spi`, `uart`), written in Rust as a list
    of steps: sample a stream, feed a signal's changes, run a decoder on a stream, join or check
    streams. A new one is a new list of steps, and `protocols` describes it from them.
  - **Protocols on streams**, generic, whose input is streams of other instances: `frame`
    (records grouped by a last flag, a gap or a select, with `key` a frame per value of a field,
    side by side, as AXI4's interleaved read bursts by ID; with `data`, the frame's bytes as hex
    text, `tkeep`'s bytes only for AXIS), `pair` (a request stream and a response stream, by
    order or by an ID field, with the delay; unmatched ones are violations) and `check` (rules,
    emitting a violation record where one holds). Checks answer yes or no with counterexamples.
- **Decoders** implement one trait, `Decoder { schema, feed(record) -> records }`, made by name
  (`frame`, `spi`, `uart`). `pair`, `join` (several request streams answered by one, in order, as
  AXI-Lite's AW and W by B, or by an ID, as AXI4's B by BID) and `check` work on whole streams
  instead, as functions; built-in protocols use them as steps too.
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
  (`#3 64B`); `Enter` expands it into a row per field. Violation records draw red. Field
  signals search like any other (`/len == 64`). `:decode <name> <protocol> <role>=<signal>...`
  adds an instance.
- **MCP.** Streams and fields are signal names for every tool; `records` lists a stream's records
  and `decode` declares instances in the workspace (section 15).
- Decoders need the trace's history from the start, so a stream is evaluated from the trace's
  start.
