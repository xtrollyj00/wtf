## 16.2 AXI4-Lite (axilite)

Arm's AXI5-Lite (IHI 0022 issue L, Table B2.2), which an AXI4-Lite port wires the same way:
decoded as transactions, a record per channel handshake, writes and reads paired with their
response and the delay between them, and the interface's rules as violations. A signal the port
doesn't give is what the table means by it, so a port that leaves out `wstrb`, the protections or
the responses still decodes, and a reset that is given is checked.

```
decode bus axilite clock=aclk prefix=s_axi_
```

Channels, `<prefix><role>` each, and the fields of a channel's record (one where its `valid` and
`ready` hold):

| channel | valid, ready | fields |
| --- | --- | --- |
| `aw` | `awvalid` `awready` | `addr` (`awaddr`), `prot` (`awprot`) |
| `w` | `wvalid` `wready` | `data` (`wdata`), `strb` (`wstrb`) |
| `b` | `bvalid` `bready` | `resp` (`bresp`) |
| `ar` | `arvalid` `arready` | `addr` (`araddr`), `prot` (`arprot`) |
| `r` | `rvalid` `rready` | `data` (`rdata`), `resp` (`rresp`) |

The roles the protocol cannot decode without are required; the rest are what Table B2.2 says
they mean, given or not:

| role | required | not given | it is |
| --- | --- | --- | --- |
| `awvalid` | yes | — | the write address is offered |
| `awready` | yes | — | the write address is taken |
| `awaddr` | yes | — | the write's address |
| `wvalid` | yes | — | write data is offered |
| `wready` | yes | — | write data is taken |
| `wdata` | yes | — | the write's data |
| `bvalid` | yes | — | a write response is offered |
| `bready` | yes | — | the write response is taken |
| `arvalid` | yes | — | a read address is offered |
| `arready` | yes | — | the read address is taken |
| `araddr` | yes | — | the read's address |
| `rvalid` | yes | — | read data is offered |
| `rready` | yes | — | read data is taken |
| `rdata` | yes | — | the read's data |
| `wstrb` | no | all ones, one bit per `wdata` byte | which bytes of `wdata` are data |
| `awprot` | no | `0b000` | the write's protection: privileged, secure, instruction or data |
| `arprot` | no | `0b000` | the read's protection, as `awprot` |
| `bresp` | no | OKAY (`0b000`) | how the write went |
| `rresp` | no | OKAY (`0b000`) | how the read went |
| `aresetn` | no | — | the reset, active low; `reset_active=high` for active high |

