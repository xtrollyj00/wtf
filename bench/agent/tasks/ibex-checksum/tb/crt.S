  .section .vectors, "ax"
  .option norvc
  .rept 32
  j trap
  .endr
  .section .text.start, "ax"
  .global _start
_start:
  la sp, _stack
  call main
  li t0, 0x10000
  sw a0, 0(t0)
  sw zero, 4(t0)
trap:
  j trap
