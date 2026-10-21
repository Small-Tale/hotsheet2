# Client-state reducer spike (HS2-EJ7NWH)

This is an isolated API-consumer experiment, not shipped client code. It ports
only the optimistic history fixture's transitions. It has no dependency on
`hotsheet-core`, the server, or filesystem ticket writes. The `fixture_*` C ABI
is a deliberately small example of the memory bridge a browser would need.

From the repository root, with the `wasm32-unknown-unknown` Rust target installed:

```sh
cargo test --manifest-path experiments/client-state-reducer/Cargo.toml --offline
cargo build --manifest-path experiments/client-state-reducer/Cargo.toml --target wasm32-unknown-unknown --release --offline
cd experiments/client-state-reducer
zsh -ic 'node run-wasm.mjs'
```

The runner reads `docs/contracts/v1/client-state/history.json` and fails unless
all 12 steps match. Its `Node cold instantiation` timer includes Wasm compilation
and instantiation in Node, but excludes file I/O, a browser fetch, UI wiring,
network transitions, and native bindings. A negative expectation test confirms
the native runner detects a changed oracle.

This prototype deliberately does not implement the browser's external undo
closures, multi-ticket transactions, rejected network writes, all 30-entry
boundary cases, or production async orchestration. Passing this one fixture
therefore shows portability, not production equivalence.
