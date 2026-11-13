import { afterEach, describe, expect, it, vi } from 'vitest';

import { Api } from './api';
import { githubAccountEndpoint } from './github-account-endpoint';
import {
  parseAccountIdentity,
  parseAdvisoryClaimStatus,
  parseAiToolDefaults,
  parseCheckout,
  parseCheckoutRelink,
  parseCheckouts,
  parseCheckoutTicketPage,
  parseCheckoutTicketRowsPage,
  parseCommandRun,
  parseCorruptTicketPaths,
  parseCustomView,
  parseDevelopmentStrategyReport,
  parseFeedbackDelivery,
  parseFullTicket,
  parseGitHubAuthStatus,
  parseGroupCreate,
  parseGroupList,
  parseGroupRename,
  parseGroupUngroup,
  parseGuidanceBatchReview,
  parseGuidanceInterviewReview,
  parseGuidanceOverview,
  parseGuidanceRecommendationReport,
  parseGuidanceRulePreview,
  parseGuidanceSourceUpdates,
  parseOpenedCheckout,
  parsePermissionDecision,
  parsePermissionRequest,
  parsePollResponse,
  parseProviderAccount,
  parseProviderConnection,
  parseProviderDescriptor,
  parseProviderOutboxOperation,
  parseQueuedProviderResult,
  parseRepositoryCommitPage,
  parseRepositoryFilePage,
  parseRepositoryStatus,
  parseRulesyncUpdateReport,
  parseServerCompatibility,
  parseStoredTicket,
  parseStringList,
  parseTerminalInfo,
  parseTerminalRead,
  parseTicket,
  parseTicketRow,
  parseTicketStoreSyncReport,
  parseTrashPurge,
  parseTrashSettings,
  type ResponseParser,
  ServerResponseShapeError,
} from './server-response-parsers';

describe('HS2-NBDXPW development policy response boundary', () => {
  it('accepts an empty custom map and refuses malformed effective provenance or mandatory gates', () => {
    const report = {
      policy: { version: 2, default: 'ticket-assured', green_main_goal_minutes: 360 },
      effective: {
        preset: 'ticket-assured',
        source: 'session',
        integration: 'serial',
        local_gate: 'full-local',
        require_green_main_sha: true,
        attribute_red_ci_before_completion: true,
        green_main_goal_minutes: null,
      },
      presets: ['ticket-assured', 'steady-main', 'risk-guided'],
    };
    expect(parseDevelopmentStrategyReport(report).policy.custom).toEqual({});
    expect(() =>
      parseDevelopmentStrategyReport({ ...report, effective: { ...report.effective, source: 'unknown' } }),
    ).toThrow();
    expect(() =>
      parseDevelopmentStrategyReport({ ...report, effective: { ...report.effective, require_green_main_sha: 'yes' } }),
    ).toThrow();
  });
});

describe('HS2-2MZWZT advisory status parser', () => {
  const status = {
    projection: {
      claims: [{ worker: 'w', operation_id: 'op', expires_unix_ms: 2_000_000_000_000 }],
      conflicted: false,
      invalid_markers: 0,
    },
    provisional: true,
    remote_verified: false,
    automatic_handoff_safe: false,
    pending_operation_ids: ['op'],
    pending_operations: [{ operation_id: 'op', state: 'needs_attention' }],
  };
  it('preserves pending state and refuses an unknown replay state', () => {
    expect(parseAdvisoryClaimStatus(status).pending_operations[0].state).toBe('needs_attention');
    expect(() =>
      parseAdvisoryClaimStatus({ ...status, pending_operations: [{ operation_id: 'op', state: 'lost' }] }),
    ).toThrow();
  });
});

const checkout = { id: 'c', root: '/w/c', alias: 'c', stores: ['/w/c.hs2'] };

