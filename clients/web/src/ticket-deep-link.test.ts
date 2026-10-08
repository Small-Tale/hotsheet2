import { describe, expect, it } from 'vitest';

import type { Checkout } from './api';
import { parseTicketDeepLink, ticketDeepLinkRoot } from './ticket-deep-link';

describe('ticket deep links', () => {
  it('decodes an exact ticket and project root, rejecting incomplete links', () => {
    expect(parseTicketDeepLink('?store=%2Fwork%2FUX+Review&ticket=UX-123')).toEqual({
      store: '/work/UX Review',
      ticket: 'UX-123',
    });
    expect(parseTicketDeepLink('?store=project')).toBeUndefined();
    expect(parseTicketDeepLink('?ticket=UX-123')).toBeUndefined();
    expect(parseTicketDeepLink('?store=%20&ticket=UX-123')).toBeUndefined();
  });

  it('resolves a checkout id or alias and accepts an absolute path', () => {
    const checkouts = [{ id: 'registered-id', alias: 'ux', root: '/work/ux', stores: ['/data/ux.hs2'] }] as Checkout[];
    expect(ticketDeepLinkRoot('registered-id', checkouts)).toBe('/work/ux');
    expect(ticketDeepLinkRoot('ux', checkouts)).toBe('/work/ux');
    expect(ticketDeepLinkRoot('/data/ux.hs2', checkouts)).toBe('/work/ux');
    expect(ticketDeepLinkRoot('/work/other', checkouts)).toBe('/work/other');
    expect(ticketDeepLinkRoot('unknown-id', checkouts)).toBeUndefined();
  });
});
