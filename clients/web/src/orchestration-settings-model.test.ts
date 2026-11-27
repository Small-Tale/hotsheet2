import { describe, expect, it } from 'vitest';

import type { OrchestrationBinding, OrchestrationManifest, OrchestrationPolicyView } from './api';
import {
  addChoices,
  addPolicyAccount,
  addTargetLimitsValue,
  advancedLimitsChanged,
  changePolicyDraft,
  changeTargetLimits,
  connectionStatus,
  destinationOf,
  duplicateProject,
  formatCents,
  formatMegabytes,
  modelFamily,
  modelSuggestions,
  newUlid,
  parseDollars,
  parseModels,
  parseResources,
  policyDraftFrom,
  policyFromDraft,
  policyWithTargetLimits,
  projectRows,
  projectStores,
  removePolicyAccount,
  removeTargetLimitsValue,
  resourceSuggestions,
  roleSessionsHint,
  roleSessionsLocked,
  sessionDescription,
  sessionStatus,
  siteChoices,
  siteChoicesByProject,
  tabProjectIdOf,
  targetLimitsDraftFrom,
  usablePolicyTools,
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

  it('maps a serving server’s bindings to this client’s tabs when the project is open from it (HS2-7SHGQH)', () => {
    const serving = `remote:${REMOTE}:orch-1`,
      remoteDestination = {
        kind: 'remote' as const,
        registration_id: '01ARZ3NDEKTSV4RRFFQ69G5FAZ',
        identity: { server_id: 's' },
        checkout_id: 'api-1',
        checkout_authority: 'a',
      };
    // A checkout local to the serving server opens here as a remote tab of the same registration.
    expect(tabProjectIdOf(local('web-1').destination!, serving)).toBe(`remote:${REMOTE}:web-1`);
    // A registration of the serving server is not one of this client's: no tab is known.
    expect(tabProjectIdOf(remoteDestination, serving)).toBeUndefined();
    // A local settings tab keeps the existing mapping.
    expect(tabProjectIdOf(local('web-1').destination!, 'orch-1')).toBe('web-1');
    const rows = projectRows(manifest, { T1: local('api-1'), T2: local('web-1') }, {}, undefined, undefined, serving);
    expect(rows.find((row) => row.targetId === 'T1')?.tabProjectId).toBe(`remote:${REMOTE}:api-1`);
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

  it('names the serving server on the rows of a project open from another server (HS2-DXAB4K)', () => {
    const bindings = { T1: local('api-1'), T4: { ...local('x'), destination: null } };
    const rows = projectRows(manifest, bindings, {}, undefined, 'build.example');
    expect(rows.map((row) => [row.label, row.where])).toEqual([
      ['API', 'On build.example'],
      ['Unbound', 'Not bound on build.example'],
      ['Web', ''],
    ]);
    expect(rows[0].place).toBe('On build.example');
    // Its icon is the remote one + Add Project's Open on <server> choices use.
    expect(rows.map((row) => row.location)).toEqual(['remote', 'unbound', 'remote']);
    expect(rows.map((row) => row.where).join()).not.toContain('This Computer');
    // Served here, the same bindings read as this computer's.
    expect(projectRows(manifest, bindings, {}).map((row) => row.where)).toEqual([
      'Local on This Computer',
      'Not bound on this computer',
      '',
    ]);
  });

  it('lists each project with its other sites after it (HS2-SHH7WW)', () => {
    const sites: OrchestrationManifest = {
      enabled: true,
      revision: 'r2',
      targets: [
        { target_id: 'W2', state: 'active', label: 'Widgets on the build host', site_of: 'W1' },
        { target_id: 'A1', state: 'active', label: 'Accounts' },
        { target_id: 'W1', state: 'active', label: 'Widgets' },
        { target_id: 'W3', state: 'active', label: 'Widgets laptop', site_of: 'W1' },
        { target_id: 'W4', state: 'removed', label: 'Widgets retired', site_of: 'W1' },
        // A site whose project is gone (a merge from another machine) shows as its own project.
        { target_id: 'X2', state: 'active', label: 'Orphan site', site_of: 'X1' },
      ],
    };
    const rows = projectRows(
      sites,
      {
        W1: local('widgets-1'),
        W2: {
          ...local('widgets-2'),
          destination: {
            kind: 'remote',
            registration_id: REMOTE,
            identity: { server_id: 's' },
            checkout_id: 'widgets-2',
            checkout_authority: 'a',
          },
          last_failure: 'unreachable',
        },
      },
      { [REMOTE]: 'Build host' },
    );
    expect(rows.map((row) => [row.label, row.siteOf, row.where])).toEqual([
      ['Accounts', undefined, ''],
      ['Orphan site', undefined, ''],
      // HS2-NGP2AK: every site's place, the project's own first, each place once.
      ['Widgets', undefined, '3 sites · Local on This Computer, Remote on Build host'],
      ['Widgets laptop', 'W1', 'Site of Widgets'],
      ['Widgets on the build host', 'W1', 'Site of Widgets · Remote on Build host'],
    ]);
    expect(rows[4].connection).toEqual({ label: 'Unreachable', tone: 'danger' });
    expect(rows[4].location).toBe('remote');
  });

  it('shows the OS of each site of a project with several sites (HS2-D4G81Q)', () => {
    const manifest: OrchestrationManifest = {
      enabled: true,
      revision: 'r2',
      targets: [
        { target_id: 'W1', state: 'active', label: 'Widgets' },
        { target_id: 'W2', state: 'active', label: 'Widgets laptop', site_of: 'W1' },
        { target_id: 'A1', state: 'active', label: 'Accounts' },
      ],
    };
    const site = (target_id: string, extra: object = {}) => ({
      target_id,
      label: target_id,
      project: target_id !== 'W2',
      destination: null,
      confirmed_at_ms: null,
      last_failure: null,
      stale: true,
      ...extra,
    });
    const rows = projectRows(
      manifest,
      { W1: local('widgets-1'), W2: local('widgets-2'), A1: local('accounts') },
      {},
      {
        revision: 'r2',
        projects: [
          // A one-site project's machine is its detail's business, not its row's.
          {
            project_id: 'A1',
            label: 'Accounts',
            sites: [site('A1', { capabilities: { os: 'linux', arch: 'x86_64' } })],
          },
          {
            project_id: 'W1',
            label: 'Widgets',
            sites: [
              site('W1', { capabilities: { os: 'macos', os_version: '15.1', arch: 'arm64' } }),
              site('W2', { capabilities_error: 'target_unavailable' }),
            ],
          },
        ],
      },
    );
    expect(rows.map((row) => [row.label, row.system, row.place])).toEqual([
      ['Accounts', undefined, 'Local on This Computer'],
      ['Widgets', 'macOS 15.1, arm64', 'Local on This Computer'],
      ['Widgets laptop', undefined, 'Local on This Computer'],
    ]);
    // Without the sites view, nothing changes.
    expect(projectRows(manifest, { W1: local('widgets-1') }, {}).map((row) => row.system)).toEqual([
      undefined,
      undefined,
      undefined,
    ]);
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
      // HS2-N3HWP9: an unlisted tool is Auto in both fields.
      { tool: 'claude', max_sessions: '', weight: '', accounts: [] },
    ]);
    // HS2-R2N33M: unset automatic placement is Auto (on once a plan exists).
    expect(policyDraftFrom(undefined, [])).toMatchObject({ automatic: undefined, idle_minutes: '15', keep_warm: '0' });
  });

  it('keeps Auto placement unset, and a switched value explicit (HS2-R2N33M)', () => {
    const draft = policyDraftFrom(undefined, ['codex']);
    const built = policyFromDraft(undefined, draft);
    if (!('policy' in built)) throw new Error(built.error);
    expect(built.policy.placement).toEqual({});
    for (const value of [false, true]) {
      const set = policyFromDraft(undefined, changePolicyDraft(draft, 'automatic', value));
      if (!('policy' in set)) throw new Error(set.error);
      expect(set.policy.placement).toEqual({ automatic: value });
      // Read back, it stays explicit.
      expect(policyDraftFrom({ enabled: true, revision: 'r', policy: set.policy }, []).automatic).toBe(value);
    }
  });

  it('changes one field per control and ignores unknown ones', () => {
    let draft = policyDraftFrom(stored, ['claude']);
    draft = changePolicyDraft(draft, 'automatic', true);
    draft = changePolicyDraft(draft, 'machine.max_cpu_percent', '75');
    draft = changePolicyDraft(draft, 'tool.claude.max_sessions', '1');
    draft = changePolicyDraft(draft, 'build_ahead', true);
    draft = changePolicyDraft(draft, 'redo_reopened', true);
    draft = changePolicyDraft(draft, 'idle_minutes', '30');
    expect(changePolicyDraft(draft, 'machine.unknown', '1')).toBe(draft);
    expect(changePolicyDraft(draft, 'automatic', 'yes')).toBe(draft);
    expect(draft).toMatchObject({ automatic: true, build_ahead: true, redo_reopened: true, idle_minutes: '30' });
    expect(changePolicyDraft(draft, 'redo_reopened', 'yes')).toBe(draft);
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
      { tool: 'claude', max_sessions: 1 },
    ]);
    expect(built.policy.placement).toEqual({ automatic: true, actor: 'person', aging_minutes: 30 });
    expect(built.policy.speculation).toEqual({ allowed: false, max_depth: 2, redo_reopened: false });
    // HS2-NCMATS: the redo switch round-trips through the form.
    const redo = policyFromDraft(stored, changePolicyDraft(draft, 'redo_reopened', true));
    expect('policy' in redo && redo.policy.speculation?.redo_reopened).toBe(true);
    expect(
      policyDraftFrom({ ...stored, policy: { ...stored.policy, speculation: { redo_reopened: true } } }, [])
        .redo_reopened,
    ).toBe(true);
    // HS2-N3HWP9: an emptied Sessions field is Auto, stored by leaving the member out; the stored view
    // itself is untouched.
    const auto = policyFromDraft(stored, changePolicyDraft(draft, 'tool.codex.max_sessions', ''));
    expect('policy' in auto && auto.policy.tools?.[0]).toEqual({
      tool: 'codex',
      weight: 70,
      accounts: [{ account: 'team', max_sessions: 1 }],
    });
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
      ['tool.claude.max_sessions', '65', 'claude sessions must be 0 to 64.'],
      ['tool.claude.max_sessions', 'auto', 'claude sessions must be a whole number.'],
      ['tool.claude.weight', '101', 'claude weight must be 0 to 100.'],
    ] as const)
      expect(policyFromDraft(undefined, changePolicyDraft(draft, field, value))).toEqual({ error: message });
  });

  it('reads and stores Auto, 0 (off) and numbers for each tool (HS2-N3HWP9)', () => {
    const view: OrchestrationPolicyView = {
      enabled: true,
      revision: 'p2',
      policy: {
        tools: [
          { tool: 'codex', max_sessions: 0 },
          { tool: 'claude', max_sessions: 'auto', weight: 0 },
          { tool: 'gemini', max_sessions: null, weight: 30 },
        ],
      },
    };
    const draft = policyDraftFrom(view, ['claude', 'codex', 'opencode']);
    expect(draft.tools.map(({ tool, max_sessions, weight }) => [tool, max_sessions, weight])).toEqual([
      ['codex', '0', ''],
      ['claude', '', '0'],
      ['gemini', '', '30'],
      ['opencode', '', ''],
    ]);
    // Unchanged: 0 stays 0, Auto is left out, and an all-Auto tool needs no entry.
    const same = policyFromDraft(view, draft);
    expect('policy' in same && same.policy.tools).toEqual([
      { tool: 'codex', max_sessions: 0 },
      { tool: 'claude', weight: 0 },
      { tool: 'gemini', weight: 30 },
    ]);
    // Clearing both fields returns a tool to Auto; typing numbers stores them.
    let edited = changePolicyDraft(draft, 'tool.gemini.weight', '');
    edited = changePolicyDraft(edited, 'tool.opencode.max_sessions', '3');
    edited = changePolicyDraft(edited, 'tool.opencode.weight', '80');
    const built = policyFromDraft(view, edited);
    expect('policy' in built && built.policy.tools).toEqual([
      { tool: 'codex', max_sessions: 0 },
      { tool: 'claude', weight: 0 },
      { tool: 'opencode', max_sessions: 3, weight: 80 },
    ]);
    // An account under an Auto or off tool is bounded by 64 only.
    const account = changePolicyDraft(
      changePolicyDraft(addPolicyAccount(draft, 'opencode'), 'account.opencode.0.account', 'team'),
      'account.opencode.0.max_sessions',
      '9',
    );
    const withAccount = policyFromDraft(view, account);
    expect('policy' in withAccount && withAccount.policy.tools?.at(-1)).toEqual({
      tool: 'opencode',
      accounts: [{ account: 'team', max_sessions: 9 }],
    });
    // The tools sessions may use: listed and known tools, less those turned off.
    expect(usablePolicyTools(view.policy, ['claude', 'codex', 'opencode'])).toEqual(['gemini', 'opencode']);
    expect(usablePolicyTools(null, ['claude'])).toEqual(['claude']);
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
      models: { claude: '' },
      resources: '',
      // HS2-R2N33M: not listed in role_sessions, so Auto (both).
      roleSessions: ['dispatcher', 'integrator'],
      roleSessionsAuto: true,
    });
    expect(targetLimitsDraftFrom(stored, 'T9')).toMatchObject({
      listed: false,
      enabled: true,
      tools: [],
      max_sessions: '', // HS2-N5BDE4: Auto
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
    expect(policyWithTargetLimits(stored, changeTargetLimits(draft, 'limits.max_sessions', '65'))).toEqual({
      error: 'Sessions Here must be 0 to 64.',
    });
    expect(policyWithTargetLimits(stored, changeTargetLimits(draft, 'limits.max_sessions', '4'))).toEqual({
      error: "Sessions Here must be at most 3, the machine's session limit.",
    });
    // HS2-N3HWP9: an unlisted tool is Auto and may run here; one the policy turns off may not.
    expect('policy' in policyWithTargetLimits(stored, changeTargetLimits(draft, 'limits.tools', 'gemini', true))).toBe(
      true,
    );
    const off: OrchestrationPolicyView = {
      ...stored,
      policy: { ...stored.policy, tools: [...(stored.policy?.tools ?? []), { tool: 'gemini', weight: 0 }] },
    };
    expect(policyWithTargetLimits(off, changeTargetLimits(draft, 'limits.tools', 'gemini', true))).toEqual({
      error: "gemini is turned off in the policy's tools.",
    });
    const override = changeTargetLimits(
      changeTargetLimits(draft, 'limits.machine_override', true),
      'limits.machine.max_cpu_percent',
      '0',
    );
    expect(policyWithTargetLimits(stored, override)).toEqual({ error: 'CPU Ceiling must be 1 to 100.' });
  });
});

