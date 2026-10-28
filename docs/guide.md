# wtf user guide

This guide is for opening wtf the first time. The [README](../README.md) says what wtf is, and
the [spec](../spec/README.md) is the full reference, linked below as `§N`.

- [A 5-minute tour](#a-5-minute-tour)
- [Cheat sheet](#cheat-sheet)
- [Getting a trace from your simulator](#getting-a-trace-from-your-simulator)
- [Look and feel](#look-and-feel)
- [Working with an AI agent](#working-with-an-ai-agent)

## A 5-minute tour

The tour uses `docs/demo.vcd`: an AXI4-Lite bus (`top.s_axi_*` and `aclk`) with a write of
`0xCAFEF00D` to `0x10` whose address waits two cycles for ready, a read of it back, and a read of
an unmapped address answered with SLVERR. `docs/demo.py` writes it.

`docs/demo.vcd` comes with a saved session. For a clean start, work on a copy:

```sh
cp docs/demo.vcd tour.vcd
wtf tour.vcd
```

Every step can be reproduced as text, without a terminal:
`wtf tour.vcd --snapshot 100x14 --keys '<keys>'` presses the keys (Vim notation: `<CR>`, `<Esc>`,
`<Space>`) and prints the screen (§12). The screens below come from it.

### 1. Add signals

The screen starts empty. `i` enters Edit mode and opens the signal panel, a tree of the file's
signals. In it:

| Keys | Does |
|---|---|
| `/aclk` `Enter` | search for a path; the highlight jumps to the match |
| `Enter` | add the highlighted signal as a row |
| `Space` | mark a signal |
| `n` | next match of the search |
| `Enter` with marks | add all marked signals |

Add `aclk` first. Then mark three: `/s_axi_aw` `Enter`, `Space`, `n`, `Space`, `n`, `Space`, and
`Enter` adds `s_axi_awvalid`, `s_axi_awready` and `s_axi_awaddr`. A wrong add is taken back with
`u`. `Esc` closes the panel, `Esc` again goes back to View mode.

Keys: `i/aclk<CR><CR>/s_axi_aw<CR><Space>n<Space>n<Space><CR><Esc><Esc>`

### 2. Move and zoom

`h` / `l` move the cursor one column, `w` / `b` jump to the selected signal's next / previous
change, `gg` / `G` go to the start / end, `:100ns` goes to a time. `z` / `x` zoom in / out around
the cursor, `:zoom 1ns` sets the time per column, `:fit` shows the whole trace. `j` / `k` select
another row. A count repeats a key: `5l`, `3w`. The cursor is the vertical bar; the value column
shows each signal at the cursor.

### 3. Search a value, then an expression

With `s_axi_awaddr` selected, `/0x10` `Enter` jumps to where it becomes `0x10`. `n` / `N` find the
next / previous match; the status line says `match 1 of 1`. On a 1-bit signal `/1` finds the next
rising edge. The value is found in any radix (§8).

```text
                     0ns            45ns              100ns            150ns           200ns
                      ┌─┐ ┌┐ ┌─┐┌─┐ ┃┐ ┌─┐┌─┐ ┌┐ ┌─┐┌─┐ ┌┐ ┌─┐┌─┐ ┌┐ ┌─┐┌─┐ ┌┐ ┌─┐┌─┐ ┌┐ ┌─┐┌─┐ ┌┐ ┌
aclk           1     ─┘ └─┘└─┘ └┘ └─┃└─┘ └┘ └─┘└─┘ └┘ └─┘└─┘ └┘ └─┘└─┘ └┘ └─┘└─┘ └┘ └─┘└─┘ └┘ └─┘└─┘
                                    ┃─────────┐
s_axi_awvalid  1     ───────────────┃         └─────────────────────────────────────────────────────
                                    ┃     ┌───┐
s_axi_awready  0     ───────────────┃─────┘   └─────────────────────────────────────────────────────
                     ───────────────┃───────────────────────────────────────────────────────────────
s_axi_awaddr   0x10  ───── 0x0 ─────┃──────────────────────────── 0x10 ─────────────────────────────

 VIEW  match 1 of 1                                               45ns  ·  3ns/col  ·  tour.vcd [+]
```

Keys: the keys of step 1, then `/0x10<CR>`.

An expression searches several signals at once, written as in SystemVerilog:
`/s_axi_awvalid && s_axi_awready` jumps to where the address is offered and accepted at the
same time. With a clock, `/@aclk s_axi_awvalid && !s_axi_awready` matches clock cycles (sampled
at the clock's edge), so it finds the stall (§8). The decoder in step 5 does this for you.

### 4. Marks and Δt

The cursor is cursor 1. Marks (2 to 9) stay where you put them. Select `s_axi_awvalid` (`kk`),
`/1` `Enter` goes to its rising edge, `m` puts mark 2 there. Select `s_axi_awready` (`j`), `/1`
`Enter` goes to its rising edge. A ruler appears above the waveforms with the time between
neighbours; zoom in (`zz`) until the label fits.

Keys: the keys of step 1, then `kk/1<CR>mj/1<CR>zz`

`Tab` jumps to the next mark, `'2` to mark 2, `''` back to where you were, `M` removes a mark,
`yt` copies the cursor's time (§9). Here the valid waits 20 ns for the ready.

### 5. Decode the bus

Finding handshakes by hand is slow. A decoder turns the bus into records:

```text
:decode bus axilite clock=aclk prefix=s_axi_
```

`prefix=` plays each AXI4-Lite role by `s_axi_<role>` (`awvalid`, `awaddr`, ...). The status line
counts the records of each stream it made (`bus.wr`, `bus.rd`, ...) and the violations.
`:add bus.wr` shows a stream as a row; `:records bus.rd` lists its records in a box:

```text
                      ╎    50ns           65ns 70ns      80ns      90ns      100ns     110ns
                      2────── 20ns ───────1
                      ╎────┐    ┌────┐    ┃────┐    ┌────┐    ┌────┐    ┌────┐    ┌────┐
aclk           1     ─╎    └────┘    └────┃    └────┘    └────┘    └────┘    └────┘    └────┘
                      ╎───────────────────┃─────────┐
s_axi_awvalid     records · bus.rd                           Enter jump  Esc close    ──────────────
                  2 records
s_axi_awready     ▌ #0  115ns–145ns  delay=20ns addr=0x10 data=0xCAFEF00D resp=0x0    ──────────────
                    #1  175ns–205ns  delay=20ns addr=0x40 data=0x0 resp=0x2           ──────────────
s_axi_awaddr
```

The second read returned `resp=0x2`, SLVERR. `:records bus.rd where resp != 0` lists only the
failures. In the box, `j` / `k` move and `Enter` jumps the cursor to the record. Streams and
decoders: §16.

### 6. Fold, examine, change radix

`Space` on a stream row (`bus.wr`) unfolds it into its fields (`delay`, `addr`, `data`, `strb`),
and again folds it. `Space` on a bus expands it into bits; on a group it collapses it. `e` shows
the whole value at the cursor in a box (on a stream, the whole record); `Esc` closes it.

Radix: `i` for Edit mode, select a row, `r` cycles it (`s_axi_awaddr`: hex, bin, dec, ...).
`:radix dec` sets one directly. `:radix q4.12` is fixed point (§10). `V` selects a range of rows,
`:group <name>` groups them.

### 7. Save, rerun, reload

`:w` writes `tour.vcd.session.toml` next to the trace: rows, groups, radixes, decodes, cursors,
zoom. Next time `wtf tour.vcd` opens just as you left it. `-S <file>` loads another (§11).

After you rerun the simulation, the window sees the file change and reloads it by itself
(`reloaded` on the status line). `:e` does the same by hand, keeping your rows, cursor and zoom;
signals the new trace lacks are dropped and listed. `:q` quits (it refuses with unsaved changes),
`:q!` quits without saving, `:wq` saves and quits.

## Cheat sheet

`?` in the window shows the keys of the current mode. `:help` shows the same; `:help <topic>`
shows the agents' reference (`records`, `decode`, ...). A count before a key repeats it.

### View mode (§3, §9)

| Keys | Does |
|---|---|
| `h` `l` | cursor left / right |
| `w` `b` | next / previous change of the selected signal |
| `gg` `G` | start / end |
| `H` `L` | scroll the screen left / right |
| `Ctrl-d` `Ctrl-u` | half a screen right / left |
| `c` | center the screen on the cursor |
| `z` `x` | zoom in / out |
| `j` `k` | select a row |
| `Ctrl-f` `Ctrl-b` | a screen of rows |
| `Space` | fold / unfold a group, bus or stream |
| `e` | the whole value (record) at the cursor |
| `yy` `yt` | copy the value / the time |
| `/` | search a value or expression |
| `n` `N` | next / previous match |
| `m` `M` | put / remove a mark (`3m`: mark 3) |
| `Tab` `Shift-Tab` | next / previous mark |
| `'2`...`'9`, `'1` | jump to a mark / center the cursor |
| `''` | back to before the last jump |
| `i` | Edit mode |
| `:` | command |
| `?` | help |
| `q` | quit |

### Edit mode, rows (§5, §6)

| Keys | Does |
|---|---|
| `j` `k` | select a row |
| `J` `K` | move the row down / up |
| `V` | select a range |
| `a` | open the signal panel |
| `/` | open the signal panel, searching |
| `dd` | remove the row (or all selected) |
| `r` | next radix |
| `Space` | fold / unfold |
| `e` | the whole value |
| `u` `Ctrl-r` | undo / redo |
| `Esc` | back to View |

### Signal panel (§5)

| Keys | Does |
|---|---|
| `j` `k` `gg` `G` | move |
| `l` `h` | open / close a scope |
| `/` then `Enter` | search paths; `n` `N` next / previous match |
| `Space` | mark a signal (or all in a scope) |
| `V` | mark a range |
| `Ctrl-a` | mark all matches |
| `Enter` | add the marked signals, or the highlighted one |
| `Ctrl-g` | add as a group, then name it |
| `u` `Ctrl-r` | undo / redo |
| `Esc` | clear the range, clear the marks, close the panel |

New rows go below the selected row.

### Commands (§7)

| Command | Does |
|---|---|
| `:<time>` | go to a time: `:2000ns`, `:1.5us` |
| `:zoom <time>` | time per column |
| `:fit [<from> <to>]` | whole trace, or between two cursors (`:fit '2 '3`) |
| `:add <path\|pattern>...` | add signals (`:add top.cpu.*`; a scope becomes a group) |
| `:rm [pattern]` | remove rows; `:rm **` all |
| `:signal <name> = <expr>` | a computed signal: `:signal op = cmd[7:0]` |
| `:decode <name> <protocol> ...` | decode a bus (`axis`, `axilite`, `axi4`, `spi`, `uart`, ...) |
| `:records <stream> [where <expr>]` | list a stream's records |
| `:group <name>` `:rename` `:ungroup` | groups |
| `:radix <name>` | `hex bin dec sdec oct ascii float q4.12 enum` |
| `:names <signal> 0=IDLE 1=BUSY` | name values, for the `enum` radix |
| `:color <name\|off>` | colour a row |
| `:cursor <n> [time\|off]` | place or remove a mark |
| `:scope <path>` | names resolve under this scope first |
| `:undo` `:redo` | row layout changes |
| `:e` | reload the file |
| `:w [file]` `:wq` | save the session |
| `:q` `:q!` | quit / quit without saving |
| `:help [topic]` | help |

Search expressions: `/0x40` a value, `/x` `/z` unknown / high impedance, `/valid && ready`,
`/@clk fell(valid) && !past(ready)` (also `rose()` `stable()`).

Command line (§12): `wtf [-S <session>] [--glyphs <set>] [--snapshot <w>[x<h>] [--ansi]
[--keys <keys>]] <trace>`, `wtf --attach [<id>]`, `wtf --mcp`.

## Getting a trace from your simulator

wtf reads VCD and FST, detecting the format from the content (§13). FST holds the same data,
smaller on disk and faster to open: use it for anything large. The flags below are the
simulators' own; see their manuals if yours differs.

**Icarus Verilog.** In the testbench:

```verilog
initial begin
  $dumpfile("dump.vcd");
  $dumpvars(0, tb);      // 0: tb and everything below it
end
```

Then `iverilog -o sim tb.v dut.v && vvp sim`. For FST, run `vvp sim -fst` and name the file
`dump.fst`.

**Verilator.** Build with `--trace` (VCD) or `--trace-fst` (FST). In a C++ testbench:

```cpp
Verilated::traceEverOn(true);
auto* tfp = new VerilatedVcdC;     // VerilatedFstC with --trace-fst
top->trace(tfp, 99);
tfp->open("dump.vcd");
// at each time step: tfp->dump(main_time);
tfp->close();
```

**nvc** (VHDL): `nvc -a dut.vhd tb.vhd -e tb -r --wave=dump.fst`. The default format is FST;
`--format=vcd` writes VCD.

**GHDL** (VHDL): `ghdl -r tb --fst=dump.fst` or `--vcd=dump.vcd`. Plain `--wave` writes GHDL's own
GHW format, which wtf does not read.

**cocotb.** The simulator does the dumping, so use its option from the list above. With cocotb's
makefiles, `make WAVES=1` asks the simulator for a waveform where the makefile supports it; look
for the file in the simulation build directory, and set the simulator's own flags (for example
`--trace-fst` for Verilator) if you want FST.

## Look and feel

**Themes.** `dark` is the default, `light` is built in. Pick one in the config file
`~/.config/wtf/config.toml` (or `$XDG_CONFIG_HOME/wtf/config.toml`):

```toml
glyphs = "box"         # unicode (default), box, ascii
theme = "light"        # dark (default), light, or your own

[colors]               # change single colours of the theme
cursor = "#e5c07b"
red = "#ff5555"
```

Your own theme is a TOML file `themes/<name>.toml` next to `config.toml`, with the same keys as
`[colors]`; keys it leaves out are `dark`'s. The file is read once at start. A bad entry never
stops wtf: it is skipped and the status line says what was wrong (§19).

**Glyphs.** `--glyphs unicode|box|ascii` goes over the config file. `unicode` joins the outlines
of buses with Unicode 13 "legacy computing" glyphs; if your font shows boxes or question marks
where a bus value changes, use `box` (box drawing and blocks only, which nearly every font has).
`ascii` is for plain terminals and for pasting a snapshot into a mail or an issue. All three show
the same distinctions (§14).

**Colour.** With `NO_COLOR` set (to anything but empty) wtf draws without colour: the cursor
column and the selected row's name are reversed and marks keep their glyph. `NO_COLOR` wins over
the theme.

**Mouse.** Everything works from the keys, and the mouse does the common moves. Click a row to
select it, and on its waveform also to put the cursor there; click the time axis to put the
cursor there; the wheel moves the selection; `Ctrl` + wheel zooms; `Shift` + wheel scrolls the
time (§3).

## Working with an AI agent

`wtf --mcp` is an MCP server: an AI agent can open your trace, decode it and show you what it
found. Register it once, for example with Claude Code:

```sh
claude mcp add wtf -- wtf --mcp
```

Ask about the trace as you would ask a colleague: "why does the read at 175 ns fail?", "list all
writes to 0x10 and how long each waited", "where does `valid` drop before `ready`?" Name the file
(`docs/demo.vcd`).

What it does (§15):

1. **Opens its own workspace.** Its rows and cursors are invisible to you; your window is not
   touched.
2. **Decodes first.** It finds the interfaces by their signals and runs a decoder, then reads the
   records (`records` with `where` and `group_by`). Records are exact and carry the protocol
   checks; hand-written searches are its fallback.
3. **Presents.** It prepares rows, marks and a visible range, then `present`s them in your open
   window for the same trace. Your layout is replaced as one undo step; `:undo` gives you your
   own view back. Without an open window it writes a session file and tells you how to open it
   (`wtf -S <file> <trace>`).
4. **Never saves your session.** You `:w` if you want to keep what it showed.

`wtf --attach` lists the open sessions, yours and the agents'. `wtf --attach <id>` opens a window
on an agent's workspace and follows it, so you watch it work; `:follow off` stops following.
Inside the window, `:help records`, `:help decode` and the like show the reference the agent
reads.

The tools and their arguments are in [spec §15](../spec/15-mcp.md), the decoders in
[§16](../spec/16-decoders.md).
