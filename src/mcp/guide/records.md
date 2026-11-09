`records` lists a stream as a table: n (the record's number), start, end and a field per column,
headed `20 of 157`. `stream` may name a set: a decode (`bus`: its results and violations, not the
streams they are made from), a pattern (`{streams}`, `help names`) or several (`bus.wr bus.rd`); their records come
in order of start with a `stream` column, fields of the same name and kind sharing a column, empty
where a stream has none. `start` is the rising edge that starts the record's first clock cycle, as
`find @clk` reports a match; `end` is the edge that samples its last. With more than one record long values are shortened (a frame's data to 8
bytes and its size); `count: 1` with `from` shows one whole.
- `from` / `to` pick records by their start; `count` how many (default 20).
- `where` keeps the records a search expression over the fields holds for, the fields named bare
  and evaluated on each record: `addr != 0x1C && resp != 0`, `rule == dropped`, `delay > 40ns`.
  `past(x)` is x of the record before; `n` is the record's number (`n == 138`, a testbench's
  packet #138); a field compares with another field (`req_addr != resp_addr`, a pair's two
  sides, as a scoreboard checks them). On a set it keeps the streams having every field it names.
- `group_by` names fields and answers a line per distinct value, with how many records have it
  and the first and last start: `128 × addr=0x1C data=0x3 (2.975us .. 69.015us)`.
- Fields show in hex; `radix` gives another for every number field (`dec`) or for one
  (`len:dec`, several with commas). Times stay in the unit.
- `count_only` only counts, with the first and last start.