describe("a project's models and required resources (HS2-YSY7AR)", () => {
  const stored: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: {
      tools: [
        { tool: 'codex', max_sessions: 3 },
        { tool: 'claude', max_sessions: 2 },
      ],
      targets: [
        { target_id: 'T1', tools: [{ tool: 'codex', models: ['gpt-x', 'gpt-y'] }], max_sessions: 1 },
        { target_id: 'T2', tools: [{ tool: 'claude' }], max_sessions: 1 },
      ],
      placement: {
        automatic: true,
        aging_minutes: 30,
        resources: [
          { target_id: 'T2', resources: [{ namespace: 'gpu', resource: 'a100' }] },
          { target_id: 'T1', resources: [{ namespace: 'vm', resource: 'macos-vm' }] },
        ],
      },
    },
  };
  const build = (draft: ReturnType<typeof targetLimitsDraftFrom>) => {
    const built = policyWithTargetLimits(stored, draft);
    if (!('policy' in built)) throw new Error(built.error);
    return built.policy;
  };

  it('reads models and resources as typed lists, and stores them unchanged when untouched', () => {
    const draft = targetLimitsDraftFrom(stored, 'T1');
    expect(draft.models).toEqual({ codex: 'gpt-x, gpt-y' });
    expect(draft.resources).toBe('vm/macos-vm');
    const policy = build(draft);
    expect(policy.targets?.[0].tools).toEqual(stored.policy!.targets![0].tools);
    expect(policy.placement).toEqual(stored.policy!.placement);
  });

  it('replaces only this project models and resources, in place, keeping other entries and members', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    draft = changeTargetLimits(draft, 'limits.models.codex', ' gpt-z ,, gpt-x ');
    draft = changeTargetLimits(draft, 'limits.resources', 'vm/macos-vm, sim/iphone-16');
    const policy = build(draft);
    expect(policy.targets?.[0].tools).toEqual([{ tool: 'codex', models: ['gpt-z', 'gpt-x'] }]);
    expect(policy.placement).toEqual({
      automatic: true,
      aging_minutes: 30,
      resources: [
        { target_id: 'T2', resources: [{ namespace: 'gpu', resource: 'a100' }] },
        {
          target_id: 'T1',
          resources: [
            { namespace: 'vm', resource: 'macos-vm' },
            { namespace: 'sim', resource: 'iphone-16' },
          ],
        },
      ],
    });
    // Emptied: any model, and no resources entry for this project; others stay.
    draft = changeTargetLimits(changeTargetLimits(draft, 'limits.models.codex', ''), 'limits.resources', ' ');
    const emptied = build(draft);
    expect(emptied.targets?.[0].tools).toEqual([{ tool: 'codex' }]);
    expect(emptied.placement?.resources).toEqual([stored.policy!.placement!.resources![0]]);
    // A project with no resources before gets an entry appended; a policy with none gets none.
    const appended = build(changeTargetLimits(targetLimitsDraftFrom(stored, 'T3'), 'limits.resources', 'vm/linux'));
    expect(appended.placement?.resources?.map((item) => item.target_id)).toEqual(['T2', 'T1', 'T3']);
    const bare: OrchestrationPolicyView = {
      enabled: true,
      revision: 'p',
      policy: { tools: [{ tool: 'codex', max_sessions: 1 }] },
    };
    const none = policyWithTargetLimits(bare, targetLimitsDraftFrom(bare, 'T1'));
    expect('policy' in none && none.policy.placement).toBeUndefined();
  });

  it('drops a tool models with the tool, and starts again from any model, in any order', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    // Models of a tool not checked here are ignored.
    expect(changeTargetLimits(draft, 'limits.models.claude', 'm')).toEqual(draft);
    for (const checked of [false, false, true, true])
      draft = changeTargetLimits(draft, 'limits.tools', 'codex', checked);
    expect(draft.models).toEqual({});
    expect(build(draft).targets?.[0].tools).toEqual([{ tool: 'codex' }]);
    draft = changeTargetLimits(draft, 'limits.tools', 'claude', true);
    draft = changeTargetLimits(draft, 'limits.models.claude', 'opus');
    expect(build(draft).targets?.[0].tools).toEqual([{ tool: 'codex' }, { tool: 'claude', models: ['opus'] }]);
  });

  it('refuses models and resources the server would refuse', () => {
    expect(parseModels('a b', 'codex models')).toBe('codex models: separate models with commas; "a b" has a space.');
    expect(parseModels('a, a', 'codex models')).toBe('codex models lists a twice.');
    expect(parseModels('x'.repeat(129), 'codex models')).toBe('codex models: a model name is at most 128 bytes.');
    expect(parseModels(Array.from({ length: 33 }, (_v, i) => `m${String(i)}`).join(','), 'codex models')).toBe(
      'codex models lists more than 32 models.',
    );
    expect(parseResources('vm/a, vm/b, vm/c, vm/d, vm/e')).toBe('Required Resources lists at most 4 resources.');
    expect(parseResources('vm/a, vm/a')).toBe('Required Resources lists vm/a twice.');
    for (const bad of ['vm', 'VM/a', 'vm/a/b', '/a', 'vm/', '-vm/a', 'v..m/a', 'vm/-a'])
      expect(parseResources(bad)).toMatch(/is not <namespace>\/<resource>/u);
    expect(parseResources('vm.mac_os-1/Mac.OS_1-x')).toEqual([{ namespace: 'vm.mac_os-1', resource: 'Mac.OS_1-x' }]);
    const draft = changeTargetLimits(targetLimitsDraftFrom(stored, 'T1'), 'limits.resources', 'vm');
    expect(policyWithTargetLimits(stored, draft)).toEqual({
      error:
        '"vm" is not <namespace>/<resource>, such as vm/macos-vm or vm/*: a lowercase namespace and a resource name or pattern.',
    });
  });
});

