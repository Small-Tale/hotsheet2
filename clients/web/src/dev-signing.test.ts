import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { spawnSync } from './child-process';
import { signMacOSSnapshotIfConfigured } from './dev-signing';

const identity = '1234567890abcdef1234567890abcdef12345678';
const result = (status: number, stderr = '') => ({ status, stderr }) as ReturnType<typeof spawnSync>;

describe('macOS development signing (HS2-FYXRRM)', () => {
  it('does nothing outside macOS or without an explicit machine identity', () => {
    const calls: string[] = [];
    const run: typeof spawnSync = (command) => {
      calls.push(command);
      return result(0);
    };
    signMacOSSnapshotIfConfigured('/source', '/snapshot', 'hotsheet-server', '/absent', 'linux', run);
    signMacOSSnapshotIfConfigured('/source', '/snapshot', 'hotsheet-server', '/absent', 'darwin', run);
    expect(calls).toEqual([]);
  });

  it('signs only ad hoc snapshots with stable, separate server and CLI identifiers', () => {
    const home = mkdtempSync(resolve(tmpdir(), 'hs-dev-signing-'));
    try {
      writeFileSync(resolve(home, 'dev-signing-identity'), `${identity}\n`);
      const calls: string[][] = [];
      const run: typeof spawnSync = (_command, args) => {
        calls.push([...args]);
        return result(0, args[0] === '--display' ? 'Signature=adhoc\n' : '');
      };
      signMacOSSnapshotIfConfigured('/source/server', '/copy/server', 'hotsheet-server', home, 'darwin', run);
      signMacOSSnapshotIfConfigured('/source/cli', '/copy/cli', 'hotsheet-cli', home, 'darwin', run);
      expect(calls).toEqual([
        ['--display', '--verbose=2', '/source/server'],
        ['--force', '--sign', identity, '--identifier', 'com.smalltale.hotsheet2.dev.server', '/copy/server'],
        ['--verify', '--strict', '/copy/server'],
        ['--display', '--verbose=2', '/source/cli'],
        ['--force', '--sign', identity, '--identifier', 'com.smalltale.hotsheet2.dev.cli', '/copy/cli'],
        ['--verify', '--strict', '/copy/cli'],
      ]);
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });

  it('preserves distribution signatures and rejects bad config or failed signing', () => {
    const home = mkdtempSync(resolve(tmpdir(), 'hs-dev-signing-'));
    try {
      writeFileSync(resolve(home, 'dev-signing-identity'), 'bad\n');
      expect(() => {
        signMacOSSnapshotIfConfigured('/source', '/copy', 'hotsheet-server', home, 'darwin');
      }).toThrow('Invalid Hot Sheet development signing identity');
      writeFileSync(resolve(home, 'dev-signing-identity'), identity);
      const signedCalls: string[][] = [];
      const distribution: typeof spawnSync = (_command, args) => {
        signedCalls.push([...args]);
        return result(0, 'Signature=Developer ID Application\n');
      };
      signMacOSSnapshotIfConfigured('/release', '/copy', 'hotsheet-server', home, 'darwin', distribution);
      expect(signedCalls).toEqual([['--display', '--verbose=2', '/release']]);
      const failed: typeof spawnSync = (_command, args) => result(args[0] === '--force' ? 1 : 0, 'Signature=adhoc\n');
      expect(() => {
        signMacOSSnapshotIfConfigured('/debug', '/copy', 'hotsheet-server', home, 'darwin', failed);
      }).toThrow('Cannot sign hotsheet-server');
    } finally {
      rmSync(home, { recursive: true, force: true });
    }
  });
});
