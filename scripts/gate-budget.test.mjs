import assert from 'node:assert/strict';
import test from 'node:test';
import { parseBudget, selectBudget, stageId } from './gate-budget.mjs';

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

test('HS2-GN06JT fallback stays full and unrelated stages remain omitted otherwise', () => {
  const omitted = stage('migrator', 'npm run test:coverage', 'migrator', false);
  const selected = choose({ stages: [...all, omitted], full: true, budgetMs: 50 });
  assert.equal(selected.stages.find((item) => item.command === omitted.command).selected, false);
  assert.equal(selected.selected.length, all.length);
  assert.equal(selected.floorOverBudget, false);
});
