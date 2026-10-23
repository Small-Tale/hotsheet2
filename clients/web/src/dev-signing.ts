/** Stable local code identity for Keychain access across macOS development rebuilds (HS2-FYXRRM). */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { spawnSync } from './child-process';

type Run = typeof spawnSync;

export function signMacOSSnapshotIfConfigured(
  source: string,
  snapshot: string,
  name: 'hotsheet-server' | 'hotsheet-cli',
  machineHome: string,
  platform: NodeJS.Platform = process.platform,
  run: Run = spawnSync,
): void {
  if (platform !== 'darwin') return;
  let identity: string;
  try {
    identity = readFileSync(resolve(machineHome, 'dev-signing-identity'), 'utf8').trim();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return;
    throw error;
  }
  if (!/^[0-9a-f]{40}$/i.test(identity)) throw new Error('Invalid Hot Sheet development signing identity');

  const inspect = run('codesign', ['--display', '--verbose=2', source], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (inspect.error || inspect.status !== 0) throw new Error(`Cannot inspect ${name} code signature`);
  // A release signed with a distribution identity must retain that identity.
  if (!String(inspect.stderr).includes('Signature=adhoc')) return;

  const identifier = `com.smalltale.hotsheet2.dev.${name === 'hotsheet-server' ? 'server' : 'cli'}`;
  const signed = run('codesign', ['--force', '--sign', identity, '--identifier', identifier, snapshot], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (signed.error || signed.status !== 0)
    throw new Error(`Cannot sign ${name} with the configured development identity`);
  const verified = run('codesign', ['--verify', '--strict', snapshot], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (verified.error || verified.status !== 0) throw new Error(`Cannot verify signed ${name} snapshot`);
}
