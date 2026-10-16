import { readdir, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { assertInitialAssetBudget } from './production-bundle-policy.mjs';

const dist = resolve('dist');
const initialAssets = assertInitialAssetBudget(await readFile(resolve(dist, 'index.html'), 'utf8'));
console.log(
  `Production entry point uses ${initialAssets.length} assets / ${initialAssets.length + 1} requests including the document.`,
);

// The development latency monitor (HS2-ZGBQ5Q) must never ship in a production bundle.
for (const name of await readdir(resolve(dist, 'assets'))) {
  if (!name.endsWith('.js')) continue;
  const source = await readFile(resolve(dist, 'assets', name), 'utf8');
  if (source.includes('hotsheet dev latency') || source.includes('/__hotsheet/dev/latency'))
    throw new Error(`The development latency monitor leaked into the production bundle (${name}).`);
}
console.log('Production bundle excludes the development latency monitor.');
