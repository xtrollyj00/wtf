# Changelog

What changed in each release. Commits follow [Conventional Commits](https://www.conventionalcommits.org/),
so `git log v0.1.0..` has the details.

## 0.1.0 (2026-10-28)

The first release.

- A Vim-like terminal viewer for VCD and FST: View and Edit modes, a searchable signal tree,
  groups, buses split into bits, radixes (hex, binary, decimal, signed, octal, ASCII, float,
  fixed point, FST enum names) and computed signals (`data[7:4]`, `{a, b}`).
- Search by value or expression, cycle by cycle with a clock (`/@clk fell(valid) && !past(ready)`).
- The cursor and eight marks, with the time between them on a ruler.
- Decoders for AXI-Stream, AXI4-Lite, AXI4, SPI and UART into records with latencies and protocol
  errors, and protocols of your own in a few lines.
- Sessions in a TOML file next to the trace; `:e` and a rerun of the simulation reload it.
- `wtf --mcp`: an MCP server through which an AI agent opens the trace, decodes it and shows its
  evidence in your window.
- Dark and light themes, the config file, `unicode`, `box` and `ascii` glyph sets chosen on the
  first start, `NO_COLOR`, the mouse.
- `--snapshot` renders a screen as text or, with `--ansi`, in colour, after `--keys`.
- Linux and macOS, x86_64 and arm64.
