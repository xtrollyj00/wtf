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
- A bus `open` or `info` listed: run its line, `decode bus axilite clock=aclk aresetn=aresetn
  prefix=s_axi_`, then `records` `bus` (`help records`).
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
- A B or R with nothing before it never shifts the pairing: `axilite` and `axi4` report it as
  `b_early` / `r_early` in `errors`; `timeout=1us` makes a request unanswered that long
  `no response`, and `axi4` calls the answer that comes after it `timeout`.
- A value runs to the next `key=` (spaces are fine); in quotes, to the closing one (`when="x=1"`).
- A `protocol` makes a record per clock cycle where `when` holds (`help expressions`), its
  fields sampled with it. A field is any expression of roles without past: `next=pc + 4`,
  `imm=signed(insn[31:20])` (a field that can be negative is signed, shown in decimal); later
  fields and `when` can name it (`f3=insn[14:12] ok=f3 == 0`).
- `pair` matches by order, or by `id=` / `req_id=` and `resp_id=`. `check` gives each record a
  `rule` field.
- `axis` and `axilite` read a role they aren't given as a default (`axis`: `tready` 1, `tkeep`
  the bytes of `tdata`; `axilite`: `wstrb` all ones, `*prot` 0, `*resp` OKAY), so
  `decode rx axis clock=clk tvalid=v tready=r tdata=d aresetn=rst_n` makes `rx.beat`, `rx.frame` and
  `rx.errors`; a violation carries the `rule`, what it saw (`detail`) and its `severity` (error or
  warning), and `records rx.errors where severity == warning` keeps the warnings.
- `axi4` and `axilite` read an absent role as AXI5's default (`protocols axi4` lists them);
  `wlast`/`rlast` are optional, AxLEN framing a burst without them. All three AXI need `aresetn`
  (`reset_active=high` if active high): a valid in reset or its first cycle out is `<chan>_reset`,
  an x out of reset an error, so are a bad burst type, size, length or alignment, a LAST off its
  end and a strobe outside the beat's lanes; a SLVERR or DECERR is a warning, an EXOKAY an error.
- `decode <name> off` removes a decode, its streams and the rows showing them; one another decode
  reads stays, naming it. `decode` again replaces it; decoding doesn't touch the rows.
- A protocol in the config dir's `protocols/` decodes as a built-in does (`protocols` lists them);
  `help wasm` writes one.
