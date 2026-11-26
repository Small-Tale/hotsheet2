import { defineConfig } from 'vitest/config';

// Coverage for the migrator (docs/12 §12.7.5, HS2-8WR8XF). Its own per-language summary
// — NOT merged into the Rust lcov. The manual introspect.mjs helper shares the exporter's
// database probe and has its own tests (HS2-Q77A53), so it is gated with the exporter.
export default defineConfig({
  test: {
    // HS2-T9VXEP: spawned HS2 binaries must never see the real machine home.
    globalSetup: ['./test/isolated-home.mjs'],
    // HS2-CZGY0M: real PGlite WASM startup and CLI migrations reached Vitest's 5s
    // default under root-gate load. Keep a finite bound without weakening assertions.
    testTimeout: 20_000,
    hookTimeout: 20_000,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      include: ['src/**/*.mjs'],
      // Conservative floors under the current numbers; raise them as the exporter gains tests.
      // HS2-XKYSVK: Vitest 4's AST-aware v8 remapping measures the same suite stricter than
      // Vitest 3 did (export.mjs 75.9% lines, 68.2% branches, 70.0% functions, was 80.7/77.5/79.2).
      thresholds: { lines: 70, functions: 70, branches: 60 },
    },
  },
});
