import { lstat, mkdir, mkdtemp, readFile, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { createStableSnapshot, removeStableSnapshot } from './stable-dev.mjs';

const cleanup = [];
afterEach(async () => {
  await Promise.all(cleanup.splice(0).map(path => removeStableSnapshot(path)));
});

describe('stable dev snapshot', () => {
  it('copies source once, excludes generated output, and shares installed dependencies', async () => {
    const source = await mkdtemp(resolve(tmpdir(), 'hotsheet-web-source-'));
    cleanup.push(source);
    await mkdir(resolve(source, 'src'));
    await mkdir(resolve(source, 'node_modules'));
    await mkdir(resolve(source, 'dist'));
    await writeFile(resolve(source, 'src/main.ts'), 'before');
    await writeFile(resolve(source, 'dist/generated.js'), 'excluded');

    const snapshot = await createStableSnapshot(source);
    cleanup.push(snapshot);
    await writeFile(resolve(source, 'src/main.ts'), 'after');

    expect(await readFile(resolve(snapshot, 'src/main.ts'), 'utf8')).toBe('before');
    expect((await lstat(resolve(snapshot, 'node_modules'))).isSymbolicLink()).toBe(true);
    await expect(stat(resolve(snapshot, 'dist'))).rejects.toMatchObject({ code: 'ENOENT' });
  });
});
