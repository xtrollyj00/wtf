# The visible test: an AXI4-Lite master configures a block of 64 registers behind the converter,
# mostly a word at a time, now and then two words in one access, while other masters read; then
# it reads every register back and compares it with what it wrote. Only the read-back is checked.
import cocotb
from cocotb.triggers import ClockCycles, Combine

from testbench import get_testbench

REGS = 64
WRITES = 200


async def configure(tb, written):
    for _ in range(WRITES):
        words = 2 if tb.rng.random() < 0.08 else 1
        reg = tb.rng.randrange(0, REGS - words + 1)
        data = tb.rng.randbytes(4 * words)
        await tb.write(4 * reg, data)
        for i in range(words):
            written[reg + i] = data[4 * i : 4 * i + 4]
        await ClockCycles(tb.dut.CLK, tb.rng.randint(0, 6))


async def poll(tb, count):
    for _ in range(count):
        await tb.read(4 * tb.rng.randrange(REGS), 4)
        await ClockCycles(tb.dut.CLK, tb.rng.randint(2, 20))


@cocotb.test(timeout_time=400, timeout_unit="us")
async def registers(dut):
    tb = await get_testbench(dut, seed=7)
    tb.set_axi_backpressure(0.1, 0.2, 0.1, 0.1, 0.1)
    tb.set_mi(0.2, 0, 3)
    written = {}
    await Combine(cocotb.start_soon(configure(tb, written)), cocotb.start_soon(poll(tb, 150)), cocotb.start_soon(poll(tb, 150)))
    await tb.drain()
    tb.log.info("configured %d registers with %d writes; reading them back", len(written), WRITES)
    bad = []
    for reg in range(REGS):
        got = await tb.read(4 * reg, 4)
        want = written.get(reg, tb.mi.read_word(reg))
        if got != want:
            bad.append(f"register 0x{4 * reg:02X}: read 0x{int.from_bytes(got, 'little'):08X}, expected 0x{int.from_bytes(want, 'little'):08X}")
    for line in bad:
        tb.log.error(line)
    assert not bad, f"{len(bad)} of {REGS} registers read back wrong"
