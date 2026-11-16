## 16.3 AXI4 (axi4)

AXI4's channels as AXI4-Lite's, with the bursts and the IDs: every handshake is a record, a W or
R burst is a `frame` of its channel, and bursts pair with their addresses by ID.

It is a `Protocol`, as `axis` is: its roles are read a cycle at a time, and each handshake's rules
are decided there, with the clause and the severity each rule carries (16.1).

```
decode mem axi4 clock=aclk prefix=m_axi_
```

| channel | valid, ready | fields |
| --- | --- | --- |
| `aw` | `awvalid` `awready` | `addr` (`awaddr`), `id` (`awid`), `len` (`awlen`), `size` (`awsize`), `burst` (`awburst`), `prot` (`awprot`) |
| `w` | `wvalid` `wready` | `data` (`wdata`), `strb` (`wstrb`), `last` (`wlast`) |
| `b` | `bvalid` `bready` | `id` (`bid`), `resp` (`bresp`) |
| `ar` | `arvalid` `arready` | `addr` (`araddr`), `id` (`arid`), `len` (`arlen`), `size` (`arsize`), `burst` (`arburst`), `prot` (`arprot`) |
| `r` | `rvalid` `rready` | `data` (`rdata`), `id` (`rid`), `resp` (`rresp`), `last` (`rlast`) |

Streams:

| stream | it is |
| --- | --- |
| `<name>.wburst` | the W beats of a burst up to wlast; `data` the bytes wstrb marks |
| `<name>.rburst` | the R beats of a burst up to rlast, each ID's apart (bursts of different IDs interleave) |
| `<name>.wr` | writes: AW and its W burst in order, then the B with its ID; `delay` from the later of AW and the burst's first beat to B |
| `<name>.wr.unmatched` | a B without a write (no request), a write never answered, or not within `timeout` (no response), a W burst without an AW (no aw) or the reverse (no wburst) |
| `<name>.rd` | reads: AR, then the R burst with its ID; `delay` (latency) from AR to the first R beat |
| `<name>.rd.unmatched` | an R burst without an AR (no request), an AR never answered, or not within `timeout` (no response) |
| `<name>.errors` | a record per cycle where a rule holds, `rule` naming the channel and what it saw, for `aw`, `w`, `b`, `ar` and `r`: `<chan>_dropped` (a valid fell before its ready), `<chan>_changed` (the payload moved while stalled), `<chan>_resp` (a response not OKAY); `detail` says what it saw, and `severity` is `error` for all three (the spec forbids them: A3.2.1 and A6.1) |

Params: `timeout` as AXI4-Lite's. The sides, the one-sided port and the error an incomplete side
gives are AXI4-Lite's too (16.2).
