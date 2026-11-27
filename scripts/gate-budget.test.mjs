import assert from 'node:assert/strict';
import test from 'node:test';
import {
  highImpactBrowserChange,
  parseBudget,
  selectBudget,
  stageId,
  staticCheckStage,
} from './gate-budget.mjs';
import {
  BROWSER_COMMAND_OVERHEAD_MS,
  BROWSER_RANK,
  capBrowserUnits,
  caseEstimates,
  countDeclaredTests,
  KEPT_EXTRA_MS,
  unitEstimate,
} from './gate-browser-budget.mjs';

test('HS2-RRNZYN identifies auth, claims, sync and shared-state boundaries for browser proof', () => {
  for (const path of [
    'crates/hotsheet-server/src/auth.rs',
    'crates/hotsheet-ticketing/src/claim.rs',
    'crates/hotsheet-ticketing/src/sync.rs',
    'crates/hotsheet-cli/src/dispatch/claims.rs',
    'crates/hotsheet-cli/src/claim_process.rs',
    'crates/hotsheet-aitools/src/session_push/mod.rs',
    'clients/web/src/features/shared-state.ts',
    'clients/web/src/features/shared_state.ts',
  ])
    assert.equal(highImpactBrowserChange([path]), true, path);
  assert.equal(highImpactBrowserChange(['docs/README.md']), false);
  assert.equal(highImpactBrowserChange(['migrator/src/export.mjs']), false, 'migrator keeps its direct suite');
});

const stage = (cwd, command, group, selected = true) => ({ cwd, command, group, selected, reasons: ['affected'] });
const format = stage('.', 'cargo fmt --all --check', 'always');
const lint = stage('clients/web', 'npm run lint', 'always');
const typecheck = stage('clients/web', 'npm run typecheck', 'web');
const unit = stage('clients/web', 'npm run test:unit', 'web');
const build = stage('clients/web', 'npm run build', 'web');
const fixtures = stage('clients/web', 'npm run test:fixtures', 'web');
const browser = stage('clients/web', "npx playwright test 'tests/contracts.spec.ts'", 'web');
const all = [format, lint, typecheck, unit, build, fixtures, browser];
const history = {
  stages: Object.fromEntries(
    all.map((item, index) => [
      `${item.cwd}: ${item.command}`,
      [{ ms: [10, 80, 20, 100, 30, 5, 40][index], host: 'local' }],
    ]),
  ),
};
const choose = (overrides = {}) =>
  selectBudget({ stages: all, changes: ['clients/web/src/app.ts'], history, budgetMs: 300, ...overrides });

test('HS2-GN06JT parses bounded positive durations', () => {
  assert.equal(parseBudget('5m'), 300000);
  assert.equal(parseBudget('300s'), 300000);
  assert.equal(parseBudget('1h'), 3600000);
  for (const invalid of ['0s', '-1m', '5', 'huge', '999999999999999999999h'])
    assert.throws(() => parseBudget(invalid), /Budget/);
});

test('HS2-GN06JT mandatory floor runs even over budget and reports it', () => {
  const result = choose({ budgetMs: 50 });
  assert.equal(result.floorOverBudget, true);
  assert.equal(result.floorEstimateMs, 255);
  for (const item of [format, lint, typecheck, unit, browser, fixtures])
    assert.ok(
      result.selected.some((chosen) => stageId(chosen) === stageId(item)),
      item.command,
    );
  assert.deepEqual(
    result.deferred.map((item) => item.command),
    ['npm run build'],
  );
});

test('HS2-3ZJBRM web lint remains mandatory when its declared inputs change', () => {
  const scoped = stage('clients/web', 'npm run lint:web', 'web_lint');
  const result = selectBudget({
    stages: [scoped],
    changes: ['clients/web/src/app.ts'],
    history: { stages: {} },
    budgetMs: null,
    tier: 'T2',
  });
  assert.equal(result.selected[0]?.command, scoped.command);
  assert.equal(result.selected[0]?.budgetReason, 'mandatory floor');
});

