+++
title = "Get started"
description = "Install wtf, then take a five-minute tour of it on a demo trace: decode the bus, find the failing read, add the signals around it, time it, save."
+++

Install wtf, then take a five-minute tour of it on a demo trace.

## Install

wtf runs on Linux and macOS, on x86_64 and arm64. Windows is not supported.

Install the latest release with the script:

```sh
curl -fsSL https://raw.githubusercontent.com/xtrollyj00/wtf/main/install.sh | sh
```

The script downloads the release archive for your machine, checks it against the published SHA-256
checksum, and copies `wtf` to `~/.local/bin`, or to `$WTF_INSTALL_DIR` if you set it. If that
directory is not on your `PATH`, the script says so. Add it in your shell profile:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Check the install with `wtf --version`.

To build from source instead:

```sh
cargo install --locked --git https://github.com/xtrollyj00/wtf wtf-wave
```

The package is `wtf-wave`, and the command it installs is `wtf`, in `~/.cargo/bin`.

## Get the demo

The tour uses `docs/demo.vcd`, an AXI4-Lite bus with a write, a read of it back, and a read of an
unmapped address that fails. The install does not ship it, so clone the repository:

```sh
git clone https://github.com/xtrollyj00/wtf
cd wtf
```

## Take the tour

The demo comes with a saved session, which would skip the first-start screen, so work on a copy:

```sh
cp docs/demo.vcd tour.vcd
wtf tour.vcd
```

The first time, wtf asks which drawing your terminal shows cleanly and whether you like dark or
light; [troubleshooting](@/troubleshooting.md) says more.

## Start from what wtf found

wtf opens to what to do first, each with its key. It has already looked through the trace for
interfaces a built-in protocol decodes: `1 found` is the demo's AXI4-Lite bus.

{{ <screen name="tour-start" /> }}

`Space` opens a menu of everything else, and every entry shows its key, so you learn them as you
go. Pause after any key and wtf shows what can follow.

## Decode the bus

`Space` `d` `d` lists the interfaces found, with the signal each role takes. A role is one of the protocol's ports, such as `valid`, `ready` or
`addr`:

{{ <screen name="tour-interfaces" caption="`Enter` decodes it, named `s_axi` after its prefix." /> }}

The decode turns the handshakes into transactions: writes and reads with their address, data,
response and delay, and the protocol's rules checked on every cycle. They come in as rows, in a
group: `s_axi.wr`, `s_axi.rd`, `s_axi.errors` for the broken rules, and the `unmatched` streams
(`s_axi.wr.unmatched`, `s_axi.rd.unmatched`) for responses with no request.

## Find the failing read

`:records s_axi` lists every write, read and error in one table, in time order. Type `/` and a
filter, `resp != 0`:

{{ <screen name="tour-records" caption="One read failed: `0x40`, answered with `0x2`, SLVERR." /> }}

`Enter` puts the cursor on it and selects its row. `Enter` on the row unfolds the record into its
fields, each a row of its own:

{{ <screen name="tour-jump" /> }}

`e` shows the whole value at the cursor in a box, and `Esc` closes it.

## Add the signals around it

`Space` `f` finds a signal by name: type a few letters in order (`s_axi_ar`), and the preview
shows the highlighted one's width, the decodes it is in and its value at the cursor. `Tab`
marks and moves on:

{{ <screen name="tour-find" caption="`Enter` adds the three marked, below the selected row. `u` takes it back." /> }}

`a` opens the signal panel instead, the trace as a tree of scopes, for when you'd rather browse.

## Move, search, time it

| Keys | Does |
|---|---|
| `h` `l` | the cursor one column left or right |
| `w` `b` | the selected signal's next or previous change |
| `gg` `G` | the start or the end |
| `:100ns` | go to a time |
| `z` `x` | zoom in or out around the cursor; `:fit` shows the whole trace |
| `j` `k` | select another row |

A count repeats a key: `5l`, `3w`.

Searching takes an expression, written as in SystemVerilog. `/0x40` finds a value on the selected
row; with a clock first, it matches whole clock cycles, sampled at its edges. Put mark 2 at the
read's address with `m`, then go to its data:

```
/@aclk s_axi_rvalid && s_axi_rready
```

{{ <screen name="tour-marks" caption="The ruler shows the time between the mark and the cursor: 20 ns." /> }}

`n` / `N` go to the next or previous match, `Tab` to the next mark, `'2` to mark 2, and `''` back
to where you were. The [reference](@/reference.md#expressions) has every operator: `rose`,
`fell` and `past` write protocol checks, as in the [recipes](@/recipes.md).

## Make it readable

`r` cycles the selected row's radix (hex, binary, decimal, ...), `:radix q4.12` reads it as fixed
point. `V` selects a range of rows and `Space` `v` `g` groups them; `Enter` folds a group, a
stream or a bus. `J` and `K` move rows, `dd` removes one, and `u` undoes any of it.

The mouse does the common moves: a click selects a row or puts the cursor, `Ctrl` + wheel zooms,
`Shift` + wheel scrolls in time, a drag across the waveforms zooms to that range, and a row
dragged by its name moves. A right click opens a menu of what can be done there, each with its
key.

## Save, rerun, reload

`:w` writes `tour.vcd.session.toml` next to the trace, and the next `wtf tour.vcd` opens as you left
it. [How it works](@/how-it-works.md) has the full account of sessions and reloading.

## Your own trace

Give wtf a path to a VCD, FST or GHW file:

```sh
wtf trace.fst
```

Run `wtf` with no path to see the start screen. It lists the traces you opened recently: press
the number beside one to open it, or `o` to type a path.

`Space` `d` `d` lists the interfaces wtf found in it. If the list is empty, the signal names did
not match a built-in protocol. `Space` `d` `D`, or a `:decode` line, names the signal for every role:

```
:decode bus axilite clock=aclk awvalid=m_awv awready=m_awr awaddr=m_awa wvalid=m_wv wready=m_wr wdata=m_wd bvalid=m_bv bready=m_br bresp=m_bre arvalid=m_arv arready=m_arr araddr=m_ara rvalid=m_rv rready=m_rr rdata=m_rd rresp=m_rre
```

When the names share a prefix, `prefix=m_` plus the role names is enough. Each protocol needs these
roles (`:help decode` lists the optional ones):

| protocol | roles |
|---|---|
| `axis` | `tvalid` `tready` `tdata` |
| `axilite` | `awvalid` `awready` `awaddr` `wvalid` `wready` `wdata` `bvalid` `bready` `bresp` `arvalid` `arready` `araddr` `rvalid` `rready` `rdata` `rresp` |
| `axi4` | the `axilite` roles, plus `wlast` and `rlast` |
| `spi` | `sclk` `cs` `mosi` |
| `uart` | `line` |

Next: [How it works](@/how-it-works.md) for the model, [Recipes](@/recipes.md) for errors,
latency and your own handshakes. Stuck on a trace? [Give it to an agent](@/agents.md): it decodes
and searches the same way, and shows you what it found.
