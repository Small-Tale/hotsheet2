import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import {
  chooseTicket,
  developmentStrategyFromSettings,
  profileFromTags,
  resolveConfiguredDevelopmentStrategy,
  withDevelopmentStrategy,
} from './gate-ticket-profile.mjs';

const ticket = (slug, tags = [], claimed_by = null, connection_id = 'git') => ({
  slug,
  tags,
  claimed_by,
  connection_id,
  native_id: slug,
  qualified_id: `${connection_id}:${slug}`,
});

test('HS2-BF79YX unset policy preserves existing local selection', () => {
  const profile = profileFromTags(ticket('HS2-A', ['tier:T1']));
  assert.equal(withDevelopmentStrategy(profile, {}, undefined), profile);
  assert.equal(developmentStrategyFromSettings({}), null);
  assert.equal(developmentStrategyFromSettings({ development_strategy: { version: 1 } }).preset, 'ticket-assured');
});

test('HS2-RRNZYN configured strategy uses the CLI resolver and fails when unavailable', () => {
  const profile = profileFromTags(ticket('HS2-RRNZYN', ['tier:T1', 'development-strategy:steady-main']));
  const settings = { development_strategy: { version: 2, default: 'ticket-assured' } };
  const resolved = resolveConfiguredDevelopmentStrategy('/synthetic/project', profile, settings, (_binary, args) => {
    assert.ok(args.includes('development-strategy-resolve'));
    assert.deepEqual(args.slice(-2), ['--ticket', 'steady-main']);
    return JSON.stringify({
      preset: 'steady-main',
      source: 'ticket',
      integration: 'ci-staged',
      local_gate: 'targeted',
    });
  });
  assert.equal(resolved.developmentStrategy.preset, 'steady-main');
  assert.equal(resolved.deep, false);
  assert.throws(
    () =>
      resolveConfiguredDevelopmentStrategy('/synthetic/project', profile, settings, () => {
        throw new Error('unrecognized subcommand development-strategy-resolve');
      }),
    /requires a current hotsheet-cli development-strategy-resolve/,
  );
  const unconfigured = resolveConfiguredDevelopmentStrategy(
    '/synthetic/project',
    profileFromTags(ticket('HS2-RRNZYN')),
    {},
    () => {
      throw new Error('CLI must not run for unconfigured projects');
    },
  );
  assert.equal(unconfigured.developmentStrategy, undefined);
});

test('HS2-RRNZYN worktree locator lets CLI resolve an external store from cwd', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'hs2-rrnzyn-locator-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const repo = join(root, 'worktree');
  const store = join(root, 'external.hs2');
  mkdirSync(join(repo, '.hotsheet2'), { recursive: true });
  mkdirSync(store);
  writeFileSync(join(repo, '.hotsheet2/store'), `${store}\n`);
  const settings = { development_strategy: { version: 2, default: 'ticket-assured' } };
  const profile = profileFromTags(ticket('HS2-RRNZYN', ['tier:T1']));
  const resolved = resolveConfiguredDevelopmentStrategy(repo, profile, settings, (_binary, args, options) => {
    assert.deepEqual(args.slice(0, 4), ['coordination', '--project', repo, 'development-strategy-resolve']);
    assert.equal(options.cwd, repo);
    assert.equal(readFileSync(join(repo, '.hotsheet2/store'), 'utf8').trim(), store);
    return JSON.stringify({
      preset: 'ticket-assured',
      source: 'project',
      integration: 'serial',
      local_gate: 'full-local',
    });
  });
  assert.equal(resolved.deep, true);
});

