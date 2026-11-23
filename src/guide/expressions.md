Search expressions, for `find` (and `when`, fields and `where` in decoding).

Form
- Written as in SystemVerilog: `valid && ready && data == 0x40`, `state == 3 || state == 5`,
  `!rst`. Compare with `==`, never `=`.
- Over time, at a clock, SVA's functions without the `$` (IEEE 1800 §16.9.3), of any expression:
  `rose(e)`, `fell(e)`, `stable(e)`, `past(e)` (one cycle earlier), `past(e, n)` (n cycles):
  `@clk rose(a == 0 && b == 0)`. Those four only: an edge is `rose(x)`, not `rising` or `posedge`;
  a change is `!stable(x)`. Without the clock: `rose(x) needs a clock, as in SVA: @clk rose(x)`.
- `@<clock> ` first samples each cycle: `@top.clk valid && ready`. Without it, a match is where
  the expression becomes true.

Recipes
- Each transfer, with its data: `@clk valid && ready`, `show` `["data"]`; `count_only` counts.
- Stall cycles: `@clk valid && !ready`, `count_only`.
- Rising edges: `irq` without a clock, `@clk rose(irq)` by cycle; `show` lists what else was there.
- The cycles a condition holds: `@clk state == 3` (a row per run of cycles).
- A state machine's moves: `@clk state != past(state)`, `show` `["state"]`.
- Is it idle: `@clk !stable(x)`, `count_only`, over the window; 0 means it never changed.
- A protocol check, as a search for its violations: `@clk fell(valid) && !past(ready)`.
- A log string: `contains(msg, "error")`; a frame's bytes in `records` `where`:
  `contains(data, "de ad")`.
- A request and its response: decode each as a stream, match them with `pair` (`help decode`).

Details
- `@clk`: a match is each clock cycle whose ending rising edge samples the expression true, as
  a flip-flop does, reported at the cycle's starting edge; consecutive matching cycles are one
  row (`from`, `last`, `cycles`). `@clk x` is the cycles x is high, not x's edges.
- Without a clock, back-to-back handshakes are one match, read after that time's changes.
- As SVA's: `rose(e)` / `fell(e)` are e's bit 0 becoming 1 / 0 from anything, x and z too (on a bus
  `rose(n != 0)`); `stable(e)` compares as `===`, so x staying x is stable; all three are 1 or 0.
  The value before the clock's first edge is x; that edge samples before the first cycle.
- Operators: == != < <= > >= (unsigned), && || ! ?: and parentheses. A bare signal is true when
  1 (some bit 1, none unknown). A comparison or `!x` is a value, 1 or 0. As in SV, a
  comparison with an unknown operand is x: neither `d != 5` nor `!(d == 5)` matches where d is
  x (`d == x` finds those); `0 && x` is 0, `1 || x` is 1.
- Arithmetic as in SystemVerilog but stricter: + - * / % & | ^ ~ << >> and signed(x). Values
  are exact integers: nothing wraps unless sliced, so a 32-bit add is `(a + b)[31:0]`;
  `signed(x)` reads x as two's complement (`signed(imm[11:0]) < 0`), `signed(x)[31:0]` is its
  sign extension. `/` truncates toward zero, `%` takes the dividend's sign; dividing by zero is
  x. `* / %` bind tighter than `+ -`; `?:` is lowest.
- `&`, `|`, `^` need parentheses beside another operator, `<<` and `>>` beside arithmetic:
  `(x & 3) == 0`, `wmask != 1 << addr[1:0]`. A ternary `c ? a : b` is a value; a nested one
  needs parentheses. An unknown operand makes the result x; an unknown condition gives a if
  a == b, else x. Operands up to 64 bits; an error shows each fix.
- Values: 0x40, 64, 0b1000000, -12, SV numbers (8'h40, 'd64, 4'b10x0, '1), x, z, a name the
  file gives a value, a time (`40ns`), or another signal (`mi_addr != reg_waddr`); a field's
  bits are `b[4:0]`, as a signal's.
- `contains(x, "text")`: x's text holds text, case and spaces as written; x is a string signal
  or a text field, not bits. `x == idle` compares the whole text.
