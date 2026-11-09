Times you give and times you get back.

Form
- A number with its unit, no quotes, no space: `120ns`, `1.5us`, `3.386665ms`, `2595ps`; any
  unit, whatever the answers use. A plain number is file ticks.
- Units: s, ms, us, ns, ps, fs.

Recipes
- A window: `from` `120ns` `to` `1.5us`; both ends are included.
- Exact times, unrounded: the call's `unit` `fs`.
- All answers in one unit: `command` `unit ns`; `unit auto` goes back to the clocks'.

Details
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