test('HS2-V3R4JR the Windows cfg clippy pass stays in the floor like cargo lint', () => {
  const rust = [
    stage('.', 'cargo lint', 'rust'),
    stage('.', 'node scripts/windows-cfg-check.mjs', 'rust'),
    stage('.', 'cargo doc --workspace', 'rust'),
  ];
  const result = selectBudget({ stages: rust, changes: ['crates/x/src/lib.rs'], history: { stages: {} }, budgetMs: 1 });
  assert.deepEqual(
    result.selected.filter((item) => item.budgetReason === 'mandatory floor').map((item) => item.command),
    ['cargo lint', 'node scripts/windows-cfg-check.mjs'],
  );
});

test('HS2-GN06JT public assets and contracts keep their direct suites in the floor', () => {
  for (const path of ['clients/web/public/favicon-32.png', 'docs/contracts/api.json']) {
    const result = choose({ changes: [path], budgetMs: 1 });
    assert.ok(
      result.selected.some((item) => item.command === unit.command),
      path,
    );
    assert.ok(
      result.selected.some((item) => item.command === browser.command),
      path,
    );
    assert.equal(result.floorOverBudget, true);
  }
  const rust = stage('.', 'cargo nextest run --all-features --status-level leak', 'rust');
  const assets = choose({
    stages: [...all, rust],
    changes: ['clients/web/public/favicon-32.png'],
    budgetMs: 1,
    history: {
      stages: {
        ...history.stages,
        ['.: cargo nextest run --all-features --status-level leak']: [{ ms: 100, host: 'local' }],
      },
    },
  });
  assert.ok(assets.selected.some((item) => item.command === rust.command));
});

test('HS2-GN06JT relevance fills measured headroom and defers unknown cost to CI', () => {
  const docs = choose({ changes: ['docs/06-clients.md'], budgetMs: 220 });
  assert.ok(docs.selected.some((item) => item.command === 'npm run test:unit'));
  assert.ok(!docs.selected.some((item) => item.command.startsWith('npx playwright')));
  assert.ok(docs.deferred.some((item) => item.command.startsWith('npx playwright')));
  const unknown = choose({
    changes: ['docs/06-clients.md'],
    history: {
      stages: {
        ...history.stages,
        ['clients/web: npm run test:unit']: [],
      },
    },
    budgetMs: 1000,
  });
  assert.ok(
    unknown.deferred.some(
      (item) => item.command === 'npm run test:unit' && item.budgetReason === 'unknown cost deferred to CI',
    ),
  );
  const ciOnly = choose({
    changes: ['docs/06-clients.md'],
    history: {
      stages: {
        ...history.stages,
        ['clients/web: npm run test:unit']: [{ ms: 1, host: 'ci' }],
      },
    },
    budgetMs: 1000,
  });
  assert.ok(ciOnly.deferred.some((item) => item.command === 'npm run test:unit' && item.estimateHost === 'ci'));
});

test('HS2-GN06JT T2 analyzed selection has no budget and never widens by cost', () => {
  const result = choose({ changes: ['docs/06-clients.md'], tier: 'T2', budgetMs: null, analyzed: [stageId(browser)] });
  assert.ok(result.selected.some((item) => item.command === browser.command));
  assert.ok(result.selected.some((item) => item.command === fixtures.command));
  assert.ok(!result.selected.some((item) => item.command === unit.command));
  assert.equal(result.budgetMs, null);
  assert.throws(() => choose({ tier: 'T2', budgetMs: null, analyzed: ['unknown'] }), /Unknown or unaffected/);
  assert.throws(() => choose({ tier: 'T2', budgetMs: null }), /explicit analyzed/);
  assert.throws(() => choose({ analyzed: [stageId(browser)] }), /requires T2/);
});

