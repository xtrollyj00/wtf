# probe: the example wasm protocol

A protocol written as a WebAssembly component (spec §16), the one `tests/data/wasm/probe.wasm`
holds and `src/wasm.rs` is tested against. It decodes one beat a cycle and is deliberately simple:
the point is to show what a component exports, not to decode a real bus.

| part | what it does |
|---|---|
| `describe` | the card: the roles, the params `spin` and `hog`, the streams `beat` and `errors` |
| `create` | one bit per role, else an error; `spin` loops and `hog` megabytes held, per cycle |
| `feed` | one `beat` record: `n` counting the cycles so far |
| `finish` | nothing |

`beat` is the decode; `errors` takes the host's own violations, so it declares the three fields they
are filled in by (`rule`, `detail`, `severity`). `spin=10000000` spends the fuel one call may and
`hog=6` holds more memory than one instance may: both make one `host` violation and kill the
instance, which is how the caps are tested.

## Build

```sh
cd wit/example
cargo component build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/wtf_probe.wasm ../../tests/data/wasm/probe.wasm
```

`wasm32-wasip2` is the default target of the installed toolchain and would import `wasi:io/poll`,
which wtf does not link: name `wasm32-unknown-unknown` every time. `cargo install cargo-component`
gets the subcommand.

The tests read the committed `.wasm`, so a rebuild that changes behaviour changes what they expect:
`cargo test --bin wtf wasm` in the repository root is what checks it. Behaviour is what is checked,
not bytes.