describe("a project's coordination sessions (HS2-TK9ADC)", () => {
  const stored: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: {
      machine: { max_sessions: 4 },
      tools: [{ tool: 'codex', max_sessions: 3 }],
      targets: [
        { target_id: 'T1', tools: [{ tool: 'codex' }], max_sessions: 3 },
        { target_id: 'T2', tools: [{ tool: 'codex' }], max_sessions: 1 },
      ],
      placement: {
        automatic: true,
        role_sessions: [
          { target_id: 'T2', roles: ['integrator'] },
          { target_id: 'T1', roles: ['integrator', 'dispatcher'] },
        ],
      },
    },
  };
  const build = (draft: ReturnType<typeof targetLimitsDraftFrom>, view = stored) => {
    const built = policyWithTargetLimits(view, draft);
    if (!('policy' in built)) throw new Error(built.error);
    return built.policy;
  };
  const toggle = (draft: ReturnType<typeof targetLimitsDraftFrom>, role: string, checked: boolean) =>
    changeTargetLimits(draft, 'limits.role_sessions', role, checked);

  it('reads the roles in the form order and stores them unchanged when untouched', () => {
    const draft = targetLimitsDraftFrom(stored, 'T1');
    expect(draft.roleSessions).toEqual(['dispatcher', 'integrator']);
    // HS2-R2N33M: an unlisted project is Auto, shown as both, and stays unlisted when untouched.
    expect(targetLimitsDraftFrom(stored, 'T3')).toMatchObject({
      roleSessions: ['dispatcher', 'integrator'],
      roleSessionsAuto: true,
    });
    expect(build(targetLimitsDraftFrom(stored, 'T3')).placement?.role_sessions).toEqual(
      stored.policy?.placement?.role_sessions,
    );
    expect(build(draft).placement?.role_sessions).toEqual([
      { target_id: 'T2', roles: ['integrator'] },
      { target_id: 'T1', roles: ['dispatcher', 'integrator'] },
    ]);
  });

  it('turns roles off and on again in any order, in place, keeping other projects and members', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    // Repeated and out of order: off twice, on again, an unknown role ignored.
    draft = toggle(toggle(draft, 'dispatcher', false), 'dispatcher', false);
    expect(draft.roleSessions).toEqual(['integrator']);
    draft = toggle(draft, 'worker', true);
    expect(draft.roleSessions).toEqual(['integrator']);
    draft = toggle(toggle(draft, 'dispatcher', true), 'dispatcher', true);
    expect(draft.roleSessions).toEqual(['dispatcher', 'integrator']);
    // Emptied: HS2-R2N33M keeps the choice of none in place; the other project's stays where it was.
    draft = toggle(toggle(draft, 'integrator', false), 'dispatcher', false);
    let policy = build(draft);
    expect(policy.placement).toEqual({
      automatic: true,
      role_sessions: [
        { target_id: 'T2', roles: ['integrator'] },
        { target_id: 'T1', roles: [] },
      ],
    });
    // Refilled: in place.
    policy = build(toggle(draft, 'dispatcher', true), { ...stored, policy: policy });
    expect(policy.placement?.role_sessions).toEqual([
      { target_id: 'T2', roles: ['integrator'] },
      { target_id: 'T1', roles: ['dispatcher'] },
    ]);
    // From Auto, a change becomes this project's explicit choice, appended after the others.
    const auto = toggle(targetLimitsDraftFrom(stored, 'T3'), 'integrator', false);
    expect(auto).toMatchObject({ roleSessions: ['dispatcher'], roleSessionsAuto: false });
    expect(build(auto).placement?.role_sessions).toEqual([
      ...(stored.policy?.placement?.role_sessions ?? []),
      { target_id: 'T3', roles: ['dispatcher'] },
    ]);
    // An entry turned to none stays as the choice of none; a policy without any gets none.
    const only: OrchestrationPolicyView = {
      enabled: true,
      revision: 'p',
      policy: {
        tools: [{ tool: 'codex', max_sessions: 1 }],
        placement: { role_sessions: [{ target_id: 'T1', roles: ['dispatcher'] }] },
      },
    };
    expect(build(toggle(targetLimitsDraftFrom(only, 'T1'), 'dispatcher', false), only).placement).toEqual({
      role_sessions: [{ target_id: 'T1', roles: [] }],
    });
    const bare: OrchestrationPolicyView = {
      enabled: true,
      revision: 'p',
      policy: { tools: [{ tool: 'codex', max_sessions: 1 }] },
    };
    expect(build(targetLimitsDraftFrom(bare, 'T1'), bare).placement).toBeUndefined();
  });

  it('names a role session in its row, and a worker by its tool', () => {
    const base = {
      session_id: 'S',
      target_id: 'T1',
      tool: 'codex',
      state: 'running' as const,
      owner: 'orchestrator' as const,
      paused: false,
      created_at_ms: 1,
      updated_at_ms: 1,
    };
    const label = (tool: string) => tool.toUpperCase();
    expect(sessionDescription({ ...base, role: 'dispatcher' }, label, 'now')).toBe('Dispatcher · CODEX · since now');
    expect(sessionDescription({ ...base, role: 'integrator', model: 'm' }, label, 'now')).toBe(
      'Integrator · CODEX · m · since now',
    );
    expect(sessionDescription({ ...base, role: 'worker' }, label, 'now')).toBe('CODEX · since now');
  });
});

