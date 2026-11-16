## 16.7 Protocols as WebAssembly components (wasm)

A protocol written in any language that compiles to a WebAssembly component, run in wasmtime. It is
a `Protocol` as the Rust built-ins are (section 16): the same roles, card, streams and violations,
so the core cannot tell one from `axis`, and everything built on records works with it unchanged.

### Where they come from

`protocols/<name>.wasm` in the config directory (section 19), read once at start. A window, a
snapshot and `--mcp` load the ones there, and decode with them in every workspace they open. The
protocol is named after the file — `probe.wasm` is `probe` — and `protocols` lists it under
`loaded from files:`, its card as a built-in's. A module that doesn't load (not a component, a card
`describe` refuses, no `describe` at all) is skipped and named on the status line, and the others
still decode: nothing in `protocols/` is a reason for wtf not to start.

### The contract

The component exports one interface, `wtf:protocol/api`, of the `protocol` world
(`wit/protocol.wit` in wtf's source). Its four calls are the trait's:

| call | when | answers |
| --- | --- | --- |
| `describe` | loading, with no trace | the `card`: what it is, its example line, whether it takes a clock, its sides, its roles each with what it is, its params |
| `create` | a `decode` | the streams it makes, from the roles with the widths the trace gave them and the params; an error says why it can't decode this instance |
| `feed` | a cycle of its clock | its records and its violations |
| `finish` | the trace's end | what is still open |

A `role` carries a default (`zero`, `ones`, `bytes`, `same`) for a trace with no signal for it, as
`axis` reads `tready` 1. A `stream-desc` says whether it holds violations, and a record's fields are
filled by name, so a violations stream declares `rule`, `detail` and `severity` — the three the host
writes too. Checking in `create` is the way to refuse what a component can't take: its error is the
`decode` error, in its words.

### Caps

A component is a decoder inside a window, so it can't be trusted with the window. One call may spend
1,000,000 fuel and one instance hold 32 MiB. A loop or an allocation past that, and a trap, are one
violation at the cycle being fed, under the rule `host`, with wasmtime's own words as its `detail`.
The instance is then dead — wasmtime can't enter it again — so the cycles after are silent rather
than one violation each, and `:e` makes a fresh one. An instance's death leaves the other decodes
alone: each has a store of its own.

### Writing one

`wit/example/` in wtf's source is the example, and the protocol `tests/data/wasm/probe.wasm` holds:
one beat a cycle, counting. Copy it, and:

```sh
cargo install cargo-component            # once
cd wit/example
cargo component build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wtf_probe.wasm <config dir>/protocols/
```

The target matters: `wasm32-wasip2` is `cargo-component`'s default and imports `wasi:io/poll`, which
wtf doesn't link, so the component fails to load. One that imports nothing loads against an empty
linker. `cargo test --bin wtf wasm` checks the host against the committed `probe.wasm`.
