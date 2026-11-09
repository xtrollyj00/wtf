Search expressions, for `find` (and `when`, fields and `where` in decoding).

Form
- Written as in SystemVerilog: `valid && ready && data == 0x40`, `state == 3 || state == 5`,
  `!rst`. Compare with `==`, never `=`.
- Over time, SVA's functions without the `$`: `rose(x)`, `fell(x)`, `stable(x)`, `past(x)` (x
  one cycle earlier). No others: an edge is `rose(x)`, not `rising` or `posedge`.
- `@<clock> ` first samples each cycle: `@top.clk valid && ready`. Without it, a match is where
  the expression becomes true.

Recipes
- Each transfer, with its data: `@clk valid && ready`, `show` `["data"]`; `count_only` counts.
- Stall cycles: `@clk valid && !ready`, `count_only`.
- The rising edges of a signal: `rose(irq)`; `show` lists what else was there at each.
- The cycles a condition holds: `@clk state == 3` (a row per run of cycles).
- A state machine's moves: `@clk state != past(state)`, `show` `["state"]`.
- Is it idle: `x != past(x)`, `count_only`, over the window; 0 means it never changed.
- A protocol check, as a search for its violations: `@clk fell(valid) && !past(ready)`.
- Two events at different times (a request and its response): decode each as a stream and
  match them with `pair` (`help decode`).

Details
- `@clk`: a match is each clock cycle whose ending rising edge samples the expression true, as
  a flip-flop does, reported at the cycle's starting edge; consecutive matching cycles are one
  row (`from`, `last`, `cycles`). `@clk x` is the cycles x is high, not x's edges.
- Without a clock, back-to-back handshakes are one match; a condition reads the values after
  the changes at that time, and `past(y)` (one tick earlier) is y just before them.
- Operators: == != < <= > >= (unsigned), && || ! ?: and parentheses. A bare signal is true when
  1 (some bit 1, none unknown). A comparison or `!x` is a value, 1 or 0.
- Arithmetic as in SystemVerilog but stricter: + - * / % & | ^ ~ << >> and signed(x). Values
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