describe('HS2-FE0HBN provider account parser matches the Rust Account wire shape', () => {
  // The serialized shape of `hotsheet_ticketing::accounts::Account` for an unused github.com
  // sign-in: `host` and `managed` always present, unset `base_url`/`identity` skipped.
  const account = {
    id: 'github-app-01live',
    provider: 'github',
    host: 'github.com',
    managed: true,
    sources: [{ connection_id: 'github-acme', name: 'Issues', locator: 'acme/app', disabled: false, projects: [] }],
    projects: [],
  };
  const without = (key: keyof typeof account) => Object.fromEntries(Object.entries(account).filter(([k]) => k !== key));

  it.each([
    ['base_url', 'https://ghe.test/api/v3'],
    ['identity', 'dev@acme.test'],
  ])('accepts %s absent or a string and refuses null or another type', (key, value) => {
    expect(parseProviderAccount(account)).toBe(account);
    const present = { ...account, [key]: value };
    expect(parseProviderAccount(present)).toBe(present);
    for (const wrong of [null, 7, {}])
      expect(() => parseProviderAccount({ ...account, [key]: wrong })).toThrow(ServerResponseShapeError);
  });

  it.each([
    ['host', 'ghe.test', ''],
    ['managed', false, true],
  ] as const)('requires %s, which the server always sends', (key, value, other) => {
    for (const present of [value, other]) {
      const body = { ...account, [key]: present };
      expect(parseProviderAccount(body)).toBe(body);
    }
    expect(() => parseProviderAccount(without(key))).toThrow(ServerResponseShapeError);
    for (const wrong of [null, 7, 'yes'].filter((candidate) => typeof candidate !== typeof value))
      expect(() => parseProviderAccount({ ...account, [key]: wrong })).toThrow(ServerResponseShapeError);
  });

  it('refuses a whole account list carrying one drifted entry', () => {
    const list = [account, { ...account, id: 'github-app-02', base_url: 'https://ghe.test/api/v3' }];
    expect(parseProviderAccount.list(list)).toBe(list);
    expect(() => parseProviderAccount.list([...list, { ...account, id: 'github-app-03', host: null }])).toThrow(
      ServerResponseShapeError,
    );
    expect(parseProviderAccount.list([])).toEqual([]);
  });

  it('hands the endpoint helper only declared values: missing, value, legacy empty host', () => {
    // Missing base_url on a proven public host: the public default, no endpoint.
    expect(githubAccountEndpoint(parseProviderAccount(account))).toEqual({});
    // A present value transports unchanged.
    expect(
      githubAccountEndpoint(
        parseProviderAccount({ ...account, host: 'ghe.test', base_url: 'https://ghe.test/api/v3' }),
      ),
    ).toEqual({ apiBase: 'https://ghe.test/api/v3', webOrigin: 'https://ghe.test' });
    // The legacy unknown-site refusal (HS2-WV6QCR) still applies to a parsed account.
    expect(githubAccountEndpoint(parseProviderAccount({ ...account, host: '' }))).toHaveProperty('error');
  });
});

