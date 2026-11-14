import { describe, expect, it } from 'vitest';

import { parseRemoteProjectId, remoteProjectId, remoteProjectTabName } from './identity';

const first = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const second = '01ARZ3NDEKTSV4RRFFQ69G5FAW';

describe('HS2-BT4KN6 remote project identity', () => {
  it('keeps the same checkout on two reviewed servers separate and round trips reserved characters', () => {
    const left = remoteProjectId(first, 'same/checkout?name');
    const right = remoteProjectId(second, 'same/checkout?name');
    expect(left).not.toBe(right);
    expect(parseRemoteProjectId(left)).toEqual({ bindingId: first, checkoutId: 'same/checkout?name' });
    expect(parseRemoteProjectId(right)).toEqual({ bindingId: second, checkoutId: 'same/checkout?name' });
  });

  it('rejects aliases and malformed ids before a route is constructed', () => {
    expect(parseRemoteProjectId('remote:missing:checkout')).toBeUndefined();
    expect(parseRemoteProjectId(`remote:${first}:%ZZ`)).toBeUndefined();
    expect(parseRemoteProjectId(`remote:${first}:a%2fb`)).toBeUndefined();
    expect(() => remoteProjectId(first, '')).toThrow();
  });
});

describe('HS2-HGG426 remote project tab name', () => {
  it('names the remote host the peer reports, then the project', () => {
    expect(remoteProjectTabName('synthetic-studio', 'https://10.0.0.8:4174', 'hotsheet2')).toBe(
      'synthetic-studio · hotsheet2',
    );
    expect(remoteProjectTabName('  synthetic-studio  ', 'https://10.0.0.8:4174', 'hotsheet2')).toBe(
      'synthetic-studio · hotsheet2',
    );
  });

  it('falls back to the reviewed origin host when an older peer omits the host name', () => {
    expect(remoteProjectTabName(undefined, 'https://studio.example.ts.net:4174', 'hotsheet2')).toBe(
      'studio.example.ts.net · hotsheet2',
    );
    expect(remoteProjectTabName(null, 'http://10.0.0.8:4174', 'p')).toBe('10.0.0.8 · p');
    expect(remoteProjectTabName('   ', 'http://[::1]:4174', 'p')).toBe('[::1] · p');
    expect(remoteProjectTabName(undefined, 'not a url', 'p')).toBe('not a url · p');
  });
});
