import { describe, expect, it } from 'vitest';

import type { OrchestrationBinding, OrchestrationManifest, OrchestrationPolicyView } from './api';
import {
  addChoices,
  addPolicyAccount,
  changePolicyDraft,
  changeTargetLimits,
  connectionStatus,
  destinationOf,
  formatCents,
  formatMegabytes,
  newUlid,
  parseDollars,
  policyDraftFrom,
  policyFromDraft,
  policyWithTargetLimits,
  projectRows,
  removePolicyAccount,
  sessionStatus,
  tabProjectIdOf,
  targetLimitsDraftFrom,
} from './orchestration-settings-model';

const REMOTE = '01JSYNTHREMTE0000000000001';
const local = (checkout: string): OrchestrationBinding => ({
  revision: 1,
  destination: { kind: 'local', server_id: 'server', checkout_id: checkout, checkout_authority: 'a' },
  checked_at_ms: 1,
  confirmed_at_ms: 1,
  last_failure: null,
  stale: true,
});
const manifest: OrchestrationManifest = {
  enabled: true,
  revision: 'r1',
  targets: [
    { target_id: 'T2', state: 'active', label: 'Web' },
    { target_id: 'T1', state: 'active', label: 'API' },
    { target_id: 'T3', state: 'removed', label: 'Old' },
    { target_id: 'T4', state: 'active', label: 'Unbound' },
  ],
};

describe('orchestration settings model (HS2-W27N1D)', () => {
  it('maps tab ids to destinations and bound destinations back to tabs', () => {
    expect(destinationOf('web-1')).toEqual({ kind: 'local', checkout_id: 'web-1' });
    const remote = `remote:${REMOTE}:api-1`;
    expect(destinationOf(remote)).toEqual({ kind: 'remote', registration_id: REMOTE, checkout_id: 'api-1' });
    expect(
      tabProjectIdOf({
        kind: 'remote',
        registration_id: REMOTE,
        identity: { server_id: 's' },
        checkout_id: 'api-1',
        checkout_authority: 'a',
      }),
    ).toBe(remote);
  });

  it('lists active projects by label with place and connection, never removed ones', () => {
    const rows = projectRows(
      manifest,
      {
        T1: local('api-1'),
        T2: { ...local('web-1'), last_failure: 'unreachable' },
        T4: { ...local('x'), destination: null },
      },
      {},
    );
    expect(rows.map((row) => row.label)).toEqual(['API', 'Unbound', 'Web']);
    expect(rows[0]).toMatchObject({
      where: 'Local on This Computer',
      tabProjectId: 'api-1',
      connection: { label: 'Bound' },
    });
    expect(rows[1]).toMatchObject({ location: 'unbound', where: 'Not bound on this computer' });
    expect(rows[2].connection).toEqual({ label: 'Unreachable', tone: 'danger' });
    expect(projectRows(undefined, {}, {})).toEqual([]);
  });

  it('prefers a fresh resolution over the cached binding', () => {
    const binding = local('a');
    expect(connectionStatus(undefined)).toEqual({ label: 'Checking', tone: 'neutral' });
    const resolution = {
      declaration: manifest.targets[0],
      binding,
      state: 'reachable' as const,
      reason: null,
      stale: false,
      usable: true,
    };
    expect(connectionStatus(binding, resolution)).toEqual({ label: 'Connected', tone: 'success' });
    expect(
      connectionStatus(binding, { ...resolution, usable: false, state: 'unreachable', reason: 'revoked' }),
    ).toEqual({
      label: 'Server Removed',
      tone: 'danger',
    });
    expect(connectionStatus(binding, { ...resolution, usable: false, state: 'unbound' }).tone).toBe('warning');
  });

  it('offers every other open project, checked when it is already bound here', () => {
    const choices = addChoices(
      [
        { id: 'owner', name: 'Launch' },
        { id: 'api-1', name: 'API' },
        { id: `remote:${REMOTE}:mobile-1`, name: 'Mobile' },
      ],
      'owner',
      { T1: local('api-1'), T3: local('mobile-1') },
      manifest,
      { [REMOTE]: 'Build Host' },
    );
    expect(choices.map((choice) => [choice.name, choice.added, choice.machine])).toEqual([
      ['API', true, 'This Computer'],
      ['Mobile', false, 'Build Host'],
    ]);
  });

  it('mints canonical, increasing ULIDs', () => {
    const zero = () => new Uint8Array(16);
    const first = newUlid(1_000, zero),
      second = newUlid(2_000, zero);
    expect(first).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    expect(second > first).toBe(true);
  });

  it('formats sessions and sizes', () => {
    const session = {
      session_id: 's',
      target_id: 't',
      role: 'worker' as const,
      tool: 'claude',
      state: 'running' as const,
      owner: 'orchestrator' as const,
      paused: false,
      created_at_ms: 0,
      updated_at_ms: 0,
    };
    expect(sessionStatus(session)).toEqual({ label: 'Running', tone: 'success' });
    expect(sessionStatus({ ...session, owner: 'human' }).label).toBe('Taken Over');
    expect(sessionStatus({ ...session, paused: true }).label).toBe('Paused');
    expect(sessionStatus({ ...session, state: 'lost' }).tone).toBe('danger');
    expect(formatMegabytes(512)).toBe('512 MB');
    expect(formatMegabytes(2048)).toBe('2.0 GB');
    expect(formatMegabytes(null)).toBe('Unknown');
  });
});

