import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from 'vitest/config';

const viteCacheDir = mkdtempSync(join(tmpdir(), 'hotsheet-vitest-vite-'));
process.once('exit', () => {
  rmSync(viteCacheDir, { recursive: true, force: true });
});

export default defineConfig({
  cacheDir: viteCacheDir,
  test: {
    include: ['src/**/*.test.ts', 'scripts/**/*.test.mjs'],
    // Builds the real CLI/server/migrator once before any worker runs (HS2-A0M8CM).
    globalSetup: ['./scripts/vitest-build-binaries.mjs'],
    setupFiles: ['./src/source-format-matchers.ts'],
  },
});
