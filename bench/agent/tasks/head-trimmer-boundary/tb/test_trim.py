# The visible test: ndk-fpga's head_trimmer test (cocotb_test._run_test), random packets with
# random trims under backpressure, short enough to rerun while debugging.
import os

import cocotb
from cocotbext.ofm.ver.backpressure import BackpressureConfig

from cocotb_test import _run_test


@cocotb.test()
async def trim(dut):
    await _run_test(
        dut, pkt_count=int(os.environ.get("PKTS", "400")),
        pkt_range=(60, 1500),
        tx_cfg=BackpressureConfig(1, 5, 0.5),
        test_name="trim",
    )