describe('limits-only policy draft (HS2-W27N1D, HS2-XHQ1EH)', () => {
  const stored: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: {
      machine: { max_sessions: 3 },
      tools: [{ tool: 'codex', max_sessions: 2, weight: 70, accounts: [{ account: 'team', max_sessions: 1 }] }],
      // Retired members (HS2-XHQ1EH) the form passes through for the server to drop.
      targets: [{ target_id: 'T1', actions: ['tickets_read'] }],
      approval: { required_for: ['work_start'] },
      speculation: { allowed: false, max_depth: 2 },
      placement: { automatic: false, actor: 'person', aging_minutes: 30 },
    },
  };

  it('starts from the stored policy or the defaults, with a row for every known tool', () => {
    const draft = policyDraftFrom(stored, ['claude', 'codex']);
    expect(draft.machine).toEqual({
      max_sessions: '3',
      min_free_ram_mb: '2048',
      max_cpu_percent: '90',
      min_free_disk_mb: '5120',
    });
    expect(draft.tools).toEqual([
      {
        tool: 'codex',
        max_sessions: '2',
        weight: '70',
        accounts: [{ account: 'team', max_sessions: '1', budget: '' }],
      },
      { tool: 'claude', max_sessions: '', weight: '50', accounts: [] },
    ]);
    expect(policyDraftFrom(undefined, [])).toMatchObject({ automatic: false, idle_minutes: '15', keep_warm: '0' });
  });

  it('changes one field per control and ignores unknown ones', () => {
    let draft = policyDraftFrom(stored, ['claude']);
    draft = changePolicyDraft(draft, 'automatic', true);
    draft = changePolicyDraft(draft, 'machine.max_cpu_percent', '75');
    draft = changePolicyDraft(draft, 'tool.claude.max_sessions', '1');
    draft = changePolicyDraft(draft, 'build_ahead', true);
    draft = changePolicyDraft(draft, 'idle_minutes', '30');
    expect(changePolicyDraft(draft, 'machine.unknown', '1')).toBe(draft);
    expect(changePolicyDraft(draft, 'automatic', 'yes')).toBe(draft);
    expect(draft).toMatchObject({ automatic: true, build_ahead: true, idle_minutes: '30' });
    expect(draft.machine.max_cpu_percent).toBe('75');
    expect(draft.tools.find((tool) => tool.tool === 'claude')?.max_sessions).toBe('1');
  });

  it('stores only the limits and keeps every member the form does not show', () => {
    let draft = policyDraftFrom(stored, ['claude', 'codex']);
    draft = changePolicyDraft(draft, 'tool.claude.max_sessions', '1');
    draft = changePolicyDraft(draft, 'automatic', true);
    const built = policyFromDraft(stored, draft);
    expect('policy' in built).toBe(true);
    if (!('policy' in built)) return;
    expect(built.policy.targets).toEqual(stored.policy!.targets);
    expect(built.policy.approval).toEqual(stored.policy!.approval);
    expect(built.policy.tools).toEqual([
      { tool: 'codex', max_sessions: 2, weight: 70, accounts: [{ account: 'team', max_sessions: 1 }] },
      { tool: 'claude', max_sessions: 1, weight: 50 },
    ]);
    expect(built.policy.placement).toEqual({ automatic: true, actor: 'person', aging_minutes: 30 });
    expect(built.policy.speculation).toEqual({ allowed: false, max_depth: 2 });
    // An emptied tool leaves the policy; the stored view itself is untouched.
    const removed = policyFromDraft(stored, changePolicyDraft(draft, 'tool.codex.max_sessions', ''));
    expect('policy' in removed && removed.policy.tools?.map((tool) => tool.tool)).toEqual(['claude']);
    expect(stored.policy!.tools).toHaveLength(1);
  });

  it('refuses values the server would refuse, naming the field', () => {
    const draft = policyDraftFrom(undefined, ['claude']);
    for (const [field, value, message] of [
      ['machine.max_sessions', '0', 'Sessions per Machine must be 1 to 64.'],
      ['machine.max_cpu_percent', '101', 'CPU Ceiling must be 1 to 100.'],
      ['machine.min_free_ram_mb', '1.5', 'Free Memory Floor must be a whole number.'],
      ['idle_minutes', '0', 'Wind Down After Idle must be 1 to 1440.'],
      ['keep_warm', '65', 'Idle Sessions Kept per Project must be 0 to 64.'],
      ['tool.claude.max_sessions', '65', 'claude sessions must be 1 to 64.'],
    ] as const)
      expect(policyFromDraft(undefined, changePolicyDraft(draft, field, value))).toEqual({ error: message });
    const weight = changePolicyDraft(
      changePolicyDraft(draft, 'tool.claude.max_sessions', '1'),
      'tool.claude.weight',
      '0',
    );
    expect(policyFromDraft(undefined, weight)).toEqual({ error: 'claude weight must be 1 to 100.' });
  });
});

