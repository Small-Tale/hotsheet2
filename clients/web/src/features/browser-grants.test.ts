import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ChangeEvent } from '../api';
import {
  browserSessionGrants,
  browserSessionLifetime,
  resetBrowserSessionGrants,
  setBrowserSessionGrants,
} from '../client-origin';
import {
  BROWSER_GRANTS_EVENT,
  BROWSER_SESSION_PATH,
  createBrowserGrants,
  remoteSessionEndCode,
} from './browser-grants';

const event = (cursor: number | undefined, kind = BROWSER_GRANTS_EVENT): ChangeEvent => ({
  ...(cursor === undefined ? {} : { cursor }),
  store: '',
  kind,
  id: '',
  slug: '',
});

/** A host whose `/remote-auth/me` answers are released by the test, so reads can overlap events. */
function scriptedSession() {
  const pending: ((response: Response) => void)[] = [];
  const waiters: (() => void)[] = [];
  const request = vi.fn<typeof fetch>(
    () =>
      new Promise<Response>((resolve) => {
        pending.push(resolve);
        waiters.splice(0).forEach((wake) => {
          wake();
        });
      }),
  );
  /** Answer the oldest unanswered read, waiting for it to be sent. */
  const answer = async (response: Response) => {
    while (pending.length === 0) await new Promise<void>((resolve) => waiters.push(resolve));
    pending.shift()?.(response);
  };
  return { request, answer, pending };
}

const device = (grants: { admin?: boolean; local?: boolean }) =>
  Response.json({ id: 'SYNTHETIC', label: 'Synthetic tablet', role: 'read_write', ...grants });

