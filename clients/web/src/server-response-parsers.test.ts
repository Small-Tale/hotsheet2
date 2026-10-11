import { afterEach, describe, expect, it, vi } from 'vitest';

import { Api } from './api';
import {
  parseAccountIdentity,
  parseAiToolDefaults,
  parseCheckout,
  parseCheckoutRelink,
  parseCheckouts,
  parseCheckoutTicketPage,
  parseCheckoutTicketRowsPage,
  parseCommandRun,
  parseCorruptTicketPaths,
  parseCustomView,
  parseFullTicket,
  parseGitHubAuthStatus,
  parseOpenedCheckout,
  parsePermissionDecision,
  parsePollResponse,
  parseProviderConnection,
  parseProviderDescriptor,
  parseProviderOutboxOperation,
  parseQueuedProviderResult,
  parseRepositoryCommitPage,
  parseRepositoryFilePage,
  parseRepositoryStatus,
  parseServerCompatibility,
  parseStoredTicket,
  parseStringList,
  parseTerminalInfo,
  parseTerminalRead,
  parseTicket,
  parseTicketRow,
  parseTrashPurge,
  parseTrashSettings,
  type ResponseParser,
  ServerResponseShapeError,
} from './server-response-parsers';

const checkout = { id: 'c', root: '/w/c', alias: 'c', stores: ['/w/c.hs2'] };

describe('server response parsers (HS2-34P6XY)', () => {
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
    [parseTerminalRead, { id: 't', alive: true, busy: false, scrollback: '' }],
    [parseCommandRun, { id: 'r', command_id: 'c', state: 'running', output: [] }],
    [parseAiToolDefaults, { tool: 'claude', providers: { claude: { model: 'opus' } } }],
    [parseGitHubAuthStatus, { state: 'pending' }],
    [parsePermissionDecision, { connection: 'c', decision: 'allow', persisted: false }],
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
    [parsePermissionDecision, { connection: 'c', decision: 'maybe' }],
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