Params: `timeout=<time>`, the wait after a request completes before it is `no response`
(`timeout=1us`, section 16's decoder bullet); `reset_active`, `low` (the default) or `high`.

The interface's signals, from Table B2.2's AXI5-Lite column: `Y` both sides, `YM` manager only,
`YS` subordinate only, `O` optional. A signal that is `not implemented` is a port wtf decodes
without: it is no role, and nothing checks it. Rows group signals that mean the same.

| signal | presence | default | role |
| --- | --- | --- | --- |
| `ACLK` | `Y` | — | the `clock=` argument: a handshake is one edge of it |
| `ARESETn` | `Y` | — | `aresetn`, optional here: a trace without it decodes, reset rules off |
| `AWVALID` | `Y` | — | `awvalid` |
| `AWREADY` | `Y` | — | `awready` |
| `AWADDR` | `Y` | — | `awaddr` |
| `AWPROT` | `YM` | `0b000` | `awprot` |
| `WVALID` | `Y` | — | `wvalid` |
| `WREADY` | `Y` | — | `wready` |
| `WDATA` | `Y` | — | `wdata` |
| `WSTRB` | `YS` | all ones | `wstrb` |
| `BVALID` | `Y` | — | `bvalid` |
| `BREADY` | `Y` | — | `bready` |
| `BRESP` | `O` | OKAY | `bresp` |
| `ARVALID` | `Y` | — | `arvalid` |
| `ARREADY` | `Y` | — | `arready` |
| `ARADDR` | `Y` | — | `araddr` |
| `ARPROT` | `YM` | `0b000` | `arprot` |
| `RVALID` | `Y` | — | `rvalid` |
| `RREADY` | `Y` | — | `rready` |
| `RDATA` | `Y` | — | `rdata` |
| `RRESP` | `O` | OKAY | `rresp` |
| `AWID` `BID` `ARID` `RID` | `YS` | — | not implemented: one write and one read at a time, paired by order |
| `AWIDUNQ` `BIDUNQ` `ARIDUNQ` `RIDUNQ` | `O` | — | not implemented |
| `AWSUBSYSID` `ARSUBSYSID` | `O` | — | not implemented |
| `AWSIZE` `ARSIZE` | `O` | — | not implemented: no size or width check |
| `AWUSER` `WUSER` `BUSER` `ARUSER` `RUSER` | `O` | — | not implemented |
| `WPOISON` `RPOISON` | `O` | — | not implemented |
| `AWTRACE` `WTRACE` `BTRACE` `ARTRACE` `RTRACE` | `O` | — | not implemented |
| `AWAKEUP` | `O` | — | not implemented: the low-power handshake of A14.1.1 |

Streams, `<name>` being the instance's:

| stream | it is |
| --- | --- |
| `<name>.wr` | writes: AW and W in either order, then B; `delay` from the later of AW and W to B |
| `<name>.wr.unmatched` | a write never answered, or not within `timeout` (no response), its B after that (no request), a W without an AW (no aw) or the reverse (no w) |
| `<name>.rd` | reads: AR, then R; `delay` from AR to R |
| `<name>.rd.unmatched` | an AR never answered, or not within `timeout` (no response), its R after that (no request) |
| `<name>.errors` | a record per cycle where a rule holds, `rule` naming it, `detail` what it saw, `severity` `error` or `warning` |

An AXI-Lite or AXI4 port that only writes or only reads is decoded too: a side none of whose
roles is given (explicitly or under the prefix) is left out, and the instance makes only the
other side's streams. A side given in part is still an error, naming the missing roles; where
the file has that protocol's ports, the error ends with the line that decodes them (`no signal
for awvalid (the file has: decode bus axilite clock=top.aclk prefix=top.s_axi_)`).

The rules `errors` reports, a record per cycle each holds. A clause of 16.x is this spec's own
rule; the others are Arm's:

| rule | clause | severity | it reports | pinned by |
| --- | --- | --- | --- | --- |
| `aw_dropped` `w_dropped` `b_dropped` `ar_dropped` `r_dropped` | A2.3 | error | a valid fell before its ready: it stays until the handshake | `axilite_error_rules`, `axilite_errors_conformance` |
| `aw_changed` `w_changed` `b_changed` `ar_changed` `r_changed` | A2.3 | error | a payload signal changed while its valid was stalled | `axilite_payload_changed_while_stalled` |
| `b_resp` `r_resp` | A3.3 | warning | a SLVERR or DECERR response | `axilite_warnings_filter_by_severity`, `axilite_transactions_and_errors` |
| `b_exokay` `r_exokay` | A3.3 | error | an EXOKAY response: Lite has no exclusive access | `axilite_exokay_is_an_error` |
| `aw_reset` `w_reset` `b_reset` `ar_reset` `r_reset` | A2.1.2 | error | a valid while the interface is in reset, or in its first cycle out | `axilite_reset_takes_no_records` |
| `aw_unknown` `w_unknown` `b_unknown` `ar_unknown` `r_unknown` | A2.3 | warning | a valid or ready is x or z out of reset | `axilite_unknown_out_of_reset` |
| `aw_payload_unknown` `w_payload_unknown` `b_payload_unknown` `ar_payload_unknown` `r_payload_unknown` | A2.3 | warning | a payload signal is x or z while its valid | `axilite_unknown_out_of_reset` |
| `b_early` `r_early` | A2.3.2 | error | a response before its request's handshakes | `axilite_b_early`, `axilite_r_early`, `axilite_b_early_same_cycle`, `axilite_bvalid_before_request`, `axilite_stray_response_once` |

The `reset` rules fire only when `aresetn` is bound. A `detail` is the handshake's own values:
`awvalid fell before awready`, `wdata changed while wvalid stalled`, `rresp is SLVERR or DECERR`,
`rresp is EXOKAY`, `awvalid in reset`, `awvalid in the cycle reset is released`, `awvalid is x or
z out of reset`, `awaddr is x or z while awvalid`, `response 1 before write 1's AW and W`.

The rest of the conformance, clause by clause:

| clause | what `axilite` does | pinned by |
| --- | --- | --- |
| A2.1.2 | a valid while `aresetn` holds the reset, or in the first cycle it is released, is `<chan>_reset`; no handshake, write or read is decoded there | `axilite_reset_takes_no_records` |
| A2.3 | a valid is held until its ready, so one that falls first is `<chan>_dropped`, once | `axilite_error_rules`, `axilite_errors_conformance` |
| A2.3 | a payload signal that moves while its valid is stalled is `<chan>_changed` | `axilite_payload_changed_while_stalled` |
| A2.3 | a valid or a ready that is x or z out of reset is `<chan>_unknown`, a payload that is x or z while its valid `<chan>_payload_unknown` | `axilite_unknown_out_of_reset` |
| A2.3.2.1 | writes: AW and W in either order, B only after both, `delay` from the later of them to B; a BVALID raised before them is `b_early`, once, even when its handshake comes after them, and the write's own B still pairs; an early B answers no write, so the B after it is not early too | `axilite_b_early`, `axilite_b_early_same_cycle`, `axilite_bvalid_before_request`, `axilite_transactions_and_errors` |
| A2.3.2.2 | reads: R only after AR, `delay` from AR to R; an RVALID raised before it is `r_early`, and an early R answers no read, so one stray R is one row | `axilite_r_early`, `axilite_stray_response_once`, `axilite_transactions_and_errors` |
| A3.3 | a SLVERR or DECERR response is a warning (`b_resp`, `r_resp`), an EXOKAY one an error (`b_exokay`, `r_exokay`) | `axilite_warnings_filter_by_severity`, `axilite_exokay_is_an_error` |
| Table B2.2 | the defaults above: `wstrb` all ones, `awprot`/`arprot` `0b000`, `bresp`/`rresp` OKAY, no reset | `axilite_defaults_for_absent_roles`, `axilite_required_roles` |
| 16.2 | unmatched: a request never answered, or not within `timeout`, is `no response`, and its response after that `no request`; a W without an AW is `no aw`, an AW without a W `no w` | `axilite_no_request_only_when_late`, `axilite_timeout` |
| 16.2 | a side none of whose roles is given makes only the other side's streams; a side given in part is an error naming the missing roles | `axilite_one_side` |

Axilite reports `no request` only for a response after `timeout`: a response with no request
before it at all is `b_early` or `r_early` in `errors`, and not said again in `wr.unmatched` or
`rd.unmatched`.

Not implemented, and so reported by nothing:

| clause | check | why not |
| --- | --- | --- |
| B2.2 | the AXI5-Lite signals with no role above | not implemented: listed in the matrix, decoded without them |
| bind | static widths (`wdata` whole bytes, `wstrb` one bit per byte of it, the addresses as wide as the file has) | not implemented: the fields are as wide as the signal |
| A2.3.2 | more than one write or read in flight | not implemented: one of each at a time |
| A3.1, A3.2, A6.3 | bursts and exclusive access | an AXI5-Lite interface has neither: no `AWLEN`, `AWBURST`, `WLAST`, `RLAST`, `AWLOCK`; [axi4](axi4.md) has them |
| A4 | region, cache, QoS, domain and snoop attributes | not implemented |
| A14.1.1 | the `AWAKEUP` low-power handshake | not implemented |
