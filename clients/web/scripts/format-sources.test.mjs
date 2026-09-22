import { spawnSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it } from 'vitest';

import { formattingTargets } from './format-sources.mjs';

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const cleanup = [];
afterEach(async () => {
  await Promise.all(cleanup.splice(0).map((path) => rm(path, { recursive: true, force: true })));
});

describe('repository formatting scope (HS2-G9K0NY)', () => {
  it('skips only an absent local config and preserves mandatory paths', () => {
    const absent = formattingTargets('/repo/clients/web', () => {
      throw Object.assign(new Error('missing'), { code: 'ENOENT' });
    });
    expect(absent).toEqual(['.', '../../migrator', '../../spikes/kerf-webawesome', '../../docs', '../../.github']);
    expect(formattingTargets('/repo/clients/web', () => ({}))).toEqual([...absent, '../../opencode.json']);
    const denied = Object.assign(new Error('denied'), { code: 'EACCES' });
    expect(() =>
      formattingTargets('/repo/clients/web', () => {
        throw denied;
      }),
    ).toThrow(denied);
  });

  it('runs real package scripts through clean checkout, local config, drift, and missing tracked paths', async () => {
    const root = await mkdtemp(join(tmpdir(), 'hotsheet-format-checkout-'));
    cleanup.push(root);
    const web = join(root, 'clients/web');
    for (const directory of ['clients/web/scripts', 'migrator', 'spikes/kerf-webawesome', 'docs', '.github', 'src'])
      await mkdir(join(root, directory), { recursive: true });
    const { scripts } = JSON.parse(await readFile(join(webRoot, 'package.json'), 'utf8'));
    await writeFile(
      join(web, 'package.json'),
      JSON.stringify({ scripts: { format: scripts.format, 'format:check': scripts['format:check'] } }, null, 2) + '\n',
    );
    await cp(join(webRoot, 'scripts/format-sources.mjs'), join(web, 'scripts/format-sources.mjs'));
    await symlink(join(webRoot, 'node_modules'), join(web, 'node_modules'), 'dir');
    await cp(resolve(webRoot, '../../.prettierrc.json'), join(root, '.prettierrc.json'));
    await writeFile(join(root, '.prettierignore'), '**/node_modules\n');
    for (const directory of ['migrator', 'spikes/kerf-webawesome', 'docs', '.github'])
      await writeFile(join(root, directory, 'fixture.json'), '{}\n');
    await writeFile(
      join(root, 'Cargo.toml'),
      '[package]\nname = "format-fixture"\nversion = "0.1.0"\nedition = "2021"\n',
    );
    await writeFile(join(root, 'src/lib.rs'), 'pub fn formatted() {}\n');
    const run = (script) => {
      const result = spawnSync('npm', ['run', script], { cwd: web, encoding: 'utf8' });
      if (result.error) throw result.error;
      return { status: result.status, output: result.stdout + result.stderr };
    };
    // Normalize the generated manifest, then verify the untouched clean-checkout gate.
    expect(run('format').status).toBe(0);
    expect(run('format:check').status).toBe(0);
    await writeFile(join(root, 'opencode.json'), '{"local":true}');
    expect(run('format:check')).toMatchObject({ status: 1, output: expect.stringContaining('opencode.json') });
    expect(run('format').status).toBe(0);
    expect(run('format:check').status).toBe(0);
    await writeFile(join(root, 'opencode.json'), '{ invalid JSON');
    expect(run('format:check').status).not.toBe(0);
    await rm(join(root, 'opencode.json'));
    await writeFile(join(root, 'docs/fixture.json'), '{"tracked":true}');
    expect(run('format:check')).toMatchObject({ status: 1, output: expect.stringContaining('docs/fixture.json') });
    expect(run('format').status).toBe(0);
    await rm(join(root, 'docs'), { recursive: true });
    expect(run('format:check')).toMatchObject({ status: 2, output: expect.stringContaining('No files matching') });
    await mkdir(join(root, 'docs'));
    await writeFile(join(root, 'docs/fixture.json'), '{}\n');
    await writeFile(join(root, 'src/lib.rs'), 'pub fn formatted( ){}\n');
    expect(run('format:check').status).not.toBe(0);
    expect(run('format').status).toBe(0);
    expect(run('format:check').status).toBe(0);
  }, 30000);
});