describe('server response parsers (HS2-34P6XY)', () => {
  it('HS2-1YXCCW validates the exact reviewed source pin set', () => {
    const review = {
      requests: [{ id: 'fixture', repository: 'https://example.test/rules', refs: ['v1.*'] }],
      pins: [
        {
          id: 'fixture-a',
          group_id: 'fixture',
          repository: 'https://example.test/rules',
          requested_ref: 'v1.0',
          commit: 'a'.repeat(40),
          rules: 2,
          skills: 1,
        },
      ],
    };
    expect(parseGuidanceBatchReview(review)).toEqual(review);
    expect(() => parseGuidanceBatchReview({ ...review, pins: [{ ...review.pins[0], commit: 3 }] })).toThrow(
      ServerResponseShapeError,
    );
  });
  it('HS2-B8M0QB accepts bounded other refs and rejects malformed discovery results', () => {
    const update = {
      id: 'fixture',
      requested_ref: 'main',
      pinned_commit: 'a'.repeat(40),
      observed_commit: null,
      state: 'pinned',
      other_refs: [{ reference: 'refs/tags/v2', commit: 'b'.repeat(40) }],
      other_refs_truncated: false,
    };
    expect(parseGuidanceSourceUpdates([update])).toEqual([update]);
    expect(() => parseGuidanceSourceUpdates([{ ...update, other_refs: [{ reference: 3, commit: 'b' }] }])).toThrow(
      ServerResponseShapeError,
    );
    expect(() => parseGuidanceSourceUpdates([{ ...update, other_refs_truncated: 'false' }])).toThrow(
      ServerResponseShapeError,
    );
  });
  it('HS2-019VVA validates read-only rule preview outcomes and bodies', () => {
    const preview = { owner: 'builtin', current: null, pinned: 'Pinned rule', after: 'Pinned rule', outcome: 'add' };
    expect(parseGuidanceRulePreview(preview)).toEqual(preview);
    expect(() => parseGuidanceRulePreview({ ...preview, outcome: 'applied' })).toThrow(ServerResponseShapeError);
    expect(() => parseGuidanceRulePreview({ ...preview, after: 42 })).toThrow(ServerResponseShapeError);
  });
  it('validates the project-local Rulesync rule inventory (HS2-H99RRW)', () => {
    const overview = {
      owner: 'rulesync',
      explicit_selection: false,
      default_repository: '',
      sources: [],
      local_rules: ['project-local'],
    };
    expect(parseGuidanceOverview(overview).local_rules).toEqual(['project-local']);
    expect(() => parseGuidanceOverview({ ...overview, local_rules: [42] })).toThrow(ServerResponseShapeError);
    expect(
      parseGuidanceOverview({ ...overview, pending_consolidation_review: true }).pending_consolidation_review,
    ).toBe(true);
    expect(() => parseGuidanceOverview({ ...overview, pending_consolidation_review: 'yes' })).toThrow(
      ServerResponseShapeError,
    );
  });
  it('validates the optional guidance interview contract (HS2-YGV9RN)', () => {
    const review = {
      state: 'awaiting_answer',
      preview: {
        fingerprint: 'f',
        context: {
          signals: ['react'],
          candidates: [
            {
              id: 'candidate-1',
              source: 'source',
              kind: 'rule',
              name: 'react-review',
              version: 1,
              pinned_commit: 'a',
              selected: false,
              evidence: ['package.json: react'],
            },
          ],
        },
        questions: [{ id: 'active-framework', prompt: 'Which?', choices: ['react', 'vue'] }],
        outbound_prompt: 'exact prompt',
      },
      answer: null,
    };
    expect(parseGuidanceInterviewReview(review)).toEqual(review);
    expect(() => parseGuidanceInterviewReview({ ...review, state: 'sent' })).toThrow(ServerResponseShapeError);
    expect(() =>
      parseGuidanceInterviewReview({
        ...review,
        preview: {
          ...review.preview,
          context: {
            ...review.preview.context,
            candidates: [{ ...review.preview.context.candidates[0], kind: 'command' }],
          },
        },
      }),
    ).toThrow(ServerResponseShapeError);
  });
  it('validates guidance recommendation decisions and review changes (HS2-QBXRFY)', () => {
    const report = {
      fingerprint: 'scan',
      signals: [{ key: 'rust', evidence: 'Cargo.toml' }],
      candidates: [
        {
          source: 'rules',
          kind: 'rule',
          name: 'rust-lint',
          version: 2,
          description: 'Rust checks',
          pinned_commit: 'abc',
          evidence: ['Cargo.toml'],
          fingerprint: 'candidate',
          selected: false,
          installed_version: null,
          state: 'new',
        },
      ],
      changes: [{ source: 'rules', kind: 'rule', name: 'rust-lint', change: 'new' }],
      source_registration_suggested: false,
      first_accept_replaces_bundled_default: true,
    };
    expect(parseGuidanceRecommendationReport(report)).toEqual(report);
    expect(() =>
      parseGuidanceRecommendationReport({ ...report, candidates: [{ ...report.candidates[0], state: 'unknown' }] }),
    ).toThrow(ServerResponseShapeError);
    expect(() =>
      parseGuidanceRecommendationReport({ ...report, changes: [{ ...report.changes[0], change: 'removed' }] }),
    ).toThrow(ServerResponseShapeError);
  });
  it('keeps standalone Rulesync lock state separate from the executable version', () => {
    expect(parseRulesyncUpdateReport(null)).toBeNull();
    const report = {
      state: 'outdated',
      executable_version: '38.0.0',
      sources: [
        { source: 'fixture', state: 'outdated', requested_ref: 'main', locked_ref: 'first', latest_ref: 'second' },
      ],
    };
    expect(parseRulesyncUpdateReport(report)).toEqual(report);
    expect(() =>
      parseRulesyncUpdateReport({ ...report, sources: [{ ...report.sources[0], state: 'updated' }] }),
    ).toThrow(ServerResponseShapeError);
  });
  it('accepts documented shapes and keeps extra fields', () => {
    const compatibility = {
      generation: 'hs2',
      build_revision: null,
      protocol: { min: 1, max: 1 },
      capabilities: { lifecycle_restart: true },
      extra: 1,
    };
    expect(parseServerCompatibility(compatibility)).toBe(compatibility);
    const full = {
      ...checkout,
      sources: [{ connection_id: 'git', provider: 'git', locator: '/s' }],
      default_source: 'git',
      unverified_store_sources: { git: '/old' },
    };
    expect(parseCheckout(full)).toBe(full);
    expect(parseCheckouts([checkout])).toEqual([checkout]);
    expect(parseOpenedCheckout({ checkout: { ...checkout, sources: [] } }).checkout.id).toBe('c');
    expect(parseCorruptTicketPaths([{ path: '/x.md', error: 'bad' }])).toEqual([{ path: '/x.md', error: 'bad' }]);
    expect(parseAccountIdentity({ identity: 'me' })).toEqual({ identity: 'me' });
    expect(parseTrashPurge({ purged: 1, tickets: ['a'] })).toEqual({ purged: 1, tickets: ['a'] });
    expect(parseGroupRename({ name: 'Board', updated: 2 })).toEqual({ name: 'Board', updated: 2 });
    expect(parseGroupUngroup({ updated: 2 })).toEqual({ updated: 2 });
    // A server before the registry (HS2-9KZSD4) lists names only: each reads as a legacy group.
    expect(parseGroupList({ groups: [{ name: 'Board', count: 2 }] })).toEqual({
      registry: false,
      groups: [{ id: 'Board', name: 'Board', count: 2, named: true, legacy: true, duplicate: false }],
    });
    const summary = { id: '7K3M9P2QXA', name: 'Sprint', count: 3, named: true, legacy: false, duplicate: true };
    expect(parseGroupList({ registry: true, groups: [summary] })).toEqual({ registry: true, groups: [summary] });
    expect(parseGroupList({ groups: [{ id: '7K3M9P2QXA', name: 'Group 7K3M9P2QXA', count: 1 }] }).groups[0]).toEqual({
      id: '7K3M9P2QXA',
      name: 'Group 7K3M9P2QXA',
      count: 1,
      named: true,
      legacy: false,
      duplicate: false,
    });
    expect(parseGroupCreate({ id: '7K3M9P2QXA', name: 'Sprint', created: true })).toEqual({
      id: '7K3M9P2QXA',
      name: 'Sprint',
      created: true,
    });
    expect(parseGroupRename({ id: '7K3M9P2QXA', name: 'Sprint', updated: 0 }).id).toBe('7K3M9P2QXA');
    expect(parseStringList(['a'])).toEqual(['a']);
  });

  it.each([
    ['compatibility', () => parseServerCompatibility({ generation: 2 })],
    ['compatibility', () => parseServerCompatibility({ generation: 'g', protocol: { min: '1' } })],
    ['compatibility', () => parseServerCompatibility({ generation: 'g', capabilities: { lifecycle_restart: 'y' } })],
    ['compatibility', () => parseServerCompatibility(undefined)],
    ['checkout', () => parseCheckout({ ...checkout, stores: 'one' })],
    ['checkout source', () => parseCheckout({ ...checkout, sources: [{ connection_id: 'g' }] })],
    ['checkout', () => parseCheckout({ ...checkout, unverified_store_sources: { g: 1 } })],
    ['checkout list', () => parseCheckouts({ checkouts: [] })],
    ['project open', () => parseOpenedCheckout({ checkout: { ...checkout } })],
    ['corrupt ticket', () => parseCorruptTicketPaths([{}])],
    ['account identity', () => parseAccountIdentity([])],
    ['trash purge', () => parseTrashPurge({ purged: '1', tickets: [] })],
    ['group rename', () => parseGroupRename({ name: 'Board', updated: '2' })],
    ['group ungroup', () => parseGroupUngroup({})],
    ['group list', () => parseGroupList({ groups: [{ name: 'Board' }] })],
    ['group list', () => parseGroupList({ groups: [{ id: 7, name: 'Board', count: 1 }] })],
    ['group list', () => parseGroupList({ groups: [{ id: 'X', name: 'Board', count: 1, duplicate: 'yes' }] })],
    ['group list', () => parseGroupList({ registry: 'yes', groups: [] })],
    ['group create', () => parseGroupCreate({ id: 'X', name: 'Board' })],
    ['group rename', () => parseGroupRename({ id: 1, name: 'Board', updated: 0 })],
    ['string list', () => parseStringList(['a', 1])],
  ])('rejects a malformed %s body', (what, parse) => {
    expect(parse).toThrow(new ServerResponseShapeError(what));
  });
});

