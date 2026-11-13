import { afterEach, describe, expect, it } from 'vitest';

import {
  browserSessionGrants,
  canManageRemoteDevices,
  isLoopbackHostname,
  isRemoteClient,
  opensProjectsRemotely,
  resetBrowserSessionGrants,
  setBrowserSessionGrants,
  stampedBrowserAuthMode,
} from './client-origin';

describe('client origin', () => {
  it('treats loopback hostnames as same-device', () => {
    for (const host of ['localhost', '127.0.0.1', '::1', '[::1]', 'LOCALHOST', 'app.localhost']) {
      expect(isLoopbackHostname(host), host).toBe(true);
    }
  });

  it('treats routable hostnames as remote', () => {
    for (const host of ['100.101.102.103', '192.168.1.5', 'my-mac.tailabc.ts.net', 'example.com']) {
      expect(isLoopbackHostname(host), host).toBe(false);
    }
  });

  it('derives the remote flag from a location', () => {
    expect(isRemoteClient({ hostname: 'localhost' })).toBe(false);
    expect(isRemoteClient({ hostname: '100.64.0.1' })).toBe(true);
  });

  it('honors the ?device override', () => {
    expect(isRemoteClient({ hostname: 'localhost', search: '?device=remote' })).toBe(true);
    expect(isRemoteClient({ hostname: '100.64.0.1', search: '?device=local' })).toBe(false);
  });
});

describe('HS2-3HC9HT stamped browser auth mode', () => {
  const doc = (content?: string) => ({
    querySelector: () => (content === undefined ? null : ({ content } as HTMLMetaElement)),
  });

  it('reads only the two stamped modes', () => {
    expect(stampedBrowserAuthMode(doc('remote') as Pick<Document, 'querySelector'>)).toBe('remote');
    expect(stampedBrowserAuthMode(doc('local') as Pick<Document, 'querySelector'>)).toBe('local');
    expect(stampedBrowserAuthMode(doc('other') as Pick<Document, 'querySelector'>)).toBeUndefined();
    expect(stampedBrowserAuthMode(doc() as Pick<Document, 'querySelector'>)).toBeUndefined();
  });

  it('opens projects locally when there is no document', () => {
    expect(opensProjectsRemotely()).toBe(false);
  });
});

describe('HS2-85W6BE session grants decide local versus remote on a remote-mode host', () => {
  const doc = (content?: string) =>
    ({
      querySelector: () => (content === undefined ? null : ({ content } as HTMLMetaElement)),
    }) as Pick<Document, 'querySelector'>;
  const loopback = { hostname: '127.0.0.1' };
  const tailnet = { hostname: 'synthetic-host.example.ts.net' };
  afterEach(() => {
    resetBrowserSessionGrants();
  });

  it('treats a loopback page as remote until its session proves the local grant', () => {
    expect(isRemoteClient(loopback, doc('remote'))).toBe(true);
    setBrowserSessionGrants({ id: 'synthetic', local: false, admin: true });
    expect(isRemoteClient(loopback, doc('remote'))).toBe(true);
    setBrowserSessionGrants({ id: 'synthetic', local: true, admin: false });
    expect(isRemoteClient(loopback, doc('remote'))).toBe(false);
    // The grant, not the hostname, decides: the same session through Serve is local too.
    expect(isRemoteClient(tailnet, doc('remote'))).toBe(false);
    resetBrowserSessionGrants();
    expect(isRemoteClient(tailnet, doc('remote'))).toBe(true);
  });

  it('keeps hostname detection on a local-mode host and honors the override everywhere', () => {
    setBrowserSessionGrants({ local: true });
    expect(isRemoteClient(tailnet, doc('local'))).toBe(true);
    expect(isRemoteClient(loopback, doc('local'))).toBe(false);
    expect(isRemoteClient({ ...loopback, search: '?device=remote' }, doc('remote'))).toBe(true);
    resetBrowserSessionGrants();
    expect(isRemoteClient({ ...tailnet, search: '?device=local' }, doc('remote'))).toBe(false);
  });

  it('reads only literal true grants', () => {
    expect(setBrowserSessionGrants({ local: 'yes', admin: 1 })).toEqual({ local: false, admin: false });
    expect(setBrowserSessionGrants(undefined)).toEqual({ local: false, admin: false });
    expect(browserSessionGrants()).toEqual({ local: false, admin: false });
  });

  it('lets only the local-mode owner or an admin session manage devices (HS2-DXM0YN)', () => {
    expect(canManageRemoteDevices(doc('local'))).toBe(true);
    expect(canManageRemoteDevices(undefined)).toBe(true);
    expect(canManageRemoteDevices(doc('remote'))).toBe(false);
    setBrowserSessionGrants({ local: true, admin: false });
    expect(canManageRemoteDevices(doc('remote'))).toBe(false);
    setBrowserSessionGrants({ local: false, admin: true });
    expect(canManageRemoteDevices(doc('remote'))).toBe(true);
  });
});