describe("a site's coordination sessions (HS2-SYZB33)", () => {
  it('point at the home and allow only clearing roles that have no effect', () => {
    // The home, or a one-site project: the ordinary hint, never locked.
    expect(roleSessionsHint(undefined, [])).toMatch(/^Checked, the orchestrator starts/u);
    // HS2-R2N33M: an unlisted project's roles are Auto and follow placement.
    expect(roleSessionsHint(undefined, ['dispatcher', 'integrator'], true)).toMatch(/Auto: follows placement/u);
    expect(roleSessionsHint('Widgets', ['dispatcher', 'integrator'], true)).not.toMatch(/no effect/u);
    expect(roleSessionsLocked('Widgets', ['dispatcher', 'integrator'], true)).toBe(true);
    expect(roleSessionsLocked(undefined, [])).toBe(false);
    expect(roleSessionsLocked(undefined, ['dispatcher'])).toBe(false);
    // Another site with nothing checked: locked, pointing at the home.
    expect(roleSessionsHint('Widgets', [])).toBe(
      "Coordination sessions start only on Widgets, this project's home site. Set them there.",
    );
    expect(roleSessionsLocked('Widgets', [])).toBe(true);
    // A stored entry on the site: it can be cleared, and the hint says it has no effect.
    expect(roleSessionsHint('Widgets', ['dispatcher'])).toBe(
      "Coordination sessions start only on Widgets, this project's home site. Set them there. The roles checked here have no effect; clear them.",
    );
    expect(roleSessionsLocked('Widgets', ['dispatcher'])).toBe(false);
  });
});

