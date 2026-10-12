import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { assertInitialAssetBudget } from './production-bundle-policy.mjs';

const dist = resolve('dist');
const initialAssets = assertInitialAssetBudget(await readFile(resolve(dist, 'index.html'), 'utf8'));
console.log(
  `Production entry point uses ${initialAssets.length} assets / ${initialAssets.length + 1} requests including the document.`,
);
