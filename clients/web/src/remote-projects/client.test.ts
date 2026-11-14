import { afterEach, describe, expect, it, vi } from 'vitest';

import { desktopRemoteBeginPair, desktopRemoteServers, remoteRequestFailure } from './client';

afterEach(() => vi.unstubAllGlobals());

function reply(body: string, status: number, type = 'application/json') {
  const fetch = vi.fn<typeof globalThis.fetch>(
    async () => new Response(body, { status, headers: { 'content-type': type } }),
  );
  vi.stubGlobal('fetch', fetch);
  return fetch;
}

describe('HS2-HBM3JN desktop remote client replies', () => {
  it('turns a plain-text 404 into a readable message, never a parser error', async () => {
    reply('404 Not Found', 404, 'text/plain');
    await expect(desktopRemoteServers()).rejects.toThrow(
      'This Hot Sheet host does not let this browser manage remote servers.',
    );
  });

  it('passes the host JSON error through and names the status otherwise', async () => {
    reply(
      JSON.stringify({ error: 'Use a protocol and host, with an optional port, and no path or credentials.' }),
      400,
    );
    await expect(desktopRemoteBeginPair('https://synthetic.example', 'Synthetic')).rejects.toThrow(
      'Use a protocol and host, with an optional port, and no path or credentials.',
    );
    for (const body of ['', '<html></html>', JSON.stringify({ error: '' }), JSON.stringify(['no'])]) {
      reply(body, 502, 'text/html');
      await expect(desktopRemoteServers(), body).rejects.toThrow('Remote request failed (502).');
    }
  });

  it('rejects a non-JSON success instead of returning garbage', async () => {
    for (const body of ['', '<!doctype html><html></html>']) {
      reply(body, 200, 'text/html');
      await expect(desktopRemoteServers(), body).rejects.toThrow(
        'This Hot Sheet host sent an unexpected reply. Reload and try again.',
      );
    }
  });

  it('returns the parsed body and posts JSON', async () => {
    const fetch = reply(JSON.stringify([{ id: 'synthetic' }]), 200);
    expect(await desktopRemoteServers()).toEqual([{ id: 'synthetic' }]);
    expect(fetch).toHaveBeenCalledWith(
      '/__hotsheet/remotes',
      expect.objectContaining({ headers: expect.any(Headers) }),
    );
  });

  it('names each status without a JSON body', () => {
    expect(remoteRequestFailure(401)).toBe('Sign in to this Hot Sheet host again, then retry.');
    expect(remoteRequestFailure(403)).toBe('This Hot Sheet host refused the request. Reload and try again.');
    expect(remoteRequestFailure(500)).toBe('Remote request failed (500).');
  });
});
