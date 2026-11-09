+++
title = "Reference"
description = "Names, patterns, times, radixes, search expressions, decoders, records and commands, as wtf explains them."
+++

<!-- Written by `UPDATE_SITE=1 cargo test --test mcp site_tools` from wtf's help topics; don't edit. -->

One topic per concept, word for word as wtf explains it: `:help <topic>` shows one in the window, and an agent reads it with the `help` tool. The [tour](@/get-started.md) and the [recipes](@/recipes.md) show them at work.

## names

How every tool names a signal.

**Form**

- The full path, `top.dut.s_axi_awaddr`, or the end of one if only one signal ends that way,
  `s_axi_awaddr`; an ambiguous name is an error listing the candidates.
- One bit `data[3]`; a slice `tdata[63:32]`; a concatenation `{a, b}`.
- VHDL or Verilog forms alike, in any case: `gen(4).x` or `gen[4].x`, `data(7 downto 4)` or
  `data[7:4]`.
- With a radix wherever tools take signals: `awaddr:dec` (`help radix`).

**Recipes**

- Find a name you don't know: `signals` with a pattern (`help patterns`).
- Short names under one instance: `command` `scope top.dut` (or `workspace` `scope`); then
  `s_axi_awvalid` is `top.dut.s_axi_awvalid` in every tool. `scope` with no path: none.
- A field of a packed struct a VCD dumped as one vector: slice it, `pkt[15:8]`.
- A name of your own for an expression: `command` `signal op = cmd[7:0]`, then `op` everywhere.

**Details**

- A struct, record or array, or part of one (`pkt`, `pkts[1]`, `grid[1]`), is a signal of its
  whole value: its fields joined, the first the most significant (`pkt == 0x5A5`, `pkt[10]`);
  with a text field (an nvc enum) text, `'{valid:1, state:busy, pair:'{hi:A, lo:5}}`. `value_at`
  adds a line per field.
