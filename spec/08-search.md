## 8. Value search

`/` opens a search line at the bottom, like the command line, and searches the selected signal
for a value. `Enter` jumps to the first match after the cursor, `Esc` cancels.
`↑` / `↓` or `Ctrl-p` / `Ctrl-n` go through earlier searches.

```
clk      ▁▁▁▁▔▔▔▔▁▁▁▁▔▔▔▔
▌data    ▏ 0x43 ▏ 0x96 ┃▏ 0x40 ▏ 0xE1
/0x40_
```

- **Matching is by number**, in any radix: `/0x40`, `/64` and `/0b1000000` find the same value,
  and leading zeros don't matter (`/0x0040`). A number without a prefix is decimal.
  On 1-bit signals, `/1` and `/0` find the next rising / falling edge.
- **Names**: on a signal whose values the file names (`enum`, section 10), a name is a value
  too, in any case: `/READ`, `/state == read`. Another word is an expression, as below.
- **Unknown values**: `/x` finds the next value with any unknown bit (`x` or `u`),
  `/z` the next value with any high-impedance bit.
- **A match is where the signal changes to the value**, so the cursor lands on the start of
  the segment. If the cursor is already inside a matching segment, `n` goes to the next one.
- `n` / `N` repeat the last search forward / backward on the **currently selected** signal, so you
  can search one signal, select another, and press `n` to find the same value there.
- **No wrap-around**: when nothing matches, the cursor stays and the status line says so,
  e.g. `not found after 1.2us: 0x40`.
- A value wider than the signal, or a selected group header, shows an error on the status line.
- **Expressions** search several signals at once, written as in SystemVerilog:
  `/s_axi_wvalid && s_axi_wready && s_axi_wdata == 0x40`, `/state == 3 || state == 5`, `/valid && !last`.
  Operators are `==`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, `!` and parentheses; values are
  written as above. `<` and the like compare unsigned numbers (`count >= 6`, `delay > 40ns`), and
  a value with an unknown or high-impedance bit is neither less nor more; `x`, `z` and text compare
  only with `==` and `!=`. A bare
  signal is true when it is 1 (on a bus: nonzero, with no unknown bits). Signals are named as in the
  name column, by full path, or by the end of a path if only one signal ends that way
  (`s_axi_wvalid`); of several, the one shown, the one in the workspace's scope, or the
  shallowest if only one is (a port passed down the hierarchy, `clk` in every instance, is the top
  one); `data[3]` is one bit. They don't have to be shown. A name is matched as
  written first, then **in either HDL's form**, since VHDL ignores case and the two write
  indices differently: case doesn't matter (`HEADERS_VLD` is the trace's `headers_vld`), and
  parentheses are brackets, so `gen_stages(4).stage_i.x` and `gen_stages[4].stage_i.x` name the
  same signal whichever the simulator wrote, `data(3)` is `data[3]`, `data(7 downto 4)` is
  `data[7:4]`, and an array element is named as written in the HDL (`axi_tdata(6)` for nvc's
  `axi_tdata.[6]`). A search is an expression
  when it contains a name or an operator; a bare value searches the selected signal as above.
- A match of an expression is where it **becomes true**. With a clock, `/@aclk wvalid && wready`,
  a match is **every clock cycle** whose ending rising edge of `aclk` samples the expression true
  (just before the edge, as a flip-flop does), so each beat of a burst is one match. The cursor goes
  to the cycle's start, the edge where the transfer's signals are set.
- **Over time**: `past(x)` is x's value one sample earlier: with a clock, at the cycle before (just
  before the previous rising edge); without, one tick earlier. It nests (`past(past(x))`) and
  compares like a signal (`past(state) == 2`). `rose(x)` is `x && !past(x)`, `fell(x)` is
  `x == 0 && past(x) != 0` (on a bus: becomes nonzero / zero), `stable(x)` is x unchanged since
  the sample before (false at the first, which has none). So a protocol check is a search for its
  violations: `/@clk fell(valid) && !past(ready)` finds a valid dropped before its ready,
  `/@clk past(valid) && !past(ready) && !stable(data)` a payload changed while stalled.
- A signal compares with a constant, never with another signal (`data != past(data)`, `a == b`):
  such a rule is most often a protocol's, which a decoder checks already (section 16), and a
  change is `!stable(x)`. The error says both. A `records` `where` (section 15) is the
  exception: there a field compares with another field of the same record, as a `pair`'s two
  sides are what a scoreboard compares.
- On a bit line of an expanded bus, the search looks at that bit. String and real signals are
  searched by their text.
