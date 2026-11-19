// Writes a synthetic standby snapshot for the screen saver hosting check (HS2-MZM988): two stores, one
// stalled, in the exact shape the server leaves for the saver. No customer content.
import { writeFileSync } from 'node:fs';

import { demoLiveState } from '../src/ux-demo/live-section-fixtures.ts';

const [, , out] = process.argv;
if (!out) throw new Error('usage: write-standby-saver-fixture <file>');
const now = Date.now();
const stores = [
  demoLiveState('attention'),
  { ...demoLiveState('stalled'), store: 'second-store', name: 'Second project' },
];
writeFileSync(
  out,
  JSON.stringify({ version: 1, written_at_ms: now, valid_until_ms: now + 60 * 60_000, live: { epoch_ms: 1, stores } }),
);
