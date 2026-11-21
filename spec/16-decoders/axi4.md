## 16.3 AXI4 (axi4)

Arm's AXI5 (IHI 0022 issue L, Table B2.2), whose AXI4 interface class is what an AXI4 port wires:
the five channels, a record per handshake, a W or R burst as a `frame` of its channel, and each
burst paired with its address by ID. A signal the port doesn't give is what the table means by it,
so a port that leaves out the sizes, the burst types, `wlast`, the protections, the responses or
the reset still decodes; a role that is given is checked, and a reset that is given is honoured.

It is a `Protocol`, as `axis` is: its roles are read a cycle at a time, and each handshake's rules
are decided there, with the clause and the severity each rule carries (16.1).

```
decode mem axi4 clock=aclk prefix=m_axi_
```

Channels, `<prefix><role>` each, and the fields of a channel's record (one where its `valid` and
`ready` hold; a payload role not given is not a field):

| channel | valid, ready | fields |
| --- | --- | --- |
| `aw` | `awvalid` `awready` | `addr` (`awaddr`), `id` (`awid`), `len` (`awlen`), `size` (`awsize`), `burst` (`awburst`), `prot` (`awprot`), `lock` (`awlock`), `cache` (`awcache`), `qos` (`awqos`), `region` (`awregion`) |
| `w` | `wvalid` `wready` | `data` (`wdata`), `strb` (`wstrb`), `last` (`wlast`, only when given) |
| `b` | `bvalid` `bready` | `id` (`bid`), `resp` (`bresp`) |
| `ar` | `arvalid` `arready` | `addr` (`araddr`), `id` (`arid`), `len` (`arlen`), `size` (`arsize`), `burst` (`arburst`), `prot` (`arprot`), `lock` (`arlock`), `cache` (`arcache`), `qos` (`arqos`), `region` (`arregion`) |
| `r` | `rvalid` `rready` | `data` (`rdata`), `id` (`rid`), `resp` (`rresp`), `last` (`rlast`, only when given) |

The roles the protocol cannot decode without are required; the rest are what Table B2.2 says they
mean, given or not:

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
| `wlast` | no | — | the write burst's last beat; without it AxLEN frames the burst |
| `rlast` | no | — | the read burst's last beat; without it AxLEN frames the burst |
| `awlen` | no | `0`, one beat | the write burst's length in beats, less one |
| `arlen` | no | `0`, one beat | the read burst's length in beats, less one |
| `awsize` | no | `log2(wdata/8)`: one beat is the bus | the write's bytes per beat, a power of two |
| `arsize` | no | `log2(rdata/8)`: one beat is the bus | the read's bytes per beat |
| `awburst` | no | INCR (`0b01`) | the write's burst type: FIXED, INCR or WRAP |
| `arburst` | no | INCR (`0b01`) | the read's burst type |
| `awlock` | no | normal (`0`) | an exclusive write (`1`) or a normal one (`0`) |
| `arlock` | no | normal (`0`) | an exclusive read or a normal one |
| `awcache` | no | `0b0000` | the write's cacheability |
| `arcache` | no | `0b0000` | the read's cacheability |
| `awprot` | no | `0b000` | the write's protection: privileged, secure, instruction or data |
| `arprot` | no | `0b000` | the read's protection, as `awprot` |
| `awqos` | no | `0b0000` | the write's quality of service |
| `arqos` | no | `0b0000` | the read's quality of service |
| `awregion` | no | `0b0000` | the write's region |
| `arregion` | no | `0b0000` | the read's region |
| `awid` `bid` | no | — | the write's ID: its AW with its B; without them writes pair in order |
| `arid` `rid` | no | — | the read's ID: its AR with its R burst; without them reads pair in order |
| `wstrb` | no | all ones, one bit per `wdata` byte | which bytes of `wdata` are data |
| `bresp` | no | OKAY | how the write went |
| `rresp` | no | OKAY | how the read went |
| `aresetn` | no | — | the reset, active low; `reset_active=high` for active high |

