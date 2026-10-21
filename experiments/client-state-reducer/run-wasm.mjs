import { readFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';

const wasmPath = new URL('./target/wasm32-unknown-unknown/release/client_state_reducer_spike.wasm', import.meta.url);
const fixturePath = new URL('../../docs/contracts/v1/client-state/history.json', import.meta.url);
const input = readFileSync(fixturePath);
const wasm = readFileSync(wasmPath);
const start = performance.now();
const { instance } = await WebAssembly.instantiate(wasm);
const instantiatedMs = performance.now() - start;
const pointer = instance.exports.fixture_alloc(input.length);
try {
  new Uint8Array(instance.exports.memory.buffer, pointer, input.length).set(input);
  const count = instance.exports.fixture_run(pointer, input.length);
  if (count !== 12) throw new Error(`fixture parity failed: ${count}`);
  process.stdout.write(`WebAssembly fixture parity: ${count} steps\n`);
  process.stdout.write(`Node cold instantiation: ${instantiatedMs.toFixed(1)} ms\n`);
} finally {
  instance.exports.fixture_free(pointer, input.length);
}
