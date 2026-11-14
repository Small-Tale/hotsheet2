import { afterEach, describe, expect, it, vi } from 'vitest';

import { beginRemotePairing, normalizeRemoteAddress } from './host';
import { remoteRequest } from './transport';

vi.mock('./transport', () => ({ remoteRequest: vi.fn() }));

const SERVER_ID = '01ARZ3NDEKTSV4RRFFQ69G5FAW';

afterEach(() => vi.mocked(remoteRequest).mockReset());

describe('HS2-H4REBS remote server address', () => {
  it('accepts an address without a port and drops a default port from the origin', () => {
    expect(normalizeRemoteAddress('https://peer.example.ts.net')).toBe('https://peer.example.ts.net');
    expect(normalizeRemoteAddress('https://peer.example.ts.net/')).toBe('https://peer.example.ts.net');
    expect(normalizeRemoteAddress('https://peer.example.ts.net:443')).toBe('https://peer.example.ts.net');
    expect(normalizeRemoteAddress('http://peer.example.test')).toBe('http://peer.example.test');
    expect(normalizeRemoteAddress('http://peer.example.test:4174')).toBe('http://peer.example.test:4174');
    expect(normalizeRemoteAddress('https://[::1]:8443')).toBe('https://[::1]:8443');
  });

  it('still refuses paths, credentials, queries, fragments and other schemes', () => {
    for (const value of [
      'https://peer.example.ts.net/path',
      'https://user@peer.example.ts.net',
      'https://user:pass@peer.example.ts.net',
      'https://peer.example.ts.net?query',
      'https://peer.example.ts.net#fragment',
      'ftp://peer.example.test',
    ])
      expect(() => normalizeRemoteAddress(value), value).toThrow(
        'Use a protocol and host, with an optional port, and no path or credentials.',
      );
    for (const value of ['peer.example.ts.net', ' https://peer.example.ts.net', 'https://peer\\example'])
      expect(() => normalizeRemoteAddress(value), value).toThrow('Enter an explicit HTTP or HTTPS server address.');
  });

  it('pairs over publicly trusted HTTPS without a CA file or a pre-session identity read', async () => {
    vi.mocked(remoteRequest).mockResolvedValueOnce(
      Response.json({ id: 'ABCD1234', challenge: 'synthetic-challenge', server_id: SERVER_ID, expires_at: 9e9 }),
    );
    const pairing = await beginRemotePairing('https://peer.example.ts.net', 'Synthetic', 'http://127.0.0.1:4174');
    expect(pairing).toMatchObject({
      approvalId: 'ABCD1234',
      serverId: SERVER_ID,
      origin: 'https://peer.example.ts.net',
    });
    const calls = vi.mocked(remoteRequest).mock.calls;
    // `/remote-identity` is authenticated; the CLI reads it after approval (docs/08 §8.2.1).
    expect(calls.map(([url]) => url.toString())).toEqual(['https://peer.example.ts.net/remote-auth/pair']);
    expect(new Headers(calls[0]?.[1].headers).get('origin')).toBe('http://127.0.0.1:4174');
    // No CA, certificate or key: Node verifies the peer against its public roots.
    expect(calls[0]?.[2]).toEqual({ ca_pem: undefined, client_certificate_pem: undefined, client_key_pem: undefined });
  });

  it('refuses a pairing answer without a valid server id or challenge', async () => {
    for (const answer of [
      { id: 'ABCD1234', challenge: 'c', server_id: 'not-a-ulid', expires_at: 9e9 },
      { id: 'ABCD1234', challenge: '', server_id: SERVER_ID, expires_at: 9e9 },
      { id: '', challenge: 'c', server_id: SERVER_ID, expires_at: 9e9 },
    ]) {
      vi.mocked(remoteRequest).mockResolvedValueOnce(Response.json(answer));
      await expect(
        beginRemotePairing('https://peer.example.ts.net', 'Synthetic', 'http://127.0.0.1:4174'),
      ).rejects.toThrow('This server is not a compatible Hot Sheet 2 peer.');
    }
  });
});
