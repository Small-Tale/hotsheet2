import { lstat, mkdir, mkdtemp, readFile, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { createStableSnapshot, removeStableSnapshot, stableDevEnvironment } from './stable-dev.mjs';

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

  it('preserves the original repository root for every snapshot-side bridge', () => {
    const environment = stableDevEnvironment('/work/hotsheet2/clients/web', {});
    expect(environment.HOTSHEET_REPO_ROOT).toBe('/work/hotsheet2');
    expect(environment.HOTSHEET_DEV_REVIEW_REPO_ROOT).toBe('/work/hotsheet2');

    const overridden = stableDevEnvironment('/snapshot/web', {
      HOTSHEET_REPO_ROOT: '/real/repository',
      HOTSHEET_DEV_REVIEW_REPO_ROOT: '/review/repository',
    });
    expect(overridden.HOTSHEET_REPO_ROOT).toBe('/real/repository');
    expect(overridden.HOTSHEET_DEV_REVIEW_REPO_ROOT).toBe('/review/repository');
  });
});
