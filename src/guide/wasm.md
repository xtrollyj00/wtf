A protocol in the config dir's `protocols/`, written as a WebAssembly component in any language.

Form
- `protocols/<name>.wasm` next to `config.toml`: the protocol is named after the file, and wtf
  reads them at start, so restart to pick one up.
- The component exports one interface, `wtf:protocol/api` (`wit/protocol.wit` in wtf's source):
  `describe` says what it is, `create` takes a decode's roles and params, `feed` a cycle of its
  clock, `finish` the trace's end.
- It decodes as a built-in does: `decode <name> <protocol> clock=<clk> <role>=<signal>...
  <param>=<value>...`, then `records` on its streams (`help decode`).
- `protocols` lists the loaded ones under `loaded from files:`; `protocols <name>` is its card.

Recipes
- Write one: copy `wit/example/` from wtf's source, `cargo install cargo-component` once, then
  `cargo component build --release --target wasm32-unknown-unknown` and copy the `.wasm` into
  `<config dir>/protocols/`.
- What a component may expect of a decode: `protocols <name>` shows its roles, each with what it
  is, and the params `decode` takes.
- A component that traps, or passes its caps, ends in a violation: `records` on its `errors`
  stream.

Details
- A role comes with the width the trace gives it (none for a text signal), and a default for a
  trace with no signal for it. `create` refusing a role or a param is the `decode` error, in the
  component's words.
- A trap and a cap hit are one violation each, on the stream that holds violations: its `rule` is
  `host` and its `detail` wasmtime's words. The instance is dead after it, so the cycles after
  are silent, and `:e` makes a fresh one.
- One call may spend 1,000,000 fuel and one instance hold 32 MiB: a loop or an allocation past
  that is that violation, not a hung window.
- A module that doesn't load is skipped and named on the status line; the rest still decode.
- The target matters: `wasm32-wasip2`, `cargo-component`'s default, imports `wasi:io/poll`, which
  wtf doesn't link, so the component fails to load.
