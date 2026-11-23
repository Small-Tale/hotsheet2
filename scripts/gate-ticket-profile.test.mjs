import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { execFileSync } from 'node:child_process';
import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';
import {
  bestEffortProfile,
  chooseTicket,
  developmentStrategyFromSettings,
  linkedGitConnections,
  profileFromTags,
  readTicketProfile,
  resolveConfiguredDevelopmentStrategy,
  strategyCliCandidates,
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

test('HS2-E5EX0M a machine-wide provider the gate cannot list does not hide the ticket', () => {
  const dir = mkdtempSync(join(tmpdir(), 'hs2-e5ex0m-'));
  const savedPath = process.env.PATH;
  const savedWorker = process.env.HOTSHEET_WORKER_ID;
  try {
    const repo = join(dir, 'repo');
    mkdirSync(join(repo, '.hotsheet2/store'), { recursive: true });
    const git = (...args) =>
      execFileSync('git', args, {
        cwd: repo,
        env: withoutGitRepositoryEnv(process.env),
        stdio: 'pipe',
      });
    git('init', '-q');
    git(
      '-c',
      'user.name=Synthetic',
      '-c',
      'user.email=synthetic@example.invalid',
      'commit',
      '-q',
      '--allow-empty',
      '-m',
      'HS2-AAAAAA Synthetic change',
    );
    const bin = join(dir, 'bin');
    mkdirSync(bin);
    const fake = join(bin, 'hotsheet-cli');
    const listed = JSON.stringify([
      { slug: 'HS2-AAAAAA', connection_id: 'git', native_id: 'A', qualified_id: 'git:A', claimed_by: null, tags: [] },
    ]);
    const detail = JSON.stringify({ slug: 'HS2-AAAAAA', tags: ['tier:T0'] });
    const providers = JSON.stringify([
      { connection_id: 'other-credentialed', provider: 'git', locator: join(dir, 'unavailable.hs2') },
      { connection_id: 'git', provider: 'git', locator: join(dir, 'other.hs2') },
    ]);
    writeFileSync(
      fake,
      [
        '#!/bin/sh',
        'case "$1" in',
        "  providers) printf %s '" + providers + "' ;;",
        '  provider-ls) if [ "$2" = git ]; then printf %s \'' +
          listed +
          '\'; else echo "credential store unavailable" >&2; exit 1; fi ;;',
        "  provider-get) printf %s '" + detail + "' ;;",
        '  *) exit 2 ;;',
        'esac',
        '',
      ].join('\n'),
    );
    chmodSync(fake, 0o755);
    process.env.PATH = `${bin}:${savedPath}`;
    delete process.env.HOTSHEET_WORKER_ID;
    const profile = readTicketProfile(repo);
    assert.match(profile.reason, /HEAD commit subject/);
    assert.deepEqual(profile.tags, ['tier:T0']);
  } finally {
    process.env.PATH = savedPath;
    if (savedWorker === undefined) delete process.env.HOTSHEET_WORKER_ID;
    else process.env.HOTSHEET_WORKER_ID = savedWorker;
    rmSync(dir, { recursive: true, force: true });
  }
});

/** HS2-EBZ0CS: a fake hotsheet-cli on PATH that records each call and the secrets mode it ran with. */
function recordingCli(dir, connections, listed) {
  const bin = join(dir, 'bin');
  mkdirSync(bin, { recursive: true });
  const log = join(dir, 'calls.log');
  const fake = join(bin, 'hotsheet-cli');
  writeFileSync(
    fake,
    [
      '#!/bin/sh',
      `printf '%s|%s\\n' "\${HOTSHEET_NATIVE_SECRETS:-unset}" "$*" >> '${log}'`,
      'case "$1" in',
      `  providers) printf %s '${JSON.stringify(connections)}' ;;`,
      `  provider-ls) printf %s '${JSON.stringify(listed)}' ;;`,
      `  provider-get) printf %s '${JSON.stringify({ slug: 'HS2-AAAAAA', tags: ['tier:T0'] })}' ;;`,
      '  *) exit 2 ;;',
      'esac',
      '',
    ].join('\n'),
  );
  chmodSync(fake, 0o755);
  return { bin, calls: () => readFileSync(log, 'utf8').trim().split('\n') };
}

function syntheticCheckout(dir, link) {
  const repo = join(dir, 'repo');
  mkdirSync(join(repo, '.hotsheet2'), { recursive: true });
  link(repo);
  const git = (...args) => execFileSync('git', args, { cwd: repo, env: withoutGitRepositoryEnv(process.env), stdio: 'pipe' });
  git('init', '-q');
  git(
    '-c',
    'user.name=Synthetic',
    '-c',
    'user.email=synthetic@example.invalid',
    'commit',
    '-q',
    '--allow-empty',
    '-m',
    'HS2-AAAAAA Synthetic change',
  );
  return repo;
}

for (const layout of ['in-tree store', 'external store locator'])
  test(`HS2-EBZ0CS ticket lookup lists only the checkout's linked Git store, with secrets denied (${layout})`, () => {
    const dir = mkdtempSync(join(tmpdir(), 'hs2-ebz0cs-'));
    const savedPath = process.env.PATH;
    const savedWorker = process.env.HOTSHEET_WORKER_ID;
    const savedSecrets = process.env.HOTSHEET_NATIVE_SECRETS;
    try {
      const external = join(dir, 'project.hs2');
      mkdirSync(external);
      const repo = syntheticCheckout(dir, (root) => {
        if (layout === 'in-tree store') mkdirSync(join(root, '.hotsheet2/store'));
        else writeFileSync(join(root, '.hotsheet2/store'), `${external}\n`);
      });
      const store = layout === 'in-tree store' ? join(repo, '.hotsheet2/store') : external;
      // Machine-wide configuration (HS2-7QPJ05): another project's credentialed GitHub connection,
      // another project's Git store, and this checkout's Git store.
      const fake = recordingCli(
        dir,
        [
          { connection_id: 'github-other-project', provider: 'github', locator: 'synthetic-org/other' },
          { connection_id: 'other-git', provider: 'git', locator: join(dir, 'other.hs2') },
          { connection_id: 'linked-git', provider: 'git', locator: store },
        ],
        [
          {
            slug: 'HS2-AAAAAA',
            connection_id: 'linked-git',
            native_id: 'A',
            qualified_id: 'linked-git:A',
            claimed_by: null,
            tags: [],
          },
        ],
      );
      process.env.PATH = `${fake.bin}:${savedPath}`;
      delete process.env.HOTSHEET_WORKER_ID;
      delete process.env.HOTSHEET_NATIVE_SECRETS;
      const profile = readTicketProfile(repo);
      assert.match(profile.reason, /HEAD commit subject/);
      assert.deepEqual(profile.tags, ['tier:T0']);
      const calls = fake.calls();
      for (const call of calls) assert.match(call, /^deny\|/, `hotsheet-cli ran without NATIVE_SECRETS=deny: ${call}`);
      const listed = calls.filter((call) => call.startsWith('deny|provider-ls '));
      assert.deepEqual(listed, ['deny|provider-ls linked-git'], 'provider-ls ran only for the linked Git store');
      assert.ok(!calls.some((call) => call.includes('github-other-project')), 'no credentialed provider was called');
    } finally {
      process.env.PATH = savedPath;
      if (savedWorker === undefined) delete process.env.HOTSHEET_WORKER_ID;
      else process.env.HOTSHEET_WORKER_ID = savedWorker;
      if (savedSecrets === undefined) delete process.env.HOTSHEET_NATIVE_SECRETS;
      else process.env.HOTSHEET_NATIVE_SECRETS = savedSecrets;
      rmSync(dir, { recursive: true, force: true });
    }
  });

test('HS2-EBZ0CS never lists a non-Git connection, even when no Git connection names the linked store', () => {
  const connections = [
    { connection_id: 'github-a', provider: 'github', locator: 'synthetic-org/a' },
    { connection_id: 'jira-b', provider: 'jira', locator: 'https://jira.example.invalid' },
    { connection_id: 'git-c', provider: 'git', locator: '/nonexistent/synthetic.hs2' },
  ];
  assert.deepEqual(linkedGitConnections(connections, ['/synthetic/linked']), ['git-c']);
  assert.deepEqual(linkedGitConnections(connections.slice(0, 2), ['/synthetic/linked']), []);
});

test('HS2-Y183ZJ strategy resolution tries the newest CLI first and fails plainly when none can resolve it', (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'hs2-y183zj-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const older = join(dir, 'older-hotsheet-cli');
  const newer = join(dir, 'newer-hotsheet-cli');
  writeFileSync(older, '');
  writeFileSync(newer, '');
  utimesSync(older, new Date('2026-01-01T00:00:00Z'), new Date('2026-01-01T00:00:00Z'));
  const repo = join(dir, 'repo');
  mkdirSync(join(repo, 'target/debug'), { recursive: true });
  const debug = join(repo, 'target/debug/hotsheet-cli');
  writeFileSync(debug, '');
  utimesSync(debug, new Date('2025-01-01T00:00:00Z'), new Date('2025-01-01T00:00:00Z'));
  const bin = join(dir, 'bin');
  mkdirSync(bin);
  writeFileSync(join(bin, 'hotsheet-cli'), '');
  // The installed (PATH) build is newer than target/debug, so it is tried first.
  assert.deepEqual(strategyCliCandidates(repo, { PATH: bin }), [join(bin, 'hotsheet-cli'), debug]);
  assert.deepEqual(strategyCliCandidates(join(dir, 'none'), { PATH: join(dir, 'missing') }), ['hotsheet-cli']);

  const settings = { development_strategy: { version: 1, default: 'ci-first' } };
  const profile = profileFromTags(ticket('HS2-Y183ZJ', ['tier:T1']));
  const tried = [];
  const resolved = resolveConfiguredDevelopmentStrategy(
    repo,
    profile,
    settings,
    (binary, _args, options) => {
      tried.push(binary);
      assert.equal(options.env.HOTSHEET_NATIVE_SECRETS, 'deny');
      assert.ok(options.timeout > 0);
      if (binary === newer) throw Object.assign(new Error('failed'), { stderr: 'unknown strategy ci-first' });
      return JSON.stringify({ preset: 'ci-first', source: 'project', local_gate: 'best-effort', local_budget_minutes: 10 });
    },
    [newer, older],
  );
  assert.deepEqual(tried, [newer, older], 'a newer build from older code falls through to the next');
  assert.equal(resolved.bestEffort, true);
  assert.throws(
    () =>
      resolveConfiguredDevelopmentStrategy(
        repo,
        profile,
        settings,
        () => {
          throw Object.assign(new Error('failed'), { stderr: 'unknown strategy ci-first' });
        },
        [newer, older],
      ),
    (error) =>
      /Rebuild hotsheet-cli/.test(error.message) &&
      /"ci-first"/.test(error.message) &&
      error.message.includes(newer) &&
      error.message.includes(older),
  );
});

test('HS2-ZGS7JP CI First maps to serial best effort with a bounded, configurable budget', () => {
  const settings = { development_strategy: { version: 1, default: 'ci-first' } };
  assert.deepEqual(developmentStrategyFromSettings(settings), {
    preset: 'ci-first',
    source: 'project',
    integration: 'serial',
    local_gate: 'best-effort',
    local_budget_minutes: 10,
    push_without_receipt: true,
  });
  const configured = { development_strategy: { version: 1, default: 'ci-first', best_effort_budget_minutes: 4 } };
  assert.equal(developmentStrategyFromSettings(configured).local_budget_minutes, 4);
  for (const invalid of [0, 1441, 2.5, '10'])
    assert.throws(
      () =>
        developmentStrategyFromSettings({
          development_strategy: { version: 1, default: 'ci-first', best_effort_budget_minutes: invalid },
        }),
      /best-effort budget/,
    );
  // Ticket over session over project, in both directions.
  const t2 = profileFromTags(ticket('HS2-ZGS7JP', ['tier:T2']));
  const project = withDevelopmentStrategy(t2, configured, undefined);
  assert.equal(project.bestEffort, true);
  assert.equal(project.budgetMs, 4 * 60_000, 'T2 is budgeted under CI First');
  assert.equal(project.deep, false);
  const session = withDevelopmentStrategy(t2, configured, 'ticket-assured');
  assert.equal(session.bestEffort, undefined);
  assert.equal(session.developmentStrategy.source, 'session');
  assert.equal(session.deep, true);
  const ticketTag = withDevelopmentStrategy(
    profileFromTags(ticket('HS2-ZGS7JP', ['tier:T2', 'development-strategy:ci-first'])),
    { development_strategy: { version: 1, default: 'ticket-assured' } },
    'ticket-assured',
  );
  assert.equal(ticketTag.developmentStrategy.source, 'ticket');
  assert.equal(ticketTag.bestEffort, true);
  assert.equal(ticketTag.budgetMs, 10 * 60_000);
  // The CLI resolver's selection takes the same path.
  const resolved = resolveConfiguredDevelopmentStrategy('/synthetic/project', t2, settings, () =>
    JSON.stringify({
      preset: 'ci-first',
      source: 'project',
      integration: 'serial',
      local_gate: 'best-effort',
      local_budget_minutes: 3,
      push_without_receipt: true,
    }),
  );
  assert.equal(resolved.bestEffort, true);
  assert.equal(resolved.budgetMs, 3 * 60_000);
  assert.throws(
    () => bestEffortProfile(t2, { preset: 'ci-first', source: 'project', local_budget_minutes: 0 }),
    /invalid best-effort/,
  );
  // A custom strategy can neither take the preset's name nor its best-effort gate.
  const withCustom = (name, local_gate) => ({
    development_strategy: {
      version: 2,
      default: 'ticket-assured',
      custom: { [name]: { ...strictCustom, minimum_tier: 'T1', local_gate } },
    },
  });
  assert.throws(() => developmentStrategyFromSettings(withCustom('ci-first', 'targeted')), /reserved/);
  assert.throws(() => developmentStrategyFromSettings(withCustom('quick', 'best-effort')), /local gate/);
});
