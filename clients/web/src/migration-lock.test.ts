import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { acquireMigrationLock, migrationLockBinary } from './migration-lock';

describe('migrationLockBinary (HS2-KFKE8W)', () => {
  it('follows the checkout root instead of the running module location', () => {
    // The stable dev host serves a snapshot from the user cache directory and names the checkout.
    const snapshot = '/Users/someone/Library/Caches/hotsheet-web-stable/snapshot-abc';
    expect(migrationLockBinary({ HOTSHEET_REPO_ROOT: '/work/hotsheet2' }, snapshot)).toBe(
      '/work/hotsheet2/target/debug/hotsheet-migrate',
    );
    // A plain dev host runs from clients/web inside the checkout.
    expect(migrationLockBinary({}, '/work/hotsheet2/clients/web')).toBe(
      '/work/hotsheet2/target/debug/hotsheet-migrate',
    );
    // The production host pins its snapshot copy explicitly.
    expect(migrationLockBinary({ HOTSHEET_MIGRATE_BIN: '/tmp/snapshot/bin/hotsheet-migrate' }, snapshot)).toBe(
      '/tmp/snapshot/bin/hotsheet-migrate',
    );
    expect(migrationLockBinary({ HOTSHEET_MIGRATE_BIN: '', HOTSHEET_REPO_ROOT: '/work/hotsheet2' }, snapshot)).toBe(
      '/work/hotsheet2/target/debug/hotsheet-migrate',
    );
  });
});

describe('acquireMigrationLock preflight (HS2-KFKE8W)', () => {
  const previous = process.env.HOTSHEET_MIGRATE_BIN;
  afterEach(() => {
    if (previous === undefined) delete process.env.HOTSHEET_MIGRATE_BIN;
    else process.env.HOTSHEET_MIGRATE_BIN = previous;
  });

  it('explains a missing migrator instead of surfacing a raw spawn error', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-lock-missing-'));
    try {
      const missing = resolve(directory, 'hotsheet-migrate');
      process.env.HOTSHEET_MIGRATE_BIN = missing;
      await expect(acquireMigrationLock([resolve(directory, 'a')])).rejects.toThrow(
        `Hot Sheet migrator is not built at ${missing}. Run cargo build -p hotsheet-cli --bin hotsheet-migrate.`,
      );
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
});
