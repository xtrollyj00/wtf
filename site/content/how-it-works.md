+++
title = "How it works"
description = "The model behind wtf on one page: rows, names, decodes and streams, search, sessions. Read it once and the keys make sense."
+++

wtf draws a trace as rows, one per signal or per decoded bus, with time running left to
right. Names, search, decodes and sessions all work on those rows. This page explains the model
with `docs/demo.vcd`, an AXI4-Lite port with one write and two reads, the second of which fails. Copy the
trace alone, without the session saved next to it, so you start from the plain signals:

```sh
cp docs/demo.vcd tour.vcd
wtf tour.vcd
```

## Rows, groups and cursors

A row is one signal. `j` and `k` select a row; its name turns bold, with a background across the
row. `r` changes its radix, `dd` removes it, and `u` takes the change back.

A group is a set of rows under a header. Select rows with `V`, `j` and `k`, then `:group write`:

{{ <screen name="how-rows" caption="`write` holds three rows. Marks 2 and 3 sit at 45 ns and 65 ns, with the ruler showing the time between them." /> }}

`Enter` on a header folds the group. A folded group shows one row, with activity wherever any of
its signals changes.

Cursor 1 is the cursor. It is where the value column reads values, and `h`, `l`, `w`, `b`, `/`
and `:100ns` move it. Cursors 2 to 9 are marks. They stay where you put them while you look
around. `m` puts a mark at the cursor with the lowest free number, and `3m` puts mark 3 there.
`'2` jumps to mark 2, `Tab` goes to the next mark in time, and `''` goes back to where the cursor
was before the last jump.

A struct, record or array that the trace writes as one signal is one row too, named as in the
HDL. Its value is its fields joined, the first the most significant. `Enter` opens it one level,
into a row for each field.

## Names

A signal has a full path, `top.s_axi_awaddr`. Its short name is the last part of the path,
`s_axi_awaddr`, and it works wherever only one signal ends that way. Search and `:decode` take the
short name. `:add` and `:rm` take a full path or a pattern such as `top.s_axi_aw*`; a short name
there gives `no match`. `Space` `f` finds a signal by its short name and adds it.

A bit is `s_axi_awaddr[3]`, a slice is `s_axi_awaddr[7:4]`, and `{a, b}` joins two signals. A
computed signal names an expression and is a signal like any other:

```
:signal lo = top.s_axi_awaddr[3:0]
:add lo
```

The [reference](@/reference.md#names) has the VHDL forms and the scope rules.

## Decodes, streams and records

A decode reads an interface's signals and writes what happened on it as records. `Space` `d` `d`
lists the interfaces wtf found in the trace and names the decode after the signals' prefix,
`s_axi` here. The command form lets you pick the name:

```
:decode bus axilite clock=top.aclk prefix=top.s_axi_
```

`axilite` is the protocol. `bus` is the name of this instance, and each of its streams takes the
name as a prefix: `bus.wr`, `bus.rd` and `bus.errors`. A write is a record with its address, data,
strobe, response and delay. `bus.errors` holds the records where a protocol rule broke.

`bus` also keeps the five channels (`bus.aw`, `bus.w`, `bus.b`, `bus.ar` and `bus.r`) that
make up the writes and reads, and `bus.wr.unmatched` and `bus.rd.unmatched` for a request or
response that found no partner. `:records bus` leaves the channels out and lists the writes, the
reads and the errors.

```
:records bus
```

lists them in one table, in time order, with a column for each field. In the demo it shows one
write, two reads and one error. Type `/` and `resp != 0`, and the table keeps the read that
failed: `rd #1` at 175 ns, answered with `0x2`.

Each record is also a signal. A stream's row draws a segment per record, and `Enter` on the row
expands it into a row per field:

{{ <screen name="how-stream" caption="`bus.wr` at 60 ns: the record is `#0`, and each field is a row of its own, with its value at the cursor." /> }}

Fields search like any other signal.

An interface of your own uses `:protocol`. It names roles and a `when` condition, and each cycle
where the condition holds is one record with the fields you list:

```
:protocol aw roles=valid,ready,addr when=valid && ready addr=addr
:decode aw1 aw clock=top.aclk valid=top.s_axi_awvalid ready=top.s_axi_awready addr=top.s_axi_awaddr
```

The [recipes](@/recipes.md) use it for request and response pairs and for checks.

## Search

`/` opens a search line on the selected row. A value matches by number in any radix, so `/0x10`
on `s_axi_awaddr` moves the cursor to the first time the address is 0x10. `Enter` jumps there,
and the status line counts the matches, `match 1 of 1`. `n` and `N` step through them.

An expression searches several signals at once:

```
/@aclk s_axi_bvalid && s_axi_bready
```

`@aclk` makes the search sample at the clock. A match is then each clock cycle where the
expression holds at the rising edge, the way a flip-flop samples it, so one handshake is one
match. Without `@aclk`, a match is the first time the expression becomes true. `past(x)` is the
value one cycle earlier, and `rose(x)`, `fell(x)` and `stable(x)` compare with it. So a protocol
rule is a search for its violations: `@aclk fell(valid) && !past(ready)` finds a valid that
dropped before its ready. The [keys](@/keys.md) and the [reference](@/reference.md#expressions)
list the operators.

## Sessions

A session is what to show of a trace. It keeps the rows, groups, radixes and colours, the
expanded buses and streams, the cursor, the marks with their names, the zoom, and the decodes and
computed signals. It is a TOML file next to the trace, `tour.vcd.session.toml`. `:w` saves it.
When you open the trace again, wtf loads it. `wtf -S shared.toml tour.vcd` loads one session
instead, for a set of runs that share a layout.

When you rerun the simulation, wtf reloads the trace once the file has stopped changing for a
second. It keeps the rows, cursor and zoom, and runs the decodes again. `:e` reloads by hand.

Changes to the rows or the marks show `[+]` on the status line. `:q` refuses to quit while there
are unsaved changes, `:wq` saves and quits, and `:q!` quits without saving. Moving the cursor and
zooming don't count as changes. `u` undoes a change to the rows or the marks, and `Ctrl-r` redoes
it. The [reference](@/reference.md#commands) lists the commands.

`--snapshot` prints one screen as plain text and exits, and `--keys` presses keys first. The
[recipes](@/recipes.md#debug-a-failing-test-in-ci) use it for CI logs and bug reports.
