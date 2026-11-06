import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  browserStages,
  importCiTimings,
  inspectTimings,
  parseToolVersion,
  performanceStages,
  prepare,
  runGate,
  smokeStages,
} from './local-gate.mjs';
import {
  catalogSpecimenCount,
  demoEntryIds,
  exerciseIndex,
  exerciseIndexForRepo,
  ownHooks,
  routeHooks,
  specimenInventoryTitles,
  withSpecimenInventory,
} from './gate-browser-exercise.mjs';
import { profileFromTags } from './gate-ticket-profile.mjs';
import {
  changedLines,
  parentModuleFiles,
  platformCfgChanges,
  platformCfgEvidence,
  platformCfgRanges,
} from './platform-cfg.mjs';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const fullScript = readFileSync(new URL('./check-full.sh', import.meta.url), 'utf8');
test('HS2-RS0FFT tool versions exclude changing login-shell session messages', () => {
  assert.equal(parseToolVersion('rustc --version', 'Restored session: synthetic time\nrustc 1.97.0\n'), 'rustc 1.97.0');
  assert.equal(parseToolVersion('node --version', 'Restored session: another time\nv22.23.2\n'), 'v22.23.2');
  assert.throws(() => parseToolVersion('cargo --version', 'no version'), /unavailable/);
});
function fixture(t) {
  const repo = mkdtempSync(join(tmpdir(), 'hs2-rs0fft-'));
  t.after(() => rmSync(repo, { recursive: true, force: true }));
  const git = (...args) =>
    execFileSync('git', args, { cwd: repo, env: withoutGitRepositoryEnv(process.env), encoding: 'utf8' }).trim();
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
  assert.equal(p.edgePreview.mode, 'broad');
  assert.equal(p.edgePreview.requiredGate, 'unchanged pending HS2-DAA8AB');
  assert.ok(!p.checks.some((stage) => stage.command === 'cargo lint'));
  assert.ok(p.checks.some((stage) => stage.command.includes('contracts.spec.ts')));
  assert.ok(p.checks.some((stage) => stage.command.includes('check-test-coverage')));
  assert.ok(p.stages.filter((stage) => !stage.selected).every((stage) => stage.reasons.length));
});

test('HS2-55MWQ9 a private web candidate cannot replace the required unit stage or receipt inputs', (t) => {
  const f = fixture(t);
  f.put('clients/web/src/trailing-task-internal.ts', 'export const delay = 250;\n');
  f.put('clients/web/src/trailing-task.ts', "import { createTrailingTaskInternal } from './trailing-task-internal';\n");
  f.put('clients/web/eslint.config.mjs', "'**/trailing-task-internal'\n");
  f.put('clients/web/src/trailing-task.test.ts', 'HS2-K5E2KF preserves the public delay\n');
  f.put('clients/web/tests/providers.spec.ts', 'defers ticket refresh without hiding an open metadata popup\n');
  f.put(
    'clients/web/src/app/runtime.tsx',
    "import { createTrailingTask } from '../trailing-task';\ncreateTrailingTask<TicketView>();\n",
  );
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'synthetic boundary');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  f.put('clients/web/src/trailing-task-internal.ts', 'export const delay = 251;\n');
  f.git('commit', '--quiet', '-am', 'private timer edit');
  const p = prepare(f.repo, undefined, false, f.options.versions);
  assert.equal(p.edgePreview.mode, 'candidate');
  assert.ok(p.checks.some((item) => item.command === 'npm run test:unit'));
  assert.ok(!p.checks.some((item) => item.command.startsWith('npx vitest run src/trailing-task.test.ts')));
  assert.equal(runGate({ ...f.options, runner: () => ({ status: 0 }) }), 0);
  assert.equal(runGate({ ...f.options, verify: true }), 0);
});

