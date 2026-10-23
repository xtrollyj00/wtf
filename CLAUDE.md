# CLAUDE.md

Guidance for coding agents working in this repository.

wtf (Waveform Terminal Frontend) is a Vim-like terminal viewer for VCD / FST waveforms, with an MCP
server (`wtf --mcp`) so AI agents can debug traces and show their evidence in the user's window.

## Documents

- `spec/` is the design and the source of truth for behaviour, one file per section
  (`spec/README.md` is the index: §15 MCP server is `spec/15-mcp.md`). Change it first (or with
  the code) whenever behaviour changes; code comments cite it as `spec §N`. Read only the
  sections a change touches, not the whole spec.
- Spec §18 "Known limits" lists shortcuts taken; add one there when you take one.

## Commands

```sh
cargo build                          # both crates (default-members covers core, not bench)
cargo test                           # all tests, wtf and wtf-core
cargo test -p wtf-core               # core only
cargo test -q --test mcp <name>      # one MCP integration test (tests/mcp.rs)
cargo test -p wtf-core --test session round_trip   # one core integration test
cargo test <name>                    # unit / TUI tests in src/main.rs by name
cargo clippy -q --all-targets        # keep it clean
INSTA_UPDATE=always cargo test <name>  # accept changed insta snapshots (check the diff)
cargo run -- tests/data/simple.vcd --snapshot 100   # render one screen as text
cargo run -p wtf-bench --release -- gen --size 1G --format fst /tmp/b   # a huge trace (spec §18)
cargo build --release && cargo run -p wtf-bench --release -- run /tmp/b/bench.fst   # time wtf on it
cargo run -p wtf-bench --release -- mcp /tmp/b/bench.fst   # time the MCP tools on it
cargo build --release && cargo run -p wtf-bench --release -- agent run [--runs 3] [--review] [<task>...]
                                     # agent benchmark: claude -p solves bench/agent/tasks (costs money)
cargo run -p wtf-bench --release -- agent report           # results.jsonl per task, commit, model
```

`bench/` (`wtf-bench`) is not a default member: `cargo test` and `cargo clippy` skip it unless
given `-p wtf-bench`.

## Architecture

Two crates (spec §1 "Architecture"):

- **`core/` (`wtf-core`)** — no UI dependency. `Workspace` (`workspace.rs`) is the centre: a
  `Trace` plus rows, selection, `View` (zoom / cursor), marks, undo, named signals, protocols,
  decodes and streams; the decoding methods (`add_decode`, `bind`, `decode_*`) are in
  `workspace/decoding.rs`. `command::run` executes every `:` command; both interfaces go through it.
  `session.rs` reads / writes session TOML, which is also the wire format between processes.
- **`src/` (`wtf-wave`, the `wtf` command)** — interfaces: `tui/` (modes and keys in `app.rs`, signal panel, command
  completions in `commands.rs`), `mcp.rs` (rmcp tools: parse args, call the core, format dense
  bounded answers; their argument structs in `mcp/args.rs`, the `workspace` declaration in
  `mcp/declare.rs`), `draw.rs` (draws a workspace into a ratatui buffer, shared by the TUI, MCP
  `render` and `--snapshot`; glyph sets and the palette), `config.rs` / `theme.rs` (the config
  file and themes, spec §19; built-in themes are `themes/*.toml`, compiled in), `live.rs` (unix-socket link: windows and agent workspaces listen on
  `wtf-<id>.sock` in `$XDG_RUNTIME_DIR`; `GET` / `PUT` session TOML for `present`, `user_view`,
  `--attach`).

Ideas that span files:

- **Everything is a var.** Computed signals (slices `x[7:4]`, concatenations `{a,b}`, `:signal`
  names) are vars appended after the file's in `Trace`, materialized once into change lists.
  Decoded streams (`stream.rs`) are also vars: the stream itself (`rx.frame`, value `#n`) and one
  per field (`rx.frame.len`). So rows, search, `find`, `values` and `render` need no special cases.
- **Name resolution** is `Workspace::resolve`: roles of an instance being decoded, bit `x[3]`,
  the workspace's `scope`, names as shown in rows, full path, computed forms, then a unique path
  ending (ties broken by shown rows, scope, non-computed). `command::resolve` is the `:add` /
  `:rm` variant (paths, globs, scopes).
- **Search expressions** (`search.rs`) parse to `Expr`, evaluated by `search::holds` over a value
  function; `query.rs` finds matches (runs of clock cycles with `@clk`). `records` `where` reuses
  the same parser on a stream's fields.
- **Decoding**: stateless `Sampler`s (`[[protocol]]`) sample at a clock; Rust protocols
  (`protocol.rs`: `axis`, `axilite`, `axi4`, `spi`, `uart`) are lists of `Step`s, with params of
  their own; stateful `Decoder`s (`decode.rs`, registry `make`: `frame`, `spi`, `uart`; plus the
  functions `pair`, `join`, and `check`) stack on streams. SPI and UART sample by time: a `Changes`
  step feeds a signal's changes to their decoder, so they take no clock. Instances (`Decode`) are
  kept in the workspace and re-run on `:e`.
- **Undo** covers row layout only: `Workspace::undoable` / `mark` + `record` compare rows.
  Cursors, zoom and view are not undo steps and don't mark the session modified.
- **Times** are `Tick`s (u64) everywhere in the core, durations too (a `Field` with `time: true`,
  e.g. a pair's `delay`). The core never picks a display unit: `time::format_as(t, fpt, unit,
  decimals)` is the one formatter and the interfaces pass the unit (the TUI: what fits; MCP: the
  asked or workspace unit, to a thousandth). Session files store exact times.

## Tests

- Fixtures in `tests/data/` (`simple.vcd` / `.fst` are the same trace; `axis`, `axilite`, `axi4`,
  `spi`, `uart`, `handshake`, `reqresp`, `skew` for decoders and timing; `enum.fst` with an FST
  enum table, written by `enum.c`; `hdl.vcd` and `vhdl.fst`, dumped by nvc from `vhdl.vhd`, for
  names in either HDL's form). The fixture's contents are documented
  where tests use them; changing one means updating every test that reads it.
- TUI tests live in `src/main.rs` (`keys(&mut app, "jj'2")` drives keys, insta snapshots in
  `src/snapshots/`). MCP tests in `tests/mcp.rs` run the real binary over stdio, each with its
  own `XDG_RUNTIME_DIR` (`Dir::new`). Core tests in `core/tests/` use `common::{open, with}`.

## Conventions

- Changes reach `main` through pull requests, one topic each.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/):
  `<type>(<scope>): <summary>`, e.g. `feat(mcp): records group_by`, `fix(tui): keep the
  selection on undo`, `docs(spec): enum radix`. Types: `feat`, `fix`, `docs`, `test`,
  `refactor`, `perf`, `chore`, `ci`. Scopes: `core`, `tui`, `mcp`, `spec`, or none. A breaking
  change to the session format or a tool's arguments gets `!` and a `BREAKING CHANGE:` footer.
  The body is prose paragraphs on why.
- Prose (spec, comments, tool descriptions, error messages) is plain, short and concrete;
  errors say what to do next and list candidates. MCP tool descriptions double as agent docs, so
  keep them in step with the spec.
