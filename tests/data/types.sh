#!/bin/sh
# Dumps types.sv and types.vhd with each simulator installed, into tests/data/. Run from anywhere.
set -e
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if command -v verilator >/dev/null; then
    for fmt in vcd fst; do
        for structs in "" -structs; do
            flags="--trace"
            [ $fmt = fst ] && flags="--trace-fst"
            [ -n "$structs" ] && flags="$flags --trace-structs"
            dir="$work/verilator-$fmt$structs"
            verilator --binary --timing $flags -Wno-fatal --top-module types -Mdir "$dir" "$here/types.sv" >/dev/null
            (cd "$dir" && ./Vtypes +verilator+quiet >/dev/null)
            mv "$dir"/dump "$here/types-verilator$structs.$fmt"
        done
    done
else
    echo "verilator not found: skipped" >&2
fi

if command -v iverilog >/dev/null; then
    iverilog -g2012 -o "$work/types.vvp" "$here/types.sv"
    (cd "$work" && vvp -n types.vvp >/dev/null && vvp -n types.vvp -fst >/dev/null)
    mv "$work/dump.vcd" "$here/types-icarus.vcd"
    mv "$work/dump.fst" "$here/types-icarus.fst"
else
    echo "iverilog not found: skipped" >&2
fi

if command -v ghdl >/dev/null; then
    ghdl() { (cd "$work" && command ghdl "$@"); }
elif command -v docker >/dev/null; then
    ghdl() { docker run --rm -u "$(id -u):$(id -g)" -v "$here:$here" -v "$work:$work" -w "$work" ghdl/ghdl:6.0.0-mcode-ubuntu-24.04 ghdl "$@"; }
fi
if command -v ghdl >/dev/null || command -v docker >/dev/null; then
    ghdl -a --std=08 "$here/types.vhd"
    ghdl -e --std=08 types
    ghdl -r --std=08 types --stop-time=30ns --vcd="$here/types-ghdl.vcd"
    ghdl -r --std=08 types --stop-time=30ns --fst="$here/types-ghdl.fst"
    ghdl -r --std=08 types --stop-time=30ns --wave="$here/types-ghdl.ghw"
    # wellen can't read a GHW with a 2-D array: the same design without grid
    grep -v grid "$here/types.vhd" > "$work/nogrid.vhd"
    ghdl -a --std=08 --work=nogrid "$work/nogrid.vhd"
    ghdl -e --std=08 --work=nogrid types
    ghdl -r --std=08 --work=nogrid types --stop-time=30ns --wave="$here/types-ghdl-nogrid.ghw"
else
    echo "ghdl not found (nor docker): skipped" >&2
fi

if command -v nvc >/dev/null; then
    cd "$work"
    nvc -a "$here/types.vhd" -e types
    nvc -r types --wave="$here/types-nvc.fst" --dump-arrays --stop-time=30ns
    nvc -r types --wave="$here/types-nvc.vcd" --format=vcd --dump-arrays --stop-time=30ns
    nvc -r types --wave="$here/types-nvc-noarrays.fst" --stop-time=30ns
else
    echo "nvc not found: skipped" >&2
fi
