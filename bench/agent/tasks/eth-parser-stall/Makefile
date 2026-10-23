# AXIS_ETH_PARSER from ndk-fpga (CESNET, BSD-3-Clause), simulated by nvc with cocotb, with a
# 128-bit data bus and 8-deep header FIFOs. The design under test is rtl/; the rest comes from
# the ndk-fpga checkout in UPSTREAM.
# make sim     the test: sim.log and trace.fst; fails if the test does
# make hidden  ndk-fpga's three test profiles, longer, another seed

UPSTREAM ?= $(error UPSTREAM: the ndk-fpga checkout)
BASE := $(UPSTREAM)/comp/base
PARSER := $(UPSTREAM)/comp/axis_tools/logic/eth_parser
DEPS := $(BASE)/pkg/math_pack.vhd $(BASE)/pkg/type_pack.vhd $(PARSER)/axis_eth_parser_types.vhd \
	$(addprefix $(BASE)/logic/, carry_chain/carry_chain.vhd \
		gen_nor/comp/gen_nor_fixed/gen_nor_fixed_ent.vhd gen_nor/comp/gen_nor_fixed/gen_nor_fixed.vhd \
		gen_nor/comp/gen_and_fixed/gen_and_fixed_ent.vhd gen_nor/comp/gen_and_fixed/gen_and_fixed.vhd \
		gen_nor/gen_nor_ent.vhd gen_nor/gen_nor.vhd enc/enc_logic.vhd enc/enc_ent.vhd enc/enc.vhd \
		mux/mux.vhd mux/mux_onehot.vhd mux/mux_piped.vhd \
		barrel_shifter/barrel_shifter_gen.vhd barrel_shifter/barrel_shifter.vhd \
		barrel_shifter/barrel_shifter_gen_piped.vhd) \
	$(addprefix $(BASE)/mem/, sdp_bram/sdp_bram_intel/sdp_bram_intel_ent.vhd \
		sdp_bram/sdp_bram_intel/sdp_bram_intel_empty.vhd sdp_bram/sdp_bram_xilinx/sdp_bram_xilinx_ent.vhd \
		sdp_bram/sdp_bram_xilinx/sdp_bram_xilinx.vhd sdp_bram/sdp_bram_behav.vhd sdp_bram/sdp_bram.vhd \
		sdp_bram/sdp_bram_be.vhd gen_lutram/altdpram/altdpram_wrap_ent.vhd \
		gen_lutram/altdpram/altdpram_wrap_empty.vhd gen_lutram/gen_lutram.vhd \
		dp_uram_xilinx/dp_uram_xilinx_ent.vhd dp_uram_xilinx/dp_uram_xilinx_arch.vhd \
		sdp_uram_xilinx/sdp_uram_xilinx_ent.vhd sdp_uram_xilinx/sdp_uram_xilinx_arch.vhd) \
	$(addprefix $(BASE)/shreg/sh_reg_base/, sh_reg_base_dynamic_ent.vhd sh_reg_base_dynamic_arch.vhd \
		sh_reg_base_static.vhd) \
	$(BASE)/fifo/fifox/fifox.vhd $(PARSER)/axis_eth_parser_sniffer.vhd $(PARSER)/axis_eth_parser_unit.vhd

NVC := nvc --work=work -H 1G -M 4G
TOP := axis_eth_parser
export TOPLEVEL := $(TOP)
export TOPLEVEL_LANG := vhdl
export COCOTB_RESOLVE_X := ZEROS
export PYTHONPATH := $(CURDIR)/tb:$(PARSER)/cocotb
RUN = PYGPI_PYTHON_BIN=$$(cocotb-config --python-bin) \
	GPI_USERS="$$(cocotb-config --libpython);$$(cocotb-config --pygpi-entry-point)" \
	$(NVC) -r $(TOP) --ieee-warnings=off --load $$(cocotb-config --lib-name-path vhpi nvc)

.PHONY: build sim hidden
build:
	@$(NVC) --std=2008 -a --relaxed $(DEPS) rtl/axis_eth_parser.vhd
	@$(NVC) -e -O3 --no-collapse -gAXI_TDATA_WIDTH=128 -gFIFO_DEPTH=8 -gDEVICE=ULTRASCALE $(TOP)

sim: build
	@COCOTB_TEST_MODULES=test_parser COCOTB_RANDOM_SEED=1 $(RUN) -w --dump-arrays --wave=trace.fst 2>&1 | tee sim.log
	@grep -q "FAIL=0 " sim.log

hidden: build
	@COCOTB_TEST_MODULES=test_hidden COCOTB_RANDOM_SEED=2 $(RUN) > hidden.log 2>&1
	@grep -q "FAIL=0 " hidden.log
