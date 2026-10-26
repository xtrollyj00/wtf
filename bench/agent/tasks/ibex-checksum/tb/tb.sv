`timescale 1ns/1ps
// The test bench: ibex_top in its maxperf configuration, with one memory (64 KiB at 0, loaded
// from +hex=FILE) behind its instruction and data ports, each granting and answering after random
// wait states (seeded by +seed=N). The program writes its checksum to 0x10000, compared with
// +expect=HEX, and anything to 0x10004 to stop. +trace dumps trace.fst.
module tb;
  logic clk_i = 0, rst_ni = 0;
  always #5 clk_i = ~clk_i;

  logic [31:0] mem [16384];
  int unsigned seed = 1, expect_sum = 0;
  int cycles = 0;
  string hex;

  logic        instr_req, instr_gnt, instr_rvalid;
  logic [31:0] instr_addr, instr_rdata;
  logic        data_req, data_gnt, data_rvalid, data_we;
  logic [3:0]  data_be;
  logic [31:0] data_addr, data_wdata, data_rdata;

  logic        rvfi_valid;
  logic [31:0] rvfi_insn, rvfi_pc_rdata, rvfi_rd_wdata;
  logic [4:0]  rvfi_rd_addr;

  ibex_top #(.WritebackStage(1'b1), .BranchTargetALU(1'b1), .RV32M(ibex_pkg::RV32MSingleCycle), .RV32B(ibex_pkg::RV32BNone)) dut (
    .clk_i, .rst_ni, .test_en_i(1'b0),
    .ram_cfg_icache_tag_i('0), .ram_cfg_icache_tag_o(), .ram_cfg_icache_data_i('0), .ram_cfg_icache_data_o(),
    .hart_id_i(32'h0), .boot_addr_i(32'h0),
    .instr_req_o(instr_req), .instr_gnt_i(instr_gnt), .instr_rvalid_i(instr_rvalid),
    .instr_addr_o(instr_addr), .instr_rdata_i(instr_rdata), .instr_rdata_intg_i('0), .instr_err_i(1'b0),
    .data_req_o(data_req), .data_gnt_i(data_gnt), .data_rvalid_i(data_rvalid), .data_we_o(data_we),
    .data_be_o(data_be), .data_addr_o(data_addr), .data_wdata_o(data_wdata), .data_wdata_intg_o(),
    .data_rdata_i(data_rdata), .data_rdata_intg_i('0), .data_err_i(1'b0),
    .irq_software_i(1'b0), .irq_timer_i(1'b0), .irq_external_i(1'b0), .irq_fast_i('0), .irq_nm_i(1'b0),
    .scramble_key_valid_i(1'b0), .scramble_key_i('0), .scramble_nonce_i('0), .scramble_req_o(),
    .debug_req_i(1'b0), .crash_dump_o(), .double_fault_seen_o(),
    .rvfi_valid, .rvfi_order(), .rvfi_insn, .rvfi_trap(), .rvfi_halt(), .rvfi_intr(), .rvfi_mode(),
    .rvfi_ixl(), .rvfi_rs1_addr(), .rvfi_rs2_addr(), .rvfi_rs3_addr(), .rvfi_rs1_rdata(),
    .rvfi_rs2_rdata(), .rvfi_rs3_rdata(), .rvfi_rd_addr, .rvfi_rd_wdata, .rvfi_pc_rdata,
    .rvfi_pc_wdata(), .rvfi_mem_addr(), .rvfi_mem_rmask(), .rvfi_mem_wmask(), .rvfi_mem_rdata(),
    .rvfi_mem_wdata(), .rvfi_ext_pre_mip(), .rvfi_ext_post_mip(), .rvfi_ext_nmi(), .rvfi_ext_nmi_int(),
    .rvfi_ext_debug_req(), .rvfi_ext_debug_mode(), .rvfi_ext_rf_wr_suppress(), .rvfi_ext_mcycle(),
    .rvfi_ext_mhpmcounters(), .rvfi_ext_mhpmcountersh(), .rvfi_ext_ic_scr_key_valid(),
    .rvfi_ext_irq_valid(), .rvfi_ext_expanded_insn_valid(), .rvfi_ext_expanded_insn(),
    .rvfi_ext_expanded_insn_last(),
    .fetch_enable_i(ibex_pkg::IbexMuBiOn), .alert_minor_o(), .alert_major_internal_o(),
    .alert_major_bus_o(), .core_sleep_o(), .scan_rst_ni(1'b1)
  );

  // a port: grants a request after 0-2 wait states, answers in order 1-3 cycles after the grant
  logic [31:0] iq [$], dq [$];
  int unsigned iwait, dwait;
  always_comb instr_gnt = instr_req && iwait == 0;
  always_comb data_gnt = data_req && dwait == 0;

  always @(posedge clk_i) begin
    cycles <= cycles + 1;
    if (cycles == 4) rst_ni <= 1;
    if (cycles > 2000000) begin $display("timeout after %0d cycles: FAIL", cycles); $finish; end
    instr_rvalid <= 0;
    data_rvalid <= 0;
    if (iq.size() > 0 && $urandom % 2 == 0) begin
      instr_rvalid <= 1;
      instr_rdata <= mem[iq.pop_front()[15:2]];
    end
    if (dq.size() > 0 && $urandom % 3 == 0) begin
      data_rvalid <= 1;
      data_rdata <= mem[dq.pop_front()[15:2]];
    end
    if (instr_gnt) iq.push_back(instr_addr);
    if (data_gnt) begin
      if (data_we) begin
        if (data_addr == 32'h10000) begin
          $display("checksum %08x, expected %08x: %s", data_wdata, expect_sum,
                   data_wdata == expect_sum ? "PASS" : "FAIL");
        end else if (data_addr == 32'h10004) begin
          $display("done after %0d cycles", cycles);
          $finish;
        end else
          for (int b = 0; b < 4; b++)
            if (data_be[b]) mem[data_addr[15:2]][8*b +: 8] <= data_wdata[8*b +: 8];
      end
      dq.push_back(data_addr);
    end
    iwait <= instr_req && !instr_gnt ? iwait - 1 : $urandom % 3;
    dwait <= data_req && !data_gnt ? dwait - 1 : $urandom % 3;
  end

  initial begin
    void'($value$plusargs("seed=%d", seed));
    void'($value$plusargs("expect=%h", expect_sum));
    void'($urandom(seed));
    if (!$value$plusargs("hex=%s", hex)) hex = "prog.hex";
    $readmemh(hex, mem);
    if ($test$plusargs("trace")) begin
      $dumpfile("trace.fst");
      $dumpvars(0, tb);
    end
  end
endmodule
