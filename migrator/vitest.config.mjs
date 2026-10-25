import { defineConfig } from 'vitest/config';

// Coverage for the migrator (docs/12 §12.7.5, HS2-8WR8XF). Its own per-language summary
// — NOT merged into the Rust lcov. Only the shipped export logic is gated; introspect.mjs
// is a manual DB-introspection helper run by hand, so it's excluded rather than dragging
// the number down with untested dev tooling.
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
      exclude: ['src/introspect.mjs'],
      // Conservative starting floors under the current numbers (export.mjs ~80% lines);
      // raise them as the exporter gains tests.
      thresholds: { lines: 70, functions: 70, branches: 60 },
    },
  },
});
