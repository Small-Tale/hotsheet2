import { describe, expect, it } from 'vitest';

import type { OrchestrationBinding, OrchestrationManifest, OrchestrationPolicyView } from './api';
import {
  addChoices,
  changePolicyDraft,
  connectionStatus,
  destinationOf,
  formatMegabytes,
  newUlid,
  policyDraftFrom,
  policyFromDraft,
  projectRows,
  sessionStatus,
  tabProjectIdOf,
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
      { tool: 'codex', max_sessions: '2', weight: '70' },
      { tool: 'claude', max_sessions: '', weight: '50' },
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
