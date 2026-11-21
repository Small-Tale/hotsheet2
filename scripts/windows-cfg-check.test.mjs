import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';

import { cargoArgs, checkEnv, LINT_FLAGS, main, STUBS, TARGET, TARGET_DIR, writeStubs } from './windows-cfg-check.mjs';

const posixOnly = { skip: process.platform === 'win32' ? 'the stubs are POSIX shell scripts' : false };

function temp(t) {
  const dir = mkdtempSync(join(tmpdir(), 'windows-cfg-check-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}

test('HS2-V3R4JR runs the cargo lint deny set for the Windows GNU target', () => {
  const alias = /^lint = "(.+)"$/m.exec(readFileSync(new URL('../.cargo/config.toml', import.meta.url), 'utf8'));
  assert.ok(alias, 'the cargo lint alias exists');
  const lint = alias[1].split(' -- ')[1].split(' ');
  assert.deepEqual(LINT_FLAGS, lint, 'the Windows cfg check denies exactly what `cargo lint` denies');
  const args = cargoArgs(TARGET_DIR, '/stubs');
  assert.deepEqual(args.slice(0, 8), [
    'clippy',
    '--workspace',
    '--all-targets',
    '--all-features',
    '--target',
    'x86_64-pc-windows-gnu',
    '--target-dir',
    'target/windows-cfg-check',
  ]);
  assert.ok(args.includes('target.x86_64-pc-windows-gnu.dav1d.include="/stubs/include"'));
  assert.deepEqual(args.slice(args.indexOf('--') + 1), LINT_FLAGS);
});

test('HS2-V3R4JR points only the Windows target at the stubs and drops an sccache wrapper', () => {
  const base = { PATH: '/usr/bin', RUSTC_WRAPPER: 'sccache', CARGO_BUILD_RUSTC_WRAPPER: 'sccache', HOME: '/h' };
  const env = checkEnv(base, '/s');
  assert.equal(env.CC_x86_64_pc_windows_gnu, '/s/tools/cc');
  assert.equal(env.CXX_x86_64_pc_windows_gnu, '/s/tools/cc');
  assert.equal(env.AR_x86_64_pc_windows_gnu, '/s/tools/ar');
  assert.equal(env.CMAKE, '/s/tools/cmake');
  assert.equal(env.SYSTEM_DEPS_DAV1D_NO_PKG_CONFIG, '1');
  assert.equal(env.SYSTEM_DEPS_DAV1D_LIB, 'dav1d');
  assert.equal(env.RUSTC_WRAPPER, undefined);
  assert.equal(env.CARGO_BUILD_RUSTC_WRAPPER, undefined);
  // Host builds keep the real cc and ar: only nasm joins PATH, which no host build uses.
  assert.equal(env.PATH, ['/s/path', '/usr/bin'].join(delimiter));
  assert.equal(env.CC, undefined);
  assert.equal(env.HOME, '/h');
  assert.equal(base.RUSTC_WRAPPER, 'sccache', 'the caller environment is not changed');
  assert.deepEqual(
    Object.keys(STUBS).filter((name) => name.startsWith('path/')),
    ['path/nasm'],
  );
});

test('HS2-V3R4JR stubs answer the probes and write the outputs build scripts expect', posixOnly, (t) => {
  const dir = temp(t);
  writeStubs(dir);
  const sh = (name, args) => spawnSync(join(dir, name), args, { cwd: dir, encoding: 'utf8' });
  assert.equal(sh('tools/cc', ['--version']).status, 0);
  assert.match(sh('tools/cc', ['-E', 'detect.c']).stdout, /#pragma message "gcc"/);
  assert.equal(sh('tools/cc', ['-O0', '-o', 'a.o', '-c', 'a.c']).status, 0);
  assert.ok(existsSync(join(dir, 'a.o')));
  sh('tools/cc', ['-Fob.o', '-c', 'b.c']);
  assert.ok(existsSync(join(dir, 'b.o')), 'MSVC-style -Fo output');
  sh('tools/ar', ['cq', 'libx.a', 'a.o']);
  assert.ok(existsSync(join(dir, 'libx.a')));
  assert.match(sh('path/nasm', ['-v']).stdout, /^NASM version 2\.(1[5-9]|[2-9]\d)/);
  sh('path/nasm', ['-f', 'win64', '-o', 'x.o', 'x.asm']);
  assert.ok(existsSync(join(dir, 'x.o')));
  assert.match(sh('tools/cmake', ['--version']).stdout, /^cmake version 3\./);
  assert.equal(sh('tools/cmake', ['--build', '.', '--target', 'install']).status, 0);
  // A rerun leaves unchanged stubs alone, so their mtimes stay stable.
  const before = statSync(join(dir, 'tools/cc')).mtimeMs;
  writeStubs(dir);
  assert.equal(statSync(join(dir, 'tools/cc')).mtimeMs, before);
});

test('HS2-V3R4JR adds a missing target, runs the stubbed clippy and reports its failure', (t) => {
  const repo = temp(t);
  const calls = [];
  const logs = [];
  const outcome = { cargo: 101, installed: 'aarch64-apple-darwin\n' };
  const run = (command, args, options) => {
    calls.push({ command, args, options });
    if (command === 'rustup' && args[1] === 'list') return { status: 0, stdout: outcome.installed };
    if (command === 'rustup') return { status: 0 };
    return { status: outcome.cargo };
  };
  const options = { repo, platform: 'linux', env: { PATH: '/usr/bin', RUSTC_WRAPPER: 'sccache' }, run, log: (line) => logs.push(line) };
  assert.equal(main([], options), 101);
  assert.deepEqual(
    calls.map((call) => [call.command, ...call.args.slice(0, 2)]),
    [
      ['rustup', 'target', 'list'],
      ['rustup', 'target', 'add'],
      ['cargo', 'clippy', '--workspace'],
    ],
  );
  const cargo = calls.at(-1);
  assert.equal(cargo.options.cwd, repo);
  assert.equal(cargo.options.env.RUSTC_WRAPPER, undefined);
  assert.equal(cargo.options.env.CC_x86_64_pc_windows_gnu, join(repo, TARGET_DIR, 'stubs/tools/cc'));
  if (process.platform !== 'win32') assert.ok(existsSync(join(repo, TARGET_DIR, 'stubs/path/nasm')));
  assert.ok(logs.some((line) => line.includes('the Windows cfg check failed')));

  // An installed target is not added again, and a pass prints no failure hint.
  calls.length = 0;
  logs.length = 0;
  outcome.installed = `aarch64-apple-darwin\n${TARGET}\n`;
  outcome.cargo = 0;
  assert.equal(main([], options), 0);
  assert.deepEqual(
    calls.map((call) => call.command),
    ['rustup', 'cargo'],
  );
  assert.deepEqual(logs, []);
});

test('HS2-V3R4JR skips on a Windows host and refuses unknown arguments', () => {
  const run = () => assert.fail('nothing runs');
  const logs = [];
  assert.equal(main([], { platform: 'win32', run, log: (line) => logs.push(line) }), 0);
  assert.match(logs[0], /Windows host/);
  assert.equal(main(['--fast'], { platform: 'linux', run, log: () => {} }), 2);
});

test('HS2-V3R4JR per-commit CI runs the check on Linux', () => {
  const ci = readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const job = /\n {2}windows-cfg:\n([\s\S]*?)(?=\n {2}[\w-]+:\n|$)/.exec(ci);
  assert.ok(job, 'ci.yml has a windows-cfg job');
  assert.match(job[1], /runs-on: ubuntu-latest/);
  assert.match(job[1], /run: node scripts\/windows-cfg-check\.mjs/);
});
