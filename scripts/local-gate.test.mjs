import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import {
  browserStages,
  importCiTimings,
  inspectTimings,
  parseToolVersion,
  performanceStages,
  prepare,
  runGate,
} from './local-gate.mjs';
import { profileFromTags } from './gate-ticket-profile.mjs';

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
  put('scripts/gate-budget.json', JSON.stringify({ defaultBudgetMs: 300000 }));
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

test('HS2-GN06JT budget and analyzed set are exact receipt inputs', (t) => {
  const f = fixture(t);
  const one = prepare(f.repo, undefined, false, f.options.versions, { tier: 'T1', budgetMs: 300000 });
  const two = prepare(f.repo, undefined, false, f.options.versions, { tier: 'T1', budgetMs: 600000 });
  assert.notEqual(one.key, two.key);
  assert.ok(one.deferred.every((stage) => stage.budgetReason.includes('deferred to CI')));
  assert.equal(runGate({ ...f.options, runner: () => ({ status: 0 }), tier: 'T1', budgetMs: 300000 }), 0);
  assert.throws(() => runGate({ ...f.options, verify: true, tier: 'T1', budgetMs: 600000 }), /No valid/);
  assert.equal(prepare(f.repo, undefined, false, f.options.versions, { budgetMs: 600000 }).budgetMs, 600000);
  assert.throws(() => prepare(f.repo, undefined, false, f.options.versions, { tier: 'T2' }), /explicit analyzed/);
  const build = 'clients/web::npm run build';
  const t2 = prepare(f.repo, undefined, false, f.options.versions, { tier: 'T2', analyzed: [build] });
  assert.equal(t2.budgetMs, null);
  assert.ok(t2.checks.some((stage) => stage.command === 'npm run build'));
  assert.throws(
    () => prepare(f.repo, undefined, false, f.options.versions, { tier: 'T2', budgetMs: 300000, analyzed: [build] }),
    /no time budget/,
  );
});

test('HS2-GN06JT gate and config changes keep the full fallback', (t) => {
  const f = fixture(t);
  f.put('scripts/gate-budget.json', JSON.stringify({ defaultBudgetMs: 600000 }));
  f.git('add', 'scripts/gate-budget.json');
  f.git('commit', '--quiet', '-m', 'budget config change');
  const result = prepare(f.repo, undefined, false, f.options.versions, { tier: 'T1' });
  assert.equal(result.mode, 'full');
  assert.equal(result.budgetMs, 600000);
  assert.equal(result.deferred.length, 0);
  assert.ok(result.stages.filter((stage) => stage.group !== undefined).every((stage) => stage.selected));
});

test('HS2-VQ5ND5 missing ticket uses the default budgeted profile', (t) => {
  const f = fixture(t);
  const result = prepare(f.repo, undefined, false, f.options.versions);
  assert.equal(result.tier, 'T1');
  assert.equal(result.budgetMs, 300000);
  assert.match(result.profile.reason, /ticket source unavailable/);
  assert.ok(result.deferred.some((stage) => stage.command === 'npm run build'));
  assert.throws(
    () => prepare(f.repo, undefined, false, f.options.versions, { ticket: 'HS2-SYNTHETIC' }),
    /no linked ticket source/,
  );
});

test('HS2-VQ5ND5 tagged ticket yields a deeper affected plan', (t) => {
  const f = fixture(t);
  const profile = profileFromTags({
    qualified_id: 'git:synthetic',
    tags: ['tier:T1', 'deep-testing', 'testing:browser', 'testing:platform', 'test-budget:15m'],
  });
  const result = prepare(f.repo, undefined, false, f.options.versions, { profile });
  assert.equal(result.budgetMs, null);
  assert.equal(result.profile.platform, true);
  assert.ok(result.checks.some((stage) => stage.command === 'npm run build'));
  assert.ok(
    result.checks.some(
      (stage) => stage.command.startsWith('npx playwright test ') && !stage.command.includes('--grep'),
    ),
  );
  assert.ok(result.stages.filter((stage) => stage.selected).some((stage) => stage.budgetReason === 'deep-testing tag'));
  assert.throws(() => prepare(f.repo, undefined, false, f.options.versions, { profile, tier: 'T2' }), /conflicts/);
});

