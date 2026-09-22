import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export type MigrationLock = (paths: string[]) => Promise<() => Promise<void>>;

/** Native advisory locks release on process exit; lock files are never unlinked. */
export const acquireMigrationLock: MigrationLock = (paths) =>
  new Promise((resolveLock, reject) => {
    const binary =
      process.env.HOTSHEET_MIGRATE_BIN ??
      resolve(fileURLToPath(new URL('../../../', import.meta.url)), 'target/debug/hotsheet-migrate');
    const child = spawn(binary, ['--hold-job-lock', ...paths.sort()], { stdio: ['pipe', 'pipe', 'pipe'] });
    let output = '',
      diagnostic = '',
      acquired = false;
    const closed = new Promise<void>((resolveClosed) =>
      child.once('close', () => {
        resolveClosed();
      }),
    );
    child.stderr.on('data', (chunk: Buffer) => {
      diagnostic = (diagnostic + chunk.toString()).slice(-4096);
    });
    child.stdout.on('data', (chunk: Buffer) => {
      output += chunk.toString();
      if (!acquired && output.includes('locked\n')) {
        acquired = true;
        resolveLock(async () => {
          child.stdin.end();
          await closed;
        });
      }
    });
    child.once('error', reject);
    child.once('close', () => {
      if (!acquired) reject(new Error(diagnostic.trim() || 'Another bridge owns this migration repository.'));
    });
  });
