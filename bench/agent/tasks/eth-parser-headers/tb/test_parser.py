# The visible test: random packets, light backpressure on TX (ready low ~2% of the time) and
# moderate on HEADERS_READY.
import cocotb
from cocotbext.ofm.ver.backpressure import BackpressureConfig

from run import run


@cocotb.test()
async def parse(dut):
    await run(dut, pkt_count=1500,
              tx_cfg=BackpressureConfig(1, 2, 0.02), hdr_cfg=BackpressureConfig(1, 5, 0.5))
