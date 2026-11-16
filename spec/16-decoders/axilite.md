## 16.2 AXI4-Lite (axilite)

AXI4-Lite decoded as transactions: a record per channel handshake, writes and reads paired with
their response and the delay between them, and the handshake's rules as violations.

```
decode bus axilite clock=aclk prefix=s_axi_
```

Channels, `<prefix><role>` each, and the fields of a channel's record (one where its `valid` and
`ready` hold; a payload role the port doesn't give is left out):

| channel | valid, ready | fields |
| --- | --- | --- |
| `aw` | `awvalid` `awready` | `addr` (`awaddr`), `prot` (`awprot`) |
| `w` | `wvalid` `wready` | `data` (`wdata`), `strb` (`wstrb`) |
| `b` | `bvalid` `bready` | `resp` (`bresp`) |
| `ar` | `arvalid` `arready` | `addr` (`araddr`), `prot` (`arprot`) |
| `r` | `rvalid` `rready` | `data` (`rdata`), `resp` (`rresp`) |

Streams, `<name>` being the instance's:

| stream | it is |
| --- | --- |
| `<name>.wr` | writes: AW and W in either order, then B; `delay` from the later of AW and W to B |
| `<name>.wr.unmatched` | a B without a write (no request), a write never answered, or not within `timeout` (no response), a W without an AW (no aw) or the reverse (no w) |
| `<name>.rd` | reads: AR, then R; `delay` from AR to R |
| `<name>.rd.unmatched` | an R without an AR (no request), an AR never answered, or not within `timeout` (no response) |
| `<name>.errors` | a record per cycle where a rule holds, `rule` naming it: `<chan>_dropped` (a valid fell before its ready), `<chan>_changed` (the payload moved while stalled), `<chan>_resp` (a response not OKAY); `detail` says what it saw, and `severity` is `error` for all three (the spec forbids them: A3.2.1 and A6.1) |

An AXI-Lite or AXI4 port that only writes or only reads is decoded too: a side none of whose
roles is given (explicitly or under the prefix) is left out, and the instance makes only the
other side's streams. A side given in part is still an error, naming the missing roles; where
the file has that protocol's ports, the error ends with the line that decodes them (`no signal
for awvalid (the file has: decode bus axilite clock=top.aclk prefix=top.s_axi_)`).

Params: `timeout=<time>`, the wait after a request completes before it is `no response`
(`timeout=1us`, section 16's decoder bullet).