test('HS2-ARJNJV proposed checks add only affected registered stages without dropping the floor', () => {
  const result = choose({ changes: ['docs/06-clients.md'], budgetMs: 1, proposed: [stageId(browser)] });
  assert.ok(
    result.selected.some(
      (item) => item.command === browser.command && item.budgetReason === 'AI-proposed additional check',
    ),
  );
  assert.ok(result.selected.some((item) => item.command === fixtures.command));
  assert.ok(result.selected.some((item) => item.command === format.command));
  assert.throws(() => choose({ proposed: ['.: rm -rf /'] }), /Unknown or unaffected proposed stage/);
  assert.throws(
    () =>
      choose({
        stages: [...all, stage('migrator', 'npm run test:coverage', 'migrator', false)],
        proposed: ['migrator::npm run test:coverage'],
      }),
    /Unknown or unaffected proposed stage/,
  );
  const t2 = choose({ changes: ['docs/06-clients.md'], tier: 'T2', budgetMs: null, proposed: [stageId(browser)] });
  assert.ok(t2.selected.some((item) => item.command === browser.command));
});

test('HS2-GN06JT fallback stays full and unrelated stages remain omitted otherwise', () => {
  const omitted = stage('migrator', 'npm run test:coverage', 'migrator', false);
  const selected = choose({ stages: [...all, omitted], full: true, budgetMs: 50 });
  assert.equal(selected.stages.find((item) => item.command === omitted.command).selected, false);
  assert.equal(selected.selected.length, all.length);
  assert.equal(selected.floorOverBudget, false);
});

test('HS2-ZGS7JP best effort budgets T2 like T1, takes analyzed stages as requests, and names its deferrals', () => {
  const docs = ['docs/06-clients.md'];
  const result = choose({ changes: docs, tier: 'T2', budgetMs: 220, bestEffort: true });
  assert.equal(result.bestEffort, true);
  assert.equal(result.tier, 'T2', 'the ticket tier is still reported');
  assert.equal(result.budgetMs, 220);
  assert.ok(result.selected.some((item) => item.command === unit.command), 'relevance fills the budget');
  const deferredBrowser = result.deferred.find((item) => item.command === browser.command);
  assert.equal(deferredBrowser?.budgetReason, 'deferred to CI by best-effort budget');
  const requested = choose({
    changes: docs,
    tier: 'T2',
    budgetMs: 220,
    bestEffort: true,
    analyzed: [stageId(browser)],
  });
  assert.ok(requested.selected.some((item) => item.command === browser.command));
  assert.equal(
    requested.selected.find((item) => item.command === browser.command).budgetReason,
    'AI-proposed additional check',
  );
  assert.throws(() => choose({ tier: 'T2', budgetMs: null, bestEffort: true }), /positive budget/);
  assert.throws(() => choose({ tier: 'T1', budgetMs: 0, bestEffort: true }), /positive budget/);
  const over = choose({ changes: ['clients/web/src/app.ts'], tier: 'T2', budgetMs: 1, bestEffort: true });
  assert.equal(over.floorOverBudget, true, 'the floor is planned and the gate cuts it at run time');
});

