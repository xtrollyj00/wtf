//! The `help` tool's topics: the reference the tool descriptions point to, each concept explained
//! once (spec §15). Kept in step with the spec, as the descriptions are.

/// (topic, what it covers, the reference).
pub const TOPICS: &[(&str, &str, &str)] = &[
    (
        "names",
        "naming signals: paths, bits, slices, VHDL forms, scope, streams",
        "\
A signal is named by its full path (`top.dut.s_axi_awaddr`) or by the end of a path if only one
signal ends that way (`s_axi_awaddr`); an ambiguous name is an error listing the candidates.
- One bit: `data[3]`. A slice `tdata[63:32]` and a concatenation `{a, b}` are signals too, as
  is a name made with `command` `signal op = cmd[7:0]`.
- VHDL or Verilog forms both: any case, `gen(4).x` or `gen[4].x`, `data(7 downto 4)` or
  `data[7:4]`.
- A scope (`command` `scope top.dut`, or `workspace` `scope`) makes names resolve under it
  first in every tool: `s_axi_awvalid` is `top.dut.s_axi_awvalid`. `scope` with no path: none.
- A radix goes with a name wherever tools take signals: `awaddr:dec` (`help radix`).
- A decoded stream is a signal (`bus.wr`, its value the record's number `#n`), and so is each
  of its fields (`bus.wr.addr`, `z` between records), for `find`, `values` and rows alike.",
    ),
    (
        "times",
        "time units, the workspace's unit and rounding",
        "\
Times take units: `120ns`, `1.5us`, `2595ps`; a plain number is file ticks.
- Answers use one unit per workspace, so they compare across calls: once a search names a clock
  (`@clk`, also in a protocol or a decode), the unit is the largest that keeps that clock's
  shortest phase at 1 or more (`ns` for a 10 ns or a 200 MHz clock). `command` `unit ns` sets
  it, `unit auto` goes back to the clocks'. A call's `unit` argument overrides it for that call.
- Times are rounded to a thousandth of the unit (`2595ns` for 2595.000001ns); `unit: \"fs\"`
  gives them exactly.
- Durations a decoder works out (a pair's `delay`) are times: shown in the unit, and compared
  with a time in `where` (`delay > 40ns`).",
    ),
    (
        "radix",
        "radixes and colours of values and rows",
        "\
Radixes: hex, bin, dec, sdec (signed), oct, ascii, float, fixed point like q4.12, and enum (the
names the file gives values, as `state` READ). The default is enum where the file names values,
otherwise hex; 1-bit signals always show 0 / 1 / X / Z.
- A call's `radix` sets it for all its signals; `data:dec` for one signal.
- Rows (`help workspace`) take a radix and a colour: `awaddr:dec`, `wvalid:red`,
  `awaddr:hex:red`. Colours: red orange yellow green cyan blue magenta gray (`off` removes it).",
    ),
    (
        "expressions",
        "search expressions for `find`: operators, values, @clk, past / rose / fell / stable",
        "\
`valid && ready && data == 0x40`, `state == 3 || state == 5`, `!rst`.
- Operators: == != < <= > >= (unsigned numbers), &&, ||, ! and parentheses. A bare signal is
  true when 1 (some bit 1, none unknown).
- Values: 0x40, 64, 0b1000000, -12, x (unknown), z (high impedance), a name the file gives a
  value, or a time (`40ns`). A signal compares with a value, not with another signal: such a rule
  is usually a protocol's, which a decoder checks already (`help decode`), and a change is
  `!stable(x)`. (`records` `where` compares two fields: `help records`.)
- Without a clock, a match is where the expression becomes true, so back-to-back handshakes
  are one match: use `@clk` to see each cycle.
- `@clk ` first (a clock signal): a match is each clock cycle whose ending rising edge samples
  the expression true, as a flip-flop does, reported at the cycle's starting edge. Consecutive
  matching cycles are one row (`from`, `last`, `cycles`).
- Over time: `past(x)` is x one cycle earlier (without a clock, one tick); `rose(x)`, `fell(x)`
  and `stable(x)` compare x with it. So a protocol check is a search for its violations:
  `@clk fell(valid) && !past(ready)`.
- `find` `show` lists signals' values on each match, so `@clk awvalid && awready` with show
  `[\"awaddr\", \"wdata\"]` is a table of the writes, a row per cycle; `count_only` counts them.",
    ),
    (
        "decode",
        "decoding interfaces into records: built-in protocols, protocol, frame, pair, check",
        "\
Decode first: records are exact and their handshake checks built in. `open` and `info` list the
interfaces a built-in protocol decodes, each with its line; `command` runs it:
  decode bus axilite clock=aclk prefix=s_axi_
The answer lists the streams made (`bus.wr`, `bus.rd`, ...) with their counts, and the
violations (`bus.errors`, `bus.wr.unmatched`). `protocols` lists the protocols, `protocols <name>`
each one's roles, params, streams and fields.
- `decode <name> <protocol> clock=<clk> <role>=<signal>... <param>=<value>...`: built in `axis`,
  `axilite`, `axi4`, and without a clock `spi` (`mode=0`) and `uart` (`baud=115200`).
  `prefix=s_axi_` plays each role not given by `<prefix><role>`. A value runs to the next `key=`,
  so it can have spaces.
- An interface of the design's own: `protocol <name> clock=<clk> when=<expr> <field>=<signal>...`
  makes a record per clock cycle where `when` holds (`help expressions`), the fields sampled with
  it. With `roles=a,b` it is a type for `decode` instead.
- On streams: `decode f frame input=rx.beat last=tlast` groups records into frames;
  `decode m pair request=axi.wr response=mi` matches requests with responses (by order, or
  `id=` / `req_id=` and `resp_id=`), each record both sides with its `delay`;
  `decode c check clock=clk dropped=fell(valid) && !past(ready)` is a violation record, field
  `rule`, per cycle where a rule holds.
- `signal <name> = <expr>` names a computed signal (`help names`).
Decoding doesn't touch the rows. Read a stream with `records` (`help records`).",
    ),
    (
        "records",
        "reading streams: where, n, field comparisons, group_by, count_only",
        "\
`records` lists a stream as a table: n (the record's number), start, end and a field per column,
headed `20 of 157`. `start` is the rising edge that starts the record's first clock cycle, as
`find @clk` reports a match; `end` is the edge that samples its last. With more than one record long values are shortened (a frame's data to 8
bytes and its size); `count: 1` with `from` shows one whole.
- `from` / `to` pick records by their start; `count` how many (default 20).
- `where` keeps the records a search expression over the fields holds for, the fields named bare
  and evaluated on each record: `addr != 0x1C && resp != 0`, `rule == dropped`, `delay > 40ns`.
  `past(x)` is x of the record before; `n` is the record's number (`n == 138`, a testbench's
  packet #138); a field compares with another field (`req_addr != resp_addr`, a pair's two
  sides, as a scoreboard checks them).
- `group_by` names fields and answers a line per distinct value, with how many records have it
  and the first and last start: `128 × addr=0x1C data=0x3 (2.975us .. 69.015us)`.
- `count_only` only counts, with the first and last start.",
    ),
    (
        "workspace",
        "showing evidence: the `workspace` declaration, `view`, `render`, `present`",
        "\
To show the user evidence, declare the workspace, check it with `view` / `render`, then `present`
it into the user's wtf window (they can undo with u). `workspace` replaces everything at once, as
one undo step:
- `rows`, top to bottom: `\"name\"`, `\"name:radix\"`, `\"name:radix:colour\"` (`help radix`), or a
  group `{\"group\": \"write\", \"rows\": [\"awaddr\", \"awvalid\"], \"collapsed\": false}`. Groups
  hold signals only.
- `cursors` by number, `{\"1\": \"80ns\", \"2\": \"390ns\"}`: 1 is the cursor, where values are
  read; 2 - 9 are marks on the events to show. The user jumps to a mark with 'N.
- `range`: the visible range, `[\"60ns\", \"480ns\"]`; default the whole trace.
- `scope` (`help names`); `signals`: computed signals, `[\"op = cmd[7:0]\"]`.
- `protocols`: `{name, clock, when, fields: [\"tdata = tdata\"], roles}`, as `protocol` makes them;
  `decode`: `{name, protocol, clock, signals: [\"tvalid = rx_tvalid\"], params: [\"prefix =
  rx_\"]}`, as `decode` does (`help decode`). Streams can then be rows.
Every name, radix and time is checked first and all the errors come back at once. `command`
edits the workspace step by step instead (`help commands`).",
    ),
    (
        "commands",
        "the `:` commands `command` runs",
        "\
`command` runs one of wtf's `:` commands, without the colon, on the workspace.
- Rows: `add <path|pattern>...` (below the selected row, joining its group; a scope becomes a
  group), `rm <path|pattern>...` (`rm **` clears), `select <row> [<last row>]` (the rows the next
  commands act on), `group <name>`, `rename <name>`, `ungroup`, `radix <radix>`, `color
  <colour|off>` (`help radix`), `undo`, `redo`. The answer lists the rows added and removed.
- Cursors and view: `cursor <1-9> [time|off]` (without a time, jump to that mark), `<time>`
  (moves the cursor), `zoom <time per column>`, `fit`.
- Names and times: `scope <path>` (`help names`), `unit <ns|ps|...|auto>` (`help times`).
- Decoding: `decode`, `protocol`, `signal` (`help decode`).",
    ),
];

/// The topics with what each covers, or one topic's reference.
pub fn answer(topic: Option<&str>) -> Result<String, String> {
    let list = || TOPICS.iter().map(|(t, what, _)| format!("{t}: {what}")).collect::<Vec<_>>().join("\n");
    match topic {
        None => Ok(format!("help <topic>:\n{}", list())),
        Some(t) => TOPICS.iter().find(|(name, ..)| name.eq_ignore_ascii_case(t.trim())).map(|(.., text)| text.to_string()).ok_or_else(|| format!("no help topic {t}; topics:\n{}", list())),
    }
}
