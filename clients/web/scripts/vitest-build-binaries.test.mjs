import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it } from 'vitest';

import { BINARY_SUITES, NOT_BINARY_SUITES, launchesTestBinaries } from './vitest-binary-suites.mjs';
import {
  BUILD_INPUTS,
  TEST_BINARIES,
  binariesAreFresh,
  buildDecision,
  newestInputMtime,
} from './vitest-build-binaries.mjs';

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** An in-memory tree: path → mtime (files) or a nested object (directories). */
function fakeFs(tree) {
  const lookup = (path) => {
    let node = tree;
    for (const part of path.split('/').filter(Boolean)) {
      if (node === undefined || typeof node === 'number') return undefined;
      node = node[part];
    }
    return node;
  };
  return {
    statSync(path) {
      const node = lookup(path);
      if (node === undefined) throw Object.assign(new Error(`ENOENT ${path}`), { code: 'ENOENT' });
      return typeof node === 'number'
        ? { isDirectory: () => false, mtimeMs: node }
        : { isDirectory: () => true, mtimeMs: 0 };
    },
    readdirSync: (path) => Object.keys(lookup(path)),
  };
}

const binaries = (mtime) => Object.fromEntries(TEST_BINARIES.map((name) => [name, mtime]));

describe('fresh-binary detection (HS2-3PX1E4)', () => {
  const tree = (overrides = {}) => ({
    repo: {
      'Cargo.toml': 10,
      'Cargo.lock': 20,
      crates: { a: { src: { 'lib.rs': 50 }, target: { 'huge.o': 999 } } },
      plugins: { shared: { 'testing.md': 40 } },
      target: { debug: binaries(100) },
      ...overrides,
    },
  });

  it('finds the newest build input and ignores target and node_modules', () => {
    expect(newestInputMtime('/repo', BUILD_INPUTS, fakeFs(tree()))).toBe(50);
    expect(newestInputMtime('/repo', ['missing'], fakeFs(tree()))).toBe(Infinity);
  });

  it('skips the build only when every binary is newer than every input', () => {
    expect(binariesAreFresh('/repo', { fs: fakeFs(tree()) })).toBe(true);
    const edited = tree({ plugins: { shared: { 'testing.md': 150 } } });
    expect(binariesAreFresh('/repo', { fs: fakeFs(edited) })).toBe(false);
    const oneStale = tree({ target: { debug: { ...binaries(100), 'hotsheet-migrate': 30 } } });
    expect(binariesAreFresh('/repo', { fs: fakeFs(oneStale) })).toBe(false);
    const missing = tree({ target: { debug: { 'hotsheet-cli': 100 } } });
    expect(binariesAreFresh('/repo', { fs: fakeFs(missing) })).toBe(false);
  });

  it('honors a custom target directory', () => {
    const fs = fakeFs({ ...tree(), elsewhere: { debug: binaries(10) } });
    expect(binariesAreFresh('/repo', { targetDir: '/elsewhere', fs })).toBe(false);
  });

  it('lets HOTSHEET_VITEST_BUILD force or skip the build', () => {
    expect(buildDecision({}, () => true)).toBe('skip');
    expect(buildDecision({}, () => false)).toBe('build');
    expect(buildDecision({ HOTSHEET_VITEST_BUILD: 'always' }, () => true)).toBe('build');
    expect(buildDecision({ HOTSHEET_VITEST_BUILD: 'skip' }, () => false)).toBe('skip');
    expect(() => buildDecision({ HOTSHEET_VITEST_BUILD: 'sometimes' }, () => true)).toThrow(/auto, always, or skip/);
  });
});

describe('binary suite list (HS2-3PX1E4)', () => {
  const testFiles = (dir) =>
    readdirSync(join(webRoot, dir), { recursive: true })
      .filter((file) => /\.test\.(ts|mjs)$/.test(file))
      .map((file) => `${dir}/${file}`);

  it('lists every test file that launches a real binary', () => {
    const launching = [...testFiles('src'), ...testFiles('scripts')].filter(
      (file) => !(file in NOT_BINARY_SUITES) && launchesTestBinaries(readFileSync(join(webRoot, file), 'utf8')),
    );
    expect(launching.filter((file) => !BINARY_SUITES.includes(file))).toEqual([]);
  });

  it('names only files that exist', () => {
    for (const file of [...BINARY_SUITES, ...Object.keys(NOT_BINARY_SUITES)])
      expect(existsSync(join(webRoot, file)), file).toBe(true);
  });

  it('detects a launch only alongside a binary reference', () => {
    expect(launchesTestBinaries("spawn(resolve(root, 'target/debug/hotsheet-cli'))")).toBe(true);
    expect(launchesTestBinaries("expect(path).toBe('/repo/target/debug/hotsheet-cli')")).toBe(false);
    expect(launchesTestBinaries("spawn('ls')")).toBe(false);
  });
});

describe('Vitest runs the cargo build only for binary suites (HS2-3PX1E4)', () => {
  let scratch;
  afterEach(() => rmSync(scratch, { recursive: true, force: true }));

  // Runs the real Vitest CLI on this package's config with a recording fake `cargo` first on
  // PATH, forcing the build so freshness cannot hide whether the setup ran. `-t` matches no
  // test, so selected files are collected but their bodies (and any real binaries) never run.
  function vitestCargoCalls(file) {
    scratch = mkdtempSync(join(tmpdir(), 'hs2-vitest-build-'));
    const marker = join(scratch, 'cargo-calls');
    writeFileSync(join(scratch, 'cargo'), `#!/bin/sh\necho "$@" >> "${marker}"\n`);
    chmodSync(join(scratch, 'cargo'), 0o755);
    const result = spawnSync(
      process.execPath,
      [
        join(webRoot, 'node_modules/vitest/vitest.mjs'),
        'run',
        file,
        '-t',
        '^no test has this name$',
        '--passWithNoTests',
      ],
      {
        cwd: webRoot,
        encoding: 'utf8',
        env: { ...process.env, PATH: `${scratch}:${process.env.PATH}`, HOTSHEET_VITEST_BUILD: 'always' },
      },
    );
    expect(result.status, result.stdout + result.stderr).toBe(0);
    return existsSync(marker) ? readFileSync(marker, 'utf8') : '';
  }

  it('skips cargo when no selected file needs the binaries', () => {
    expect(vitestCargoCalls('scripts/remify-css.test.mjs')).toBe('');
  }, 60_000);

  it('builds once before a binary suite runs', () => {
    expect(vitestCargoCalls('scripts/real-ticket-server-owner.e2e.test.mjs')).toMatch(
      /^build --manifest-path .*Cargo\.toml -p hotsheet-cli -p hotsheet-server --bins\n$/,
    );
  }, 60_000);
});
