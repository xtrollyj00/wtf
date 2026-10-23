# AXIS_HEAD_TRIMMER from ndk-fpga (CESNET, BSD-3-Clause), simulated by nvc with cocotb. The
# design under test is rtl/; the rest comes from the ndk-fpga checkout in UPSTREAM.
# make sim     the test: sim.log and trace.fst; fails if the test does
# make hidden  ndk-fpga's own regression (4 tests, a few minutes), another seed

UPSTREAM ?= $(error UPSTREAM: the ndk-fpga checkout)
DEPS := $(addprefix $(UPSTREAM)/comp/base/, \
	pkg/math_pack.vhd pkg/type_pack.vhd \
	logic/carry_chain/carry_chain.vhd \
	logic/gen_nor/comp/gen_nor_fixed/gen_nor_fixed_ent.vhd logic/gen_nor/comp/gen_nor_fixed/gen_nor_fixed.vhd \
	logic/gen_nor/comp/gen_and_fixed/gen_and_fixed_ent.vhd logic/gen_nor/comp/gen_and_fixed/gen_and_fixed.vhd \
	logic/gen_nor/gen_nor_ent.vhd logic/gen_nor/gen_nor.vhd \
	logic/enc/enc_logic.vhd logic/enc/enc_ent.vhd logic/enc/enc.vhd \
	logic/mux/mux.vhd logic/mux/mux_onehot.vhd logic/mux/mux_piped.vhd \
	logic/barrel_shifter/barrel_shifter_gen.vhd logic/barrel_shifter/barrel_shifter.vhd \
	logic/barrel_shifter/barrel_shifter_gen_piped.vhd)

NVC := nvc --work=work -H 1G -M 4G
TOP := axis_head_trimmer
export TOPLEVEL := $(TOP)
export TOPLEVEL_LANG := vhdl
export COCOTB_RESOLVE_X := ZEROS
export PYTHONPATH := $(CURDIR)/tb:$(UPSTREAM)/comp/axis_tools/edit/head_trimmer/cocotb
RUN = PYGPI_PYTHON_BIN=$$(cocotb-config --python-bin) \
	GPI_USERS="$$(cocotb-config --libpython);$$(cocotb-config --pygpi-entry-point)" \
	$(NVC) -r $(TOP) --ieee-warnings=off --load $$(cocotb-config --lib-name-path vhpi nvc)

.PHONY: build sim hidden
build:
	@$(NVC) --std=2008 -a --relaxed $(DEPS) rtl/axis_head_trimmer.vhd
	@$(NVC) -e -O3 --no-collapse $(TOP)

sim: build
	@COCOTB_TEST_MODULES=test_trim COCOTB_RANDOM_SEED=1 $(RUN) -w --dump-arrays --wave=trace.fst 2>&1 | tee sim.log
	@grep -q "FAIL=0 " sim.log

hidden: build
	@COCOTB_TEST_MODULES=cocotb_test COCOTB_RANDOM_SEED=2 $(RUN) > hidden.log 2>&1
	@grep -q "FAIL=0 " hidden.log