test('HS2-VQ5ND5 profile tags change receipt identity and select performance checks', (t) => {
  const f = fixture(t);
  const basic = profileFromTags({ qualified_id: 'git:synthetic', tags: ['tier:T1'] });
  const perf = profileFromTags({ qualified_id: 'git:synthetic', tags: ['tier:T1', 'testing:perf'] });
  const before = prepare(f.repo, undefined, false, f.options.versions, { profile: basic });
  const after = prepare(f.repo, undefined, false, f.options.versions, { profile: perf });
  assert.notEqual(before.key, after.key);
  assert.ok(after.checks.some((stage) => stage.command === 'npm run test:performance'));
  assert.ok(after.checks.some((stage) => stage.reasons.includes('testing:perf selects the web performance baseline')));
});

test('HS2-A4HD9X maps testing:perf to direct Rust, migrator and web baselines', (t) => {
  assert.deepEqual(performanceStages(['docs/07-migration.md']), []);
  assert.equal(performanceStages(['scripts/perf-ticketing.mjs'])[0].command, 'node scripts/perf-ticketing.mjs');
  assert.deepEqual(
    performanceStages(['crates/hotsheet-cli/src/main.rs', 'migrator/src/export.mjs', 'clients/web/src/app.ts']).map(
      (stage) => `${stage.cwd}::${stage.command}`,
    ),
    [
      '.::node scripts/perf-ticketing.mjs',
      'migrator::npm run test:performance',
      'clients/web::npm run test:performance',
    ],
  );
  const f = fixture(t);
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  f.put('crates/hotsheet-ticketing/src/example.rs', 'pub fn synthetic() {}\n');
  f.git('add', 'crates/hotsheet-ticketing/src/example.rs');
  f.git('commit', '--quiet', '-m', 'rust change');
  const profile = profileFromTags({ qualified_id: 'git:synthetic', tags: ['tier:T1', 'testing:perf'] });
  const rust = prepare(f.repo, undefined, false, f.options.versions, { profile });
  assert.ok(rust.checks.some((stage) => stage.command === 'node scripts/perf-ticketing.mjs'));
  assert.ok(!rust.deferred.some((stage) => stage.command === 'node scripts/perf-ticketing.mjs'));
  f.put('migrator/src/example.mjs', 'export const synthetic = true;\n');
  f.git('add', 'migrator/src/example.mjs');
  f.git('commit', '--quiet', '-m', 'migrator change');
  const both = prepare(f.repo, undefined, false, f.options.versions, { profile });
  assert.ok(both.checks.some((stage) => stage.cwd === 'migrator' && stage.command === 'npm run test:performance'));
  assert.notEqual(rust.key, both.key, 'new baseline changes the exact receipt');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  f.put('docs/example.md', 'synthetic documentation\n');
  f.git('add', 'docs/example.md');
  f.git('commit', '--quiet', '-m', 'docs change');
  assert.throws(
    () => prepare(f.repo, undefined, false, f.options.versions, { profile }),
    /testing:perf has no configured baseline/,
  );
});

test('HS2-GN06JT invalid checked-in budget config fails closed', (t) => {
  const f = fixture(t);
  f.put('scripts/gate-budget.json', JSON.stringify({ defaultBudgetMs: 0, typo: true }));
  assert.throws(() => prepare(f.repo, undefined, false, f.options.versions), /Checked-in gate budget config/);
});

test('HS2-TCCKRZ completed stage samples exclude receipt reuse', (t) => {
  const f = fixture(t);
  const runner = () => {
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 2);
    return { status: 0 };
  };
  assert.equal(runGate({ ...f.options, runner }), 0);
  const first = inspectTimings(f.repo);
  assert.ok(first.stages.some((entry) => entry.samples === 1 && entry.ms > 0));
  assert.equal(runGate({ ...f.options, runner }), 0);
  assert.deepEqual(inspectTimings(f.repo), first);
});