describe('tool accounts and daily budgets (HS2-7WF6JD)', () => {
  const stored: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: {
      tools: [
        {
          tool: 'codex',
          max_sessions: 3,
          weight: 60,
          accounts: [{ account: 'team', max_sessions: 2, budget_cents_per_day: 1250 }],
        },
      ],
    },
  };

  it('converts budgets between dollars and whole cents exactly, never rounding', () => {
    for (const [cents, text] of [
      [0, '0'],
      [5, '0.05'],
      [250, '2.50'],
      [300, '3'],
      [123_456, '1234.56'],
    ] as const) {
      expect(formatCents(cents)).toBe(text);
      expect(parseDollars(text, 'Budget')).toBe(cents);
    }
    expect(parseDollars('2.5', 'Budget')).toBe(250);
    for (const bad of ['1.005', '-1', '1e3', '$4', '', '.5'])
      expect(parseDollars(bad, 'Budget')).toBe('Budget must be an amount in dollars, such as 12.50.');
  });

  it('round-trips accounts, and adds, edits and removes rows in any order', () => {
    let draft = policyDraftFrom(stored, ['codex']);
    expect(draft.tools[0].accounts).toEqual([{ account: 'team', max_sessions: '2', budget: '12.50' }]);
    // Unchanged: stored exactly as it was.
    const same = policyFromDraft(stored, draft);
    expect('policy' in same && same.policy.tools).toEqual(stored.policy!.tools);
    // Add two, edit the second, remove the first: the edited one keeps its values.
    draft = addPolicyAccount(addPolicyAccount(draft, 'codex'), 'codex');
    draft = changePolicyDraft(draft, 'account.codex.2.account', 'night');
    draft = changePolicyDraft(draft, 'account.codex.2.budget', '3');
    draft = removePolicyAccount(draft, 'codex', 0);
    // The empty row (index 0 now) is dropped when stored; the budget is in cents.
    const built = policyFromDraft(stored, draft);
    expect('policy' in built && built.policy.tools?.[0].accounts).toEqual([
      { account: 'night', max_sessions: 1, budget_cents_per_day: 300 },
    ]);
    // Removing every row stores the tool without accounts; adding one back refills it.
    let emptied = removePolicyAccount(removePolicyAccount(draft, 'codex', 1), 'codex', 0);
    const none = policyFromDraft(stored, emptied);
    expect('policy' in none && none.policy.tools?.[0]).toEqual({ tool: 'codex', max_sessions: 3, weight: 60 });
    emptied = changePolicyDraft(addPolicyAccount(emptied, 'codex'), 'account.codex.0.account', 'team');
    const refilled = policyFromDraft(stored, emptied);
    expect('policy' in refilled && refilled.policy.tools?.[0].accounts).toEqual([{ account: 'team', max_sessions: 1 }]);
    // Unknown tools or rows change nothing.
    expect(addPolicyAccount(draft, 'nope')).toEqual(draft);
    expect(changePolicyDraft(draft, 'account.codex.9.account', 'x')).toEqual(draft);
  });

  it('refuses accounts the server would refuse', () => {
    const draft = policyDraftFrom(stored, ['codex']);
    expect(policyFromDraft(stored, changePolicyDraft(draft, 'account.codex.0.max_sessions', '4'))).toEqual({
      error: 'codex account team sessions must be 1 to 3.',
    });
    expect(policyFromDraft(stored, changePolicyDraft(draft, 'account.codex.0.budget', '1.234'))).toEqual({
      error: 'codex account team daily budget must be an amount in dollars, such as 12.50.',
    });
    const twice = changePolicyDraft(addPolicyAccount(draft, 'codex'), 'account.codex.1.account', 'team');
    expect(policyFromDraft(stored, twice)).toEqual({ error: 'codex account team is listed twice.' });
  });
});

