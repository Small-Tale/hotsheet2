import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { installedDependencies, verifyInstalledDependencies } from './installed-dependencies.mjs';

// HS2-PNBH4X: synthetic packages only; nothing here runs npm.
function packageFixture(t, { lock, installed, manifest } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'hs2-pnbh4x-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const put = (path, value) => {
    mkdirSync(join(dir, path, '..'), { recursive: true });
    writeFileSync(join(dir, path), `${JSON.stringify(value, null, 2)}\n`);
  };
  const deps = { kerf: '1.0.0' };
  put('package.json', manifest ?? { name: 'synthetic', dependencies: deps });
  put(
    'package-lock.json',
    lock ?? {
      lockfileVersion: 3,
      packages: {
        '': { name: 'synthetic', dependencies: deps },
        'node_modules/kerf': { version: '1.0.0' },
        'node_modules/@esbuild/win32-x64': { version: '0.1.0', optional: true },
      },
    },
  );
  if (installed !== null)
    put('node_modules/.package-lock.json', {
      lockfileVersion: 3,
      packages: installed ?? { 'node_modules/kerf': { version: '1.0.0' } },
    });
  return { dir, put };
}

test('HS2-PNBH4X an exact install passes and an absent optional platform package is allowed', (t) => {
  const { dir } = packageFixture(t);
  const state = installedDependencies(dir);
  assert.deepEqual(state.problems, []);
  assert.match(state.digest, /^[0-9a-f]{64}$/);
});

test('HS2-PNBH4X a lockfile advanced without npm ci is rejected with the installed and locked versions', (t) => {
  // The worktree-branch-update shape: the lockfile now wants 1.0.1, node_modules still has 1.0.0.
  const { dir } = packageFixture(t, {
    lock: {
      lockfileVersion: 3,
      packages: {
        '': { name: 'synthetic', dependencies: { kerf: '1.0.0' } },
        'node_modules/kerf': { version: '1.0.1' },
      },
    },
  });
  assert.deepEqual(installedDependencies(dir).problems, [
    'node_modules/kerf is installed at 1.0.0 but locked at 1.0.1',
  ]);
});

test('HS2-PNBH4X missing installs, missing packages, and extraneous packages are each named', (t) => {
  assert.deepEqual(installedDependencies(packageFixture(t, { installed: null }).dir).problems, [
    'dependencies are not installed (node_modules/.package-lock.json is missing)',
  ]);
  assert.deepEqual(installedDependencies(packageFixture(t, { installed: {} }).dir).problems, [
    'node_modules/kerf 1.0.0 is locked but not installed',
  ]);
  const extra = packageFixture(t, {
    installed: { 'node_modules/kerf': { version: '1.0.0' }, 'node_modules/left-pad': { version: '1.3.0' } },
  });
  assert.deepEqual(installedDependencies(extra.dir).problems, [
    'node_modules/left-pad is installed but not in package-lock.json',
  ]);
});

test('HS2-PNBH4X a manifest that drifted from its lockfile is rejected', (t) => {
  const { dir } = packageFixture(t, { manifest: { name: 'synthetic', dependencies: { kerf: '2.0.0' } } });
  assert.deepEqual(installedDependencies(dir).problems, ['package.json dependencies differs from package-lock.json']);
});

test('HS2-PNBH4X a fresh npm ci (new installed tree) changes the receipt digest', (t) => {
  const { dir, put } = packageFixture(t);
  const before = installedDependencies(dir).digest;
  put('node_modules/.package-lock.json', {
    lockfileVersion: 3,
    packages: { 'node_modules/kerf': { version: '1.0.0', resolved: 'https://registry.invalid/kerf-1.0.0.tgz' } },
  });
  const after = installedDependencies(dir);
  assert.deepEqual(after.problems, []);
  assert.notEqual(after.digest, before);
});

