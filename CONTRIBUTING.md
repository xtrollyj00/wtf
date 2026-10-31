# Contributing

All kinds of help are welcome: bug reports, ideas for the screen, benchmark tasks for agents,
speedups, docs. For anything bigger than a fix, open an issue first so we can agree on the
design before you spend time on it.

## Reporting a bug

Say what you did, what you expected and what you got, with:

- `wtf --version`, your OS, terminal and font;
- the screen as text: `wtf trace.vcd --snapshot 120x30 --keys '<the keys you pressed>'` prints
  what the window shows after those keys (Vim notation: `<CR>`, `<Esc>`, `<C-d>`). Add `--ansi`
  if the bug is in the colours;
- the trace, if you can share it. A trace you can't share can often be cut down to a few signals
  that show the bug.

## Ideas for the screen

Suggestions about how wtf looks and feels are as welcome as bug reports: what is hard to read,
what you reached for and didn't find, what another viewer does better. A `--snapshot --ansi`
of the screen as it is, and a sketch (text is fine) of how you'd want it, make the idea easy to
discuss.

## Changing the code

[`CLAUDE.md`](CLAUDE.md) has the commands, the architecture and the conventions. It is written
for coding agents and people alike, so a pull request made with an agent's help is fine.

- **The spec comes first.** [`spec/`](spec/README.md) is the source of truth for what wtf does.
  A change in behaviour changes the spec in the same pull request. It describes wtf as it is meant to work: a shortcut you
  take gets an issue labelled [`known-limit`](https://github.com/xtrollyj00/wtf/issues?q=is%3Aissue+label%3Aknown-limit),
  titled with the spec section it falls short of (`§8: ...`), and a `shortcut (#N):` comment in
  the code.
- **One topic per pull request**, with tests: TUI tests in `src/main.rs` (insta snapshots),
  MCP tests in `tests/mcp.rs`, terminal tests (the window in a pseudo-terminal, the mouse as
  escape sequences) in `tests/terminal.rs`, core tests in `core/tests/`. `cargo test` and
  `cargo clippy --all-targets` pass; CI runs both on Linux and macOS.
- **Commit messages** follow [Conventional Commits](https://www.conventionalcommits.org/):
  `feat(tui): ...`, `fix(core): ...`, `docs(spec): ...`. The body says why.
- **Prose** (spec, comments, errors, tool descriptions) is plain, short and concrete. An error
  says what to do next.

## Agent benchmarks

`wtf-bench agent` measures how much wtf helps an AI agent debug: it gives a headless agent
(`claude -p`) a task, grades its answer, and records time, tokens and cost, with wtf and without
it. The results so far are in [`bench/RESULTS.md`](bench/RESULTS.md). More tasks make the numbers
mean more, and tasks from designs we haven't covered are the most useful.

There are two kinds, both in `bench/agent/tasks/`:

- **A question about a trace**: `<name>.toml` with `trace`, `prompt`, `answer` (the JSON shape
  asked for) and `expect` (the right answer). See `axilite-errors.toml`.
- **A bug to fix**: a directory `<name>/` with `task.toml` (`upstream` and `commit` of an open
  source design, `[files]` to take from it, `prompt`, `answer`, `expect`), the `bug.patch` that
  breaks it, a `Makefile` whose `sim` target fails with the bug and writes the trace, and a
  `hidden` target with a test the agent never sees. See `eth-parser-stall/`.

For a new fix task:

- Take the design from upstream at a fixed commit; don't copy it into the repository. Name the
  project and its licence in the task's first comment.
- Make the bug one a real design could have, and the expected answer something only the trace
  shows for sure (a time, a count, a signal), so the benchmark measures reading the waveform.
- `cargo run -p wtf-bench --release -- agent check <task>` proves it without an agent: with the
  bug `make sim` and `make hidden` fail, without it they pass.
- Then `agent run --runs 5 <task>` and `agent run --runs 5 --no-mcp <task>`, and put
  `agent report`'s lines for it in the pull request. Runs call `claude -p` and cost money: about
  $0.01 to $0.06 a run so far with Haiku.

Results from your own runs are welcome too, on existing tasks or another model: open an issue
with the report.

## Performance

wtf should stay fast on the gigabyte dumps of real regressions; the targets are in
[spec §18 Performance](spec/18-limits.md#performance). To measure:

```sh
cargo run -p wtf-bench --release -- gen --size 1G --format fst /tmp/b   # a big trace
cargo build --release && cargo run -p wtf-bench --release -- run /tmp/b/bench.fst   # the window
cargo run -p wtf-bench --release -- mcp /tmp/b/bench.fst                # the agents' tools
```

A pull request that speeds something up shows these numbers before and after.

## Licence

Unless you say otherwise, what you contribute is dual licensed under MIT or Apache-2.0, as the
rest of wtf (see the [README](README.md#license)).
