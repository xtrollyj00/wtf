How every tool names a signal.

Form
- The full path, `top.dut.s_axi_awaddr`, or the end of one if only one signal ends that way,
  `s_axi_awaddr`; an ambiguous name is an error listing the candidates.
- One bit `data[3]`; a slice `tdata[63:32]`; a concatenation `{a, b}`.
- VHDL or Verilog forms alike, in any case: `gen(4).x` or `gen[4].x`, `data(7 downto 4)` or
  `data[7:4]`.
- With a radix wherever tools take signals: `awaddr:dec` (`help radix`).

Recipes
- Find a name you don't know: `signals` with a pattern (`help patterns`).
- Short names under one instance: `command` `scope top.dut` (or `workspace` `scope`); then
  `s_axi_awvalid` is `top.dut.s_axi_awvalid` in every tool. `scope` with no path: none.
- A field of a packed struct a VCD dumped as one vector: slice it, `pkt[15:8]`.
- A name of your own for an expression: `command` `signal op = cmd[7:0]`, then `op` everywhere.

Details
- A struct, record or array, or part of one (`pkt`, `pkts[1]`, `grid[1]`), is a signal of its
  whole value: its fields joined, the first the most significant (`pkt == 0x5A5`, `pkt[10]`);
  with a text field (an nvc enum) text, `'{valid:1, state:busy, pair:'{hi:A, lo:5}}`. `value_at`
  adds a line per field.
- A decoded stream is a signal (`bus.wr`, its value the record's number `#n`), and so is each
  of its fields (`bus.wr.addr`, `z` between records), for `find`, `values` and rows alike.
- An unknown name says what the trace has instead: a scope's signals, a struct's or array's
  fields, a signal without fields (a packed struct in a VCD: slice it), or the nearest scopes
  and signals.