test('HS2-TCCKRZ headless CI import seeds timing estimates without running a gate', (t) => {
  const f = fixture(t);
  const artifacts = join(f.repo, 'target/ci-artifacts');
  mkdirSync(artifacts, { recursive: true });
  writeFileSync(
    join(artifacts, 'vitest-report.json'),
    JSON.stringify({
      success: true,
      testResults: [{ name: '/ci-root/clients/web/src/example.test.ts', startTime: 0, endTime: 1800 }],
    }),
  );
  importCiTimings(f.repo, artifacts);
  assert.deepEqual(inspectTimings(f.repo).files, [
    { key: 'vitest:src/example.test.ts', ms: 1800, samples: 1, host: 'ci' },
  ]);
  assert.throws(() => importCiTimings(f.repo, join(f.repo, 'missing')), /No CI timing artifacts/);
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
  assert.deepEqual(inspectTimings(f.repo), { stages: [], files: [] });
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

test('HS2-5PTT2Y a sandboxed failure names the sandbox; a pass or an unsandboxed failure does not', (t) => {
  const run = (env, status) => {
    const f = fixture(t);
    const lines = [];
    const code = runGate({ ...f.options, runner: () => ({ status }), env, log: (line) => lines.push(line) });
    return { code, text: lines.join('\n') };
  };
  const sandboxed = run({ SANDBOX_RUNTIME: '1' }, 1);
  assert.equal(sandboxed.code, 1, 'the notice never turns a failure into a pass');
  assert.match(sandboxed.text, /No receipt recorded/);
  assert.match(sandboxed.text, /HS2-5PTT2Y.*Rerun outside the sandbox/);
  assert.doesNotMatch(run({}, 1).text, /HS2-5PTT2Y/);
  assert.doesNotMatch(run({ SANDBOX_RUNTIME: '' }, 1).text, /HS2-5PTT2Y/);
  const passed = run({ SANDBOX_RUNTIME: '1' }, 0);
  assert.equal(passed.code, 0);
  assert.doesNotMatch(passed.text, /HS2-5PTT2Y/);
});

test('HS2-BNAYKG a bare check reuses only the exact passing T2 receipt for the tree', (t) => {
  const f = fixture(t);
  const build = 'clients/web::npm run build';
  let runs = 0;
  const runner = () => {
    runs += 1;
    return { status: 0 };
  };
  assert.throws(() => runGate({ ...f.options, runner, tier: 'T2' }), /--t2-stage/, 'no receipt yet: still refused');
  assert.equal(runs, 0);
  assert.equal(runGate({ ...f.options, runner, tier: 'T2', analyzed: [build] }), 0);
  const ran = runs;
  const lines = [];
  assert.equal(runGate({ ...f.options, runner, tier: 'T2', log: (line) => lines.push(line) }), 0);
  assert.equal(runGate({ ...f.options, runner, tier: 'T2', verify: true }), 0);
  assert.equal(runs, ran, 'the push-time check reuses the receipt without rerunning a stage');
  assert.match(lines.join('\n'), /already passed/);
  assert.throws(
    () => runGate({ ...f.options, runner, tier: 'T2', versions: [['synthetic-toolchain', '2']] }),
    /--t2-stage/,
    'changed inputs never match the recorded receipt',
  );
  f.put('clients/web/src/example.ts', 'export const value = 3;\n');
  f.git('commit', '--quiet', '-am', 'another client change');
  assert.throws(() => runGate({ ...f.options, runner, tier: 'T2', verify: true }), /--t2-stage/);
  assert.equal(runs, ran);
});

test('HS2-Q5XAAK bare push check reuses the explicit ticket receipt without weakening identity', (t) => {
  const f = fixture(t);
  const base = profileFromTags({ qualified_id: 'git:synthetic', tags: ['tier:T2'] });
  const explicit = { ...base, reason: 'explicit --ticket; tier from tier:T2' };
  const inferred = { ...base, reason: 'HEAD commit subject; tier from tier:T2' };
  const analyzed = ['clients/web::npm run build'];
  const first = prepare(f.repo, undefined, false, f.options.versions, { profile: explicit, analyzed });
  const second = prepare(f.repo, undefined, false, f.options.versions, { profile: inferred, analyzed });
  assert.equal(first.key, second.key, 'resolution explanation does not change the effective profile');
  assert.notEqual(first.profile.reason, second.profile.reason, 'the plan still explains how the ticket was resolved');

  let runs = 0;
  const runner = () => {
    runs += 1;
    return { status: 0 };
  };
  assert.equal(runGate({ ...f.options, profile: explicit, analyzed, runner }), 0);
  const passedStages = runs;
  assert.equal(runGate({ ...f.options, profile: inferred, runner, verify: true }), 0);
  assert.equal(runs, passedStages, 'push reuses the exact passing receipt');

  const changed = { ...inferred, ticket: 'git:another' };
  assert.notEqual(prepare(f.repo, undefined, false, f.options.versions, { profile: changed, analyzed }).key, first.key);
  assert.throws(() => runGate({ ...f.options, profile: changed, runner, verify: true }), /--t2-stage/);
});
