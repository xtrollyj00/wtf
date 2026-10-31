## 18. Limits and performance

### Outside wtf

What terminals, simulators, file formats and libraries don't give wtf. It shows what it is given
and doesn't guess the rest.

- **Terminals** decide which clicks reach wtf. The mouse protocol (SGR, mode 1006) carries Shift,
  Alt and Ctrl only, so `Cmd` on macOS can't be told from a plain click: Mac users press `Ctrl`.
  Most terminals keep `Shift` + click for their own text selection while an application has the
  mouse (xterm, VTE, kitty, Alacritty, WezTerm by default), so a `Shift` + click reaches wtf only
  where the terminal passes it on (kitty `terminal_select_modifiers`, WezTerm
  `bypass_mouse_reporting_modifiers`). `Ctrl` + click, `V` and `Space` do the same everywhere.
- **Composite types** are shown as the simulator dumps them (fixtures `types-*`, made by
  `tests/data/types.sh`; pinned in `core/tests/types.rs`):
  - a packed struct dumped as one var (Verilator without `--trace-structs`, Icarus) has no field
    names (`pkt.state`) and no enum names, and `x[1]` of a flat packed 2-D vector is bit 1, not
    element 1: the file has one var; a field is a slice the user names (`:signal pkt.state =
    pkt[9:8]`);
  - nvc dumps an enum as a string var (`pkt.state` is `busy`), its type not kept: it is a
    string to wtf (`signals` says so), compared as text, with no enum radix;
  - a VCD has no scope types: Verilator writes a struct as a module scope, as an FST doesn't, so
    it is no composite row (nor is a packed struct dumped as one var); its fields are shown by
    their leaf (`hi`, not `pkt.pair.hi`), listed scopes first, and adding it takes only its
    direct fields;
  - Icarus dumps the blocks of its `foreach` loops (`$ivl_foreach0.i`) and generate loops'
    genvars (`gen[0].g`, a parameter): they are in the file, so they are signals, added by
    `:add **` as any other;
  - GHDL's VCD has no records or arrays at all.
- **wellen** 0.25.7, which reads the files:
  - can't read a GHW file (GHDL) with a multidimensional array ("not yet implemented"): opening
    it is an error, though the panic's message still goes to stderr (the panic hook is
    process-wide, so it isn't swapped around the read);
  - misnames the elements of a Verilator FST's 2-D unpacked array whose indices are equal
    (`grid[1][1]` becomes `types.grid[1].grid[1].`).

### Performance

#### Size targets

wtf must stay comfortable with traces up to **10 GB** and hierarchies of
**hundreds of thousands of signals**.

| Action | Target |
|---|---|
| open an FST trace, first screen shown | under 1 s |
| open a VCD trace, first screen shown | under 2 s; values become available while the file is indexed in the background |
| add a signal, its waveform shown (FST) | under 1 s |
| any key, screen redrawn | under 50 ms, at any zoom |
| signal panel search, per typed character | under 50 ms with 500k signals |
| value search (`/`) on a signal with 10M transitions | under 1 s |
| memory after opening, before adding signals | proportional to the hierarchy, never to the file size |

#### Rules that follow from the targets

- **Lazy loading**: opening a file reads only the hierarchy. A signal's values are read when it is
  added, and memory grows with the shown signals, not with the file.
- **VCD indexing**: VCD has no index, so a background pass over the file builds one. The UI stays
  usable during the pass, shows its progress on the status line, and signals fill in as their data
  becomes available.
- **Drawing cost depends on the screen, not the data**: drawing a row looks up only the columns
  on screen (e.g. by binary search over transitions), so zooming out to the whole simulation is as
  fast as any other zoom.
- **Long operations never block**: anything that can take longer than the redraw target
  (indexing, loading a signal, value search) runs in the background and can be cancelled with `Esc`.
- `:e` (reload) follows the same rules as opening.

#### Measuring

The targets are checked against generated reference traces (a 10 GB FST, a 10 GB VCD, a hierarchy
with 500k signals), so results are comparable between versions and implementations.

`bench/` (`wtf-bench`) writes them and times wtf on them. `gen --size 10G --format fst|vcd <dir>`
writes `bench.fst` / `.vcd`: a 10 ns clock, a reset, `top.hot` (a 32-bit count of the cycles, a
transition every cycle) and `--modules` (200) AXI4-Lite slaves `top.u<i>` with random
transactions, run until the file has the size asked for; the same arguments write the same trace.
`--modules 26316 --size 100M` is the 500k-signal hierarchy. `run <trace>` times, against the
targets: opening in the core, adding `top.hot`, a value search for its last value, a clocked
search over the whole trace, the first screen and the whole trace by `wtf --snapshot`, and peak
memory (Linux only).

What was measured, and on what, is in [`bench/RESULTS.md`](../bench/RESULTS.md).