describe('Api parsed endpoints (HS2-34P6XY)', () => {
  afterEach(() => vi.restoreAllMocks());
  const respond = (body: string, status = 200) =>
    vi.spyOn(globalThis, 'fetch').mockImplementation(async () => new Response(body, { status }));

  it('validates successful bodies before callers see them', async () => {
    respond(JSON.stringify({ generation: 'hs2' }));
    await expect(new Api('/api', '', { trackBusy: false }).compatibility()).resolves.toEqual({ generation: 'hs2' });
    respond(JSON.stringify({ generation: 7 }));
    await expect(new Api('/api', '', { trackBusy: false }).compatibility()).rejects.toBeInstanceOf(
      ServerResponseShapeError,
    );
    respond(JSON.stringify([1]));
    await expect(new Api('/api', '', { trackBusy: false }).commandGroups()).rejects.toThrow(
      'The server returned an invalid string list response.',
    );
  });

  it('sends the group registry routes their documented bodies (HS2-9KZSD4)', async () => {
    const fetch = vi.spyOn(globalThis, 'fetch'),
      api = new Api('/api', '', { trackBusy: false }),
      sent = () => {
        const [url, init] = fetch.mock.lastCall!;
        return [url as string, JSON.parse(init?.body as string) as Record<string, unknown>] as const;
      };
    fetch.mockImplementation(
      async () => new Response(JSON.stringify({ id: '7K3M9P2QXA', name: 'Sprint', created: true })),
    );
    await expect(api.createCheckoutGroup('c', 'Sprint')).resolves.toMatchObject({ id: '7K3M9P2QXA' });
    expect(sent()).toEqual(['/api/checkouts/c/groups', { name: 'Sprint' }]);
    fetch.mockImplementation(async () => new Response(JSON.stringify({ id: '7K3M9P2QXA', name: 'Next', updated: 0 })));
    await api.renameCheckoutGroup('c', '7K3M9P2QXA', 'Next');
    expect(sent()).toEqual([
      '/api/checkouts/c/groups/rename',
      { group: '7K3M9P2QXA', to: 'Next', actor: { role: 'human' } },
    ]);
    await api.mergeCheckoutGroups('c', '8B4N0R3SYB', '7K3M9P2QXA');
    expect(sent()).toEqual([
      '/api/checkouts/c/groups/merge',
      { from: '8B4N0R3SYB', into: '7K3M9P2QXA', actor: { role: 'human' } },
    ]);
    fetch.mockImplementation(async () => new Response(JSON.stringify({ updated: 2 })));
    await api.ungroupCheckoutGroup('c', '7K3M9P2QXA');
    expect(sent()).toEqual(['/api/checkouts/c/groups/ungroup', { group: '7K3M9P2QXA', actor: { role: 'human' } }]);
  });

  it('accepts empty acknowledgements and keeps rejecting non-JSON bodies', async () => {
    respond('');
    await expect(new Api('/api', '', { trackBusy: false }).signOutAccount('a')).resolves.toBeUndefined();
    respond('<html>');
    await expect(new Api('/api', '', { trackBusy: false }).commandGroups()).rejects.toBeInstanceOf(SyntaxError);
    respond(JSON.stringify({ error: 'nope' }), 409);
    await expect(new Api('/api', '', { trackBusy: false }).commandGroups()).rejects.toThrow('nope');
  });
});

