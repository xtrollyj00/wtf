`valid && ready && data == 0x40`, `state == 3 || state == 5`, `!rst`.
- Operators: == != < <= > >= (unsigned numbers), &&, ||, !, ?: and parentheses. A bare signal is
  true when 1 (some bit 1, none unknown). `* / %` bind tighter than `+ -`; `?:` is lowest.
- Arithmetic, written as in SystemVerilog but stricter: + - * / % & | ^ ~ << >> and signed(x). Values
  are exact integers: nothing wraps unless sliced, so a 32-bit add is `(a + b)[31:0]`; `signed(x)`
  reads x as two's complement (`signed(imm[11:0]) < 0`), and `signed(x)[31:0]` is its sign
  extension to 32 bits. `/` truncates toward zero, `%` takes the dividend's sign; dividing by
  zero is x.
- Ternary `c ? a : b` is a value; a nested one needs parentheses: `rd != (a < b ? 1 : 0)`.
  An unknown bit in `c` makes the result x.
- `&`, `|`, `^` need parentheses beside another operator, `<<` and `>>` beside arithmetic:
  `(x & 3) == 0`, `wmask != 1 << addr[1:0]`. A comparison or `!x` is a value, 1 or 0. A field's
  bits are `b[4:0]`, as a signal's, on either side of a comparison. An unknown operand makes the
  result x. Operands up to 64 bits; an error shows each fix.
- Values: 0x40, 64, 0b1000000, -12, SV numbers (8'h40, 'd64, 4'b10x0, '1), x (unknown), z (high impedance), a name the file gives a
  value, a time (`40ns`), or another signal (`mi_addr != reg_waddr`, `data != past(data)`). Two
  events at different times (a request and its response) are compared as records instead: a
  stream of each, matched with `pair` (`help decode`), their fields in `records` `where`.
- Without a clock, a match is where the expression becomes true, so back-to-back handshakes
  are one match: use `@clk` to see each cycle.
- `@clk ` first (a clock signal): a match is each clock cycle whose ending rising edge samples
  the expression true, as a flip-flop does, reported at the cycle's starting edge. Consecutive
  matching cycles are one row (`from`, `last`, `cycles`).
- Over time: `past(x)` is x one cycle earlier (without a clock, one tick); `rose(x)`, `fell(x)`
  and `stable(x)` compare x with it. So a protocol check is a search for its violations:
  `@clk fell(valid) && !past(ready)`.
- `find` `show` lists signals' values on each match, so `@clk awvalid && awready` with show
  `["awaddr", "wdata"]` is a table of the writes, a row per cycle; `count_only` counts them.
