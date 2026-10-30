# wtf

[![ci](https://github.com/xtrollyj00/wtf/actions/workflows/ci.yml/badge.svg)](https://github.com/xtrollyj00/wtf/actions/workflows/ci.yml)

**W**aveform **T**erminal **F**rontend: a Vim-like terminal viewer for VCD and FST waveforms,
with an MCP server so AI agents can debug a trace with you. Also what you say when the bus goes
`X`.

![wtf in a terminal: signals added from the signal panel, an AXI4-Lite bus decoded into writes and reads, a search for the cycles where the address waits for ready, and a jump from the list of reads to the one answered with SLVERR, unfolded into its fields](docs/demo.gif)

Signals added from the tree, an AXI4-Lite bus decoded into its writes and reads with their
latency, a search for the cycles where the address waits for ready (mark 2), and a jump from the
list of reads to the one answered with SLVERR, unfolded into its fields. Try it:
`wtf docs/demo.vcd`.

## Features

- **Vim-like.** Time is the line axis: `h` / `l` move the cursor, `w` / `b` jump between
  transitions, `gg` / `G` go to the start / end, `:2000ns` goes to a time, `z` / `x` zoom.
  Viewing is the default; changing what is shown is a separate Edit mode (`i`). The mouse works
  too: click to place the cursor, Ctrl+wheel to zoom.
- **Signals.** A searchable signal tree, groups, buses expanded into bits, radixes (hex, binary,
  decimal, signed, octal, ASCII, float, fixed point, and enum names from FST files), computed
  signals (`data[7:4]`, `{a, b}`).
- **Search.** `/0x40` finds a value; `/valid && ready && data == 0x40` an expression;
  `/@clk fell(valid) && !past(ready)` a protocol violation, cycle by cycle.
- **Cursors.** The cursor and up to eight marks, with the time between them on a ruler.
- **Decoding.** AXI-Stream, AXI4-Lite, AXI4, SPI and UART into records (transfers, frames,
  transactions with their latency, protocol errors), and your own protocols defined in a few
  lines.
- **Sessions.** What you look at is saved in a TOML file next to the trace and comes back the
  next time. After you rerun the simulation, `:e` reloads it.
- **Display.** Dark and light themes or your own colours, X and Z told apart by shape as
  well as colour, `NO_COLOR`, and plain box drawing or ASCII for fonts and terminals that need
  it (`--glyphs`).
- **AI agents.** `wtf --mcp` lets an agent open your trace, find and decode what matters, and
  show its evidence in your window.

## Install

On Linux or macOS (x86_64 or arm64):

```sh
curl -fsSL https://raw.githubusercontent.com/xtrollyj00/wtf/main/install.sh | sh
```

This puts the `wtf` command in `~/.local/bin`, checked against the release's checksum. The
archives are on the [releases page](https://github.com/xtrollyj00/wtf/releases) too. Or build it
with a recent stable Rust (`rustup update`):

```sh
cargo install --git https://github.com/xtrollyj00/wtf wtf-wave
```

Windows is not supported.

## Use

```sh
wtf trace.fst              # or .vcd; the format is read from the content
wtf -S my.session.toml trace.fst
wtf --snapshot 120 trace.fst   # one screen as text, for CI logs and bug reports
wtf --snapshot 120x30 --ansi --keys '?' trace.fst   # in colour, after pressing keys
```

wtf shows what the file holds. A VCD keeps less than an FST: no enum names and no scope types,
so a struct or union is a plain scope.

The first start asks which glyphs your terminal draws well and whether you want dark or light
(`wtf --setup` asks again). Then the screen is empty: press `i` to add signals (`/` searches,
`Space` marks, `Enter` adds), `Esc` to go back to viewing, `?` for the keys of the mode you are
in, `:` for commands, `:w` to save the session, `q` to quit.

## With an AI agent

Register the MCP server once, for example with Claude Code:

```sh
claude mcp add wtf -- wtf --mcp
```

Then ask the agent about a trace. It opens it in a workspace of its own and answers with
evidence, not guesses. Asked for failed reads in `docs/demo.vcd`, an agent calls the `command`
tool with `decode bus axilite clock=aclk prefix=s_axi_`, then `records` on `bus.rd` with
`where: "resp != 0"`, and gets:

```
1 of 1 matching, of 2
start	end	delay	addr	data	resp
175ns	205ns	20ns	0x40	0x0	0x2
```

When it has found something, it prepares the rows and puts marks on the events, then `present`s
them in your open wtf window (`:undo` gives you your own view back). With no window open, it writes
a session file for you to open. `wtf --attach` lists what agents have open.

https://github.com/user-attachments/assets/7c244cad-1cd9-48b3-ba10-d42ba655e726

How much it helps is measured, not claimed: `wtf-bench agent` gives an agent failing RTL
testbenches to debug and fix, with wtf and without it (logs, prints and reruns only). Without it
the agent still fixed every bug, as these bugs show in the code; with it, it could more often
also say when and where things went wrong, in fewer simulation runs, and mostly sooner and for less. The numbers and how they were taken are in
[spec §18](spec/18-deferred.md).

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/agents-dark.svg">
  <img alt="Claude Haiku fixing four failing RTL testbenches, 5 runs each with and without wtf. Every run fixed its bug. Every answer right, with wtf / without: eth-parser-headers 5 / 2, eth-parser-stall 5 / 5, head-trimmer-boundary 5 / 3, axil2mi-stale-addr 2 / 1. Median time: 134 / 270 s, 76 / 136 s, 85 / 203 s, 62 / 57 s. Median cost: $0.029 / $0.058, $0.014 / $0.024, $0.012 / $0.024, $0.015 / $0.015." src="docs/agents.svg">
</picture>

## Before 1.0.0

wtf is young and changes fast. Until 1.0.0 nothing is kept backward compatible: session files,
the MCP tools' arguments, keys and commands change whenever a better design turns up, and the
old form stops working. A commit that breaks something says so (`feat(core)!: ...`, with a
`BREAKING CHANGE:` note on what to do instead).

## Documentation

Start with the [user guide](docs/guide.md): a 5-minute tour of `docs/demo.vcd`, a cheat sheet,
how it looks, and how to work with an agent.

The design is in [`spec/`](spec/README.md), one file per topic: the keys, commands, search,
sessions, the MCP server and the decoders. [Known limits](spec/18-deferred.md#known-limits)
lists the shortcuts taken so far. [`ROADMAP.md`](ROADMAP.md) says what comes next.

## Contributing

Bug reports, ideas for the screen, agent benchmark tasks and speedups are all welcome:
[`CONTRIBUTING.md`](CONTRIBUTING.md) says how.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
wtf by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
