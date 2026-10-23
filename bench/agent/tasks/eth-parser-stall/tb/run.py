# ndk-fpga's eth_parser test loop (cocotb_test._run_test, CESNET, BSD-3-Clause), with a stall
# timeout of 50k cycles instead of 10M, so a parser that stops is a failure soon.
from typing import Optional, Tuple

import cocotb
from cocotb.clock import Clock
from cocotb.triggers import ClockCycles
from cocotbext.ofm.ver.backpressure import BackpressureConfig, apply_backpressure

from testbench import Testbench


async def run(
    dut,
    pkt_count: int,
    pkt_range: Tuple[int, int] = (60, 1500),
    tx_cfg: Optional[BackpressureConfig] = None,
    hdr_cfg: Optional[BackpressureConfig] = None,
    corrupt_prob: float = 0.05,
    stall_cycles: int = 50000,
):
    cocotb.start_soon(Clock(dut.CLK, 5, unit="ns").start())
    tb = Testbench(dut, debug=False, rate_limiter_config=dict(max_idles=5, zero_idles_chance=50))
    await tb.reset()
    tb.tx_driver.bus.TREADY.value = 1
    tx_task = cocotb.start_soon(apply_backpressure(dut.TX_AXI_TREADY, dut.CLK, tx_cfg))
    hdr_task = cocotb.start_soon(apply_backpressure(dut.HEADERS_READY, dut.CLK, hdr_cfg))

    for i in range(pkt_count):
        await tb.generate_and_send_packet(min_len=pkt_range[0], max_len=pkt_range[1], corrupt_prob=corrupt_prob)

    # wait while headers keep coming; a stretch of stall_cycles without one is a stall
    last, quiet = -1, 0
    while tb.headers_monitor.item_cnt < pkt_count and quiet < stall_cycles:
        if tb.headers_monitor.item_cnt != last:
            last, quiet = tb.headers_monitor.item_cnt, 0
        await ClockCycles(dut.CLK, 10)
        quiet += 10

    tx_task.cancel()
    hdr_task.cancel()
    if tb.headers_monitor.item_cnt < pkt_count:
        raise AssertionError(
            f"Only {tb.headers_monitor.item_cnt}/{pkt_count} packets processed: no header for {stall_cycles} cycles")
    if tb.scoreboard.errors > 0:
        raise AssertionError(f"Test failed with {tb.scoreboard.errors} errors")
