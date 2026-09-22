import { access, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { expect, it } from 'vitest';

import { acquireMigrationLock } from './migration-lock';
import { runMigrationProcess } from './migration-stream';

it('does not launch work until child ownership has been durably acknowledged', async () => {
  const directory = await mkdtemp(resolve(tmpdir(), 'migration-gate-'));
  const marker = resolve(directory, 'launched');
  const ownership: number[] = [];
  try {
    await expect(
      runMigrationProcess(
        process.execPath,
        ['-e', `require('node:fs').writeFileSync(${JSON.stringify(marker)},'bad')`],
        directory,
        () => {},
        false,
        async (pid) => {
          ownership.push(pid);
          if (pid) throw new Error('checkpoint refused');
        },
      ),
    ).rejects.toThrow('checkpoint refused');
    await expect(access(marker)).rejects.toThrow();
    expect(ownership[0]).toBeGreaterThan(0);
    expect(ownership.at(-1)).toBe(0);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

it.skipIf(process.platform === 'win32')(
  'kills the owned descendant group when a progress record is malformed',
  async () => {
    let descendant = 0;
    const script = `const {spawn}=require('node:child_process');const child=spawn(process.execPath,['-e',"process.on('SIGTERM',()=>{}); console.log('ready'); setInterval(()=>{},1000)"],{stdio:['ignore','pipe','inherit']});child.stdout.once('data',()=>console.log(child.pid));setInterval(()=>{},1000)`;
    await expect(
      runMigrationProcess(process.execPath, ['-e', script], tmpdir(), (line) => {
        descendant = Number(line);
        throw new Error('Malformed migration record');
      }),
    ).rejects.toThrow('Malformed migration record');
    expect(descendant).toBeGreaterThan(0);
    await expect
      .poll(() => {
        try {
          process.kill(descendant, 0);
          return true;
        } catch {
          return false;
        }
      })
      .toBe(false);
  },
);

it.skipIf(!process.env.HOTSHEET_MIGRATE_BIN)(
  'uses native cross-owner locks and releases ownership when stdin closes',
  async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-native-lock-'));
    const paths = [resolve(directory, 'a'), resolve(directory, 'b')];
    let release: (() => Promise<void>) | undefined;
    try {
      release = await acquireMigrationLock(paths);
      await expect(acquireMigrationLock([...paths].reverse())).rejects.toThrow();
      await release();
      release = await acquireMigrationLock(paths);
    } finally {
      await release?.();
      await rm(directory, { recursive: true, force: true });
    }
  },
);
