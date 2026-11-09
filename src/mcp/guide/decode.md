Decode first: records are exact and their handshake checks built in. `open` and `info` list the
interfaces a built-in protocol decodes, each with its line; `command` runs it:
  decode bus axilite clock=aclk prefix=s_axi_
The answer lists the streams made (`bus.wr`, `bus.rd`, ...) with their counts, and the
violations (`bus.errors`, `bus.wr.unmatched`). `protocols` lists the protocols, `protocols <name>`
each one's roles, params, streams and fields.
- `decode <name> <protocol> clock=<clk> <role>=<signal>... <param>=<value>...`: built in `axis`,
  `axilite`, `axi4`, and without a clock `spi` (`mode=0`) and `uart` (`baud=115200`).
  `prefix=s_axi_` plays each role not given by `<prefix><role>`. A value runs to the next `key=`,
  so it can have spaces; in quotes it runs to the closing one (`when="x=1"`).
- An interface of the design's own: `protocol <name> roles=<a,b> when=<expr> <field>=<expr>...`
  defines a protocol on its roles, no signals: a record per clock cycle where `when` holds (`help
  expressions`), the fields sampled with it. `decode <name> <protocol> clock=<clk> <role>=<signal>...`
  puts it on signals, as a built-in: `protocol hs roles=valid,ready,data when=valid && ready
  d=data` then `decode rx hs clock=clk valid=rx_valid ready=rx_ready data=rx_data`. A field is
  any expression of roles without past: `next=pc + 4`, `imm=signed(insn[31:20])` (a field that
  can be negative is signed, shown in decimal); later fields and `when` can name it
  (`f3=insn[14:12] ok=f3 == 0`).
- On streams: `decode f frame input=rx.beat last=tlast` groups records into frames;
  `decode m pair request=axi.wr response=mi` matches requests with responses (by order, or
  `id=` / `req_id=` and `resp_id=`), each record both sides with its `delay`;
  `decode c check clock=clk dropped=fell(valid) && !past(ready)` is a violation record, field
  `rule`, per cycle where a rule holds.
- `signal <name> = <expr>` names a computed signal (`help names`).
Decoding doesn't touch the rows. Read a stream with `records` (`help records`).
