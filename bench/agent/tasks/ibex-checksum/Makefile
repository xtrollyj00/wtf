# Ibex from lowRISC (Apache-2.0) with its writeback stage, simulated by Verilator: tb/tb.sv puts a
# memory with random wait states behind both ports and runs a bare-metal program built by clang.
# The design under test is rtl/; the lowRISC primitives and riscv-tests come from the Ibex
# checkout in UPSTREAM.
# make sim     tb/prog.c (CoreMark), seed 1: sim.log and trace.fst; fails if the checksum is wrong
# make hidden  the riscv-tests ISA suite (RV32IM), and tb/prog.c with other seeds

UPSTREAM ?= $(error UPSTREAM: the Ibex checkout)
IP := $(UPSTREAM)/vendor/lowrisc_ip/ip
TESTS := $(UPSTREAM)/vendor/riscv-tests/isa
CM := $(UPSTREAM)/vendor/eembc_coremark
CMSRC := $(CM)/core_list_join.c $(CM)/core_matrix.c $(CM)/core_state.c $(CM)/core_util.c
RVCC := clang --target=riscv32 -march=rv32imc -mabi=ilp32 -O2 -nostdlib -ffreestanding -fno-builtin -fuse-ld=lld -T tb/link.ld
PKGS := $(IP)/prim/rtl/prim_util_pkg.sv $(IP)/prim/rtl/prim_secded_pkg.sv $(IP)/prim/rtl/prim_mubi_pkg.sv \
	$(IP)/prim/rtl/prim_count_pkg.sv $(IP)/prim/rtl/prim_cipher_pkg.sv $(IP)/prim_generic/rtl/prim_ram_1p_pkg.sv \
	rtl/ibex_pkg.sv
SIM := obj_dir/Vtb
HEX = llvm-objcopy -O binary $< $@.bin && od -An -tx4 -v -w4 $@.bin > $@ && rm $@.bin

.PHONY: sim hidden
sim: $(SIM) prog.hex prog.sum
	@$(SIM) +seed=1 +expect=$$(cat prog.sum) +hex=prog.hex +trace 2>&1 | tee sim.log
	@grep -q PASS sim.log

hidden: $(SIM) prog.hex prog.sum $(patsubst $(TESTS)/%.S,hidden/%.hex,$(wildcard $(TESTS)/rv32ui/*.S $(TESTS)/rv32um/*.S))
	@rm -f hidden.log
	@for s in 2 3 4; do $(SIM) +seed=$$s +expect=$$(cat prog.sum) +hex=prog.hex > run.log; \
		cat run.log >> hidden.log; grep -q PASS run.log || echo "FAIL: prog.hex, seed $$s" >> hidden.log; done
	@for t in hidden/*/*.hex; do $(SIM) +seed=2 +expect=600d +hex=$$t > run.log; \
		cat run.log >> hidden.log; grep -q PASS run.log || echo "FAIL: $$t" >> hidden.log; done
	@! grep FAIL hidden.log

$(SIM): tb/tb.sv $(wildcard rtl/*.sv)
	@verilator --binary --timing --trace-fst -j 0 -Wno-fatal -Wno-lint -Wno-style -Wno-UNOPTFLAT \
		--top-module tb -DRVFI +incdir+$(IP)/prim/rtl +incdir+rtl \
		+incdir+$(UPSTREAM)/vendor/lowrisc_ip/dv/sv/dv_utils \
		-y rtl -y $(IP)/prim/rtl -y $(IP)/prim_generic/rtl $(PKGS) tb/tb.sv -o Vtb > build.log 2>&1 \
		|| { grep -A3 %Error build.log; false; }

prog.elf: tb/crt.S tb/prog.c tb/core_portme.h tb/link.ld
	@$(RVCC) -w -Itb -I$(CM) tb/crt.S tb/prog.c $(CMSRC) -o $@

prog.hex: prog.elf
	@$(HEX)

# CoreMark's known CRCs for the validation seeds, size 2000 (core_main.c): list 0xe714, matrix
# 0x1fd7, state 0x8e3a
prog.sum:
	@echo e70b593a > $@

hidden/%.elf: $(TESTS)/%.S tb/riscv_test.h
	@mkdir -p $(@D)
	@$(RVCC) -Itb -I$(TESTS)/macros/scalar $< -o $@

hidden/%.hex: hidden/%.elf
	@$(HEX)