describe('HS2-A3TDNH browser grants follow the change channel', () => {
  afterEach(() => {
    resetBrowserSessionGrants();
  });

  it('re-reads /remote-auth/me on a grants event and applies an added, then a removed grant', async () => {
    const host = scriptedSession();
    const grants = createBrowserGrants({ request: host.request, remoteMode: () => true });
    setBrowserSessionGrants({ admin: false, local: false });

    const added = grants.applyEvents([event(4)]);
    expect(host.request).toHaveBeenCalledWith(BROWSER_SESSION_PATH, { credentials: 'same-origin' });
    await host.answer(device({ admin: true, local: true }));
    await added;
    expect(browserSessionGrants()).toEqual({ admin: true, local: true });

    const removed = grants.applyEvents([event(5)]);
    await host.answer(device({ admin: false, local: false }));
    await removed;
    expect(browserSessionGrants()).toEqual({ admin: false, local: false });
    expect(host.request).toHaveBeenCalledTimes(2);
  });

  it('ignores other kinds and a grants event it already acted on through another stream', async () => {
    const host = scriptedSession();
    const grants = createBrowserGrants({ request: host.request, remoteMode: () => true });
    expect(grants.applyEvents([event(1, 'standby_settings'), event(2, 'ticket_updated')])).toBeUndefined();
    const first = grants.applyEvents([event(8)]);
    await host.answer(device({ admin: true }));
    await first;
    // The same event through a second project's stream, and an older one, read nothing.
    expect(grants.applyEvents([event(8)])).toBeUndefined();
    expect(grants.applyEvents([event(6)])).toBeUndefined();
    expect(host.request).toHaveBeenCalledTimes(1);
    // A cursorless (ephemeral) grants event always reads.
    const ephemeral = grants.applyEvents([event(undefined)]);
    await host.answer(device({ admin: false }));
    await ephemeral;
    expect(browserSessionGrants()).toEqual({ admin: false, local: false });
  });

  it('coalesces repeated changes during a read into exactly one more read, ending at the newest grants', async () => {
    const host = scriptedSession();
    const grants = createBrowserGrants({ request: host.request, remoteMode: () => true });
    const reading = grants.applyEvents([event(1)]);
    // Three more changes land while the first read is in flight.
    void grants.applyEvents([event(2)]);
    void grants.applyEvents([event(3), event(4)]);
    expect(host.request).toHaveBeenCalledTimes(1);
    await host.answer(device({ admin: true }));
    await host.answer(device({ admin: false, local: true }));
    await reading;
    expect(host.request).toHaveBeenCalledTimes(2);
    expect(browserSessionGrants()).toEqual({ admin: false, local: true });
  });

  it('re-reads on every resync, even at a cursor it saw before a server restart', async () => {
    const host = scriptedSession();
    const grants = createBrowserGrants({ request: host.request, remoteMode: () => true });
    const first = grants.applyEvents([event(30)]);
    await host.answer(device({ admin: true }));
    await first;
    // The stream reconnected (or overflowed): an event may have been missed meanwhile.
    const resync = grants.resync();
    await host.answer(device({ admin: false }));
    await resync;
    expect(browserSessionGrants()).toEqual({ admin: false, local: false });
    // A restarted server restarts its cursors; the low cursor is new after the resync.
    const restarted = grants.applyEvents([event(2)]);
    expect(restarted).toBeDefined();
    await host.answer(device({ admin: true }));
    await restarted;
    expect(browserSessionGrants()).toEqual({ admin: true, local: false });
  });

  it('drops every grant when the session itself ended, and keeps the last grants on any other failure', async () => {
    const host = scriptedSession();
    const grants = createBrowserGrants({ request: host.request, remoteMode: () => true });
    setBrowserSessionGrants({ admin: true, local: true });
    for (const failure of [
      new Response('busy', { status: 503 }),
      new Response('forbidden', { status: 403 }),
      new Response('not json', { status: 200 }),
    ]) {
      const read = grants.refresh();
      await host.answer(failure);
      await read;
    }
    // A 200 with an unreadable body is still an answer: it carries no grant.
    expect(browserSessionGrants()).toEqual({ admin: false, local: false });
    setBrowserSessionGrants({ admin: true, local: true });
    const offline = createBrowserGrants({
      request: vi.fn<typeof fetch>(() => Promise.reject(new TypeError('offline'))),
      remoteMode: () => true,
    });
    await offline.refresh();
    expect(browserSessionGrants()).toEqual({ admin: true, local: true });
    const revoked = grants.refresh();
    await host.answer(Response.json({ error: 'invalid pairing or session' }, { status: 401 }));
    await revoked;
    expect(browserSessionGrants()).toEqual({ admin: false, local: false });
  });

  it('HS2-4B0KSD reports an ended session by kind and never a host that does not answer', async () => {
    const host = scriptedSession();
    const ended = vi.fn();
    const grants = createBrowserGrants({ request: host.request, remoteMode: () => true, onSessionEnded: ended });
    for (const failure of [
      Response.json({ error: 'The Hot Sheet host is not answering.', code: 'host_unavailable' }, { status: 503 }),
      new Response('forbidden', { status: 403 }),
    ]) {
      const read = grants.refresh();
      await host.answer(failure);
      await read;
    }
    expect(ended).not.toHaveBeenCalled();
    for (const code of ['session_expired', 'session_revoked', undefined]) {
      const read = grants.refresh();
      await host.answer(Response.json({ error: 'Remote session required.', code }, { status: 401 }));
      await read;
    }
    expect(ended.mock.calls.map((call) => call[0])).toEqual(['session_expired', 'session_revoked', 'session_invalid']);
    expect(remoteSessionEndCode('not an object')).toBe('session_invalid');
  });

  it('HS2-4B0KSD records the session lifetime with its grants', () => {
    setBrowserSessionGrants({
      admin: false,
      local: false,
      renewed_at: 10,
      expires_at: 20,
      idle_expires_at: 30,
      renewal_requested_at: 15,
    });
    expect(browserSessionLifetime()).toEqual({
      renewed_at: 10,
      expires_at: 20,
      idle_expires_at: 30,
      renewal_requested_at: 15,
    });
    setBrowserSessionGrants({ admin: false, local: false });
    expect(browserSessionLifetime()?.renewed_at).toBe(10);
    resetBrowserSessionGrants();
    expect(browserSessionLifetime()).toBeUndefined();
  });

  it('never reads on a local-mode host, whose own browser has no session', async () => {
    const host = scriptedSession();
    const grants = createBrowserGrants({ request: host.request, remoteMode: () => false });
    await grants.applyEvents([event(1)]);
    await grants.resync();
    await grants.refresh();
    expect(host.request).not.toHaveBeenCalled();
    expect(browserSessionGrants()).toBeUndefined();
  });
});