// HS2-FXY4M2: CI First browser cases after the mandatory floor, from synthetic timing tables. The
// changed spec (rank 0) and the cases that exercise the changed module (rank 2) are kept even over
// budget, within KEPT_EXTRA_MS; every other rank fills only the budget left after the floor.
const pw = (command, browser, extra = {}) => ({
  cwd: 'clients/web',
  command,
  group: 'web',
  selected: true,
  reasons: ['affected'],
  browser,
  ...extra,
});
const wholeFiles = pw("npx playwright test 'tests/a.spec.ts' 'tests/b.spec.ts' 'tests/c.spec.ts'", {
  files: [
    { file: 'tests/a.spec.ts', rank: BROWSER_RANK.baseline },
    { file: 'tests/b.spec.ts', rank: BROWSER_RANK.exercised },
    { file: 'tests/c.spec.ts', rank: BROWSER_RANK.heuristic },
  ],
});
const demoGrep = pw("npx playwright test 'tests/demo.spec.ts' --grep 'one|two|three|four'", {
  file: 'tests/demo.spec.ts',
  alternatives: ['one', 'two', 'three', 'four'].map((pattern) => ({ pattern, rank: BROWSER_RANK.exercised })),
});
const regressionGrep = pw("npx playwright test 'tests/reg.spec.ts' --grep 'guard'", {
  file: 'tests/reg.spec.ts',
  alternatives: [{ pattern: 'guard', rank: BROWSER_RANK.regression }],
});
const changedGrep = pw("npx playwright test 'tests/changed.spec.ts' --grep 'edited case'", {
  file: 'tests/changed.spec.ts',
  alternatives: [{ pattern: 'edited case', rank: BROWSER_RANK.changed }],
});
const floorStages = [
  stage('clients/web', 'npm run lint', 'always'),
  stage('clients/web', 'npm run typecheck', 'web'),
  stage('clients/web', 'npm run test:unit', 'web'),
  stage('clients/web', 'npm run test:fixtures', 'web'),
];
const browserHistory = {
  stages: {
    'clients/web: npm run lint': [{ ms: 100_000, host: 'local' }],
    'clients/web: npm run typecheck': [{ ms: 50_000, host: 'local' }],
    'clients/web: npm run test:unit': [{ ms: 100_000, host: 'local' }],
    'clients/web: npm run test:fixtures': [{ ms: 5_000, host: 'local' }],
  },
  files: {
    'playwright:a.spec.ts': [{ ms: 60_000, host: 'local' }],
    'playwright:b.spec.ts': [{ ms: 40_000, host: 'local' }],
    'playwright:c.spec.ts': [{ ms: 30_000, host: 'local' }],
    'playwright:demo.spec.ts': [{ ms: 200_000, host: 'local' }],
    'playwright:reg.spec.ts': [{ ms: 20_000, host: 'local' }],
    'playwright:changed.spec.ts': [{ ms: 30_000, host: 'local' }],
  },
};
const browserCaseCounts = { 'tests/demo.spec.ts': 10, 'tests/changed.spec.ts': 3, 'tests/reg.spec.ts': 1 };
const chooseBrowser = (overrides = {}) =>
  selectBudget({
    stages: [...floorStages, wholeFiles, demoGrep, regressionGrep, changedGrep],
    changes: ['clients/web/src/app.ts'],
    history: browserHistory,
    budgetMs: 600_000,
    bestEffort: true,
    browserCaseCounts,
    ...overrides,
  });
const playwrightCommands = (stages) =>
  stages.filter((item) => item.command.startsWith('npx playwright')).map((item) => item.command);
const KEPT_REASON = 'HS2-FXY4M2: cases that exercise the changed module are kept, even over budget';

test('HS2-FXY4M2 under budget runs every browser case and defers nothing', () => {
  // The 250 s floor leaves 350 s. Changed 20 s (with the fixture build), kept demo 85 s and
  // b.spec.ts 45 s, then reg 25 s, a.spec.ts 65 s and c.spec.ts 30 s: 270 s in all.
  const result = chooseBrowser();
  assert.deepEqual(playwrightCommands(result.selected), [
    changedGrep.command,
    "npx playwright test 'tests/b.spec.ts'",
    demoGrep.command,
    regressionGrep.command,
    "npx playwright test 'tests/a.spec.ts' 'tests/c.spec.ts'",
  ]);
  assert.deepEqual(result.deferred, []);
  assert.deepEqual(
    [result.browserBudget.mandatory, result.browserBudget.kept, result.browserBudget.selected],
    [1, 5, 3],
  );
  assert.equal(result.browserBudget.keptOverBudgetMs, 0);
  assert.deepEqual(result.browserBudget.keptOverBudgetCases, []);
  assert.equal(result.browserBudget.plannedMs, 270_000);
  assert.ok(result.selected.some((item) => item.command === 'npm run test:fixtures'));
  assert.equal(result.floorOverBudget, false);
});

