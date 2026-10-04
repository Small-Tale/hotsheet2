import { mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { GIT_REPOSITORY_ENV_VARS as SCRIPT_GIT_REPOSITORY_ENV_VARS } from '../scripts/repository-env.mjs';
import { execFileAsync, GIT_REPOSITORY_ENV_VARS, spawn, spawnSync, withoutGitRepositoryEnv } from './child-process';
import { runCommand } from './dev-review/shell';
import { backupGit } from './hs1-backup';
import { runGitCommand } from './project-bridge';

const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0).reverse()) cleanup();
});
function tempDir(): string {
  const dir = mkdtempSync(join(tmpdir(), 'hotsheet-child-env-'));
  cleanups.push(() => {
    rmSync(dir, { recursive: true, force: true });
  });
  return dir;
}
/** Sets `process.env` entries for the rest of the test, restoring the previous values afterwards. */
function inheritEnv(values: Record<string, string>): void {
  const previous = Object.fromEntries(Object.keys(values).map((name) => [name, process.env[name]]));
  Object.assign(process.env, values);
  cleanups.push(() => {
    for (const [name, value] of Object.entries(previous)) {
      if (value === undefined) Reflect.deleteProperty(process.env, name);
      else process.env[name] = value;
    }
  });
}
// A child that reports which of the probed variables it received (the host may set other GIT_*
// variables, such as GIT_SSH_COMMAND, which are deliberately kept).
const REPORT_ENV = [
  '-e',
  "process.stdout.write(JSON.stringify(Object.fromEntries(Object.entries(process.env).filter(([k]) => ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_TERMINAL_PROMPT', 'HS_MARKER'].includes(k)))))",
];
function collect(child: ReturnType<typeof spawn>): Promise<Record<string, string>> {
  return new Promise((resolveOutput, reject) => {
    let output = '';
    child.stdout?.on('data', (chunk: Buffer) => (output += chunk.toString()));
    child.once('error', reject);
    child.once('close', () => {
      resolveOutput(JSON.parse(output) as Record<string, string>);
    });
  });
}
const HOSTILE = { GIT_DIR: '/elsewhere/.git', GIT_WORK_TREE: '/elsewhere', GIT_INDEX_FILE: '/elsewhere/index' };

