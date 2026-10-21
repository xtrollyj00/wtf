// Writes tests/data/enum.fst, an FST with an enum table as Verilator writes for SystemVerilog
// enums (nvc writes VHDL enums as strings instead). Built against libfst's sources:
//   cc -I<libfst>/src -o gen enum.c <libfst>/src/{fstapi,lz4,fastlz}.c -lz -lpthread && ./gen enum.fst
#include "fstapi.h"
#include <stdio.h>
int main(int argc, char **argv) {
    fstWriterContext *f = fstWriterCreate(argv[1], 1);
    fstWriterSetTimescale(f, -9);
    fstWriterSetScope(f, FST_ST_VCD_MODULE, "top", NULL);
    fstHandle clk = fstWriterCreateVar(f, FST_VT_VCD_WIRE, FST_VD_IMPLICIT, 1, "clk", 0);
    const char *names[] = {"IDLE", "READ", "WRITE", "DONE"};
    const char *vals[] = {"000", "001", "010", "011"};
    fstEnumHandle e = fstWriterCreateEnumTable(f, "state_t", 4, 3, names, vals);
    fstWriterEmitEnumTableRef(f, e);
    fstHandle state = fstWriterCreateVar(f, FST_VT_SV_LOGIC, FST_VD_IMPLICIT, 3, "state", 0);
    fstHandle count = fstWriterCreateVar(f, FST_VT_SV_LOGIC, FST_VD_IMPLICIT, 3, "count", 0);
    fstWriterSetUpscope(f);
    // state: IDLE 0, READ 10, WRITE 20, DONE 30, 101 (no name) 40, xxx 50, IDLE 60
    const char *st[] = {"000", "001", "010", "011", "101", "xxx", "000"};
    for (int t = 0; t <= 80; t += 5) {
        fstWriterEmitTimeChange(f, t);
        fstWriterEmitValueChange(f, clk, (t / 5) % 2 ? "1" : "0");
        if (t % 10 == 0 && t / 10 < 7) fstWriterEmitValueChange(f, state, st[t / 10]);
        if (t % 10 == 0) { char c[4]; int n = (t / 10) % 8; snprintf(c, 4, "%d%d%d", n >> 2 & 1, n >> 1 & 1, n & 1); fstWriterEmitValueChange(f, count, c); }
    }
    fstWriterClose(f);
    return 0;
}