test('HS2-BF79YX maps presets and ticket/session/project precedence', () => {
  const settings = {
    development_strategy: { version: 1, default: 'steady-main', green_main_goal_minutes: 360 },
    integration_strategies: { allowed: [{ strategy: 'ci-staged', reason: 'green main' }] },
  };
  const profile = profileFromTags(ticket('HS2-A', ['tier:T1']));
  assert.deepEqual(developmentStrategyFromSettings(settings), {
    preset: 'steady-main',
    source: 'project',
    integration: 'ci-staged',
  });
  assert.deepEqual(developmentStrategyFromSettings(settings, { session: 'risk-guided' }), {
    preset: 'risk-guided',
    source: 'session',
    integration: 'ci-staged',
  });
  const ticketChoice = withDevelopmentStrategy(
    profileFromTags(ticket('HS2-A', ['development-strategy:ticket-assured'])),
    settings,
    'risk-guided',
  );
  assert.deepEqual(ticketChoice.developmentStrategy, {
    preset: 'ticket-assured',
    source: 'ticket',
    integration: 'serial',
  });
  assert.equal(ticketChoice.deep, true);
  assert.equal(withDevelopmentStrategy(profile, settings, undefined).deep, false);
});

test('HS2-BF79YX rejects invalid or unauthorized gate policy', () => {
  assert.throws(
    () => developmentStrategyFromSettings({ development_strategy: { version: 1, extra: true } }),
    /Invalid versioned/,
  );
  assert.throws(
    () => developmentStrategyFromSettings({ development_strategy: { version: 3, default: 'ticket-assured' } }),
    /Invalid versioned/,
  );
  assert.throws(
    () => developmentStrategyFromSettings({ development_strategy: { version: 1, default: 'risk-guided' } }),
    /conflicts with project integration policy/,
  );
  assert.throws(
    () =>
      developmentStrategyFromSettings({
        development_strategy: { version: 1, default: 'ticket-assured', green_main_goal_minutes: 0 },
      }),
    /green-main goal/,
  );
  assert.throws(
    () =>
      withDevelopmentStrategy(
        profileFromTags(ticket('HS2-A', ['development-strategy:risk-guided', 'development-strategy:steady-main'])),
        {},
        undefined,
      ),
    /Conflicting/,
  );
});

const fastCustom = {
  integration: 'ci-staged',
  local_gate: 'targeted',
  minimum_tier: 'T1',
  budget_minutes: 5,
  browser: false,
  platform: false,
  perf: false,
  test_plan_timing: 'before-handoff',
  ci_gate: 'CI',
  staging_branch: 'integration',
  green_main_goal_minutes: 180,
  red_ci_recovery: 'fallback-to-serial',
};
const strictCustom = {
  integration: 'serial',
  local_gate: 'full-local',
  minimum_tier: 'T2',
  browser: true,
  platform: true,
  perf: false,
  test_plan_timing: 'before-work',
  ci_gate: 'CI',
  red_ci_recovery: 'attribute-and-retry',
};
const customSettings = {
  development_strategy: {
    version: 2,
    default: 'ticket-assured',
    default_custom: 'fast',
    custom: { fast: fastCustom, strict: strictCustom },
  },
  integration_strategies: { allowed: [{ strategy: 'ci-staged', staging_branch: 'integration', reason: 'green main' }] },
};

test('HS2-ARJNJV named custom strategies raise floors and preserve precedence and rollback', () => {
  assert.equal(developmentStrategyFromSettings(customSettings).preset, 'fast');
  const selected = withDevelopmentStrategy(profileFromTags(ticket('HS2-A', ['tier:T0'])), customSettings, undefined);
  assert.equal(selected.tier, 'T1');
  assert.equal(selected.budgetMs, 300_000);
  assert.equal(selected.deep, false);
  const strict = withDevelopmentStrategy(
    profileFromTags(ticket('HS2-A', ['tier:T1', 'development-strategy:strict'])),
    customSettings,
    'fast',
  );
  assert.equal(strict.developmentStrategy.source, 'ticket');
  assert.equal(strict.tier, 'T2');
  assert.equal(strict.deep, true);
  assert.equal(strict.browser, true);
  assert.equal(strict.platform, true);
  assert.equal(developmentStrategyFromSettings(customSettings, { ticket: 'risk-guided' }).preset, 'risk-guided');
  const rollback = {
    ...customSettings,
    development_strategy: {
      ...customSettings.development_strategy,
      default_custom: undefined,
      default: 'ticket-assured',
    },
  };
  assert.equal(developmentStrategyFromSettings(rollback).preset, 'ticket-assured');
});