Params: `timeout=<time>`, the wait after a request completes before it is `no response`
(`timeout=1us`, section 16's decoder bullet); `reset_active`, `low` (the default) or `high`.

The interface's signals, from Table B2.2's AXI5 column: `Y` both sides, `YM` manager only, `YS`
subordinate only, `O` optional. `axi4` decodes neither side of the port, so a presence code says
how common a signal is, not whether it is required here: the required roles above are. A signal
that is `not implemented` is a port wtf decodes without: it is no role, and nothing checks it.
Rows group signals that mean the same.

| signal | presence | default | role |
| --- | --- | --- | --- |
| `ACLK` | `Y` | — | the `clock=` argument: a handshake is one edge of it |
| `ARESETn` | `Y` | — | `aresetn`, optional here: a trace without it decodes, reset rules off |
| `AWVALID` `WVALID` `BVALID` `ARVALID` `RVALID` | `Y` | — | the required `*valid` roles |
| `AWREADY` `WREADY` `BREADY` `ARREADY` `RREADY` | `O`, since a credited transport has none | — | the required `*ready` roles: a handshake is what is decoded |
| `AWADDR` `ARADDR` | `Y` | — | `awaddr`, `araddr` |
| `AWID` `BID` `ARID` `RID` | `YS` | — | `awid` `bid` `arid` `rid`, optional: without them bursts pair in order |
| `AWLEN` `ARLEN` | `YS` | `0`, one beat | `awlen`, `arlen` |
| `AWSIZE` `ARSIZE` | `YS` | `log2(<data>/8)` | `awsize`, `arsize` |
| `AWBURST` `ARBURST` | `YS` | INCR (`0b01`) | `awburst`, `arburst` |
| `AWLOCK` `ARLOCK` | `O` | normal (`0`) | `awlock`, `arlock`: an exclusive access (A6.3) |
| `AWCACHE` `ARCACHE` | `O` | `0b0000` | `awcache`, `arcache` |
| `AWPROT` `ARPROT` | `O` | `0b000` | `awprot`, `arprot` |
| `AWREGION` `ARREGION` | `O` | `0b0000` | `awregion`, `arregion` |
| `AWQOS` `ARQOS` | `O` | `0b0000` | `awqos`, `arqos` |
| `WDATA` `RDATA` | `Y` | — | `wdata`, `rdata`: the bus `size` is measured against |
| `WSTRB` | `YS` | all ones, one bit per `wdata` byte | `wstrb` |
| `WLAST` | `YM` | — | `wlast`, optional: AxLEN frames the burst without it |
| `RLAST` | `YS` | — | `rlast`, optional: AxLEN frames the burst without it |
| `BRESP` `RRESP` | `O` | OKAY | `bresp`, `rresp` |
| `AWPENDING` `AWCRDT` `AWCRDTSH` `AWRP` `AWSHAREDCRD` `WPENDING` `WCRDT` `WCRDTSH` `WRP` `WSHAREDCRD` `BPENDING` `BCRDT` `ARPENDING` `ARCRDT` `ARCRDTSH` `ARRP` `ARSHAREDCRD` `RPENDING` `RCRDT` | `O` | — | not implemented: the credited transport (A2.4) |
| `AWATOP` | `O` | — | not implemented: atomic transactions (A6.4) |
| `AWDOMAIN` `AWSNOOP` `ARDOMAIN` `ARSNOOP` | `O` | — | not implemented: ACE's shareability and snoops (A7) |
| `AWUSER` `WUSER` `BUSER` `ARUSER` `RUSER` | `O` | — | not implemented |
| `WPOISON` `RPOISON` | `O` | — | not implemented |
| `AWTRACE` `WTRACE` `BTRACE` `ARTRACE` `RTRACE` | `O` | — | not implemented |
| `AWLOOP` `BLOOP` `ARLOOP` `RLOOP` | `O` | — | not implemented: the loopback signals |
| `WTAG` `WTAGUPDATE` `RTAG` `BTAGMATCH` `AWTAGOP` `ARTAGOP` | `O` | — | not implemented: MTE tags |
| `AWMMUVALID` `AWMMUSECSID` `AWMMUSID` `AWMMUSSIDV` `AWMMUSSID` `AWMMUATST` `AWMMUFLOW` `AWMMUPASUNKNOWN` `AWMMUPM` and their `AR` twins | `O` | — | not implemented: the MMU and PAS attributes |
| `AWPBHA` `ARPBHA` `AWMECID` `ARMECID` `AWNSAID` `ARNSAID` `AWSUBSYSID` `ARSUBSYSID` `AWMPAM` `ARMPAM` `AWIDUNQ` `ARIDUNQ` `BIDUNQ` `RIDUNQ` `AWNSE` `ARNSE` `AWPAS` `ARPAS` `AWINST` `ARINST` `AWPRIV` `ARPRIV` `AWCMO` `AWACT` `AWACTV` `ARACT` `ARACTV` `ARCHUNKEN` `RCHUNKV` `RCHUNKNUM` `RCHUNKSTRB` `BBUSY` `RBUSY` `BCOMP` `BPERSIST` | `O` | — | not implemented: AXI5's later additions (A10 to A12) |
| `ACVALID` `ACREADY` `ACPENDING` `ACCRDT` `ACADDR` `ACVMIDEXT` `ACTRACE` `CRVALID` `CRREADY` `CRPENDING` `CRCRDT` `CRTRACE` `ACWAKEUP` | `O` | — | not this interface: ACE's DVM messages (C1) |
| `AWAKEUP` | `O` | — | not implemented: the low-power handshake (A14.1.1) |
| `ACTIVATEREQ` `ACTIVATEACK` `ASKSTOP` `ACTIVATEREQD` `ACTIVATEACKD` `ASKSTOPD` `VARQOSACCEPT` `VAWQOSACCEPT` `SYSCOREQ` `SYSCOACK` | `O` | — | not this interface: credit control and the coherency connection (A14) |

A port that only writes or only reads is decoded too: a side none of whose roles is given is left
out (16.2), and the instance makes only the other side's streams; a side given in part is an
error naming the missing roles.

Streams:

| stream | it is |
| --- | --- |
| `<name>.wburst` | the W beats of a burst, framed by `wlast` when given and by AxLEN when not; `data` the bytes `wstrb` marks |
| `<name>.rburst` | the R beats of a burst, each ID's apart (bursts of different IDs interleave), framed by `rlast` when given and by AxLEN when not |
| `<name>.wr` | writes: AW and its W burst in order, then the B with its ID; `delay` from the later of AW and the burst's first beat to B |
| `<name>.wr.unmatched` | a write never answered, or not within `timeout` (`no response`), its B after that (`timeout`), a W burst without an AW (`no aw`) or the reverse (`no wburst`) |
| `<name>.rd` | reads: AR, then the R burst with its ID; `delay` (latency) from AR to the first R beat |
| `<name>.rd.unmatched` | an AR never answered, or not within `timeout` (`no response`), its R burst after that (`timeout`) |
| `<name>.errors` | a record per cycle where a rule holds, on `aw`, `w`, `b`, `ar` or `r`: `rule` names the channel and what it saw, `detail` says what it saw, `severity` is `error` or `warning` |

A B that comes before its write's handshakes, or an R before its read's, is not a `join` matter: it
is `b_early` or `r_early` in `errors`, and the burst pairs with the request it belongs to.

The rules, each with the clause of IHI 0022 issue L it comes from and the severity wtf gives it:

| rule | clause | severity | it reports | pinned by |
| --- | --- | --- | --- | --- |
| `<c>_dropped`, `<c>` in `aw` `w` `b` `ar` `r` | A2.3 | error | a valid fell before its ready: it stays until the handshake | `axi4_shared_error_rules`, `axi4_bursts_by_id` |
| `<c>_changed` | A2.3 | error | a payload signal changed while its valid was stalled | `axi4_shared_error_rules` |
| `<c>_reset` | A2.1.2 | error | a valid while the interface is in reset, or in its first cycle out | `axi4_reset_takes_no_records`, `axi4_shared_error_rules` |
| `<c>_unknown` | A2.3 | warning | a valid or ready is x or z out of reset | `axi4_unknown_valid`, `axi4_shared_error_rules` |
| `<c>_payload_unknown` | A2.3 | warning | a payload signal is x or z while its valid | `axi4_unknown_valid`, `axi4_shared_error_rules` |
| `b_resp` `r_resp` | A3.3 | warning | a SLVERR or DECERR response | `axi4_shared_error_rules`, `axi4_bursts_by_id` |
| `b_early` `r_early` | A2.3.2 | error | a response before its request's handshakes | `axi4_b_early_mid_burst`, `axi4_b_early_same_cycle`, `axi4_b_after_last_beat`, `axi4_multi_beat_read_not_early` |
| `b_exokay` `r_exokay` | A3.3 | error | an EXOKAY response to a non-exclusive access, with or without IDs | `axi4_exokay_non_exclusive_is_an_error`, `axi4_exokay_exclusive_is_silent`, `axi4_exokay_without_ids_is_an_error`, `axi4_shared_error_rules` |
| `aw_burst` `ar_burst` | A3.1.4 | error | AxBURST is reserved (`0b11`) | `axi4_burst_request_strobe_exclusive_rules` |
| `aw_wrap` `ar_wrap` | A3.1.4 | error | a WRAP burst of other than 2, 4, 8 or 16 beats, or not aligned to its size | `axi4_burst_request_strobe_exclusive_rules` |
| `aw_fixed` `ar_fixed` | A3.1.2 | error | a FIXED burst of more than 16 beats | `axi4_burst_request_strobe_exclusive_rules` |
| `aw_4k` `ar_4k` | A3.1.2 | error | an INCR burst crossing a 4KB boundary, its bytes counted from the address aligned to its size (A3.1.6) | `axi4_burst_request_strobe_exclusive_rules`, `axi4_request_4k_rule` |
| `aw_size` `ar_size` | A3.1.1 | error | AxSIZE wider than the data bus | `axi4_burst_request_strobe_exclusive_rules` |
| `aw_exclusive` `ar_exclusive` | A6.3.3 | error | an exclusive access misaligned to its bytes, not a power of two up to 128 bytes, or over 16 beats | `axi4_burst_request_strobe_exclusive_rules` |
| `w_last` `r_last` | A3.2 | error | LAST on a beat other than AxLEN+1, or missing on it | `axi4_wlast_vs_len`, `axi4_rlast_vs_len`, `axi4_burst_request_strobe_exclusive_rules` |
| `w_strb` | A3.2.1 | error | a strobe high outside the beat's byte lanes | `axi4_w_strb`, `axi4_burst_request_strobe_exclusive_rules` |

The `reset` rules fire only when `aresetn` is bound. The request rules (`burst`, `wrap`, `fixed`,
`4k`, `size`, `exclusive`) are decided at the AW or AR handshake, from that address record; `last`
and `strb` at the beat they are about. A rule whose inputs are x or z is not checked:
`payload_unknown` says that instead. Details are of the shape:

```
awvalid in reset
awvalid in the cycle reset is released
awvalid is x or z out of reset
awaddr is x or z while awvalid
awvalid fell before awready
awaddr changed while awvalid was stalled
bresp is SLVERR or DECERR
bresp is EXOKAY to a non-exclusive write
rresp is EXOKAY to a non-exclusive read
response 2 before write 2's AW and W
awburst is reserved (0b11)
awlen 2 with WRAP: length must be 2, 4, 8 or 16
awaddr 0x2 with WRAP: not aligned to 4 bytes
awlen 16 with FIXED: at most 16 beats
awaddr 0xff0, 4 beats of 4 bytes: crosses a 4KB boundary
awsize 3 wider than the data bus (4 bytes)
aw exclusive: 6 bytes is not a power of two up to 128
aw exclusive: address not aligned to 8 bytes
aw exclusive: 17 beats, at most 16
wlast on beat 2 of 4
missing wlast on beat 4 of 4
wstrb 0010 enables lane 1 outside beat 2 of 4
```

The rest of the conformance, clause by clause:

| clause | what it does | pinned by |
| --- | --- | --- |
| A1.2 | the five channels; each given `valid` and `ready` makes a record per handshake; a side given in part is an error | `axi4_one_side`, `axi4_required_roles` |
| A2.1.2 | a valid in reset, or in the cycle reset is released, is an error and records nothing | `axi4_reset_takes_no_records`, `axi4_shared_error_rules` |
| A2.3 | a valid stays until its ready (`dropped`), a payload stays while stalled (`changed`), x or z is `unknown` or `payload_unknown` | `axi4_shared_error_rules`, `axi4_unknown_valid` |
| A2.3.2 | a B only after its AW and its W burst, an R only after its AR; one before them is `b_early` / `r_early`, once | `axi4_b_early_mid_burst`, `axi4_b_early_same_cycle`, `axi4_b_after_last_beat`, `axi4_multi_beat_read_not_early` |
| A3.1.1 | AxSIZE within the bus | `axi4_burst_request_strobe_exclusive_rules` |
| A3.1.2 | FIXED at most 16 beats; INCR within 4KB | `axi4_burst_request_strobe_exclusive_rules` |
| A3.1.4 | WRAP of 2, 4, 8 or 16 beats, aligned; `0b11` reserved | `axi4_burst_request_strobe_exclusive_rules` |
| A3.2.1 | a strobe outside the beat's lanes (A3.1.6's equations) | `axi4_w_strb` |
| A3.2 | LAST on the burst's last beat, by AxLEN when the role is either given or not | `axi4_wlast_vs_len`, `axi4_rlast_vs_len`, `axi4_last_optional_axlen_framing` |
| A3.3 | SLVERR and DECERR warn; EXOKAY is an error unless the access is exclusive | `axi4_shared_error_rules`, `axi4_exokay_non_exclusive_is_an_error`, `axi4_exokay_exclusive_is_silent`, `axi4_exokay_without_ids_is_an_error` |
| A5.3, A5.6 | bursts pair by ID in the order they come, a burst's beats stay in order, different IDs interleave | `axi4_bursts_by_id` |
| A6.3.3 | an exclusive access aligned, a power of two up to 128 bytes, at most 16 beats | `axi4_burst_request_strobe_exclusive_rules` |
| Table B2.2 | the defaults above, an absent role being what AXI5 means by it | `axi4_required_roles`, `axi4_defaults_for_absent_roles`, `axi4_request_fields_without_rules` |
| 16.2 | the unmatched kinds, and `timeout` | `axi4_late_response_is_timeout`, `axi4_unmatched_without_no_request` |
| 16.2 | the card's rules, defaults and params | `axi4_protocols_describes_defaults_reset_rules` |

Not implemented, and so reported by nothing:

| clause | check | why not |
| --- | --- | --- |
| A2.4 | the credited transport's pending and credit signals | no role: `AWPENDING`, `AWCRDT` and their twins are wired without |
| A6.4 | an atomic transaction (`AWATOP`) or its response | `AWATOP` is no role |
| A6.3 | exclusivity itself: EXOKAY's promise over time | only the access's legality (A6.3.3) and an EXOKAY to a non-exclusive access are checked |
| A4 | region, cache, QoS and domain | the fields are decoded; nothing is checked |
| A7 | ACE's shareability and snoops | not this interface |
| A3.1.3 | an INCR burst over 256 beats | impossible: AxLEN is 8 bits |
| A5.3 | the order of a burst's own beats, or responses of one ID | beats are a `frame` in the order they come; a same-ID response pairs with the earliest complete request of that ID |
| A14.1.1 | the `AWAKEUP` handshake | no role |
| bind | a trace whose `awsize` disagrees with the port's real width | the fields are as wide as the signal |
