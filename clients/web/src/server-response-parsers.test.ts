import { afterEach, describe, expect, it, vi } from 'vitest';

import { Api } from './api';
import {
  parseAccountIdentity,
  parseCheckout,
  parseCheckouts,
  parseCorruptTicketPaths,
  parseOpenedCheckout,
  parseServerCompatibility,
  parseStringList,
  parseTrashPurge,
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