test('HS2-ARJNJV refuses unsupported custom capabilities and invalid constraints', () => {
  const withFast = (fast) => ({
    ...customSettings,
    development_strategy: { ...customSettings.development_strategy, custom: { fast, strict: strictCustom } },
  });
  assert.throws(() => developmentStrategyFromSettings(withFast({ ...fastCustom, ci_gate: '' })), /CI gate/);
  assert.throws(() => developmentStrategyFromSettings(withFast({ ...fastCustom, ci_gate: 'Missing CI' })), /CI gate/);
  assert.throws(
    () => developmentStrategyFromSettings(withFast({ ...fastCustom, red_ci_recovery: 'attribute-and-retry' })),
    /circuit breaker/,
  );
  assert.throws(
    () => developmentStrategyFromSettings(withFast({ ...fastCustom, staging_branch: undefined })),
    /staging branch/,
  );
  assert.throws(() => developmentStrategyFromSettings(withFast({ ...fastCustom, minimum_tier: 'T2' })), /budget a T2/);
  assert.throws(
    () => developmentStrategyFromSettings({ ...customSettings, integration_strategies: { allowed: [] } }),
    /allowed ci-staged/,
  );
  assert.throws(
    () =>
      developmentStrategyFromSettings({
        ...customSettings,
        development_strategy: { ...customSettings.development_strategy, version: 1 },
      }),
    /Version 1/,
  );
  assert.throws(
    () =>
      withDevelopmentStrategy(
        profileFromTags(ticket('HS2-A', ['tier:T1', 'test-budget:10m', 'development-strategy:strict'])),
        customSettings,
        undefined,
      ),
    /T2 custom strategy cannot use/,
  );
});

test('HS2-VQ5ND5 explicit ticket outranks commit and claim; commit outranks claim', () => {
  const tickets = [ticket('HS2-A', [], 'worker'), ticket('HS2-B'), ticket('HS2-C')];
  assert.equal(
    chooseTicket({ explicit: 'HS2-C', subject: 'HS2-B change', workerId: 'worker', tickets }).ticket.slug,
    'HS2-C',
  );
  assert.equal(chooseTicket({ subject: 'HS2-B change', workerId: 'worker', tickets }).ticket.slug, 'HS2-B');
  assert.equal(
    chooseTicket({ subject: 'HS2-B change', dirty: true, workerId: 'worker', tickets }).ticket.slug,
    'HS2-A',
  );
  assert.equal(chooseTicket({ subject: 'HS2-B change', tickets }).ticket.qualified_id, 'git:HS2-B');
});

test('HS2-VQ5ND5 ambiguous or missing ticket references use the default profile', () => {
  const tickets = [ticket('HS2-A', [], 'worker'), ticket('HS2-B', [], 'worker')];
  assert.match(chooseTicket({ subject: 'HS2-A HS2-B change', workerId: 'worker', tickets }).reason, /names 2/);
  assert.match(chooseTicket({ dirty: true, workerId: 'worker', tickets }).reason, /2 live claims/);
  assert.throws(() => chooseTicket({ explicit: 'HS2-Z', tickets }), /resolved to 0/);
  assert.throws(
    () => chooseTicket({ explicit: 'HS2-A', tickets: [...tickets, ticket('HS2-A', [], null, 'other')] }),
    /resolved to 2/,
  );
});

test('HS2-VQ5ND5 combines known tags and validates tiers and budget overrides', () => {
  const profile = profileFromTags(
    ticket('HS2-A', [
      'tier:T0',
      'deep-testing',
      'testing:browser',
      'testing:platform',
      'testing:perf',
      'test-budget:15m',
    ]),
  );
  assert.equal(profile.tier, 'T0');
  assert.equal(profile.budgetMs, 900000);
  assert.equal(profile.deep && profile.browser && profile.platform && profile.perf, true);
  assert.match(profileFromTags(ticket('HS2-B')).reason, /default T1/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['tier:T0', 'tier:T2'])), /Conflicting risk tier/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['testing:future'])), /Unknown testing tag/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['test-budget:bad'])), /positive duration/);
  assert.throws(() => profileFromTags(ticket('HS2-A', ['tier:T2', 'test-budget:15m'])), /no time budget/);
});
