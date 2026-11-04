+++
title = "Recipes"
description = "Count bus errors, measure latency, catch a dropped valid, decode a protocol of your own."
+++

Each recipe is a few commands on a trace from the repository, so you can follow along. Type them
after `:` in the window. Decodes are kept in the trace's session, and `:e` runs them again on a
new simulation, so you make them once. The first recipes use the demo trace, copied as in
[how it works](@/how-it-works.md):

```sh
cp docs/demo.vcd tour.vcd
wtf tour.vcd
```

## Count the bus errors

An AXI4-Lite port answered something with an error, and you want to know which transfer.
`tour.vcd` has one. Decode the port:

```
:decode bus axilite clock=top.aclk prefix=top.s_axi_
```

`Space` `d` `d` does the same from the list of found interfaces, and names the decode `s_axi`,
after the prefix. The decode joins the handshakes into writes and reads, each with its address, data,
response and delay, and checks the handshake rules as it goes: a dropped valid, data that
changed while waiting, an error response.

```
:records bus
```

lists them all in one table, in time order. Type `/` and a filter, here `resp != 0`:

{{ <screen name="recipe-errors" caption="The second read, of `0x40`, was answered with `0x2`, SLVERR. `Enter` puts the cursor on it." /> }}

`Tab` goes through the decode's tables one at a time: `errors` holds the rule broken at each
cycle. To count each rule, press `s` until the `rule`
column is the sorted one, then `=`.

Save the layout with `:w`, so the next recipes find the decode.

## A protocol of your own

Most designs have handshakes no standard covers. Say a request is a cycle where `req_valid` is
high, and its `req_id` says which one it is (`tests/data/reqresp.vcd`). Open it:

```sh
wtf tests/data/reqresp.vcd
```

Describe the request once, with roles in place of signal names:

```
:protocol hs roles=valid,id when=valid id=id
```