describe('Add a Site and the duplicate check (HS2-E9VVQF)', () => {
  const bound = (checkout: string, store: string): OrchestrationBinding => ({
    ...local(checkout),
    destination: { kind: 'local', server_id: 'server', checkout_id: checkout, checkout_authority: store },
  });
  const grouped: OrchestrationManifest = {
    enabled: true,
    revision: 'r2',
    targets: [
      { target_id: 'W', state: 'active', label: 'Widgets' },
      { target_id: 'S', state: 'active', label: 'Widgets laptop', site_of: 'W' },
      { target_id: 'D', state: 'active', label: 'Docs' },
      { target_id: 'U', state: 'active', label: 'Unbound' },
      { target_id: 'X', state: 'removed', label: 'Old' },
    ],
  };
  // The project's own binding is missing; its site names the store.
  const bindings = { S: bound('laptop', 'w'), D: bound('docs', 'd'), X: bound('old', 'x') };
  const open = [
    { id: 'owner', name: 'Owner' },
    { id: 'laptop', name: 'Widgets laptop' },
    { id: 'clone', name: 'Widgets clone' },
    { id: 'other', name: 'Other' },
    { id: 'unknown', name: 'Unknown' },
  ];
  const stores = { clone: 'w', other: 'o', unknown: null };

  it('knows each project by the store any bound site of it reaches', () => {
    expect([...projectStores(grouped, bindings)]).toEqual([
      ['D', 'd'],
      ['W', 'w'],
    ]);
    expect(projectStores(undefined, {}).size).toBe(0);
  });

  it('offers the open projects of the same store, or all of them with Show All Projects', () => {
    const choices = addChoices(open, 'owner', bindings, grouped, {});
    expect(siteChoices(choices, stores, 'w', false).map((choice) => choice.projectId)).toEqual(['clone']);
    expect(siteChoices(choices, stores, 'w', true).map((choice) => [choice.projectId, choice.sameStore])).toEqual([
      ['clone', true],
      ['other', false],
      ['unknown', undefined],
    ]);
    // A project with no known store offers nothing by default, everything not added with Show All.
    expect(siteChoices(choices, stores, undefined, false)).toEqual([]);
    expect(siteChoices(choices, stores, undefined, true)).toHaveLength(3);
    const rows = projectRows(grouped, bindings, {});
    const byProject = siteChoicesByProject(rows, choices, stores, grouped, bindings, false);
    // Only project rows get choices; a site row separates instead.
    expect(Object.keys(byProject).sort()).toEqual(['D', 'U', 'W']);
    expect(byProject.W.map((choice) => choice.name)).toEqual(['Widgets clone']);
    expect(byProject.D).toEqual([]);
  });

  it('finds the added project a chosen open project duplicates by its store', () => {
    expect(duplicateProject('w', grouped, bindings)).toEqual({ targetId: 'W', label: 'Widgets' });
    expect(duplicateProject('d', grouped, bindings)).toEqual({ targetId: 'D', label: 'Docs' });
    expect(duplicateProject('o', grouped, bindings)).toBeUndefined();
    // A removed project is not a duplicate, and an unknown store never is.
    expect(duplicateProject('x', grouped, bindings)).toBeUndefined();
    expect(duplicateProject(null, grouped, bindings)).toBeUndefined();
    expect(duplicateProject(undefined, grouped, bindings)).toBeUndefined();
  });
});

