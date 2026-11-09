Decode first: records are exact and their handshake checks built in.

Form
- `decode <name> <protocol> clock=<clk> <role>=<signal>... <param>=<value>...`, run by `command`.
  Built in: `axis`, `axilite`, `axi4`, and without a clock `spi` (`mode=0`) and `uart`
  (`baud=115200`). `prefix=s_axi_` plays each role not given by `<prefix><role>`.
- An interface of the design's own: `protocol <name> roles=<a,b> when=<expr> <field>=<expr>...`
  defines it on roles, then `decode` puts it on signals as a built-in.
- On streams: `frame` groups records, `pair` matches requests with responses, `check` makes a
  record per cycle a rule holds.

Recipes
- A bus `open` or `info` listed: run its line, `decode bus axilite clock=aclk prefix=s_axi_`,
  then `records` `bus` (`help records`).
- A handshake of your own: `protocol hs roles=valid,ready,data when=valid && ready d=data`, then
  `decode rx hs clock=clk valid=rx_valid ready=rx_ready data=rx_data`.
- Packets from beats: `decode f frame input=rx.beat last=tlast`.
- How long each request took: `decode m pair request=axi.wr response=mi`; each record has both
  sides and its `delay`.
- Count protocol violations: `decode c check clock=clk dropped=fell(valid) && !past(ready)`,
  then `records` `c` `count_only`.
- What a protocol takes: `protocols <name>` lists its roles, params, streams and fields.

Details
- The answer lists the streams made (`bus.wr`, `bus.rd`, ...) with their counts, and the
  violations (`bus.errors`, `bus.wr.unmatched`).
- A value runs to the next `key=`, so it can have spaces; in quotes it runs to the closing one
  (`when="x=1"`).
- A `protocol` makes a record per clock cycle where `when` holds (`help expressions`), its
  fields sampled with it. A field is any expression of roles without past: `next=pc + 4`,
  `imm=signed(insn[31:20])` (a field that can be negative is signed, shown in decimal); later
  fields and `when` can name it (`f3=insn[14:12] ok=f3 == 0`).
- `pair` matches by order, or by `id=` / `req_id=` and `resp_id=`. `check` gives each record a
  `rule` field.
- Decoding doesn't touch the rows.
