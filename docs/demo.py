# Writes docs/demo.vcd: an AXI4-Lite bus with one write whose address waits two cycles for
# ready, a read of the written value, and a read of an unmapped address answered with SLVERR.
# Run: python3 docs/demo.py docs/demo.vcd
import sys
P = 10  # clock period, ns; rising edges at 5, 15, 25 ...
sig = [  # name, width, id
    ("aclk", 1), ("aresetn", 1),
    ("s_axi_awvalid", 1), ("s_axi_awready", 1), ("s_axi_awaddr", 8),
    ("s_axi_wvalid", 1), ("s_axi_wready", 1), ("s_axi_wdata", 32), ("s_axi_wstrb", 4),
    ("s_axi_bvalid", 1), ("s_axi_bready", 1), ("s_axi_bresp", 2),
    ("s_axi_arvalid", 1), ("s_axi_arready", 1), ("s_axi_araddr", 8),
    ("s_axi_rvalid", 1), ("s_axi_rready", 1), ("s_axi_rdata", 32), ("s_axi_rresp", 2),
]
ids = {n: chr(33 + i) for i, (n, _) in enumerate(sig)}
width = dict(sig)
# changes at rising edge k (time 5 + 10k), seen by the next edge: (cycle, name, value)
ev = [
    (0, "aresetn", 0), (2, "aresetn", 1),
    (0, "s_axi_bready", 0), (0, "s_axi_rready", 0),
    (3, "s_axi_bready", 1), (3, "s_axi_rready", 1),
    # write 0xCAFEF00D to 0x10: address and data offered at 4, address accepted at 7 (stall)
    (4, "s_axi_awvalid", 1), (4, "s_axi_awaddr", 0x10), (4, "s_axi_wvalid", 1), (4, "s_axi_wdata", 0xCAFEF00D), (4, "s_axi_wstrb", 0xF),
    (5, "s_axi_wready", 1), (6, "s_axi_wvalid", 0), (6, "s_axi_wready", 0),
    (6, "s_axi_awready", 1), (7, "s_axi_awvalid", 0), (7, "s_axi_awready", 0),
    (8, "s_axi_bvalid", 1), (8, "s_axi_bresp", 0), (9, "s_axi_bvalid", 0),
    # read 0x10 back
    (11, "s_axi_arvalid", 1), (11, "s_axi_araddr", 0x10), (11, "s_axi_arready", 1),
    (12, "s_axi_arvalid", 0), (12, "s_axi_arready", 0),
    (13, "s_axi_rvalid", 1), (13, "s_axi_rdata", 0xCAFEF00D), (13, "s_axi_rresp", 0), (14, "s_axi_rvalid", 0),
    # read an unmapped address: SLVERR
    (16, "s_axi_arvalid", 1), (16, "s_axi_araddr", 0x40), (17, "s_axi_arready", 1),
    (18, "s_axi_arvalid", 0), (18, "s_axi_arready", 0),
    (19, "s_axi_rvalid", 1), (19, "s_axi_rdata", 0), (19, "s_axi_rresp", 2), (20, "s_axi_rvalid", 0),
]
END = 24
def val(n, v):
    w = width[n]
    return f"{v}{ids[n]}" if w == 1 else f"b{v:b} {ids[n]}"
out = ["$timescale 1ns $end", "$scope module top $end"]
out += [f"$var wire {w} {ids[n]} {n} $end" for n, w in sig]
out += ["$upscope $end", "$enddefinitions $end", "#0", "$dumpvars"]
init = {n: 0 for n, _ in sig}
out += [val(n, v) for n, v in init.items()] + ["$end"]
times = {}
for c in range(END):
    times.setdefault(c * P, []).append(("aclk", 0))
    times.setdefault(c * P + 5, []).append(("aclk", 1))
for c, n, v in ev:
    times.setdefault(c * P + 5, []).append((n, v))  # at the edge, as an RTL simulation dumps it
for t in sorted(times):
    if t == 0:
        continue
    out.append(f"#{t}")
    out += [val(n, v) for n, v in times[t]]
out.append(f"#{END * P}")
open(sys.argv[1], "w").write("\n".join(out) + "\n")