describe("a project's Auto limits (HS2-N5BDE4)", () => {
  const stored: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: {
      machine: { max_sessions: 3 },
      targets: [{ target_id: 'T1', max_sessions: 2, tools: [{ tool: 'codex' }] }],
    },
  };

  it('reads an unlisted project as Auto and stores Auto by leaving members out', () => {
    const draft = targetLimitsDraftFrom(stored, 'T9');
    expect(draft).toMatchObject({ listed: false, enabled: true, tools: [], max_sessions: '' });
    // Applying an untouched draft lists the project with nothing set: Auto in everything.
    const built = policyWithTargetLimits(stored, draft);
    expect('policy' in built && built.policy.targets?.at(-1)).toEqual({ target_id: 'T9', enabled: true });
    // "auto" and null read as Auto too.
    for (const max_sessions of ['auto', null] as const)
      expect(
        targetLimitsDraftFrom({ ...stored, policy: { targets: [{ target_id: 'T1', max_sessions }] } }, 'T1')
          .max_sessions,
      ).toBe('');
  });

  it('stores 0 to start nothing, a number as a cap, and clears back to Auto', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    expect(draft.max_sessions).toBe('2');
    draft = changeTargetLimits(draft, 'limits.max_sessions', '0');
    const off = policyWithTargetLimits(stored, draft);
    expect('policy' in off && off.policy.targets?.[0].max_sessions).toBe(0);
    draft = changeTargetLimits(changeTargetLimits(draft, 'limits.max_sessions', ''), 'limits.tools', 'codex', false);
    const auto = policyWithTargetLimits(stored, draft);
    expect('policy' in auto && auto.policy.targets?.[0]).toEqual({ target_id: 'T1', enabled: true });
  });
});