test('HS2-FXY4M2 over budget keeps changed-module cases and defers the other ranks by name', () => {
  // The 250 s floor of 380 s leaves 130 s. The changed case and the kept cases plan 150 s, so the
  // last kept file (b.spec.ts, 40 s plus its command) is kept 20 s over budget. Nothing else fits.
  const result = chooseBrowser({ budgetMs: 380_000 });
  assert.deepEqual(playwrightCommands(result.selected), [
    changedGrep.command,
    "npx playwright test 'tests/b.spec.ts'",
    demoGrep.command,
  ]);
  assert.equal(result.selected.find((item) => item.command === demoGrep.command).budgetReason, KEPT_REASON);
  assert.ok(result.selected.filter((item) => item.command.startsWith('npx playwright')).every((item) => item.floor));
  assert.deepEqual(playwrightCommands(result.deferred), [
    regressionGrep.command,
    "npx playwright test 'tests/a.spec.ts' 'tests/c.spec.ts'",
  ]);
  for (const item of result.deferred)
    assert.equal(item.budgetReason, 'deferred to CI by best-effort browser budget', item.command);
  assert.deepEqual(result.browserBudget.deferredCases, ['tests/reg.spec.ts --grep guard', 'tests/a.spec.ts', 'tests/c.spec.ts']);
  assert.equal(result.browserBudget.keptOverBudgetMs, 20_000);
  assert.deepEqual(result.browserBudget.keptOverBudgetCases, ['tests/b.spec.ts']);
  assert.equal(result.floorEstimateMs, 250_000 + 150_000);
  assert.equal(result.floorOverBudget, true, 'the kept cases count toward the floor');
});

test('HS2-FXY4M2 a floor over budget still runs the changed spec and the changed-module cases', () => {
  const result = chooseBrowser({ budgetMs: 200_000 });
  for (const item of floorStages)
    assert.ok(
      result.selected.some((chosen) => chosen.command === item.command),
      item.command,
    );
  assert.deepEqual(playwrightCommands(result.selected), [
    changedGrep.command,
    "npx playwright test 'tests/b.spec.ts'",
    demoGrep.command,
  ]);
  assert.deepEqual(playwrightCommands(result.deferred), [
    regressionGrep.command,
    "npx playwright test 'tests/a.spec.ts' 'tests/c.spec.ts'",
  ]);
  assert.equal(result.floorOverBudget, true);
  assert.equal(result.floorEstimateMs, 250_000 + 150_000, 'the floor counts the changed and kept cases');
  assert.equal(result.browserBudget.keptOverBudgetMs, 150_000);
  assert.equal(result.browserBudget.keptOverBudgetCases.length, 6, 'every kept case is listed as over budget');
  assert.deepEqual([result.browserBudget.selected, result.browserBudget.deferred], [0, 3]);
});

test('HS2-FXY4M2 the kept-case cap trims a large changed-module set cheapest first, deterministically', () => {
  assert.equal(KEPT_EXTRA_MS, 180_000);
  const unit = (file, estimateMs, order) => ({
    file,
    grep: `case ${order}`,
    rank: BROWSER_RANK.exercised,
    changed: false,
    kept: true,
    commandKey: 'kept',
    estimateMs,
  });
  const units = [
    unit('tests/x.spec.ts', 90_000, 1),
    unit('tests/x.spec.ts', 10_000, 2),
    unit('tests/y.spec.ts', 50_000, 3),
    unit('tests/x.spec.ts', 30_000, 4),
    unit('tests/y.spec.ts', 30_000, 5),
    unit('tests/z.spec.ts', 70_000, 6),
  ];
  const decide = (input) =>
    capBrowserUnits({ units: input, remainingMs: -40_000, keptExtraMs: 100_000 })
      .units.map((item) => `${item.grep}:${item.decision}`)
      .sort();
  // Limit 100 s past the budget: 10 s + 5 s overhead, 30 s (x before y), 30 s, then 50 s would end at 125 s.
  const expected = [
    'case 1:trimmed',
    'case 2:kept',
    'case 3:trimmed',
    'case 4:kept',
    'case 5:kept',
    'case 6:trimmed',
  ];
  assert.deepEqual(decide(units), expected);
  assert.deepEqual(decide([...units].reverse()), expected, 'input order does not change the cut');
  // Through selectBudget, a large exercised spec past the cap is deferred by name with its own reason.
  const big = pw("npx playwright test 'tests/big.spec.ts' --grep 'b1|b2|b3|b4|b5|b6'", {
    file: 'tests/big.spec.ts',
    alternatives: ['b1', 'b2', 'b3', 'b4', 'b5', 'b6'].map((pattern) => ({ pattern, rank: BROWSER_RANK.exercised })),
  });
  const result = chooseBrowser({
    stages: [...floorStages, demoGrep, big, changedGrep],
    budgetMs: 200_000,
    history: { ...browserHistory, files: { ...browserHistory.files, 'playwright:big.spec.ts': [{ ms: 360_000, host: 'local' }] } },
    browserCaseCounts: { ...browserCaseCounts, 'tests/big.spec.ts': 6 },
  });
  // Limit 20 s + 180 s: changed 20 s, demo 85 s, b1 65 s (with its command) ends at 170 s; b2 would end at 230 s.
  assert.deepEqual(playwrightCommands(result.selected), [
    changedGrep.command,
    "npx playwright test 'tests/big.spec.ts' --grep 'b1'",
    demoGrep.command,
  ]);
  assert.deepEqual(playwrightCommands(result.deferred), ["npx playwright test 'tests/big.spec.ts' --grep 'b2|b3|b4|b5|b6'"]);
  assert.equal(result.deferred[0].budgetReason, 'deferred to CI by the kept-case cap');
  assert.equal(result.browserBudget.trimmed, 5);
  assert.deepEqual(result.browserBudget.deferredCases, ['b2', 'b3', 'b4', 'b5', 'b6'].map((name) => `tests/big.spec.ts --grep ${name}`));
});