`when` is a [search expression](@/reference.md#expressions), sampled at the clock; every field
after it is a value to keep. Then put it on each side:

```
:decode req hs clock=clk valid=req_valid id=req_id
:decode resp hs clock=clk valid=resp_valid id=resp_id
:add req resp
```

{{ <screen name="recipe-protocol" caption="Each request and response is a record, drawn as a row under the signals it came from." /> }}

A field can compute: `next=pc + 4`, `imm=signed(insn[31:20])`, and later fields can name earlier
ones.

## How long did each request take

Pair the requests with their responses. With `id=` a response answers the request with the same
id; without it, the oldest one waiting:

```
:decode lat pair request=req response=resp id=id
:records lat lat.unmatched
```

{{ <screen name="recipe-latency" caption="`delay` is the time from request to response. Request 3 was never answered, and the response with id 0 answers nothing." /> }}

`s` sorts by a column, `S` the other way: the slowest first. The fields are signals too, so
`/lat.delay > 20ns` in the waveforms stops at each slow one.

The AXI decodes pair their own: `bus.wr` and `bus.rd` have a `delay` already.

## Catch a dropped valid

A sender must hold `valid` until `ready` takes the data. A `check` decode turns a rule into a
record at each cycle it holds, so a broken rule is something you can list and jump to:

```
:decode c check clock=clk dropped=fell(valid) && !past(ready)
:add c
```

{{ <screen name="recipe-dropped" caption="`valid` fell at 85ns while `ready` was low: the data `0x23` was never taken." /> }}

`fell`, `rose`, `stable` and `past` look at the cycle before. A `check` can hold several rules,
each named: `dropped=... changed=...`, and the record says which one fired. The built-in
protocols check theirs this way, in their `errors` table.

## The other built-in decoders

`axilite` is one of five. Each takes the signals' common prefix, or the names of its roles, and
a clock where the protocol has one. These lines run on the fixtures in `tests/data/`:

```
:decode s axis clock=top.clk prefix=top.            (axis.vcd: frames, with the data bytes)
:decode m axi4 clock=top.aclk prefix=top.m_axi_     (axi4.vcd: writes and reads with bursts)
:decode s spi sclk=top.spi_clk cs=top.spi_cs_n mosi=top.spi_mosi miso=top.spi_miso mode=0
:decode u uart line=top.uart_tx baud=115200         (uart.vcd: bytes and framing errors)
```

SPI and UART sample by time and take no clock. `mode=` is the SPI mode, 0 to 3, and `baud=` is the
UART's baud rate; `bits`, `order`, `parity` and the other settings are in the
[reference](@/reference.md#decode). `:records s` lists the result as in the first recipe.

## Name the states of a state machine

A state register is a number in the trace. If the simulator wrote an enum table, as Verilator does
for a SystemVerilog enum, the row shows the names already. `tests/data/enum.fst` has one:

```sh
wtf tests/data/enum.fst
```

`state` shows `IDLE`, `READ`, `WRITE` and `DONE`, and `/READ` finds the cycles in that state. A
value the table doesn't name stays hex. A trace without a table gets names from you:

```
:names count 0=START 1=RUN
```

The row switches to the `enum` radix and `/RUN` searches by name. Names are kept in the session.
`:names count off` removes them.

## Jump with the picker

`Space` `f` opens the picker on signals. `Ctrl-s` inside it goes to the next source: marks,
records, commands, sessions, the trail and the interfaces. `:pick marks` opens it on the marks, and
`:pick records bus.rd` on the reads; `Enter` puts the cursor there.

Two keys copy what you are looking at. `yy` copies the selected signal's value at the cursor, or
the record on a stream's row, and `yt` copies the cursor's time. In a design with several `state`
signals, `:scope top.cpu` makes names resolve under `top.cpu` first, so `state` means
`top.cpu.state`. The scope is saved in the session.

## Debug a failing test in CI

A test failed on the simulation server, and the trace is all you have of the run. Print the screen
that shows the failure into the job's log, so you can read it there without copying the trace off
the server. `--snapshot` draws one screen as text and exits, so it needs no terminal:

```sh
wtf tour.vcd --snapshot 100x12 --glyphs ascii --keys ':records bus<CR>'
```

`--keys` presses keys before the screen is drawn, in Vim's notation: `<CR>` is Enter. The
session saved with `:w` in the first recipe has the bus decoded, so `:records bus` finds it. `--glyphs ascii` draws the
lines with 7-bit characters, so the log has no box drawing that a plain terminal or a mail client
would garble. The command prints:

```
Name            | Value      |      40ns      60ns        85ns    100ns     120ns     140ns
  +- records - bus ------------------------------------------------------------------ 4 records -+
  | all  wr  rd  errors  wr.unmatched  rd.unmatched  aw  w  b  ar  r                             | -
a | > where / filters on delay, addr, data, strb, resp, rule, kind                               |
  |----------------------------------------------------------------------------------------------|
> |   stream    n    start    length    delay    addr    data        strb    resp    rule    kin |
  |>  wr        #0   55ns     40ns      20ns     0x10    0xCAFEF00D  0xF     0x0                 |
v |   rd        #0   115ns    30ns      20ns     0x10    0xCAFEF00D          0x0                 |
  |   rd        #1   175ns    30ns      20ns     0x40    0x0                 0x2                 |
  +-------------------- Enter jump  / filter  s S sort  = group  h l scroll  Tab set  Esc close -+ -
                |            |
 NORMAL   signals >  top.aclk                                              85ns  2ns/col  tour.vcd
```

Leave out the height (`--snapshot 100`) to print as many rows as the screen needs.

To print a layout you saved somewhere else, load its session with `-S`. It replaces the
`<trace>.session.toml` next to the trace:

```sh
wtf -S ci/bus.session.toml sim/run.fst --snapshot 100 --glyphs ascii
```

Commit the session next to the testbench. Each run in CI then prints the same rows, so the logs of
two runs compare line by line.

## Rerun and look again

You rerun the simulation, after a fix or with another seed, and want the rows and cursors you had
on the new trace. Save the layout with `:w` first, then rerun. The window reloads the trace by
itself when the file has stopped changing for a second, and `:e` reloads at once. Rows, cursor,
zoom and decodes come back; signals the new run doesn't have are dropped and listed on the status
line. [How it works](@/how-it-works.md#sessions) has the full account.

Reload clears undo, so the saved session is the way back to the layout you had.

## Look inside a struct

A packed struct is one signal in the HDL, so the trace holds it as one row. `Enter` opens the row
one level: the struct's fields, and each field that is a struct too. The fixture
`tests/data/types-verilator-structs.fst` has `pkt`, a struct of a valid bit, a two-bit state and a
pair of nibbles. Open it, add the signal and press `Enter` on its row:

```sh
wtf tests/data/types-verilator-structs.fst
```

```
:add types.pkt
```

{{ <screen name="recipe-struct" caption="`pkt` opens into `valid`, `state` and `pair`. `pair` is a struct too: `Enter` on it opens `hi` and `lo`. The value `0x5A5` is the fields joined, first the most significant." /> }}

An array opens the same way. `types.pkts` opens into `pkts[0]` and `pkts[1]`, and each of those into
its fields. The row's value is the fields' values joined, so `pkt` is `0x5A5`: valid 1, state `BUSY`
and pair `0xA5`.

What a row shows depends on what the simulator wrote:

| Simulator and dump | What the row shows |
|---|---|
| Verilator FST with `--trace-structs` | the fields, named, as above |
| Verilator VCD with `--trace-structs` | the struct is a scope, so its fields show under their own names (`hi`), with no row for the struct |
| Verilator without `--trace-structs`, Icarus | one vector with no field names. `Enter` opens it into bits. Name a field with a slice: `:signal state = types.pkt[9:8]` |
| nvc | the fields, named; the record's own value is text (`'{valid:0, state:idle, ...}`), and an enum is its name as text (`idle`) |
| GHDL | its VCD leaves records and arrays out. Dump GHW instead (`--wave=`): records open into their fields. A GHW with a multidimensional array can't be read yet |

The fixtures `tests/data/types-*` hold each simulator's dump of the same design. `tests/data/types.sh`
makes them.

## Share what you see

Send the session `:w` saved, `<trace>.session.toml`, with the trace, and whoever opens the trace
gets your rows and cursors. A session under another name opens with `-S`:

```sh
wtf -S bus-debug.session.toml run.fst
```

For an issue, paste a snapshot from the [CI recipe](#debug-a-failing-test-in-ci) in a code block,
printed in ASCII. `--ansi` keeps the colours as escape codes, for a terminal that shows them.

A session is plain TOML with signal names, times and the layout, and no waveform data. Open
it in an editor and check it before you send it.
