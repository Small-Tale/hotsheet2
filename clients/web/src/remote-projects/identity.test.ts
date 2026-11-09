import { describe, expect, it } from 'vitest';

import { parseRemoteProjectId, remoteProjectId } from './identity';

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
