Reading decoded streams with `records`.

Form
- `records` `stream` lists a stream as a table: n (the record's number), start, end and a field
  per column, headed `20 of 157`.
- `stream` may name a set: a decode (`bus`: its results and violations, not the streams they
  are made from), a pattern (`{streams}`, `help patterns`) or several (`bus.wr bus.rd`).
- `where` keeps the records an expression over the fields holds for, the fields named bare:
  `addr != 0x1C && resp != 0`, `rule == dropped`, `delay > 40ns`.

Recipes
- How many: `count_only`, with the first and last start.
- Bus errors: `records` `bus.wr` `where` `resp != 0`.
- What happened, by kind: `group_by` `["addr"]`, a line per distinct value with its count and
  first and last start: `128 × addr=0x1C data=0x3 (2.975us .. 69.015us)`.
- A testbench's packet #138: `where` `n == 138`.
- A scoreboard check on a pair: `where` `req_addr != resp_addr`.
- One record whole: `count: 1` with `from` its start.

Details
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