describe('Advanced limits are Auto until changed (HS2-25KT42)', () => {
  const stored: OrchestrationPolicyView = { enabled: true, revision: 'p1', policy: {} };
  it('counts each Advanced limit that differs from Auto, and back to none', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    expect(advancedLimitsChanged(undefined)).toBe(0);
    expect(advancedLimitsChanged(draft)).toBe(0);
    draft = changeTargetLimits(draft, 'limits.max_sessions', '2');
    draft = changeTargetLimits(draft, 'limits.build_ahead', 'off');
    draft = changeTargetLimits(draft, 'limits.role_sessions', 'dispatcher', false);
    draft = addTargetLimitsValue(draft, 'limits.resources', 'vm/*');
    draft = changeTargetLimits(draft, 'limits.machine_override', true);
    expect(advancedLimitsChanged(draft)).toBe(5);
    // Whitespace alone is still Auto; clearing each returns to none.
    draft = changeTargetLimits(draft, 'limits.max_sessions', '  ');
    draft = changeTargetLimits(draft, 'limits.build_ahead', 'default');
    draft = changeTargetLimits(draft, 'limits.role_sessions', 'dispatcher', true);
    draft = removeTargetLimitsValue(draft, 'limits.resources', 'vm/*');
    draft = changeTargetLimits(draft, 'limits.machine_override', false);
    expect(advancedLimitsChanged(draft)).toBe(0);
    // An all-Auto draft stores no entry members beyond the project and its switch.
    expect(policyWithTargetLimits(stored, draft)).toEqual({
      policy: { targets: [{ target_id: 'T1', enabled: true }] },
    });
  });
});

