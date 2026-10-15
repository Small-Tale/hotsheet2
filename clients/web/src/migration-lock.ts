import { existsSync } from 'node:fs';
import { resolve } from 'node:path';

import { spawn } from './child-process';
import { developmentRepositoryRoot } from './repository-root';

export type MigrationLock = (paths: string[]) => Promise<() => Promise<void>>;

/**
 * The migrator that holds a migration job's native lock. An explicit `HOTSHEET_MIGRATE_BIN` wins
 * (the production host pins its snapshot copy); otherwise it is the checkout's debug build. The
 * checkout comes from `HOTSHEET_REPO_ROOT` or the working directory, never from this module's own
 * location: the stable dev host serves a snapshot under the user cache directory, where a
 * module-relative path resolved to `…/Library/Caches/target/debug/hotsheet-migrate` and every HS1
 * import failed with a spawn ENOENT (HS2-KFKE8W).
 */
export function migrationLockBinary(
  environment: Readonly<Record<string, string | undefined>> = process.env,
  cwd = process.cwd(),
): string {
  return (
    environment.HOTSHEET_MIGRATE_BIN ||
    resolve(developmentRepositoryRoot(cwd, environment), 'target/debug/hotsheet-migrate')
  );
}

/** Native advisory locks release on process exit; lock files are never unlinked. */
export const acquireMigrationLock: MigrationLock = (paths) =>
  new Promise((resolveLock, reject) => {
    const binary = migrationLockBinary();
    if (!existsSync(binary)) {
      reject(
        new Error(
          `Hot Sheet migrator is not built at ${binary}. Run cargo build -p hotsheet-cli --bin hotsheet-migrate.`,
        ),
      );
      return;
    }
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