test('HS2-FXY4M2 browser selection is deterministic and independent of stage order', () => {
  const first = chooseBrowser({ budgetMs: 380_000 });
  assert.deepEqual(chooseBrowser({ budgetMs: 380_000 }), first);
  const reordered = chooseBrowser({
    stages: [changedGrep, regressionGrep, demoGrep, wholeFiles, ...floorStages],
    budgetMs: 380_000,
  });
  assert.deepEqual(playwrightCommands(reordered.selected), playwrightCommands(first.selected));
  assert.deepEqual(playwrightCommands(reordered.deferred), playwrightCommands(first.deferred));
  assert.deepEqual(reordered.browserBudget, first.browserBudget);
});

test('HS2-FXY4M2 a changed spec is always selected first, even when nothing else fits', () => {
  for (const budgetMs of [1, 260_000, 380_000, 600_000]) {
    const result = chooseBrowser({ budgetMs });
    assert.equal(playwrightCommands(result.selected)[0], changedGrep.command, `budget ${budgetMs}`);
    assert.equal(
      result.selected.find((item) => item.command === changedGrep.command).budgetReason,
      'HS2-FXY4M2: changed spec tests always run first',
    );
  }
  // A changed whole file inside a shared stage splits out ahead of the stage's other files.
  const mixed = pw("npx playwright test 'tests/a.spec.ts' 'tests/new.spec.ts'", {
    files: [
      { file: 'tests/a.spec.ts', rank: BROWSER_RANK.baseline },
      { file: 'tests/new.spec.ts', rank: BROWSER_RANK.changed },
    ],
  });
  const split = chooseBrowser({ stages: [...floorStages, mixed], budgetMs: 1 });
  assert.deepEqual(playwrightCommands(split.selected), ["npx playwright test 'tests/new.spec.ts'"]);
  assert.deepEqual(playwrightCommands(split.deferred), ["npx playwright test 'tests/a.spec.ts'"]);
});

test('HS2-FXY4M2 unknown, proposed, forced and non-best-effort browser stages keep their rules', () => {
  const unknown = chooseBrowser({ history: { ...browserHistory, files: {} } });
  assert.deepEqual(playwrightCommands(unknown.selected), [changedGrep.command], 'a changed spec runs unsampled');
  assert.ok(unknown.deferred.every((item) => item.budgetReason === 'unknown cost deferred to CI'));
  // An explicitly proposed browser stage is not capped: it runs whole, as before HS2-FXY4M2.
  const proposed = chooseBrowser({ budgetMs: 200_000, proposed: [stageId(wholeFiles)] });
  assert.ok(proposed.selected.some((item) => item.command === wholeFiles.command));
  assert.ok(!proposed.deferred.some((item) => item.command.startsWith("npx playwright test 'tests/a.spec.ts'")));
  const forced = { ...wholeFiles, force: true };
  const kept = chooseBrowser({ stages: [...floorStages, forced], budgetMs: 1 });
  assert.equal(kept.selected.find((item) => item.command === forced.command)?.budgetReason, 'mandatory floor');
  const budgeted = chooseBrowser({ bestEffort: false, budgetMs: 1 });
  assert.equal(budgeted.browserBudget, null, 'only a best-effort strategy caps browser cases');
  assert.ok(budgeted.selected.some((item) => item.command === demoGrep.command));
});

