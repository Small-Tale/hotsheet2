import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { browserStages, parseToolVersion, prepare, runGate } from './local-gate.mjs';

const fullScript = readFileSync(new URL('./check-full.sh', import.meta.url), 'utf8');
test('HS2-RS0FFT tool versions exclude changing login-shell session messages', () => {
  assert.equal(parseToolVersion('rustc --version', 'Restored session: synthetic time\nrustc 1.97.0\n'), 'rustc 1.97.0');
  assert.equal(parseToolVersion('node --version', 'Restored session: another time\nv22.23.2\n'), 'v22.23.2');
  assert.throws(() => parseToolVersion('cargo --version', 'no version'), /unavailable/);
});
function fixture(t) {
  const repo = mkdtempSync(join(tmpdir(), 'hs2-rs0fft-'));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  const git = (...args) => execFileSync('git', args, { cwd: repo, encoding: 'utf8' }).trim();
  const put = (path, body) => {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    writeFileSync(join(repo, path), body);
  };
  git('init', '--quiet', '-b', 'main');
  git('config', 'user.name', 'Synthetic');
  git('config', 'user.email', 'synthetic@invalid.example');
  put('scripts/check-full.sh', fullScript);
  put('clients/web/src/example.ts', 'export const value = 1;\n');
  git('add', '.');
  git('commit', '--quiet', '-m', 'base');
  git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  put('clients/web/src/example.ts', 'export const value = 2;\n');
  git('commit', '--quiet', '-am', 'client change');
  return { repo, git, put, options: { repo, versions: [['synthetic-toolchain', '1']], log: () => {} } };
}

test('HS2-RS0FFT client selection omits unrelated Rust and keeps policy and real-server smoke', (t) => {
  const f = fixture(t);
  const p = prepare(f.repo, undefined, false, f.options.versions);
  assert.equal(p.mode, 'selected');
  assert.ok(!p.checks.some((stage) => stage.command === 'cargo lint'));
  assert.ok(p.checks.some((stage) => stage.command.includes('contracts.spec.ts')));
  assert.ok(p.checks.some((stage) => stage.command.includes('check-test-coverage')));
  assert.ok(p.stages.filter((stage) => !stage.selected).every((stage) => stage.reasons.length));
});

test('HS2-RS0FFT receipts reuse unchanged inputs and invalidate toolchain and baseline', (t) => {
  const f = fixture(t);
  let count = 0;
  const runner = () => {
    count += 1;
    return { status: 0 };
  };
  assert.equal(runGate({ ...f.options, runner }), 0);
  const first = count;
  assert.equal(runGate({ ...f.options, runner, verify: true }), 0);
  assert.equal(count, first);
  assert.throws(() => runGate({ ...f.options, versions: [['synthetic-toolchain', '2']], verify: true }), /No valid/);
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  assert.throws(() => runGate({ ...f.options, verify: true }), /No valid/);
});

test('HS2-RS0FFT source and lock changes invalidate receipts and lock changes select full', (t) => {
  const f = fixture(t);
  runGate({ ...f.options, runner: () => ({ status: 0 }) });
  f.put('clients/web/src/example.ts', 'export const value = 3;\n');
  f.git('commit', '--quiet', '-am', 'another source change');
  assert.throws(() => runGate({ ...f.options, verify: true }), /No valid/);
  f.put('clients/web/package-lock.json', '{}\n');
  f.git('add', 'clients/web/package-lock.json');
  f.git('commit', '--quiet', '-m', 'lock change');
  assert.equal(prepare(f.repo, undefined, false, f.options.versions).mode, 'full');
});

test('HS2-RS0FFT dirty and mid-run changed inputs never produce successful evidence', (t) => {
  const f = fixture(t);
  f.put('untracked', 'synthetic\n');
  assert.throws(() => runGate({ ...f.options, runner: () => ({ status: 0 }) }), /clean checkout/);
  f.git('add', 'untracked');
  f.git('commit', '--quiet', '-m', 'new input');
  assert.throws(
    () =>
      runGate({
        ...f.options,
        runner: () => {
          f.put('clients/web/src/example.ts', 'changed during check\n');
          return { status: 0 };
        },
      }),
    /Inputs changed/,
  );
});

test('HS2-RS0FFT failure propagates and cannot be accepted by a receipt check', (t) => {
  const f = fixture(t);
  let calls = 0;
  assert.equal(
    runGate({
      ...f.options,
      runner: () => {
        calls += 1;
        return { status: 7 };
      },
    }),
    7,
  );
  assert.equal(calls, 1);
  assert.throws(() => runGate({ ...f.options, verify: true }), /No valid/);
});

test('HS2-RS0FFT a new unrelated Rust-only input reuses unchanged web evidence', (t) => {
  const f = fixture(t);
  runGate({ ...f.options, runner: () => ({ status: 0 }) });
  f.put('plugins/shared/synthetic.md', 'synthetic guidance\n');
  f.git('add', 'plugins/shared/synthetic.md');
  f.git('commit', '--quiet', '-m', 'Rust embedded input');
  const ran = [];
  assert.equal(
    runGate({
      ...f.options,
      runner: (stage) => {
        ran.push(stage.command);
        return { status: 0 };
      },
    }),
    0,
  );
  assert.ok(ran.includes('cargo lint'));
  assert.ok(!ran.includes('npm run test:unit'));
});

test('HS2-RS0FFT bounded browser selection includes affected terminal and timeline parents', () => {
  const terminal = browserStages(['clients/web/src/terminal-viewport.ts'], false);
  assert.ok(terminal.some((stage) => stage.command.includes('terminal-feedback.spec.ts')));
  const timeline = browserStages(['clients/web/src/ticket-timeline-data.ts'], false);
  assert.ok(timeline.some((stage) => stage.command.includes('ticket-timeline-usage.spec.ts')));
  assert.deepEqual(browserStages(['plugins/shared/visual-qa.md'], false), []);
  const provider = browserStages(['clients/web/tests/providers.spec.ts'], false, ['records a launch']);
  assert.ok(provider.some((stage) => stage.command.includes('--grep')));
  assert.ok(!provider[0].command.includes('providers.spec.ts'));
  const shared = browserStages(['clients/web/src/state.ts'], false);
  assert.ok(shared.some((stage) => stage.command.includes('providers.spec.ts')));
});

test('HS2-RS0FFT identifies a changed parent in a browser file larger than the default subprocess buffer', (t) => {
  const f = fixture(t);
  const path = 'clients/web/tests/providers.spec.ts';
  const content =
    '// synthetic padding\n'.repeat(60000) + "test('large fixture parent', async () => {\n  const value = 1;\n});\n";
  f.put(path, content);
  f.git('add', path);
  f.git('commit', '--quiet', '-m', 'large synthetic fixture');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  f.put(path, content.replace('const value = 1', 'const value = 2'));
  f.git('commit', '--quiet', '-am', 'changed test body');
  const result = prepare(f.repo, undefined, false, f.options.versions);
  assert.ok(
    result.checks.some((stage) => stage.command.includes('--grep') && stage.command.includes('large fixture parent')),
  );
});
