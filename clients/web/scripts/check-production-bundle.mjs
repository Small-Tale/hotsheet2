import { readdir, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { assertInitialAssetBudget } from './production-bundle-policy.mjs';

const dist = resolve('dist');
const files = (await readdir(resolve(dist, 'assets'))).filter(file => /\.(?:js|css)$/.test(file));
const initialAssets = assertInitialAssetBudget(await readFile(resolve(dist, 'index.html'), 'utf8'));
const forbidden = ['hs-dev-review', 'Discard the captured feedback regions', 'html2canvas'];
for (const file of files) {
  const contents = await readFile(resolve(dist, 'assets', file), 'utf8');
  const signature = forbidden.find(value => contents.includes(value));
  if (signature) throw new Error(`Production asset ${file} contains dev-review signature: ${signature}`);
}
console.log(`Production bundle excludes Dev Review and html2canvas (${files.length} assets checked); entry point uses ${initialAssets.length} assets / ${initialAssets.length + 1} requests including the document.`);