describe('child-process git environment isolation (HS2-T1H6NP)', () => {
  it('removes every repository-locating variable without mutating the input', () => {
    const env = Object.fromEntries(GIT_REPOSITORY_ENV_VARS.map((name) => [name, '/elsewhere']));
    const clean = withoutGitRepositoryEnv({ ...env, GIT_AUTHOR_NAME: 'kept', GIT_TERMINAL_PROMPT: '0', HOME: '/h' });
    for (const name of GIT_REPOSITORY_ENV_VARS) expect(clean).not.toHaveProperty(name);
    expect(clean).toEqual({ GIT_AUTHOR_NAME: 'kept', GIT_TERMINAL_PROMPT: '0', HOME: '/h' });
    expect(env.GIT_DIR).toBe('/elsewhere');
  });

  it('keeps the production list in parity with the test-helper and Rust lists', () => {
    expect([...GIT_REPOSITORY_ENV_VARS]).toEqual([...SCRIPT_GIT_REPOSITORY_ENV_VARS]);
    const rust = readFileSync(new URL('../../../crates/hotsheet-ticketing/src/git.rs', import.meta.url), 'utf8');
    const block = /REPOSITORY_ENV_VARS: &\[&str\] = &\[([\s\S]*?)\];/.exec(rust)?.[1] ?? '';
    expect([...block.matchAll(/"(GIT_[A-Z_]+)"/g)].map((match) => match[1])).toEqual([...GIT_REPOSITORY_ENV_VARS]);
  });

  it('covers every variable the installed git clears for other repositories', async () => {
    const { stdout } = await execFileAsync('git', ['rev-parse', '--local-env-vars']);
    const missing = stdout
      .split('\n')
      .map((line) => line.trim())
      .filter((name) => name && !GIT_REPOSITORY_ENV_VARS.includes(name));
    expect(missing).toEqual([]);
  });

  it('strips inherited and explicitly passed repository variables in every spawn form', async () => {
    inheritEnv({ ...HOSTILE, HS_MARKER: 'inherited' });
    // Inherited process.env, with and without an options object.
    expect(await collect(spawn(process.execPath, REPORT_ENV))).toEqual({ HS_MARKER: 'inherited' });
    expect(await collect(spawn(process.execPath, REPORT_ENV, { stdio: ['ignore', 'pipe', 'inherit'] }))).toEqual({
      HS_MARKER: 'inherited',
    });
    // An explicit env keeps its own non-repository values but still loses the hostile ones.
    expect(
      await collect(
        spawn(process.execPath, REPORT_ENV, {
          env: { ...process.env, HS_MARKER: 'explicit', GIT_TERMINAL_PROMPT: '0' },
        }),
      ),
    ).toEqual({ HS_MARKER: 'explicit', GIT_TERMINAL_PROMPT: '0' });
    // The (command, options) form, with a shell resolving the args.
    expect(
      await collect(
        spawn(`"${process.execPath}" ${REPORT_ENV[0]} "${REPORT_ENV[1].replaceAll('"', '\\"')}"`, { shell: true }),
      ),
    ).toEqual({ HS_MARKER: 'inherited' });
    const sync = spawnSync(process.execPath, REPORT_ENV, { encoding: 'utf8' });
    expect(JSON.parse(String(sync.stdout))).toEqual({ HS_MARKER: 'inherited' });
    const { stdout } = await execFileAsync(process.execPath, REPORT_ENV, { env: { ...HOSTILE, HS_MARKER: 'exec' } });
    expect(JSON.parse(stdout)).toEqual({ HS_MARKER: 'exec' });
  });

  it('keeps a hook-style GIT_DIR from redirecting production git runners into the calling repository', async () => {
    const root = tempDir(),
      sentinel = join(root, 'sentinel'),
      store = join(root, 'store'),
      remote = join(root, 'remote.git'),
      identity = ['-c', 'user.name=Hot Sheet Test', '-c', 'user.email=test@example.invalid'];
    await execFileAsync('git', ['init', '-q', sentinel]);
    await execFileAsync('git', ['init', '-q', store]);
    writeFileSync(join(sentinel, 'tracked.txt'), 'sentinel\n');
    await execFileAsync('git', ['-C', sentinel, 'add', 'tracked.txt']);
    await execFileAsync('git', ['-C', sentinel, ...identity, 'commit', '-q', '-m', 'sentinel']);
    const snapshot = (dir: string): Record<string, string> => {
      const files: Record<string, string> = {};
      const walk = (path: string) => {
        for (const name of readdirSync(path)) {
          const child = join(path, name);
          if (statSync(child).isDirectory()) walk(child);
          else files[child.slice(dir.length)] = readFileSync(child).toString('base64');
        }
      };
      walk(dir);
      return files;
    };
    const before = snapshot(sentinel);
    // What a git hook or `git bisect run` in the sentinel checkout exports to its children.
    inheritEnv({
      GIT_DIR: join(sentinel, '.git'),
      GIT_WORK_TREE: sentinel,
      GIT_INDEX_FILE: join(sentinel, '.git', 'index'),
    });
    writeFileSync(join(store, 'ticket.md'), 'ticket\n');
    await backupGit(store, ['add', '-A']);
    await backupGit(store, [...identity, 'commit', '-q', '-m', 'store']);
    await runGitCommand('git', ['init', '-q', '--bare', remote]);
    await runGitCommand('git', ['-C', store, 'remote', 'add', 'origin', remote]);
    await runCommand('git', ['-C', store, 'push', '-q', 'origin', 'HEAD:refs/heads/main']);
    // The sentinel repository (config, index, refs, objects) is byte-for-byte unchanged.
    expect(snapshot(sentinel)).toEqual(before);
    // Each command acted on the repository it named instead.
    expect(await backupGit(store, ['log', '--format=%s'])).toBe('store');
    expect((await execFileAsync('git', ['--git-dir', remote, 'log', '--format=%s', 'main'])).stdout.trim()).toBe(
      'store',
    );
    expect(readFileSync(join(remote, 'config'), 'utf8')).toContain('bare = true');
  });
});
