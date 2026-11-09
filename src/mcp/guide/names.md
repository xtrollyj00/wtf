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
  of its fields (`bus.wr.addr`, `z` between records), for `find`, `values` and rows alike.
- A struct, record or array, or part of one (`pkt`, `pkts[1]`, `grid[1]`), is a signal of its
  whole value: its fields joined, the first the most significant (`pkt == 0x5A5`, `pkt[10]`);
  with a text field (an nvc enum) text, `'{valid:1, state:busy, pair:'{hi:A, lo:5}}`. `value_at`
  adds a line per field; `signals` lists it as one line, its fields with `{fields}`.
{patterns}
- `signals` lists what a pattern matches, or with none everything: a scope it names is a line
  of its own, `top.cpu (scope of 3 signals)`, and an answer too long is folded to a line per
  scope below the deepest level that fits.
- An unknown name says what the trace has instead: a scope's signals, a struct's or array's
  fields, a signal without fields (a packed struct in a VCD: slice it), or the nearest scopes
  and signals; so does a pattern that matches nothing.
