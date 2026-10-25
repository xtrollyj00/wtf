# AXI4LITE2MI from ndk-fpga (CESNET, BSD-3-Clause), simulated by nvc with cocotb: an AXI4-Lite
# slave that serves each access as one request on the MI bus. The design under test is rtl/; the
# testbench models come from the ndk-fpga checkout in UPSTREAM.
# make sim     the test: sim.log and trace.fst; fails if the test does
# make hidden  ndk-fpga's own regression (every MI request checked against its AXI one)

UPSTREAM ?= $(error UPSTREAM: the ndk-fpga checkout)
NVC := nvc --work=work -H 1G -M 4G
TOP := axi4lite2mi
export TOPLEVEL := $(TOP)
export TOPLEVEL_LANG := vhdl
export COCOTB_RESOLVE_X := ZEROS
export PYTHONPATH := $(CURDIR)/tb:$(UPSTREAM)/comp/mi_tools/converters/axi4lite2mi/cocotb
RUN = PYGPI_PYTHON_BIN=$$(cocotb-config --python-bin) \
	GPI_USERS="$$(cocotb-config --libpython);$$(cocotb-config --pygpi-entry-point)" \
	$(NVC) -r $(TOP) --ieee-warnings=off --load $$(cocotb-config --lib-name-path vhpi nvc)

.PHONY: build sim hidden
build:
	@$(NVC) --std=2008 -a --relaxed rtl/axi4lite2mi.vhd
	@$(NVC) -e -O3 --no-collapse $(TOP)

sim: build
	@COCOTB_TEST_MODULES=test_regs COCOTB_RANDOM_SEED=1 $(RUN) -w --dump-arrays --wave=trace.fst 2>&1 | tee sim.log
	@grep -q "FAIL=0 " sim.log

hidden: build
	@COCOTB_TEST_MODULES=cocotb_test COCOTB_RANDOM_SEED=2 $(RUN) > hidden.log 2>&1
	@grep -q "FAIL=0 " hidden.log