- A decoded stream is a signal (`bus.wr`, its value the record's number `#n`), and so is each
  of its fields (`bus.wr.addr`, `z` between records), for `find`, `values` and rows alike.
- An unknown name says what the trace has instead: a scope's signals, a struct's or array's
  fields, a signal without fields (a packed struct in a VCD: slice it), or the nearest scopes
  and signals.

## patterns

Patterns: many signals or streams at once, and searching the design.

**Form**

- A pattern is a regex in slashes, found anywhere in the path in any case, the path
  lowercase with indices in brackets whichever HDL wrote it: `/uart/`, `/_valid$/`,
  `/^top\.cpu\./` (a scope's signals), `/mem\[0\]/`. `signals`, `add`, `rm` and
  `records` take them; a bare word is always a name.

**Recipes**

- Find a signal by part of its name: `signals` `/fifo.*full/`.
- The design's top levels, or all of it folded to fit: `signals` with no pattern.
- A struct's fields: `signals` `/pkt\./`.
- All of a decode's streams: `records` `/^bus\./`.
- Clear the workspace's rows: `command` `rm /.*/`.

**Details**

- `signals` lists what a pattern matches: a scope it names is a line of its own,
  `top.cpu (scope of 3 signals)`, a struct is one line, and an answer too long is folded to a
  line per scope below the deepest level that fits; ask again with a pattern for that scope.
- A pattern that matches nothing says so, with the nearest scopes and signals.

## times

Times you give and times you get back.

**Form**

- A number with its unit, no quotes, no space: `120ns`, `1.5us`, `3.386665ms`, `2595ps`; any
  unit, whatever the answers use. A plain number is file ticks.
- Units: s, ms, us, ns, ps, fs.

**Recipes**

- A window: `from` `120ns` `to` `1.5us`; both ends are included.
- Exact times, unrounded: the call's `unit` `fs`.
- All answers in one unit: `command` `unit ns`; `unit auto` goes back to the clocks'.

**Details**

- Answers use one unit per workspace, so they compare across calls: once a search names a clock
  (`@clk`, also in a protocol or a decode), the unit is the largest that keeps that clock's
  shortest phase at 1 or more (`ns` for a 10 ns or a 200 MHz clock). A call's `unit` argument
  overrides it for that call.
- Times are rounded to a thousandth of the unit (`2595ns` for 2595.000001ns). A time given back
  stands for every tick that rounds to it: `value_at` reads after them all (as `find` `show` and
  `records` sample), and says a change among them (`data = 0x5 (0x4 earlier in 2595ns)`);
  `from` / `to` take them all, and `find` `from` a match starts after it.
- Durations a decoder works out (a pair's `delay`) are times: shown in the unit, and compared
  with a time in `where` (`delay > 40ns`).

## radix

How values are shown, and the colours of rows.

**Form**

- hex, bin, dec, sdec (signed), oct, ascii, float, fixed point like q4.12, and enum (the names
  the file gives values, as `state` READ).
- Colours: red orange yellow green cyan blue magenta gray; `off` removes one.

**Recipes**

- Every signal of a call in decimal: the call's `radix` `dec`.
- One signal in another radix: `data:dec`, wherever tools take signals.
- A row with a radix and a colour: `awaddr:hex:red`, `wvalid:red` (`help workspace`).

**Details**

- The default is enum where the file names values, otherwise hex; 1-bit signals always show
  0 / 1 / X / Z.

## expressions

Search expressions, for `find` (and `when`, fields and `where` in decoding).

**Form**

- Written as in SystemVerilog: `valid && ready && data == 0x40`, `state == 3 || state == 5`,
  `!rst`. Compare with `==`, never `=`.
- Over time, SVA's functions without the `$`: `rose(x)`, `fell(x)`, `stable(x)`, `past(x)` (x
  one cycle earlier). No others: an edge is `rose(x)`, not `rising` or `posedge`.
- `@<clock> ` first samples each cycle: `@top.clk valid && ready`. Without it, a match is where
  the expression becomes true.

**Recipes**

- Each transfer, with its data: `@clk valid && ready`, `show` `["data"]`; `count_only` counts.
- Stall cycles: `@clk valid && !ready`, `count_only`.
- The rising edges of a signal: `rose(irq)`; `show` lists what else was there at each.
- The cycles a condition holds: `@clk state == 3` (a row per run of cycles).
- A state machine's moves: `@clk state != past(state)`, `show` `["state"]`.
- Is it idle: `x != past(x)`, `count_only`, over the window; 0 means it never changed.
- A protocol check, as a search for its violations: `@clk fell(valid) && !past(ready)`.
- Two events at different times (a request and its response): decode each as a stream and
  match them with `pair` (`help decode`).

**Details**

- `@clk`: a match is each clock cycle whose ending rising edge samples the expression true, as
  a flip-flop does, reported at the cycle's starting edge; consecutive matching cycles are one
  row (`from`, `last`, `cycles`). `@clk x` is the cycles x is high, not x's edges.
- Without a clock, back-to-back handshakes are one match; a condition reads the values after
  the changes at that time, and `past(y)` (one tick earlier) is y just before them.
- Operators: == != \< \<= \> \>= (unsigned), && || ! ?: and parentheses. A bare signal is true when
  1 (some bit 1, none unknown). A comparison or `!x` is a value, 1 or 0.
- Arithmetic as in SystemVerilog but stricter: + - \* / % & | ^ ~ \<\< \>\> and signed(x). Values
  are exact integers: nothing wraps unless sliced, so a 32-bit add is `(a + b)[31:0]`;
  `signed(x)` reads x as two's complement (`signed(imm[11:0]) < 0`), `signed(x)[31:0]` is its
  sign extension. `/` truncates toward zero, `%` takes the dividend's sign; dividing by zero is
  x. `* / %` bind tighter than `+ -`; `?:` is lowest.
- `&`, `|`, `^` need parentheses beside another operator, `<<` and `>>` beside arithmetic:
  `(x & 3) == 0`, `wmask != 1 << addr[1:0]`. A ternary `c ? a : b` is a value; a nested one
  needs parentheses. An unknown operand or condition bit makes the result x. Operands up to 64
  bits; an error shows each fix.
- Values: 0x40, 64, 0b1000000, -12, SV numbers (8'h40, 'd64, 4'b10x0, '1), x, z, a name the
  file gives a value, a time (`40ns`), or another signal (`mi_addr != reg_waddr`); a field's
  bits are `b[4:0]`, as a signal's.

## decode

Decode first: records are exact and their handshake checks built in.

**Form**

- `decode <name> <protocol> clock=<clk> <role>=<signal>... <param>=<value>...`, run by `command`.
  Built in: `axis`, `axilite`, `axi4`, and without a clock `spi` (`mode=0`) and `uart`
  (`baud=115200`). `prefix=s_axi_` plays each role not given by `<prefix><role>`.
- An interface of the design's own: `protocol <name> roles=<a,b> when=<expr> <field>=<expr>...`
  defines it on roles, then `decode` puts it on signals as a built-in.
- On streams: `frame` groups records, `pair` matches requests with responses, `check` makes a
  record per cycle a rule holds.

**Recipes**

- A bus `open` or `info` listed: run its line, `decode bus axilite clock=aclk prefix=s_axi_`,
  then `records` `bus` (`help records`).
- A handshake of your own: `protocol hs roles=valid,ready,data when=valid && ready d=data`, then
  `decode rx hs clock=clk valid=rx_valid ready=rx_ready data=rx_data`.
- Packets from beats: `decode f frame input=rx.beat last=tlast`.
- How long each request took: `decode m pair request=axi.wr response=mi`; each record has both
  sides and its `delay`.
- Count protocol violations: `decode c check clock=clk dropped=fell(valid) && !past(ready)`,
  then `records` `c` `count_only`.
- What a protocol takes: `protocols <name>` lists its roles, params, streams and fields.

**Details**

- The answer lists the streams made (`bus.wr`, `bus.rd`, ...) with their counts, and the
  violations (`bus.errors`, `bus.wr.unmatched`).
- A value runs to the next `key=`, so it can have spaces; in quotes it runs to the closing one
  (`when="x=1"`).
- A `protocol` makes a record per clock cycle where `when` holds (`help expressions`), its
  fields sampled with it. A field is any expression of roles without past: `next=pc + 4`,
  `imm=signed(insn[31:20])` (a field that can be negative is signed, shown in decimal); later
  fields and `when` can name it (`f3=insn[14:12] ok=f3 == 0`).
- `pair` matches by order, or by `id=` / `req_id=` and `resp_id=`. `check` gives each record a
  `rule` field.
- Decoding doesn't touch the rows.

## records

Reading decoded streams with `records`.

**Form**

- `records` `stream` lists a stream as a table: n (the record's number), start, end and a field
  per column, headed `20 of 157`.
- `stream` may name a set: a decode (`bus`: its results and violations, not the streams they
  are made from), a pattern (`/^bus\./`, `help patterns`) or several (`bus.wr bus.rd`).
- `where` keeps the records an expression over the fields holds for, the fields named bare:
  `addr != 0x1C && resp != 0`, `rule == dropped`, `delay > 40ns`.

**Recipes**

- How many: `count_only`, with the first and last start.
- Bus errors: `records` `bus.wr` `where` `resp != 0`.
- What happened, by kind: `group_by` `["addr"]`, a line per distinct value with its count and
  first and last start: `128 × addr=0x1C data=0x3 (2.975us .. 69.015us)`.
- A testbench's packet #138: `where` `n == 138`.
- A scoreboard check on a pair: `where` `req_addr != resp_addr`.
- One record whole: `count: 1` with `from` its start.

**Details**

- `start` is the rising edge that starts the record's first clock cycle, as `find @clk`
  reports a match; `end` is the edge that samples its last.
- A set's records come in order of start with a `stream` column, fields of the same name and
  kind sharing a column, empty where a stream has none; `where` keeps the streams having every
  field it names.
- `from` / `to` pick records by their start; `count` how many (default 20). With more than one
  record long values are shortened (a frame's data to 8 bytes and its size).
- In `where`, `past(x)` is x of the record before and `n` is the record's number.
- Fields show in hex; `radix` gives another for every number field (`dec`) or for one
  (`len:dec`, several with commas). Times stay in the unit.

## workspace

Showing the user evidence.

**Form**

- `workspace` sets everything at once, as one undo step: `rows`, `cursors`, `range`, `scope`,
  `signals`, `protocols`, `decode`. Every name, radix and time is checked first and all the
  errors come back at once.
- `view` describes it, `render` draws it as text, `present` shows it to the user.

**Recipes**

- Show a finding: declare the `workspace`, check it with `view` / `render`, then `present` it
  into the user's wtf windows on the trace (`:undo` takes it back).
- Point at an event: a mark, `{"2": {"time": "390ns", "name": "first bad write", "note": "AW
  before W, so the address latched is stale"}}`; the user jumps there with '2.
- Build on what the user has open: `user_view` `adopt`, then `command` steps.

**Details**

- `rows`, top to bottom: `"name"`, `"name:radix"`, `"name:radix:colour"` (`help radix`), or a
  group `{"group": "write", "rows": ["awaddr", "awvalid"], "collapsed": false}`. Groups
  hold signals only. A struct or array is one row (`"pkts"`); `{"signal": "pkts", "open":
  ["pkts", "pkts[1]"]}` opens levels of it, each named from the row's name; `"fields":
  ["pkts[1].pair.hi:dec:red"]` gives lines under a row a radix and colour of their own.
- `cursors` by number, `{"1": "80ns", "2": "390ns"}`: 1 is the cursor, where values are read;
  2 - 9 are marks on the events to show. A mark's name stands on the ruler, its note on the
  user's status line when they jump there.
- `range`: the visible range, `["60ns", "480ns"]`; default the whole trace.
- `scope` (`help names`); `signals`: computed signals, `["op = cmd[7:0]"]`.
- `protocols`: `{name, roles, when, fields: ["op = data[7:0]"]}`, as `protocol` defines them;
  `decode`: `{name, protocol, clock, signals: ["tvalid = rx_tvalid"], params: ["prefix =
  rx_"]}`, as `decode` does (`help decode`). Streams can then be rows.
- With no window open, `present` writes a session file that `wtf <trace>` shows when the user
  opens the trace: tell them so. The user also sees each query and change you make as a step
  (`:trail`), with its time, so each query is part of what they see.
- `command` edits the workspace step by step instead (`help commands`).

## commands

The `:` commands `command` runs on the workspace.

**Form**

- A command without the colon, as the window takes it: `add top.cpu`, `cursor 2 80ns`.

**Recipes**

- Add rows: `add <path|pattern>...`, below the selected row, joining its group; a scope becomes
  a group of its direct signals, a struct or array one row. Remove: `rm <path|pattern>...`;
  `rm /.*/` clears (`help patterns`).
- Act on rows: `select <row> [<last row>]`, then `group <name>`, `rename <name>`, `ungroup`,
  `radix <radix>`, `color <colour|off>` (`help radix`).
- Name a state machine's values: `names <signal> <value>=<name>...`, as an FST enum table gives
  them; `off` removes them.
- Mark an event: `cursor <1-9> [time|off] [name=<text>] [note=<text>]`; without a time or keys,
  jump to that mark.
- Look at a stretch: `fit [<from> <to>]` (all, or between cursors `'2 '3` or times);
  `zoom <time per column>`; `<time>` moves the cursor.

**Details**

- Names and times: `scope <path>` (`help names`), `unit <ns|ps|...|auto>` (`help times`).
- Decoding: `decode`, `protocol`, `signal` (`help decode`).
- `undo`, `redo`. The answer lists the rows added and removed.
