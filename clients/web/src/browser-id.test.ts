import { afterEach, describe, expect, it, vi } from 'vitest';

import { browserRandomId } from './browser-id';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('browser identities across secure and LAN origins (HS2-3ZBQDG)', () => {
  it('uses native UUIDs when available and preserves their receiver', () => {
    const id = '12345678-1234-4234-9234-123456789abc',
      source = {
        getRandomValues: crypto.getRandomValues.bind(crypto),
        randomUUID() {
          expect(this).toBe(source);
          return id as ReturnType<Crypto['randomUUID']>;
        },
      };
    expect(browserRandomId(source)).toBe(id);
  });

  it('generates fresh RFC4122 version-4 identities with only the LAN-safe crypto API', () => {
    let sequence = 0;
    const source = {
      getRandomValues<T extends ArrayBufferView>(array: T): T {
        new Uint8Array(array.buffer, array.byteOffset, array.byteLength).fill(++sequence);
        return array;
      },
    };
    vi.stubGlobal('crypto', source);
    const ids = Array.from({ length: 32 }, () => browserRandomId());
    expect(new Set(ids).size).toBe(32);
    expect(ids[0]).toBe('01010101-0101-4101-8101-010101010101');
    for (const id of ids) expect(id).toMatch(/^[\da-f]{8}-[\da-f]{4}-4[\da-f]{3}-[89ab][\da-f]{3}-[\da-f]{12}$/);
  });

  it('keeps legacy identities distinct when the clock and random source repeat', () => {
    vi.stubGlobal('crypto', undefined);
    vi.spyOn(Date, 'now').mockReturnValue(123);
    vi.spyOn(Math, 'random').mockReturnValue(0.5);
    const ids = Array.from({ length: 32 }, () => browserRandomId());
    expect(new Set(ids).size).toBe(32);
    expect(ids.every((id) => id.startsWith('client-3f-'))).toBe(true);
    vi.spyOn(Date, 'now').mockReturnValue(1);
    expect(ids).not.toContain(browserRandomId());
  });

  it('does not silently weaken an available but failing crypto source', () => {
    expect(() =>
      browserRandomId({
        getRandomValues: () => {
          throw new Error('entropy unavailable');
        },
      }),
    ).toThrow('entropy unavailable');
  });
});