test('HS2-PNBH4X verification skips lockfile-free directories and names the exact fix for stale ones', (t) => {
  const good = packageFixture(t);
  const stale = packageFixture(t, { installed: {} });
  const root = join(good.dir, '..');
  const name = (dir) => dir.slice(root.length + 1);
  assert.deepEqual(Object.keys(verifyInstalledDependencies(root, [name(good.dir), '.'])), [name(good.dir)]);
  assert.throws(
    () => verifyInstalledDependencies(root, [name(good.dir), name(stale.dir)]),
    (error) =>
      error.message.includes(`${name(stale.dir)}: installed dependencies do not match`) &&
      error.message.includes(`Fix: (cd ${name(stale.dir)} && npm ci)`) &&
      !error.message.includes(`${name(good.dir)}:`),
  );
});

test('HS2-R0XY7S both rebuild scripts reject stale web dependencies before cargo or server stop', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'hs2-r0xy7s-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const web = join(root, 'clients', 'web');
  const scripts = join(root, 'scripts');
  const bin = join(root, 'bin');
  mkdirSync(join(web, 'node_modules'), { recursive: true });
  mkdirSync(scripts);
  mkdirSync(bin);
  mkdirSync(join(root, 'target', 'debug'), { recursive: true });
  mkdirSync(join(root, 'target', 'release'), { recursive: true });
  copyFileSync(new URL('./installed-dependencies.mjs', import.meta.url), join(scripts, 'installed-dependencies.mjs'));
  writeFileSync(join(root, 'package.json'), '{"type":"module"}\n');

  const actualScripts = JSON.parse(
    readFileSync(new URL('../clients/web/package.json', import.meta.url), 'utf8'),
  ).scripts;
  writeFileSync(join(web, 'package.json'), JSON.stringify({ dependencies: { kerf: '1.0.1' } }));
  writeFileSync(
    join(web, 'package-lock.json'),
    JSON.stringify({
      lockfileVersion: 3,
      packages: {
        '': { dependencies: { kerf: '1.0.1' } },
        'node_modules/kerf': { version: '1.0.1' },
      },
    }),
  );
  const hiddenLock = join(web, 'node_modules', '.package-lock.json');
  writeFileSync(hiddenLock, JSON.stringify({ packages: { 'node_modules/kerf': { version: '1.0.0' } } }));

  const marker = join(root, 'rebuild-actions');
  const executable = '#!/bin/sh\nprintf "%s\\n" "$1" >> "$HS2_REBUILD_SENTINEL"\n';
  for (const path of [
    join(bin, 'cargo'),
    join(root, 'target', 'debug', 'hotsheet-cli'),
    join(root, 'target', 'release', 'hotsheet-cli'),
  ]) {
    writeFileSync(path, executable);
    chmodSync(path, 0o755);
  }
  const run = (name) =>
    spawnSync('sh', ['-c', actualScripts[name]], {
      cwd: web,
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, HS2_REBUILD_SENTINEL: marker },
      encoding: 'utf8',
      timeout: 30_000,
    });

  for (const name of ['server:rebuild', 'server:rebuild:release']) {
    const result = run(name);
    assert.equal(result.status, 1, `${name}: ${result.stderr}`);
    assert.match(result.stderr, /kerf is installed at 1\.0\.0 but locked at 1\.0\.1/);
    assert.match(result.stderr, /Fix: \(cd \. && npm ci\)/);
    assert.equal(existsSync(marker), false, `${name} ran cargo or stopped the server before preflight`);
  }

  writeFileSync(hiddenLock, JSON.stringify({ packages: { 'node_modules/kerf': { version: '1.0.1' } } }));
  for (const name of ['server:rebuild', 'server:rebuild:release']) {
    const result = run(name);
    assert.equal(result.status, 0, `${name}: ${result.stderr}`);
  }
  assert.equal(
    readFileSync(marker, 'utf8')
      .split('\n')
      .filter((line) => line === 'serve').length,
    2,
  );
});
