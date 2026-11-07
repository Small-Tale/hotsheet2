import { describe, expect, it, vi } from 'vitest';

import type { ChangeEvent, PollResponse } from './api';
import {
  createHandshakeGapReconciler,
  isTicketCollectionRead,
  responseChangeCursor,
  TicketReadCursors,
} from './change-read-gap';

const event = (cursor: number, kind = 'updated'): ChangeEvent => ({
  cursor,
  kind,
  store: 's',
  id: `t${cursor}`,
  slug: 'S',
});

const reconciler = (log: ChangeEvent[], options: { overflow?: boolean; fail?: boolean } = {}) => {
  const replays: number[] = [],
    pages: PollResponse[] = [],
    errors: unknown[] = [];
  const gap = createHandshakeGapReconciler({
    replay: async (since) => {
      replays.push(since);
      if (options.fail) throw new Error('replay failed');
      const events = log.filter((item) => item.cursor! > since);
      return {
        cursor: Math.max(since, ...log.map((item) => item.cursor!)),
        events,
        overflow: options.overflow ?? false,
      };
    },
    reconcile: async (page) => {
      pages.push(page);
    },
    onError: (reason) => errors.push(reason),
  });
  return { gap, replays, pages, errors };
};

describe('handshake gap continuity (HS2-SQKJYG)', () => {
  it('recognizes ticket collection reads and parses only valid stamps', () => {
    expect(isTicketCollectionRead('GET', '/checkouts/a/tickets?page_size=100')).toBe(true);
    expect(isTicketCollectionRead('get', '/tickets')).toBe(true);
    expect(isTicketCollectionRead('POST', '/checkouts/a/tickets')).toBe(false);
    expect(isTicketCollectionRead('GET', '/checkouts/a/tickets/HS-1')).toBe(false);
    expect(responseChangeCursor(new Headers({ 'x-hotsheet-change-cursor': '42' }))).toBe(42);
    expect(responseChangeCursor(new Headers())).toBeUndefined();
    expect(responseChangeCursor(new Headers({ 'x-hotsheet-change-cursor': '-1' }))).toBeUndefined();
    expect(responseChangeCursor(new Headers({ 'x-hotsheet-change-cursor': '1e3' }))).toBeUndefined();
    expect(responseChangeCursor(new Headers({ 'x-hotsheet-change-cursor': '99999999999999999999' }))).toBeUndefined();
  });

  it('an idle start (read stamped at the handshake) replays nothing', async () => {
    const { gap, replays, pages } = reconciler([]);
    gap.read(7);
    gap.handshake(7);
    gap.read(9);
    await gap.settled();
    expect(replays).toEqual([]);
    expect(pages).toEqual([]);
  });

  it('load then handshake: replays only the gap and hands its events to reconcile', async () => {
    const { gap, replays, pages } = reconciler([event(4), event(5, 'live_state'), event(6)]);
    gap.read(3);
    gap.handshake(5);
    await gap.settled();
    expect(replays).toEqual([3]);
    // Event 6 is after the handshake: the stream itself delivers it.
    expect(pages).toEqual([{ cursor: 5, overflow: false, events: [event(4), event(5, 'live_state')] }]);
  });

  it('handshake then a slower read stamped before it: replays that read gap once', async () => {
    const { gap, replays, pages } = reconciler([event(2), event(3)]);
    gap.handshake(3);
    await gap.settled();
    expect(replays).toEqual([]);
    gap.read(1);
    // A second page of the same load, stamped later, adds no replay; an earlier one replays only the rest.
    gap.read(2);
    gap.read(0);
    await gap.settled();
    expect(replays).toEqual([1, 0]);
    // The second replay covers only (0, 1], which holds no events, so it does not reconcile.
    expect(pages.map((page) => page.events.map((item) => item.cursor))).toEqual([[2, 3]]);
  });

  it('an empty gap page does not reconcile; an overflowed one does', async () => {
    const quiet = reconciler([]);
    quiet.gap.read(1);
    quiet.gap.handshake(4);
    await quiet.gap.settled();
    expect(quiet.replays).toEqual([1]);
    expect(quiet.pages).toEqual([]);

    const lost = reconciler([], { overflow: true });
    lost.gap.read(1);
    lost.gap.handshake(4);
    await lost.gap.settled();
    expect(lost.pages).toEqual([{ cursor: 4, events: [], overflow: true }]);
  });

  it('a failed replay reconciles as an overflow rather than losing the gap', async () => {
    const { gap, pages, errors } = reconciler([], { fail: true });
    gap.read(1);
    gap.handshake(2);
    await gap.settled();
    expect(errors).toHaveLength(1);
    expect(pages).toEqual([{ cursor: 2, events: [], overflow: true }]);
  });

  it('only the first handshake counts, and a stopped stream never reconciles', async () => {
    const { gap, replays, pages } = reconciler([event(2)]);
    gap.handshake(5);
    gap.handshake(1);
    gap.read(1);
    gap.stop();
    await gap.settled();
    expect(replays).toEqual([]);
    expect(pages).toEqual([]);
  });

  it('read stamps: kept lowest before a follower, routed to it while followed, kept again after', () => {
    const cursors = new TicketReadCursors(),
      seen = vi.fn();
    cursors.note('/a', 5);
    cursors.note('/a', 3);
    cursors.note('/a', 4);
    cursors.note('/b', 9);
    const unfollow = cursors.follow('/a', seen);
    expect(seen.mock.calls).toEqual([[3]]);
    cursors.note('/a', 8);
    expect(seen.mock.calls).toEqual([[3], [8]]);
    unfollow();
    cursors.note('/a', 8);
    expect(seen).toHaveBeenCalledTimes(2);
    // A reopened project's load precedes its new stream again: that stream starts from it.
    cursors.note('/a', 12);
    const next = vi.fn(),
      unfollowNext = cursors.follow('/a', next);
    expect(next.mock.calls).toEqual([[8]]);
    unfollowNext();
    const other = vi.fn();
    cursors.follow('/b', other);
    expect(other.mock.calls).toEqual([[9]]);
  });
});
