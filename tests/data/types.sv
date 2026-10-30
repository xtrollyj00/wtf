`timescale 1ns/1ps
// types-verilator*.{vcd,fst}'s design: composite types as Verilator dumps them (types.sh makes
// them). clk rises at 5, 15, 25ns; from 10ns every signal holds the values below, 0 before.
typedef enum logic [1:0] {IDLE, BUSY, DONE} state_t;
typedef struct packed { logic [3:0] hi; logic [3:0] lo; } pair_t;
typedef struct packed { logic valid; state_t state; pair_t pair; } pkt_t; // 11 bits
typedef struct { logic [7:0] a; logic b; } loose_t;                     // unpacked
typedef union packed { logic [7:0] byte_; pair_t pair; } word_t;

module types;
    logic clk = 0;
    always #5 clk = ~clk;
    initial begin
        $dumpfile("dump");
        $dumpvars;
        #30 $finish;
    end

    pkt_t pkt = '0;               // 10ns: valid 1, state BUSY, pair.hi 0xA, pair.lo 0x5
    loose_t loose = '{8'h0, 1'b0}; // 10ns: a 0x3C, b 1
    logic [3:0][7:0] packed2 = '0;  // 10ns: [3] 0x44, [2] 0x33, [1] 0x22, [0] 0x11
    logic [7:0] mem [0:3];        // 10ns: mem[i] = 0x10 + i
    logic [3:0] grid [0:1][0:2];  // 10ns: grid[i][j] = 3 * i + j
    pkt_t pkts [0:1];             // 10ns: pkts[1].pair.lo 0x7, the rest 0
    word_t word = '0;             // 10ns: byte_ 0xC3

    initial begin
        foreach (mem[i]) mem[i] = '0;
        foreach (grid[i, j]) grid[i][j] = '0;
        foreach (pkts[i]) pkts[i] = '0;
        #10;
        pkt = '{valid: 1'b1, state: BUSY, pair: '{hi: 4'hA, lo: 4'h5}};
        loose = '{8'h3C, 1'b1};
        packed2 = {8'h44, 8'h33, 8'h22, 8'h11};
        foreach (mem[i]) mem[i] = 8'h10 + 8'(i);
        foreach (grid[i, j]) grid[i][j] = 4'(3 * i + j);
        pkts[1].pair.lo = 4'h7;
        word.byte_ = 8'hC3;
    end

    for (genvar g = 0; g < 2; g++) begin : gen
        pair_t p = '0; // 10ns: gen[1].p is 0x96, gen[0].p stays 0
        initial #10 p = g == 1 ? 8'h96 : 8'h00;
    end
endmodule
