# The visible test: random packets, moderate backpressure on TX and HEADERS_READY.
import cocotb
from cocotbext.ofm.ver.backpressure import BackpressureConfig

from run import run


@cocotb.test()
async def parse(dut):
    await run(dut, pkt_count=1500,
              tx_cfg=BackpressureConfig(1, 5, 0.5), hdr_cfg=BackpressureConfig(1, 5, 0.5))
