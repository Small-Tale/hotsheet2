import { readdir, readFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';

import { assertInitialAssetBudget, assertUxDemoIsLazy } from './production-bundle-policy.mjs';

// `node scripts/check-production-bundle.mjs [dist]`: the build output directory, `dist` by default.
const dist = resolve(process.argv[2] ?? 'dist');
const initialAssets = assertInitialAssetBudget(await readFile(resolve(dist, 'index.html'), 'utf8'));
console.log(
  `Production entry point uses ${initialAssets.length} assets / ${initialAssets.length + 1} requests including the document.`,
);

// The development latency monitor (HS2-ZGBQ5Q) must never ship in a production bundle.
const chunks = {};
for (const name of await readdir(resolve(dist, 'assets'))) {
  if (!name.endsWith('.js')) continue;
  const source = await readFile(resolve(dist, 'assets', name), 'utf8');
  if (source.includes('hotsheet dev latency') || source.includes('/__hotsheet/dev/latency'))
    throw new Error(`The development latency monitor leaked into the production bundle (${name}).`);
  const map = await readFile(resolve(dist, 'assets', `${name}.map`), 'utf8').catch(() => undefined);
  if (map !== undefined) chunks[name] = JSON.parse(map).sources ?? [];
}
console.log('Production bundle excludes the development latency monitor.');

// The `/ux-demo` catalog is an on-demand chunk that only a `--feature ux-demo` host serves (HS2-ZF70Y9).
const lazy = assertUxDemoIsLazy(
  initialAssets.filter((path) => path.endsWith('.js')).map((path) => basename(path)),
  chunks,
);
console.log(`Production startup scripts exclude the UX demo catalog; it loads on demand from ${lazy.join(', ')}.`);