test('HS2-3ZJBRM scoped lint receipts track web inputs while global format tracks docs', (t) => {
  const f = fixture(t);
  const keys = () => {
    const checks = prepare(f.repo, undefined, false, f.options.versions).checks;
    return {
      format: checks.find((item) => item.command === 'npm run format:sources:check')?.key,
      lint: checks.find((item) => item.command === 'npm run lint:web')?.key,
    };
  };
  const first = keys();
  assert.ok(first.format && first.lint);
  f.put('docs/example.md', 'Synthetic documentation\n');
  f.git('add', 'docs/example.md');
  f.git('commit', '--quiet', '-m', 'docs change');
  const docs = keys();
  assert.notEqual(docs.format, first.format);
  assert.equal(docs.lint, first.lint);
  f.put('clients/web/src/example.ts', 'export const value = 3;\n');
  f.git('commit', '--quiet', '-am', 'web change');
  assert.notEqual(keys().lint, docs.lint);
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

test('HS2-XW38CB Playwright worker, retry and quarantine overrides are receipt inputs', (t) => {
  const f = fixture(t);
  const key = () => prepare(f.repo, undefined, false, f.options.versions, { tier: 'T1', budgetMs: 300000 }).key;
  const names = ['HOTSHEET_PLAYWRIGHT_WORKERS', 'HOTSHEET_PLAYWRIGHT_RETRIES', 'HOTSHEET_RUN_QUARANTINED'];
  const saved = Object.fromEntries(names.map((name) => [name, process.env[name]]));
  t.after(() => {
    for (const name of names)
      if (saved[name] === undefined) delete process.env[name];
      else process.env[name] = saved[name];
  });
  for (const name of names) delete process.env[name];
  const baseline = key();
  for (const [name, value] of [
    ['HOTSHEET_PLAYWRIGHT_WORKERS', '1'],
    ['HOTSHEET_PLAYWRIGHT_RETRIES', '2'],
    ['HOTSHEET_RUN_QUARANTINED', '1'],
  ]) {
    process.env[name] = value;
    assert.notEqual(key(), baseline, name);
    delete process.env[name];
    assert.equal(key(), baseline, `${name} reset restores the original identity`);
  }
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

test('HS2-PANVH5 emits unitless uncapped and millisecond numeric budgets on failure, success and reuse', (t) => {
  for (const { selection, label } of [
    { selection: { tier: 'T2', analyzed: ['clients/web::npm run build'] }, label: 'none' },
    { selection: { tier: 'T1', budgetMs: 300000 }, label: '300000ms' },
  ]) {
    const f = fixture(t),
      messages = [],
      options = { ...f.options, ...selection, log: (line) => messages.push(line) };
    let executions = 0;
    assert.equal(
      runGate({
        ...options,
        runner: () => {
          executions += 1;
          return { status: 7 };
        },
      }),
      7,
    );
    assert.equal(executions, 1);
    assert.match(
      messages.find((line) => line.startsWith('Local gate failed after ')),
      new RegExp(`^Local gate failed after \\d+ms; budget ${label}\\. No receipt recorded\\.$`),
    );

    messages.length = 0;
    assert.equal(
      runGate({
        ...options,
        runner: () => {
          executions += 1;
          return { status: 0 };
        },
      }),
      0,
    );
    assert.ok(executions > 1);
    assert.match(
      messages.at(-1),
      new RegExp(
        `^Local selected gate passed for tree [a-f0-9]+; actual \\d+ms, budget ${label}(?: \\(mandatory floor exceeded budget\\))?$`,
      ),
    );

    const completedExecutions = executions;
    messages.length = 0;
    assert.equal(
      runGate({
        ...options,
        verify: true,
        runner: () => {
          executions += 1;
          return { status: 0 };
        },
      }),
      0,
    );
    assert.equal(executions, completedExecutions);
    assert.equal(messages.length, 1);
    assert.match(
      messages[0],
      new RegExp(`^Local gate already passed for tree [a-f0-9]+; reused exact receipt for budget ${label}$`),
    );
  }
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
  // HS2-PNBH4X: a committed lockfile needs its matching install, as npm ci leaves it.
  assert.throws(() => prepare(f.repo, undefined, false, f.options.versions), /not installed/);
  f.put('clients/web/node_modules/.package-lock.json', '{"packages":{}}\n');
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

test('HS2-WWZS75 app startup changes select the degraded WebKit boot spec', () => {
  for (const path of ['clients/web/src/app/runtime.tsx', 'clients/web/src/main.tsx', 'clients/web/src/browser-id.ts']) {
    const stages = browserStages([path], false);
    assert.ok(
      stages.some((stage) => stage.command.includes('tests/terminal-safari.spec.ts')),
      path,
    );
    assert.ok(
      stages.some((stage) => stage.reasons.some((reason) => reason.startsWith('HS2-WWZS75:'))),
      path,
    );
  }
  assert.ok(
    !browserStages(['clients/web/src/ticket-timeline-data.ts'], false).some((stage) =>
      stage.command.includes('tests/terminal-safari.spec.ts'),
    ),
  );
});

test('HS2-P9A21R selects phone navigation and resize after workspace chrome changes', () => {
  const stages = browserStages(['clients/web/src/components/workspace-controls.tsx'], false);
  assert.ok(
    stages.some((stage) => stage.command.includes('projects Kerf workspace segments through mode transitions')),
  );
  assert.ok(
    stages.some(
      (stage) =>
        stage.command.includes('tests/mobile-layout.spec.ts') &&
        stage.command.includes('resizing from mobile back to desktop'),
    ),
  );
});

test('HS2-P9A21R selects bounded search-title remeasurement for every workspace chrome owner', () => {
  for (const path of [
    'clients/web/src/components/workspace-controls.tsx',
    'clients/web/src/components/app-shell.css',
    'clients/web/src/workspace-toolbar-visibility.ts',
    'clients/web/src/features/hot-sheet-shell.tsx',
  ]) {
    const stages = browserStages([path], false);
    const search = stages.filter((stage) => stage.command.includes('tests/search-scope.spec.ts'));
    assert.equal(search.length, 1, path);
    assert.ok(
      search[0].command.includes("--grep 'keeps the workspace title whole when a search changes it to a longer title'"),
    );
    assert.ok(search[0].reasons.some((reason) => reason.startsWith('HS2-P9A21R:')));
  }
  assert.ok(
    !browserStages(['clients/web/src/ticket-timeline-data.ts'], false).some((stage) =>
      stage.command.includes('tests/search-scope.spec.ts'),
    ),
  );
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

test('HS2-P9A21R selects every generated theme case through a stable title prefix', (t) => {
  const f = fixture(t);
  const path = 'clients/web/tests/providers.spec.ts';
  const content =
    'for (const theme of ["light", "dark"]) {\n' +
    '  test(`workspace mode transitions in ${theme}`, async () => {\n    const count = 4;\n  });\n}\n';
  f.put(path, content);
  f.git('add', path);
  f.git('commit', '--quiet', '-m', 'synthetic theme cases');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  f.put(path, content.replace('count = 4', 'count = 5'));
  f.git('commit', '--quiet', '-am', 'changed generated cases');
  const result = prepare(f.repo, undefined, false, f.options.versions);
  const provider = result.checks.filter((stage) => stage.command.includes('providers.spec.ts'));
  assert.equal(provider.length, 1);
  assert.ok(provider[0].command.includes('--grep'));
  assert.ok(provider[0].command.includes('workspace mode transitions in '));
});

test('HS2-P9A21R keeps shared edits after an indented test on full-file coverage', (t) => {
  const f = fixture(t);
  const path = 'clients/web/tests/providers.spec.ts';
  const content =
    'for (const theme of ["light", "dark"]) {\n' +
    '  test(`workspace mode transitions in ${theme}`, async () => {\n    const count = 4;\n  });\n' +
    '  const shared = 1;\n}\n';
  f.put(path, content);
  f.git('add', path);
  f.git('commit', '--quiet', '-m', 'synthetic shared fixture');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  f.put(path, content.replace('shared = 1', 'shared = 2'));
  f.git('commit', '--quiet', '-am', 'changed shared fixture');
  const result = prepare(f.repo, undefined, false, f.options.versions);
  const provider = result.checks.filter((stage) => stage.command.includes('providers.spec.ts'));
  assert.equal(provider.length, 1);
  assert.equal(provider[0].command.includes('--grep'), false);
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

test('HS2-PNBH4X a stale install blocks the gate and a reinstall changes every receipt key', (t) => {
  const f = fixture(t);
  const lock = {
    lockfileVersion: 3,
    packages: { '': { name: 'web', dependencies: { kerf: '1.0.0' } }, 'node_modules/kerf': { version: '1.0.1' } },
  };
  f.put('clients/web/package.json', JSON.stringify({ name: 'web', dependencies: { kerf: '1.0.0' } }));
  f.put('clients/web/package-lock.json', JSON.stringify(lock));
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'lockfile advanced');
  // The worktree moved to a lockfile wanting 1.0.1 while node_modules still holds 1.0.0.
  const installed = (version, resolved) =>
    f.put(
      'clients/web/node_modules/.package-lock.json',
      JSON.stringify({ lockfileVersion: 3, packages: { 'node_modules/kerf': { version, resolved } } }),
    );
  installed('1.0.0');
  assert.throws(() => prepare(f.repo, undefined, false, f.options.versions), /locked at 1\.0\.1[\s\S]*npm ci/);
  assert.throws(() => runGate({ ...f.options, runner: () => ({ status: 0 }) }), /HS2-PNBH4X/);
  installed('1.0.1', 'a');
  const first = prepare(f.repo, undefined, false, f.options.versions);
  installed('1.0.1', 'b');
  const second = prepare(f.repo, undefined, false, f.options.versions);
  assert.notEqual(first.key, second.key);
  const webStage = (p) => p.checks.find((stage) => stage.cwd === 'clients/web');
  assert.notEqual(webStage(first).key, webStage(second).key);
});

const repoRoot = fileURLToPath(new URL('..', import.meta.url));
const web = (path) => `clients/web/${path}`;
const attachSpecs = [
  'tests/terminal-safari.spec.ts',
  'tests/terminal-liveness.spec.ts',
  'tests/terminal-zoom-refit.spec.ts',
  'tests/webkit-memory.spec.ts',
];

/** A small synthetic client: a runtime whose attach URL comes from a helper, and a search component. */
function exerciseFixture() {
  const sources = new Map([
    [web('src/terminal-viewport.ts'), 'export const url = (id) => `/terminals/${id}/attach`;\n'],
    [
      web('src/terminal-viewport-runtime.ts'),
      "import { url } from './terminal-viewport';\nnew WebSocket(url('a'));\nnode.classList.contains('terminal-viewport--dedicated');\n",
    ],
    [web('src/inline-search.ts'), 'export const parse = (text) => text.split(" ");\n'],
    [
      web('src/components/ticket-search-field.tsx'),
      "import './ticket-search-field.css';\nimport { parse } from '../inline-search';\nexport const TicketSearchField = () => <div class=\"ticket-search-field\" />;\n",
    ],
    [web('src/components/ticket-search-field.css'), '.ticket-search-field__help { inline-size: 1px; }\n'],
    [web('src/trailing-task.ts'), 'export const later = () => undefined;\n'],
    [
      web('src/app/runtime.tsx'),
      "import { later } from '../trailing-task';\nimport { parse } from '../inline-search';\n",
    ],
    [web('src/unrelated-format.ts'), 'export const format = (n) => String(n);\n'],
    [
      web('src/components/badge-count.tsx'),
      'import { format } from \'../unrelated-format\';\nexport const B = () => <span class="badge-count" />;\n',
    ],
  ]);
  const specs = new Map([
    [
      web('tests/terminal-safari.spec.ts'),
      "test('boots', async ({ page }) => {\n  await page.routeWebSocket(/\\/terminals\\/[^/?]+\\/attach(?:\\?.*)?$/, () => {});\n});\n",
    ],
    [
      web('tests/badge.spec.ts'),
      "test('counts', async ({ page }) => {\n  await page.locator('.badge-count').click();\n});\n",
    ],
    [
      web('tests/providers.spec.ts'),
      [
        "import { expect, test } from '@playwright/test';",
        '',
        'async function haltedSessionFixture(page) {',
        '  await page.routeWebSocket(/\\/terminals\\/[^/?]+\\/attach(?:\\?.*)?$/, (route) => void route.close());',
        '}',
        '',
        "test('marks a halted terminal (HS2-HJ4D1H)', async ({ page }) => {",
        '  await haltedSessionFixture(page);',
        '});',
        '',
        'for (const width of [1280, 390])',
        '  test(`inserts a search syntax example from the help as a filter chip at ${width}px (HS2-FR1Y70)`, async ({',
        '    page,',
        '  }) => {',
        "    await page.locator('.ticket-search-field').click();",
        '  });',
        '',
        'test(',
        "  'opens the search syntax help dialog from a wrapped title',",
        '  async ({ page }) => {',
        "    await page.locator('.ticket-search-field__help').waitFor();",
        '  },',
        ');',
        '',
        "test('counts badges in the shipped list', async ({ page }) => {",
        "  await page.locator('.badge-count').click();",
        '});',
        '',
      ].join('\n'),
    ],
  ]);
  return { sources, specs };
}

test('HS2-N7EY18 a terminal runtime change selects the specs that route its attach socket', () => {
  const index = exerciseIndex([web('src/terminal-viewport-runtime.ts')], exerciseFixture());
  assert.deepEqual(index.files, ['tests/terminal-safari.spec.ts']);
  // The helper that routes the socket reaches the provider test that calls it.
  assert.deepEqual(index.providerTitles, ['marks a halted terminal (HS2-HJ4D1H)']);
  assert.ok(index.reasons[0].includes('terminals/*/attach'));
  const stages = browserStages([web('src/terminal-viewport-runtime.ts')], false, [], {}, index);
  assert.ok(stages.some((stage) => stage.command.includes("'tests/terminal-safari.spec.ts'")));
  assert.ok(
    stages.some(
      (stage) =>
        stage.command.startsWith('npx playwright test tests/providers.spec.ts --grep ') &&
        stage.command.includes('HS2-HJ4D1H'),
    ),
  );
});

test('HS2-N7EY18 the checked-in runtime, replay stream and socket bridge select every attach-socket spec', () => {
  for (const path of [
    'src/terminal-viewport-runtime.ts',
    'src/terminal-replay-stream.ts',
    'src/terminal-ws-bridge.ts',
  ]) {
    const index = exerciseIndexForRepo(repoRoot, [web(path)]);
    for (const spec of attachSpecs) assert.ok(index.files.includes(spec), `${path} selects ${spec}`);
    for (const slug of ['HS2-HJ4D1H', 'HS2-XYSXVT', 'HS2-EV1XK3', 'HS2-PTC0FD'])
      assert.ok(
        index.providerTitles.some((title) => title.includes(slug)),
        `${path} selects the ${slug} provider case`,
      );
  }
});

test('HS2-R1D0S5 a component change selects the provider cases that render it in the shipped parent', () => {
  const fixture = exerciseFixture();
  const index = exerciseIndex([web('src/components/ticket-search-field.tsx')], fixture);
  assert.deepEqual(index.providerTitles, [
    'inserts a search syntax example from the help as a filter chip at ',
    'opens the search syntax help dialog from a wrapped title',
  ]);
  assert.deepEqual(index.files, []);
  // The sibling stylesheet speaks for the component.
  assert.deepEqual(
    exerciseIndex([web('src/components/ticket-search-field.css')], fixture).providerTitles,
    index.providerTitles,
  );
  // A hook-less helper reaches the specs through its nearest shipped parent, never through app/.
  const helper = exerciseIndex([web('src/inline-search.ts')], fixture);
  assert.deepEqual(helper.providerTitles, index.providerTitles);
  assert.ok(helper.reasons[0].includes('ticket-search-field.tsx'));
  assert.ok(!helper.reasons[0].includes('app/runtime.tsx'));
  const real = exerciseIndexForRepo(repoRoot, [web('src/components/ticket-search-field.tsx')]);
  assert.ok(real.providerTitles.includes('inserts a search syntax example from the help as a filter chip at '));
  const stages = browserStages([web('src/components/ticket-search-field.tsx')], false, [], {}, real);
  assert.ok(
    stages.some(
      (stage) =>
        stage.command.startsWith('npx playwright test tests/providers.spec.ts --grep ') &&
        stage.command.includes('inserts a search syntax example from the help as a filter chip at '),
    ),
  );
});

test('HS2-N7EY18 HS2-R1D0S5 an unrelated module pulls neither the attach specs nor the syntax-help cases', () => {
  const fixture = exerciseFixture();
  const unrelated = exerciseIndex([web('src/unrelated-format.ts')], fixture);
  assert.deepEqual(unrelated.files, ['tests/badge.spec.ts']);
  assert.deepEqual(unrelated.providerTitles, ['counts badges in the shipped list']);
  // A module reached only through a composition root adds nothing; the shared rules still apply.
  const rootOnly = exerciseIndex([web('src/trailing-task.ts')], fixture);
  assert.deepEqual(rootOnly, { files: [], providerTitles: [], providerWhole: false, demoTitles: [], reasons: [] });
  const real = exerciseIndexForRepo(repoRoot, [web('src/ticket-timeline-data.ts')]);
  for (const spec of attachSpecs) assert.ok(!real.files.includes(spec), spec);
  assert.ok(!real.providerTitles.some((title) => /HS2-HJ4D1H|search syntax example/.test(title)));
  assert.ok(real.files.includes('tests/ticket-timeline-usage.spec.ts'));
});

test('HS2-N7EY18 shared spec setup, a fixture module and a direct import select the whole file', () => {
  const { sources, specs } = exerciseFixture();
  specs.set(web('tests/shared-setup.spec.ts'), "page.locator('.badge-count');\ntest.use({});\n");
  specs.set(web('tests/socket-fixture.ts'), 'export const attach = (id) => `/terminals/${id}/attach`;\n');
  specs.set(
    web('tests/proxy.spec.ts'),
    "import { attach } from './socket-fixture';\ntest('proxies', () => attach('a'));\n",
  );
  specs.set(
    web('tests/bridge.spec.ts'),
    "import { format } from '../src/unrelated-format';\ntest('formats', () => format(1));\n",
  );
  const index = exerciseIndex([web('src/unrelated-format.ts')], { sources, specs });
  assert.ok(index.files.includes('tests/bridge.spec.ts'));
  assert.ok(index.files.includes('tests/shared-setup.spec.ts'));
  assert.ok(!index.files.includes('tests/proxy.spec.ts'));
  const runtime = exerciseIndex([web('src/terminal-viewport-runtime.ts')], { sources, specs });
  assert.ok(runtime.files.includes('tests/proxy.spec.ts'));
  // A hook in shared providers setup selects the whole provider file instead of a grep.
  specs.set(web('tests/providers.spec.ts'), "page.locator('.badge-count');\ntest('x', () => {});\n");
  const whole = exerciseIndex([web('src/components/badge-count.tsx')], { sources, specs });
  assert.equal(whole.providerWhole, true);
  const stages = browserStages([web('src/components/badge-count.tsx')], false, [], {}, whole);
  assert.ok(stages.some((stage) => stage.command.includes("'tests/providers.spec.ts'")));
  assert.ok(!stages.some((stage) => stage.command.startsWith('npx playwright test tests/providers.spec.ts --grep ')));
});

test('HS2-N7EY18 route and class hooks come from template, regex and BEM sources', () => {
  assert.deepEqual(routeHooks('const r = /^\\/__hotsheet\\/project-api\\/([^/]+)\\/terminals\\/([^/]+)\\/attach$/;'), [
    'project-api/*/terminals',
    'terminals/*/attach',
  ]);
  const queried = 'el.closest(\'[data-layout-mode="grid"] .app-drawer--open\');';
  assert.deepEqual(ownHooks('clients/web/src/a.ts', queried, '', { queried: true }).classes, [
    'app-drawer',
    'app-drawer--open',
  ]);
  assert.deepEqual(ownHooks('clients/web/src/a.ts', queried).classes, []);
  // One-word stems never become hooks; Kerf classes never do.
  assert.deepEqual(ownHooks('clients/web/src/state.ts', "const s = 'state'; const k = 'kui-button';").classes, []);
});

/** A synthetic catalog: a shared fixture file whose notes another demo file also renders. */
function demoFixture() {
  const sources = new Map([
    [
      web('src/ux-demo/content-components-demo.tsx'),
      [
        'export const READER_NOTES = [{ id: "reader-note" }];',
        'export const demoModule: DemoModule = {',
        '  entries: {',
        "    'note-card': { catalogExamples: true, Demo: NoteCardDemo },",
        "    'markdown-editor': {",
        '      Demo: MarkdownEditorDemo,',
        '    },',
        "    'ticket-reader': { Demo: TicketReaderDemo },",
        '  },',
        '};',
        '',
      ].join('\n'),
    ],
    [
      web('src/ux-demo/workspace-components-demo.tsx'),
      "import { READER_NOTES } from './content-components-demo';\nexport const demoModule: DemoModule = {\n  entries: {\n    'ticket-inspector': { Demo: InspectorDemo },\n  },\n};\n",
    ],
    [
      web('src/ux-demo/list-demo.tsx'),
      'export const demoModule = {\n  entries: {\n    list: { Demo: ListDemo },\n  },\n};\n',
    ],
    [
      web('src/ux-demo/registry.ts'),
      "import { demoModule as a } from './content-components-demo';\nimport { demoModule as b } from './workspace-components-demo';\nimport { demoModule as c } from './list-demo';\n",
    ],
  ]);
  const specs = new Map([
    [
      web('tests/ux-demo.spec.ts'),
      [
        "test('presents note kinds and round-trips reader and Markdown editor compositions', async ({ page }) => {",
        "  await page.goto('/ux-demo?component=note-card');",
        '});',
        '',
        "test('navigates, toggles, closes, and reopens TicketInspector', async ({ page }) => {",
        "  await page.goto('/ux-demo?component=ticket-inspector');",
        '});',
        '',
        "test('keeps list rows aligned', async ({ page }) => {",
        "  await page.goto('/ux-demo?component=list');",
        "  await page.getByRole('list').waitFor();",
        '});',
        '',
      ].join('\n'),
    ],
    [
      web('tests/ticket-reader-stack.spec.ts'),
      "test('stacks readers', async ({ page }) => {\n  await page.locator('[data-component=\"note-card\"]').first().click();\n});\n",
    ],
  ]);
  return { sources, specs };
}

test('HS2-N7EY18 HS2-R1D0S5 a shared ux-demo fixture change selects the demo tests that render it (HS2-EWF0FT escape)', () => {
  const fixture = demoFixture();
  const changed = web('src/ux-demo/content-components-demo.tsx');
  assert.deepEqual(demoEntryIds(fixture.sources.get(changed)), ['note-card', 'markdown-editor', 'ticket-reader']);
  const index = exerciseIndex([changed], fixture);
  // The importing demo's entry (ticket-inspector) renders READER_NOTES too.
  assert.deepEqual(index.demoTitles, [
    'navigates, toggles, closes, and reopens TicketInspector',
    'presents note kinds and round-trips reader and Markdown editor compositions',
  ]);
  // Demo code is not shipped: a shipped spec's note-card locator is not selected, nor the list demo.
  assert.deepEqual(index.files, []);
  const stages = browserStages([changed], false, [], {}, index);
  assert.ok(
    stages.some(
      (stage) =>
        stage.command.startsWith("npx playwright test 'tests/ux-demo.spec.ts' --grep ") &&
        stage.command.includes('presents note kinds') &&
        !stage.command.includes('keeps list rows aligned'),
    ),
  );
  // A one-word entry id matches only its route, not a role name.
  assert.deepEqual(exerciseIndex([web('src/ux-demo/list-demo.tsx')], fixture).demoTitles, ['keeps list rows aligned']);
  // A catalog aggregator composes every entry, so it selects the whole demo spec.
  const registry = exerciseIndex([web('src/ux-demo/registry.ts')], fixture);
  assert.deepEqual(registry.files, ['tests/ux-demo.spec.ts']);
  assert.deepEqual(registry.demoTitles, []);
  // The checked-in tree: exactly the two tests main CI run 41855929835 failed are selected.
  const real = exerciseIndexForRepo(repoRoot, [changed]);
  for (const title of [
    'presents note kinds and round-trips reader and Markdown editor compositions',
    'navigates, toggles, closes, and reopens TicketInspector',
  ])
    assert.ok(real.demoTitles.includes(title), title);
});

const smokeConfig = readFileSync(new URL('./gate-smoke.json', import.meta.url), 'utf8');

test('HS2-9G0M3V the smoke profile runs only the declared checks and defers the affected suites to CI', (t) => {
  const f = fixture(t);
  f.put('scripts/gate-smoke.json', smokeConfig);
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'declare smoke checks');
  const p = prepare(f.repo, undefined, false, f.options.versions, { smoke: true });
  assert.equal(p.mode, 'smoke');
  assert.equal(p.tier, 'smoke');
  assert.equal(p.budgetMs, null);
  const declared = JSON.parse(smokeConfig).stages.map((stage) => stage.command);
  assert.deepEqual(
    p.checks.map((stage) => stage.command),
    declared,
  );
  // No affected-suite expansion and no browser suites, whatever the change touched.
  assert.ok(!p.checks.some((stage) => /playwright|test:unit|test:fixtures/.test(stage.command)));
  assert.ok(p.deferred.some((stage) => stage.command === 'cargo nextest run --all-features --status-level leak'));
  assert.ok(p.deferred.every((stage) => stage.reasons[0].includes('HS2-9G0M3V')));
  // The ordinary plan of the same tree differs, so its receipt is never reused for smoke.
  assert.notEqual(prepare(f.repo, undefined, false, f.options.versions).key, p.key);
});

test('HS2-9G0M3V a smoke receipt is exact, reusable, and never root-gate evidence', (t) => {
  const f = fixture(t);
  f.put('scripts/gate-smoke.json', smokeConfig);
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'declare smoke checks');
  const ran = [];
  const runner = (stage) => {
    ran.push(stage.command);
    return { status: 0 };
  };
  assert.throws(() => runGate({ ...f.options, smoke: true, verify: true }), /No valid/);
  assert.equal(runGate({ ...f.options, smoke: true, runner }), 0);
  assert.equal(ran.length, JSON.parse(smokeConfig).stages.length);
  assert.equal(runGate({ ...f.options, smoke: true, verify: true }), 0);
  // The ordinary gate still has no receipt for this tree.
  assert.throws(() => runGate({ ...f.options, verify: true }), /No valid/);
  const tree = f.git('rev-parse', 'HEAD^{tree}');
  const marker = join(f.repo, f.git('rev-parse', '--git-path', 'hotsheet-root-gate'), tree);
  assert.equal(existsSync(marker), false);
  // A tree change invalidates the receipt.
  f.put('README.md', 'changed\n');
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'change');
  assert.throws(() => runGate({ ...f.options, smoke: true, verify: true }), /No valid/);
});

test('HS2-9G0M3V a failed smoke check records no receipt so nothing is pushed', (t) => {
  const f = fixture(t);
  f.put('scripts/gate-smoke.json', smokeConfig);
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'declare smoke checks');
  const runner = (stage) => ({ status: stage.command === 'cargo lint' ? 101 : 0 });
  assert.equal(runGate({ ...f.options, smoke: true, runner }), 101);
  assert.throws(() => runGate({ ...f.options, smoke: true, verify: true }), /No valid/);
});

test('HS2-9G0M3V the smoke profile refuses selection flags, browser suites and malformed declarations', (t) => {
  const f = fixture(t);
  f.put('scripts/gate-smoke.json', smokeConfig);
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'declare smoke checks');
  for (const options of [{ tier: 'T1' }, { budgetMs: 1000 }, { ticket: 'HS2-AAA111' }, { analyzed: ['.::cargo lint'] }])
    assert.throws(() => prepare(f.repo, undefined, false, f.options.versions, { smoke: true, ...options }), /--smoke/);
  assert.throws(() => prepare(f.repo, undefined, true, f.options.versions, { smoke: true }), /--smoke/);
  for (const config of [
    { stages: [] },
    { stages: [{ cwd: 'clients/web', command: 'npx playwright test', reason: 'browser' }] },
    { stages: [{ cwd: '.', command: 'cargo lint' }] },
    {
      stages: [
        { cwd: '.', command: 'cargo lint', reason: 'a' },
        { cwd: '.', command: 'cargo lint', reason: 'b' },
      ],
    },
    { stages: [{ cwd: '.', command: 'cargo lint', reason: 'a' }], extra: true },
  ]) {
    f.put('scripts/gate-smoke.json', JSON.stringify(config));
    assert.throws(() => smokeStages(f.repo), JSON.stringify(config));
  }
});

test('HS2-A7X8F5 platform cfg ranges cover gated items, whole files and cfg! lines only', () => {
  const text = [
    '#[cfg(target_os = "linux")]', // 1
    'fn linux_only() {', // 2
    '    body();', // 3
    '}', // 4
    'fn shared() {}', // 5
    '#[cfg(test)]', // 6
    'mod tests;', // 7
    '#[cfg(not(any(windows, target_os = "macos")))]', // 8
    'mod other;', // 9
    'fn inline() { if cfg!(unix) { run(); } }', // 10
    '#[cfg_attr(windows, allow(dead_code))]', // 11
    'struct Kept {', // 12
    '    field: u8,', // 13
    '}', // 14
  ].join('\n');
  assert.deepEqual(platformCfgRanges(text), [
    { start: 1, end: 4, predicate: 'target_os = "linux"' },
    { start: 8, end: 9, predicate: 'not(any(windows, target_os = "macos"))' },
    { start: 10, end: 10, predicate: 'unix' },
    { start: 11, end: 14, predicate: 'windows, allow(dead_code)' },
  ]);
  assert.deepEqual(platformCfgRanges('#![cfg(target_os = "macos")]\nuse a;\nfn b() {}\n'), [
    { start: 1, end: 4, predicate: 'target_os = "macos"' },
  ]);
  assert.deepEqual(platformCfgRanges('#[cfg(feature = "x")]\nfn f() {}\n'), []);
});

test('HS2-A7X8F5 changed lines report removals, additions and cfg-declared modules', () => {
  assert.deepEqual(changedLines('@@ -3,2 +3 @@\n-a\n-b\n+c\n@@ -10,0 +11,2 @@\n+d\n+e\n'), {
    removed: [3, 4],
    added: [3, 11, 12],
  });
  const files = {
    'old\0src/lib.rs': 'fn a() {}\n#[cfg(target_os = "linux")]\nfn gone() {}\n',
    'new\0src/lib.rs': 'fn a() { changed(); }\n#[cfg(windows)]\nmod win;\n',
    'new\0src/win.rs': 'pub fn w() {}\n',
    'old\0src/win.rs': null,
    'new\0src/plain.rs': 'fn p() {}\n',
    'old\0src/plain.rs': 'fn q() {}\n',
  };
  const read = (side, path) => files[`${side}\0${path}`] ?? null;
  const diffs = {
    // Line 1 is shared code; the removed line 3 was Linux-gated.
    'src/lib.rs': '@@ -1 +1 @@\n-x\n+y\n@@ -3 +2,2 @@\n-z\n+w\n+v\n',
    'src/win.rs': '@@ -0,0 +1 @@\n+pub fn w() {}\n',
    'src/plain.rs': '@@ -1 +1 @@\n-q\n+p\n',
  };
  const changes = ['src/lib.rs', 'src/win.rs', 'src/plain.rs', 'README.md'];
  assert.deepEqual(platformCfgChanges({ changes, read, diff: (path) => diffs[path] ?? '' }), [
    { path: 'src/lib.rs', lines: [2, 3], predicates: ['target_os = "linux"', 'windows'] },
    { path: 'src/win.rs', lines: [1], predicates: ['windows'] },
  ]);
  assert.deepEqual(parentModuleFiles('a/b/mod.rs'), {
    name: 'b',
    parents: ['a/mod.rs', 'a/lib.rs', 'a/main.rs', 'a.rs'],
  });
  const none = platformCfgEvidence([], 'darwin');
  assert.equal(none.summary, 'none');
  const some = platformCfgEvidence([{ path: 'src/win.rs', lines: [1], predicates: ['windows'] }], 'darwin');
  assert.match(some.summary, /^1 Rust file change platform cfg code \(src\/win\.rs:1\); only the macOS branches/);
});

test('HS2-A7X8F5 the gate plan, receipt and output name platform cfg code deferred to CI', (t) => {
  const f = fixture(t);
  f.put('crates/demo/src/lib.rs', 'pub fn shared() {}\n');
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'rust base');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  f.put('crates/demo/src/lib.rs', 'pub fn shared() {}\n#[cfg(target_os = "linux")]\npub fn linux() {}\n');
  f.put('crates/demo/src/new_mac.rs', '#![cfg(target_os = "macos")]\npub fn mac() {}\n');
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'platform code');
  const p = prepare(f.repo, undefined, false, f.options.versions);
  assert.deepEqual(
    p.platformCfg.files.map((file) => [file.path, file.lines]),
    [
      ['crates/demo/src/lib.rs', [2, 3]],
      ['crates/demo/src/new_mac.rs', [1, 2]],
    ],
  );
  // An untracked new file counts every line as changed.
  f.put('crates/demo/src/untracked_win.rs', '#![cfg(windows)]\npub fn w() {}\n');
  assert.deepEqual(prepare(f.repo, undefined, false, f.options.versions).platformCfg.files.at(-1), {
    path: 'crates/demo/src/untracked_win.rs',
    lines: [1, 2],
    predicates: ['windows'],
  });
  rmSync(join(f.repo, 'crates/demo/src/untracked_win.rs'));
  const lines = [];
  const options = { ...f.options, log: (line) => lines.push(line), runner: () => ({ status: 0 }) };
  assert.equal(runGate(options), 0);
  assert.ok(
    lines.some((line) => line.startsWith('Platform cfg code deferred to CI: 2 Rust files')),
    lines.join('\n'),
  );
  const receipt = JSON.parse(readFileSync(join(p.cache, `${p.key}.json`), 'utf8'));
  assert.equal(receipt.platformCfg.files.length, 2);
  // A reused receipt still names what this host did not compile.
  lines.length = 0;
  assert.equal(runGate(options), 0);
  assert.ok(lines.some((line) => line.startsWith('Local gate already passed')));
  assert.ok(lines.some((line) => line.startsWith('Platform cfg code deferred to CI')));
  // Shared-only Rust changes report nothing.
  f.put('crates/demo/src/lib.rs', 'pub fn shared() { let _ = 1; }\n#[cfg(target_os = "linux")]\npub fn linux() {}\n');
  f.git('commit', '--quiet', '-am', 'shared edit');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD~1');
  const shared = prepare(f.repo, undefined, false, f.options.versions);
  assert.deepEqual(shared.platformCfg.files, []);
});

const INVENTORY = 'labels converted demo specimens through Kerf CatalogExample, not app-owned stages (HS2-Z3FMEZ)';

test('HS2-B42XMS a changed specimen count adds the catalog specimen inventory', () => {
  assert.equal(catalogSpecimenCount('<CatalogExample a/>\n<CatalogExample\n b/> <CatalogExamples/>'), 2);
  const realSpec = readFileSync(new URL('../clients/web/tests/ux-demo.spec.ts', import.meta.url), 'utf8');
  assert.deepEqual(specimenInventoryTitles(realSpec), [INVENTORY]);
  const empty = { files: [], providerTitles: [], providerWhole: false, demoTitles: ['other demo test'], reasons: [] };
  const texts = {
    base: { 'clients/web/src/ux-demo/list-demo.tsx': '<CatalogExample/><CatalogExample/>' },
    work: {
      'clients/web/src/ux-demo/list-demo.tsx': '<CatalogExample/><CatalogExample/><CatalogExample/>',
      'clients/web/src/ux-demo/new-demo.tsx': '<CatalogExample/>',
      'clients/web/src/ux-demo/same-demo.tsx': '<CatalogExample/>',
      'clients/web/src/ux-demo/catalog.ts': '<CatalogExample/>',
    },
  };
  texts.base['clients/web/src/ux-demo/same-demo.tsx'] = '<CatalogExample/> edited';
  const run = (changes, index = empty) =>
    withSpecimenInventory(index, {
      changes,
      readBase: (path) => texts.base[path] ?? null,
      readWorking: (path) => texts.work[path] ?? null,
      demoSpec: realSpec,
    });
  // An added specimen in a single-word entry's demo selects the inventory beside other titles.
  const added = run(['clients/web/src/ux-demo/list-demo.tsx']);
  assert.deepEqual(added.demoTitles, [INVENTORY, 'other demo test'].sort());
  assert.match(
    added.reasons.at(-1),
    /HS2-B42XMS: clients\/web\/src\/ux-demo\/list-demo\.tsx changes its CatalogExample/,
  );
  // A new demo file counts from zero; a removed one counts to zero.
  assert.ok(run(['clients/web/src/ux-demo/new-demo.tsx']).demoTitles.includes(INVENTORY));
  texts.base['clients/web/src/ux-demo/gone-demo.tsx'] = '<CatalogExample/>';
  assert.ok(run(['clients/web/src/ux-demo/gone-demo.tsx']).demoTitles.includes(INVENTORY));
  // An edit that keeps the count, an aggregator, a test file and non-demo code add nothing.
  for (const path of [
    'clients/web/src/ux-demo/same-demo.tsx',
    'clients/web/src/ux-demo/catalog.ts',
    'clients/web/src/ux-demo/list-demo.test.ts',
    'clients/web/src/components/list.tsx',
  ])
    assert.equal(run([path]), empty, path);
  // A whole-file demo selection already runs the inventory.
  const whole = { ...empty, files: ['tests/ux-demo.spec.ts'], demoTitles: [] };
  assert.equal(run(['clients/web/src/ux-demo/list-demo.tsx'], whole), whole);
});

test('HS2-B42XMS planning selects the inventory when a single-word demo entry gains a specimen', (t) => {
  const f = fixture(t);
  const demo = (count) =>
    [
      "import { CatalogExample } from '@kerfjs/ui';",
      'export const demoModule = {',
      '  entries: {',
      '    list: { render: () => null },',
      '  },',
      '};',
      ...Array.from(
        { length: count },
        (_, index) => `export const Specimen${index} = <CatalogExample label="s${index}" />;`,
      ),
      '',
    ].join('\n');
  f.put('clients/web/src/ux-demo/list-demo.tsx', demo(2));
  f.put(
    'clients/web/tests/ux-demo.spec.ts',
    [
      "import { test } from '@playwright/test';",
      `test('${INVENTORY}', async () => {`,
      "  const examples = [['list', 2]];",
      '});',
      "test('opens the list entry', async ({ page }) => {",
      "  await page.goto('/ux-demo?component=list');",
      '});',
      '',
    ].join('\n'),
  );
  f.git('add', '.');
  f.git('commit', '--quiet', '-m', 'demo base');
  f.git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  const demoStage = (p) => p.checks.find((stage) => stage.command.includes("'tests/ux-demo.spec.ts' --grep"));
  // An edit that keeps the specimen count runs only the tests that open the entry.
  f.put('clients/web/src/ux-demo/list-demo.tsx', demo(2).replace('s0', 'renamed'));
  f.git('commit', '--quiet', '-am', 'same count');
  const same = prepare(f.repo, undefined, false, f.options.versions);
  assert.ok(demoStage(same), same.checks.map((stage) => stage.command).join('\n'));
  assert.ok(!demoStage(same).command.includes('CatalogExample'));
  // Adding a specimen also runs the inventory that pins each entry's count.
  f.put('clients/web/src/ux-demo/list-demo.tsx', demo(3));
  f.git('commit', '--quiet', '-am', 'one more specimen');
  const added = prepare(f.repo, undefined, false, f.options.versions);
  assert.ok(demoStage(added).command.includes('Kerf CatalogExample'), demoStage(added).command);
  assert.ok(demoStage(added).command.includes('opens the list entry'));
  assert.ok(added.checks.every((stage) => !/'tests\/ux-demo\.spec\.ts'(?! --grep)/.test(stage.command)));
});