describe("one project's limits (HS2-7WF6JD)", () => {
  const stored: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: {
      machine: { max_sessions: 3 },
      tools: [
        { tool: 'codex', max_sessions: 3 },
        { tool: 'claude', max_sessions: 2 },
      ],
      targets: [
        { target_id: 'T1', tools: [{ tool: 'codex', models: ['gpt-x'] }], max_sessions: 2, actions: ['tickets_read'] },
        { target_id: 'T2', enabled: false, tools: [{ tool: 'claude' }], max_sessions: 1, speculation: true },
      ],
      placement: { automatic: true },
    },
  };

  it('starts from the entry, or from the defaults for a project the policy does not list', () => {
    expect(targetLimitsDraftFrom(stored, 'T2')).toEqual({
      targetId: 'T2',
      listed: true,
      enabled: false,
      tools: ['claude'],
      max_sessions: '1',
      machineOverride: false,
      machine: { max_sessions: '3', min_free_ram_mb: '2048', max_cpu_percent: '90', min_free_disk_mb: '5120' },
      buildAhead: 'on',
    });
    expect(targetLimitsDraftFrom(stored, 'T9')).toMatchObject({
      listed: false,
      enabled: true,
      tools: [],
      max_sessions: '1',
      buildAhead: 'default',
    });
  });

  it('replaces only its own entry, keeping models, other projects and every other member', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    draft = changeTargetLimits(draft, 'limits.tools', 'claude', true);
    draft = changeTargetLimits(draft, 'limits.max_sessions', '3');
    draft = changeTargetLimits(draft, 'limits.build_ahead', 'off');
    const built = policyWithTargetLimits(stored, draft);
    if (!('policy' in built)) throw new Error(built.error);
    expect(built.policy.targets).toEqual([
      {
        target_id: 'T1',
        enabled: true,
        tools: [{ tool: 'codex', models: ['gpt-x'] }, { tool: 'claude' }],
        max_sessions: 3,
        speculation: false,
      },
      stored.policy!.targets![1],
    ]);
    expect(built.policy.placement).toEqual({ automatic: true });
    expect(built.policy.tools).toEqual(stored.policy!.tools);
    expect(stored.policy!.targets![0].actions).toEqual(['tickets_read']);
    // A project the policy does not list is appended.
    const added = policyWithTargetLimits(
      stored,
      changeTargetLimits(targetLimitsDraftFrom(stored, 'T3'), 'limits.tools', 'codex', true),
    );
    expect('policy' in added && added.policy.targets?.map((entry) => entry.target_id)).toEqual(['T1', 'T2', 'T3']);
  });

  it('walks tool, override and build-ahead transitions in any order', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    // Repeated and out-of-order tool toggles converge.
    for (const [tool, checked] of [
      ['claude', true],
      ['claude', true],
      ['codex', false],
      ['codex', false],
      ['codex', true],
    ] as const)
      draft = changeTargetLimits(draft, 'limits.tools', tool, checked);
    expect(draft.tools).toEqual(['claude', 'codex']);
    // Empty, then refilled.
    draft = changeTargetLimits(
      changeTargetLimits(draft, 'limits.tools', 'claude', false),
      'limits.tools',
      'codex',
      false,
    );
    expect(draft.tools).toEqual([]);
    draft = changeTargetLimits(draft, 'limits.tools', 'codex', true);
    // The machine override on, edited, off (dropped), on again (kept values).
    draft = changeTargetLimits(draft, 'limits.machine_override', true);
    draft = changeTargetLimits(draft, 'limits.machine.max_sessions', '4');
    let built = policyWithTargetLimits(stored, draft);
    expect('policy' in built && built.policy.targets?.[0].machine).toEqual({
      max_sessions: 4,
      min_free_ram_mb: 2048,
      max_cpu_percent: 90,
      min_free_disk_mb: 5120,
    });
    draft = changeTargetLimits(draft, 'limits.machine_override', false);
    built = policyWithTargetLimits(stored, draft);
    expect('policy' in built && 'machine' in built.policy.targets![0]).toBe(false);
    draft = changeTargetLimits(draft, 'limits.machine_override', true);
    expect(draft.machine.max_sessions).toBe('4');
    // Build ahead back to the policy default drops the override.
    draft = changeTargetLimits(changeTargetLimits(draft, 'limits.build_ahead', 'on'), 'limits.build_ahead', 'default');
    built = policyWithTargetLimits(stored, draft);
    expect('policy' in built && 'speculation' in built.policy.targets![0]).toBe(false);
    // Unknown fields and values change nothing.
    expect(changeTargetLimits(draft, 'limits.build_ahead', 'maybe')).toBe(draft);
    expect(changeTargetLimits(draft, 'limits.unknown', '1')).toBe(draft);
    expect(changeTargetLimits(draft, 'limits.enabled', 'yes')).toBe(draft);
  });

  it('refuses limits the server would refuse', () => {
    const draft = targetLimitsDraftFrom(stored, 'T1');
    expect(policyWithTargetLimits(stored, changeTargetLimits(draft, 'limits.max_sessions', '0'))).toEqual({
      error: 'Sessions Here must be 1 to 64.',
    });
    expect(policyWithTargetLimits(stored, changeTargetLimits(draft, 'limits.max_sessions', '4'))).toEqual({
      error: "Sessions Here must be at most 3, the machine's session limit.",
    });
    expect(policyWithTargetLimits(stored, changeTargetLimits(draft, 'limits.tools', 'gemini', true))).toEqual({
      error: "gemini is not set up in the policy's tools.",
    });
    const override = changeTargetLimits(
      changeTargetLimits(draft, 'limits.machine_override', true),
      'limits.machine.max_cpu_percent',
      '0',
    );
    expect(policyWithTargetLimits(stored, override)).toEqual({ error: 'CPU Ceiling must be 1 to 100.' });
  });
});