describe('models and resources as chosen values with patterns (HS2-1X1TXC, HS2-NPAZQW)', () => {
  const stored: OrchestrationPolicyView = {
    enabled: true,
    revision: 'p1',
    policy: {
      targets: [{ target_id: 'T1', tools: [{ tool: 'claude', models: ['model-a'] }] }],
      placement: {
        resources: [
          { target_id: 'T1', resources: [{ namespace: 'vm', resource: 'mac-1' }] },
          {
            target_id: 'T2',
            resources: [
              { namespace: 'ports', resource: '8080' },
              { namespace: 'vm', resource: 'mac-*' },
            ],
          },
        ],
      },
    },
  };

  it('adds, dedupes and removes values, in order, through every transition', () => {
    let draft = targetLimitsDraftFrom(stored, 'T1');
    expect(draft.models.claude).toBe('model-a');
    draft = addTargetLimitsValue(draft, 'limits.models.claude', 'opus-*');
    expect(draft.models.claude).toBe('model-a, opus-*');
    // A repeat, a blank, and an unknown field change nothing (the same draft).
    expect(addTargetLimitsValue(draft, 'limits.models.claude', 'opus-*')).toBe(draft);
    expect(addTargetLimitsValue(draft, 'limits.models.claude', '  ')).toBe(draft);
    expect(addTargetLimitsValue(draft, 'limits.models.codex', 'x')).toBe(draft);
    expect(addTargetLimitsValue(draft, 'limits.other', 'x')).toBe(draft);
    expect(removeTargetLimitsValue(draft, 'limits.models.claude', 'absent')).toBe(draft);
    // Typed text with commas adds each once.
    draft = addTargetLimitsValue(draft, 'limits.models.claude', 'b, c, b');
    expect(draft.models.claude).toBe('model-a, opus-*, b, c');
    draft = removeTargetLimitsValue(draft, 'limits.models.claude', 'model-a');
    draft = removeTargetLimitsValue(draft, 'limits.models.claude', 'b');
    draft = removeTargetLimitsValue(draft, 'limits.models.claude', 'c');
    expect(draft.models.claude).toBe('opus-*');
    // Emptied, then refilled.
    draft = removeTargetLimitsValue(draft, 'limits.models.claude', 'opus-*');
    expect(draft.models.claude).toBe('');
    draft = addTargetLimitsValue(draft, 'limits.models.claude', 'sonnet-*');
    expect(policyWithTargetLimits(stored, draft)).toMatchObject({
      policy: { targets: [{ target_id: 'T1', tools: [{ tool: 'claude', models: ['sonnet-*'] }] }] },
    });

    draft = addTargetLimitsValue(draft, 'limits.resources', 'vm/*');
    expect(draft.resources).toBe('vm/mac-1, vm/*');
    draft = removeTargetLimitsValue(draft, 'limits.resources', 'vm/mac-1');
    const built = policyWithTargetLimits(stored, draft);
    expect(built).toMatchObject({
      policy: {
        placement: {
          resources: [{ target_id: 'T1', resources: [{ namespace: 'vm', resource: '*' }] }, { target_id: 'T2' }],
        },
      },
    });
  });

  it('accepts patterns where the server does and refuses a pattern namespace', () => {
    expect(parseModels('opus-*, *-mini', 'claude models')).toEqual(['opus-*', '*-mini']);
    expect(parseResources('vm/*, vm/mac-*')).toEqual([
      { namespace: 'vm', resource: '*' },
      { namespace: 'vm', resource: 'mac-*' },
    ]);
    expect(parseResources('v*/a')).toMatch(/is not <namespace>\/<resource>/u);
  });

  it('offers model families before the models a tool reports', () => {
    expect(modelFamily('claude-opus-4-1')).toBe('claude-opus-*');
    expect(modelFamily('gpt-5.1-codex')).toBe('gpt-*');
    expect(modelFamily('opus')).toBeUndefined();
    expect(modelFamily('4o')).toBeUndefined();
    expect(modelSuggestions(['claude-opus-4-1', 'claude-opus-4-2', 'opus', 'claude-sonnet-4-5'], ['opus'])).toEqual([
      'claude-opus-*',
      'claude-sonnet-*',
      'claude-opus-4-1',
      'claude-opus-4-2',
      'claude-sonnet-4-5',
    ]);
    expect(modelSuggestions([], [])).toEqual([]);
  });

  it('offers declared resources and a pattern per namespace, never a pattern as declared', () => {
    expect(
      resourceSuggestions(
        stored.policy,
        [
          { namespace: 'vm', resource: 'win-1' },
          { namespace: 'vm', resource: 'mac-1' },
        ],
        ['ports/8080'],
      ),
    ).toEqual(['ports/*', 'vm/*', 'vm/mac-1', 'vm/win-1']);
    expect(resourceSuggestions(undefined, [], [])).toEqual([]);
  });
});
