# axis-wasm: the built-in AXIS as a component

The built-in AXI4-Stream decoder (`core/src/protocol/axis.rs`) written as a WebAssembly component,
so `wtf-bench protocol` can time the same decode twice: as Rust, and through wasmtime. It is a
transcription, not a second design: the same roles and defaults (`DEF`), the same eight rules
(`RULES`), and the same `beat`, `frame` and `errors` streams.

| part | what it does |
|---|---|
| `describe` | the card: the built-in's `about` (ending "from a component"), this crate's example, roles with their defaults, `reset_active` |
| `create` | `reset_active`, and the streams for the roles this trace has: `beat`'s fields are those of the roles given, `frame`'s gain `pos` with `tstrb` and `data` with `tdata` |
| `feed` | a cycle: earns violations, a `beat` record per handshake, a `frame` per packet a `tlast` closes |
| `finish` | the packets still open at the end of the trace, `complete` 0 |

The host passes the roles the trace has, in `create`'s order, and not the defaults: `value` fills
the roles the trace has no signal for from `ROLES`, as `play.rs` fills them for the Rust one.

## Build

```sh
cd wit/axis
cargo component build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wtf_axis.wasm ../../tests/data/wasm/axis-wasm.wasm
```

`wasm32-wasip2` is the default target of the installed toolchain and would import `wasi:io/poll`,
which wtf does not link: name `wasm32-unknown-unknown` every time. The artifact is committed as
`axis-wasm.wasm`, not `axis.wasm`: a protocol's name is its file's stem, and `axis` at a `decode`
line means the built-in.

Behaviour is what `wtf-bench protocol` compares, not bytes: after a change here, re-run it and
update `bench/RESULTS.md`.