describe('endpoint DTO parsers (HS2-E0AG5E)', () => {
  const ticket = {
    connection_id: 'git',
    native_id: '01',
    qualified_id: 'git:01',
    id: '01',
    slug: 'HS2-ONE',
    title: 'One',
    category: null,
    priority: null,
    status: 'not_started',
    up_next: false,
    feedback_needed: false,
    tags: [],
    blocked_by: [],
    claim_count: 0,
    notes: [{ id: 'n1', kind: 'regular' as const, text: 'hi', created_at: 't', edited_at: 't' }],
    attachments: [{ id: 'a1', filename: 'x.png', created_at: 't' }],
  };
  const valid: Array<[ResponseParser<unknown>, unknown]> = [
    [parseFullTicket, ticket],
    [parseStoredTicket, { ...ticket, store: '/s' }],
    [parseStoredTicket.list, [{ ...ticket, store: '/s' }]],
    // A `fields=` projection keeps only `slug` (and `store`).
    [parseTicketRow, { slug: 'HS2-ONE', store: '/s' }],
    [parseCheckoutTicketPage, { items: [{ slug: 'HS2-ONE' }], counts: { total: 1 }, next_cursor: 'v2.x' }],
    [parseCheckoutTicketRowsPage, { items: [], counts: null }],
    [parseTicket, { qualified_id: 'git:01', native_id: '01', connection_id: 'git', title: 'One', status: 'x' }],
    [
      parseProviderDescriptor,
      { connection_id: 'git', provider: 'git', display_name: 'Git', locator: '/s', default: true, capabilities: {} },
    ],
    [parseProviderConnection, { id: 'g', provider: 'github', locator: 'o/r', name: null, settings: {} }],
    [parseQueuedProviderResult, { operation_id: 'op', state: 'queued', ticket }],
    [parseProviderOutboxOperation, { operation_id: 'op', connection_id: 'j', native_id: 'E-1', state: 'queued' }],
    [parseCheckoutRelink, { checkouts: [checkout], connection_id: 'git' }],
    [parseRepositoryStatus, { initialized: false }],
    [parseRepositoryFilePage, { items: [{ path: 'a', untracked: true, conflicted: false }], next_cursor: null }],
    [parseRepositoryCommitPage, { items: [{ sha: 'abc', short_sha: 'a', subject: 's', committed_at: 't' }] }],
    [parsePollResponse, { cursor: 1, events: [{ kind: 'updated', store: 's', id: 'i', slug: 's' }], overflow: false }],
    [parseTerminalInfo.list, [{ id: 't', alive: true, busy: false }]],
    [parseTerminalInfo, { id: 't', alive: true, busy: false, mcp_connected: true }],
    [parseTerminalInfo, { id: 't', alive: true, busy: false, feedback_primary: true }],
    [parseTerminalRead, { id: 't', alive: true, busy: false, scrollback: '' }],
    [parseCommandRun, { id: 'r', command_id: 'c', state: 'running', output: [] }],
    // A recorded shell or AI launch carries its start time and destination (HS2-PWNPM2).
    [
      parseCommandRun,
      {
        id: 'r',
        command_id: 'c',
        state: 'completed',
        exit_code: null,
        output: [],
        started_at: '2026-10-28T10:00:00Z',
        launch: { kind: 'ai', ticket_id: '01', ticket_slug: 'HS2-AAAAAA', tool: 'claude' },
      },
    ],
    [parseAiToolDefaults, { tool: 'claude', providers: { claude: { model: 'opus' } } }],
    [parseGitHubAuthStatus, { state: 'pending' }],
    [parsePermissionDecision, { connection: 'c', decision: 'allow', persisted: false }],
    [
      parsePermissionRequest,
      {
        id: 1,
        connection: 'c',
        tool: 'Bash',
        action: 'TOKEN=abcd',
        suggestions: [{ start: 6, end: 10, label: 'Secret assignment', placeholder: 'token' }],
      },
    ],
  ];
  const invalid: Array<[ResponseParser<unknown>, unknown]> = [
    [parseFullTicket, { ...ticket, notes: undefined }],
    [parseFullTicket, { ...ticket, notes: [{ id: 'n1' }] }],
    [parseStoredTicket, { ...ticket, store: 7 }],
    [parseStoredTicket.list, { ...ticket, store: '/s' }],
    [parseTicketRow, { title: 'no slug' }],
    [parseTicketRow, { slug: 'HS2-ONE', tags: 'a' }],
    [parseCheckoutTicketPage, { items: [] }],
    // A page request never gets a bare row list from a supported server (HS2-5T7QHE).
    [parseCheckoutTicketPage, [{ slug: 'HS2-ONE' }]],
    [parseCheckoutTicketRowsPage, [{ slug: 'HS2-ONE' }]],
    [parseCheckoutRelink, { checkouts: [{ id: 'c' }], connection_id: 'git' }],
    [parseRepositoryStatus, { ahead: '1' }],
    [parsePollResponse, { cursor: 1, events: [{}], overflow: false }],
    [parseTerminalInfo, { id: 't', alive: 'yes', busy: false }],
    [parseCommandRun, { id: 'r', command_id: 'c', state: 'completed', output: [], launch: { kind: 'program' } }],
    [parseCommandRun, { id: 'r', command_id: 'c', state: 'completed', output: [], started_at: 7 }],
    [parseTerminalInfo, { id: 't', alive: true, busy: false, mcp_connected: 'yes' }],
    [parseTerminalInfo, { id: 't', alive: true, busy: false, feedback_primary: 'yes' }],
    [parsePermissionDecision, { connection: 'c', decision: 'maybe' }],
    [
      parsePermissionRequest,
      {
        id: 1,
        connection: 'c',
        tool: 'Bash',
        action: 'TOKEN=abcd',
        suggestions: [{ start: 6, end: 10, label: 'Secret assignment', placeholder: 'anything' }],
      },
    ],
    [parseTrashSettings, { trash_cleanup_days: Number.NaN }],
    [parseCustomView.list, null],
    [parseAiToolDefaults, { tool: 'claude', providers: { claude: 'opus' } }],
  ];
  it('accepts the server wire shapes, including nulls for absent optional fields', () => {
    for (const [parse, body] of valid) expect(parse(body)).toBe(body);
  });
  it('rejects bodies missing a required field or carrying a wrong-typed one', () => {
    for (const [parse, body] of invalid) expect(() => parse(body)).toThrow(ServerResponseShapeError);
  });
  it('rejects malformed endpoint bodies through Api before callers see them', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockImplementation(async () => new Response('{"store":"git","ticket":{}}', { status: 200 }));
    try {
      const api = new Api('/api', '', { trackBusy: false });
      await expect(api.checkoutTicket('c', 'HS2-ONE')).rejects.toBeInstanceOf(ServerResponseShapeError);
      await expect(api.terminals()).rejects.toBeInstanceOf(ServerResponseShapeError);
      await expect(api.pollEvents()).rejects.toBeInstanceOf(ServerResponseShapeError);
      await expect(api.transfer('copy', ticket, 'other')).resolves.toBeUndefined();
    } finally {
      fetchMock.mockRestore();
    }
  });
});

