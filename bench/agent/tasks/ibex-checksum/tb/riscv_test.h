// The riscv-tests environment for tb.sv: a test passes by writing 0x600d as its checksum and
// fails by writing its test number; a trap hangs it until the timeout.
#define RVTEST_RV32U
#define RVTEST_RV32M
#define TESTNUM gp
#define RVTEST_CODE_BEGIN .section .vectors, "ax"; .rept 32; j .; .endr; .section .text.start, "ax"; .global _start; _start: li TESTNUM, 0;
#define RVTEST_CODE_END unimp
#define RVTEST_PASS li a0, 0x600d; li t0, 0x10000; sw a0, 0(t0); sw zero, 4(t0);
#define RVTEST_FAIL li t0, 0x10000; sw TESTNUM, 0(t0); sw zero, 4(t0);
#define RVTEST_DATA_BEGIN .data; .align 4;
#define RVTEST_DATA_END