test('HS2-FXY4M2 per-case estimates use recorded case timings, else the declared case share', () => {
  assert.equal(countDeclaredTests("test('a', () => {});\ntest.skip(`b`, f);\n  test.describe('c');\nmytest('d')"), 2);
  const unit = { file: 'tests/demo.spec.ts', grep: 'one' };
  assert.equal(unitEstimate(unit, { history: browserHistory, caseCounts: browserCaseCounts }).ms, 20_000);
  const observed = {
    ...browserHistory,
    attempts: [
      {
        cases: [
          { id: 'playwright:demo.spec.ts#0123abcd', outcome: 'pass', ms: 3_000 },
          { id: 'playwright:demo.spec.ts#4567abcd', outcome: 'pass', ms: 4_000 },
          { id: 'playwright:demo.spec.ts#89abcdef', outcome: 'fail', ms: 90_000 },
          { id: 'nextest:hotsheet-core x', outcome: 'pass', ms: 9 },
        ],
      },
    ],
  };
  assert.deepEqual([...caseEstimates(observed)], [['demo.spec.ts', 4_000]]);
  assert.equal(unitEstimate(unit, { history: observed, caseCounts: browserCaseCounts }).ms, 4_000);
  assert.equal(unitEstimate({ file: 'tests/a.spec.ts', grep: null }, { history: observed }).ms, 60_000);
  assert.equal(unitEstimate({ file: 'tests/missing.spec.ts', grep: 'x' }, { history: observed }).ms, null);
  assert.equal(BROWSER_COMMAND_OVERHEAD_MS, 5_000);
});

test('hs2_c8pwwv_static_checks_are_classified_and_ordered_before_suites', () => {
  for (const [cwd, command, group] of [
    ['.', 'cargo fmt --all --check', 'always'],
    ['.', 'cargo lint', 'rust'],
    ['.', 'node scripts/windows-cfg-check.mjs', 'rust'],
    ['clients/web', 'npm run format:sources:check', 'always'],
    ['clients/web', 'npm run lint:web', 'web_lint'],
    ['clients/web', 'npm run typecheck', 'web'],
    ['migrator', 'npm run lint', 'migrator'],
    ['spikes/kerf-webawesome', 'npm run lint', 'spike'],
    ['spikes/kerf-webawesome', 'npm run typecheck', 'spike'],
  ])
    assert.equal(staticCheckStage({ cwd, command, group }), true, `${cwd} ${command}`);
  for (const [cwd, command, group] of [
    ['.', 'cargo nextest run --all-features --status-level leak', 'rust'],
    ['clients/web', 'npm run test:unit', 'web'],
    ['clients/web', 'npm run build', 'web'],
    ['migrator', 'npm run test:coverage', 'migrator'],
    ['clients/web', "npx playwright test 'tests/providers.spec.ts'", 'web'],
  ])
    assert.equal(staticCheckStage({ cwd, command, group }), false, `${cwd} ${command}`);

  // A suite listed before web lint in scripts/check-full.sh still runs after it, and a full gate
  // keeps every static check ahead of every suite in its original relative order.
  const nextest = stage('.', 'cargo nextest run --all-features --status-level leak', 'rust');
  const webLint = stage('clients/web', 'npm run lint:web', 'web_lint');
  const ordered = [format, nextest, webLint, typecheck, unit, build];
  for (const full of [false, true]) {
    const result = selectBudget({ stages: ordered, changes: [], history, budgetMs: 300_000, full });
    assert.deepEqual(
      result.stages.map(stageId),
      [format, webLint, typecheck, nextest, unit, build].map(stageId),
      `full=${full}`,
    );
  }
});
