# The hidden regression: ndk-fpga's three test profiles (base, headers backpressure, corruption).
import cocotb
from cocotbext.ofm.ver.backpressure import BackpressureConfig

from run import run


@cocotb.test()
async def base(dut):
    await run(dut, pkt_count=3000,
              tx_cfg=BackpressureConfig(1, 5, 0.5), hdr_cfg=BackpressureConfig(1, 5, 0.5))


@cocotb.test()
async def headers_backpressure(dut):
    await run(dut, pkt_count=2000,
              tx_cfg=BackpressureConfig(1, 5, 0.1), hdr_cfg=BackpressureConfig(20, 200, 0.9))


@cocotb.test()
async def corruption(dut):
    await run(dut, pkt_count=2000, corrupt_prob=0.5,
              tx_cfg=BackpressureConfig(1, 5, 0.5), hdr_cfg=BackpressureConfig(1, 5, 0.5))
