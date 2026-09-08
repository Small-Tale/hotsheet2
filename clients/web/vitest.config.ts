import { mkdtempSync,rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from 'vitest/config';

const viteCacheDir=mkdtempSync(join(tmpdir(),'hotsheet-vitest-vite-'));
process.once('exit',()=>{rmSync(viteCacheDir,{recursive:true,force:true})});

export default defineConfig({
  cacheDir:viteCacheDir,
  test: { include: ['src/**/*.test.ts', 'scripts/**/*.test.mjs'] },
});
