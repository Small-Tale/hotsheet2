import { describe, expect, it } from 'vitest';

import { isLoopbackHostname, isRemoteClient } from './client-origin';

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
