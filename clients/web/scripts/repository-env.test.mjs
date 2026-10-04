import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { GIT_REPOSITORY_ENV_VARS, withoutGitRepositoryEnv } from './repository-env.mjs';

const cleanups = [];
afterEach(() => {
  for (const dir of cleanups.splice(0)) rmSync(dir, { recursive: true, force: true });
});
function tempDir() {
  const dir = mkdtempSync(join(tmpdir(), 'hotsheet-git-env-'));
  cleanups.push(dir);
  return dir;
}

describe('git repository environment stripping (HS2-RRD417)', () => {
  it('removes every repository-locating variable and keeps the rest', () => {
    const env = Object.fromEntries(GIT_REPOSITORY_ENV_VARS.map((name) => [name, '/elsewhere']));
    const clean = withoutGitRepositoryEnv({ ...env, GIT_AUTHOR_NAME: 'kept', HOTSHEET_HOME: '/home' });
    for (const name of GIT_REPOSITORY_ENV_VARS) expect(clean).not.toHaveProperty(name);
    expect(clean).toMatchObject({ GIT_AUTHOR_NAME: 'kept', HOTSHEET_HOME: '/home' });
    expect(env.GIT_DIR).toBe('/elsewhere');
  });

  it('matches the Rust shared constructor list exactly', () => {
    const rust = readFileSync(new URL('../../../crates/hotsheet-ticketing/src/git.rs', import.meta.url), 'utf8');
    const block = /REPOSITORY_ENV_VARS: &\[&str\] = &\[([\s\S]*?)\];/.exec(rust)?.[1] ?? '';
    const names = [...block.matchAll(/"(GIT_[A-Z_]+)"/g)].map((match) => match[1]);
    expect(names).toEqual([...GIT_REPOSITORY_ENV_VARS]);
  });

  it('covers every variable the installed git clears for other repositories', () => {
    const output = execFileSync('git', ['rev-parse', '--local-env-vars'], {
      env: withoutGitRepositoryEnv(process.env),
      encoding: 'utf8',
    });
    const missing = output
      .split('\n')
      .map((line) => line.trim())
      .filter((name) => name && !GIT_REPOSITORY_ENV_VARS.includes(name));
    expect(missing).toEqual([]);
  });

  it('keeps an inherited GIT_DIR from redirecting git init --bare into another repository', () => {
    const root = tempDir(),
      sentinel = join(root, 'sentinel'),
      remote = join(root, 'remote.git');
    execFileSync('git', ['init', '-q', sentinel], { env: withoutGitRepositoryEnv(process.env) });
    const configBefore = readFileSync(join(sentinel, '.git', 'config'), 'utf8');
    const hostile = {
      ...process.env,
      GIT_DIR: join(sentinel, '.git'),
      GIT_WORK_TREE: sentinel,
      GIT_INDEX_FILE: join(sentinel, '.git', 'index'),
    };
    execFileSync('git', ['init', '-q', '--bare', remote], { env: withoutGitRepositoryEnv(hostile) });
    expect(readFileSync(join(sentinel, '.git', 'config'), 'utf8')).toBe(configBefore);
    expect(readFileSync(join(remote, 'HEAD'), 'utf8')).toContain('ref:');
  });
});
