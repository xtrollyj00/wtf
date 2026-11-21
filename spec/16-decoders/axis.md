## 16.1 AXI4-Stream (axis)

Arm's AXI4-Stream (IHI 0051B), decoded a cycle at a time: a **beat** is one cycle where `tvalid`
and `tready` hold together with that cycle's payload; a **packet** is the beats of one
`(tid, tdest)`, from its first beat until the beat `tlast` marks. A transfer `tready` stalls is
held, not counted twice: its payload must not move until the handshake completes.

```
decode rx axis clock=aclk prefix=s_axis_ aresetn=aresetn
```

Each role is `<prefix><role>`, and a role the port doesn't give is what Table B-1 says it means,
so a port with only `tvalid` still decodes:

| role | required | not given | it is |
| --- | --- | --- | --- |
| `tvalid` | yes | — | the transmitter has a beat |
| `tready` | no | 1 | the receiver takes a beat |
| `tdata` | no | 0 | the beat's bytes |
| `tkeep` | no | bytes of `tdata` | which bytes are data, one bit each |
| `tstrb` | no | same as `tkeep` | which bytes of `tdata` hold information |
| `tlast` | no | 1 | the packet's last beat |
| `tid` | no | 0 | the packet's stream |
| `tdest` | no | 0 | the packet's destination |
| `tuser` | no | 0 | the beat's sideband |
| `twakeup` | no | — | activity, to wake the clock (not given: no wakeup rules) |
| `aresetn` | yes | — | the reset, active low (`reset_active=high`: active high): the spec's `ARESETn`, which every interface has |

Params: `reset_active` (`low`, the default, or `high`).

Streams, those of the roles the instance has:

| stream | fields |
| --- | --- |
| `rx.beat` | `tdata`, `tkeep`, `tstrb`, `tlast`, `tid`, `tdest`, `tuser`, those given |
| `rx.frame` | `beats`, `tid`, `tdest` (if given), `len`, `pos` (with `tstrb`), `data` (with `tdata`, the bytes as hex text), `complete` |
| `rx.errors` | `rule`, `detail`, `severity` |

`rx.frame` is made from `rx.beat`; `rx.beat` and `rx.errors` from the trace alone. A stream's
inputs are known from what made it (spec §16 "Results"), never from the trace.

A `frame`'s `len` counts the bytes `tkeep` marks and `pos` those of them `tstrb` marks as
position; `complete` is 0 when the trace ended before the packet did.

The rules `errors` reports, a record per cycle each holds:

| rule | clause | severity | it reports | pinned by |
| --- | --- | --- | --- | --- |
| `dropped` | 2.2 | error | tvalid fell before tready: it must stay until the handshake | `axis_errors_are_the_injected_ones`, `axis_error_rules` |
| `changed` | 2.2.1 | error | a payload signal changed while tvalid was stalled | `axis_errors_are_the_injected_ones` |
| `unknown` | 2.2 | error | tvalid or tready is x or z out of reset | `axis_error_rules` |
| `payload_unknown` | 2.4 | error | a payload signal is x or z while tvalid | `axis_error_rules` |
| `keep` | 2.5.3 | error | tkeep is 0 where tstrb is 1 | `axis_error_rules` |
| `reset` | 2.8.2 | error | tvalid while the interface is in reset, or in its first cycle out | `axis_reset_takes_no_beats` |
| `wakeup` | 2.3 | error | twakeup fell while a transfer was stalled | `axis_error_rules` |
| `wakeup_early` | 2.3 | warning | twakeup in the same cycle as tvalid | `axis_warning_rules` |

The rest of the conformance, clause by clause:

| clause | what `axis` does | pinned by |
| --- | --- | --- |
| 2.2 | a beat's tvalid is held until its tready, so one that falls first is `dropped`, once | `axis_errors_are_the_injected_ones` |
| 2.2.1 | a payload signal that moves while tvalid is stalled is `changed` | `axis_errors_are_the_injected_ones` |
| 2.3 | twakeup rising with tvalid is `wakeup_early`, falling while a transfer stalls is `wakeup` | `axis_warning_rules`, `axis_error_rules` |
| 2.5.1 | `beats` and `len` count the beats and the bytes `tkeep` marks | `axis_beats_and_frames` |
| 2.6 | a packet per (tid, tdest); tlast closes it, and one still open when the trace ends comes with `complete` 0 | `axis_frames_per_tid_tdest` |
| 2.8.2 | tvalid in reset, or in the cycle the reset is released, is `reset`; no beat is decoded there | `axis_reset_takes_no_beats` |
| Table B-1 | the defaults above: tready 1, tdata 0, tkeep the bytes of tdata, tstrb as tkeep, tlast 1, tid/tdest/tuser 0 | `host_defaults_and_describe`, `axis_tvalid_only_decodes` |

Not implemented, and so reported by nothing:

| clause | check | why not |
| --- | --- | --- |
| 3.3 | Continuous_Packets: tlast, tid and tdest stay put while a packet is open | not implemented |
| ch. 5 | parity: the `*CHK` and `*PAR` signals | not implemented |
| 2.1 | static widths at bind (tdata whole bytes, tkeep one bit per byte of it) | not implemented |
| B.1 | the AXI5-only signals, checked at bind | not implemented |