it('accepts the best-effort not_shared feedback delivery status (HS2-H9Z9KZ)', () => {
  expect(parseFeedbackDelivery({ status: 'not_shared', note_id: 'note-1' })).toEqual({
    status: 'not_shared',
    note_id: 'note-1',
  });
  expect(() => parseFeedbackDelivery({ status: 'not_shared' })).toThrow();
  expect(() => parseFeedbackDelivery({ status: 'unknown', note_id: 'note-1' })).toThrow();
});

describe('HS2-ADHFKZ ticket-store sync health parser matches the Rust SyncHealth wire shape', () => {
  const minimal = {
    state: 'pending',
    syncing: false,
    unpublished_commits: 2,
    incoming_commits: 0,
    consecutive_failures: 0,
  };
  const wire = (health: Record<string, unknown>) => ({
    checkout_id: 'web-1a2b3c',
    stores: [{ connection_id: 'git:web', label: 'web.hs2', locator: '/work/web.hs2', health }],
  });

  it('accepts the minimal shape (every skipped optional absent) and the full rejected shape', () => {
    expect(parseTicketStoreSyncReport(wire(minimal)).stores[0]?.health.state).toBe('pending');
    const full = {
      ...minimal,
      state: 'rejected',
      upstream: 'origin/main',
      last_attempt_at: '2026-11-10T01:00:00Z',
      last_success_at: '2026-11-09T01:00:00Z',
      consecutive_failures: 3,
      reason: 'remote: error: GH001',
      oversized: [{ path: 'attachments/a.bin', bytes: 118_971_148, limit_bytes: 104_857_600 }],
      next_action: 'Run hotsheet-cli sync.',
    };
    expect(parseTicketStoreSyncReport(wire(full)).stores[0]?.health.oversized?.[0]?.bytes).toBe(118_971_148);
    expect(parseTicketStoreSyncReport({ checkout_id: 'x', stores: [] }).stores).toEqual([]);
  });

  it('reads the HS2-QR1K8R AI repair ask and rejects an unknown outcome', () => {
    const ask = {
      outcome: 'started',
      mode: 'start',
      tool: 'codex',
      ticket: 'HS-ABC123',
      asked_at: '2026-11-13T08:00:00Z',
    };
    const health = parseTicketStoreSyncReport(wire({ ...minimal, state: 'conflict', ai_help: ask })).stores[0]?.health;
    expect(health.ai_help?.ticket).toBe('HS-ABC123');
    expect(() => parseTicketStoreSyncReport(wire({ ...minimal, ai_help: { ...ask, outcome: 'pending' } }))).toThrow();
    expect(() => parseTicketStoreSyncReport(wire({ ...minimal, ai_help: { ...ask, mode: 'always' } }))).toThrow();
  });

  it('reads the HS2-9TV6A2 merge driver status and rejects an unknown one', () => {
    const repaired = { ...minimal, merge_driver: 'registered', merge_driver_repaired_at: '2026-11-12T08:00:00Z' };
    const health = parseTicketStoreSyncReport(wire(repaired)).stores[0]?.health;
    expect(health.merge_driver).toBe('registered');
    expect(health.merge_driver_repaired_at).toBe('2026-11-12T08:00:00Z');
    expect(() => parseTicketStoreSyncReport(wire({ ...minimal, merge_driver: 'maybe' }))).toThrow();
  });

  it('rejects an unknown state, a null skipped field, and a malformed oversized entry', () => {
    expect(() => parseTicketStoreSyncReport(wire({ ...minimal, state: 'broken' }))).toThrow(ServerResponseShapeError);
    expect(() => parseTicketStoreSyncReport(wire({ ...minimal, reason: null }))).toThrow(ServerResponseShapeError);
    expect(() => parseTicketStoreSyncReport(wire({ ...minimal, oversized: [{ path: 'a' }] }))).toThrow(
      ServerResponseShapeError,
    );
    expect(() => parseTicketStoreSyncReport(wire({ ...minimal, syncing: 'no' }))).toThrow(ServerResponseShapeError);
  });
});
