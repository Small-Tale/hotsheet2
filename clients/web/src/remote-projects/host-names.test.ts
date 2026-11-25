import { describe, expect, it } from 'vitest';

import { loadRemoteHostNames, rememberRemoteHostNames, REMOTE_HOST_NAMES_KEY, saveRemoteHostNames } from './host-names';
import { remoteProjectId } from './identity';

const A = '01ARZ3NDEKTSV4RRFFQ69G5FAV',
  B = '01ARZ3NDEKTSV4RRFFQ69G5FAW';

describe('remote host-name cache (HS2-GN0WKS)', () => {
  it('reads nothing, malformed, or non-string entries as a cleaned cache', () => {
    const storage = (value: string | null) => ({ getItem: () => value });
    expect(loadRemoteHostNames(storage(null))).toEqual({});
    expect(loadRemoteHostNames(storage('{bad'))).toEqual({});
    expect(loadRemoteHostNames(storage('[1]'))).toEqual({});
    expect(loadRemoteHostNames(storage(JSON.stringify({ [A]: 'Studio', [B]: 3, x: ' ' })))).toEqual({ [A]: 'Studio' });
  });

  it('records open remote projects by binding, ignores local ones, and returns the same cache when unchanged', () => {
    const projects = [
      { id: remoteProjectId(A, 'one'), remoteHost: 'Studio' },
      { id: remoteProjectId(A, 'two'), remoteHost: 'Studio' },
      { id: 'local-checkout', remoteHost: 'ignored' },
      { id: remoteProjectId(B, 'one') },
    ];
    const first = rememberRemoteHostNames({}, projects);
    expect(first).toEqual({ [A]: 'Studio' });
    expect(rememberRemoteHostNames(first, projects)).toBe(first);
    // A renamed host replaces its entry.
    expect(rememberRemoteHostNames(first, [{ id: remoteProjectId(A, 'one'), remoteHost: 'Studio Mac' }])).toEqual({
      [A]: 'Studio Mac',
    });
  });

  it('round-trips through storage and survives a storage that throws', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value);
      },
    };
    saveRemoteHostNames(storage, { [A]: 'Studio' });
    expect(values.has(REMOTE_HOST_NAMES_KEY)).toBe(true);
    expect(loadRemoteHostNames(storage)).toEqual({ [A]: 'Studio' });
    expect(() => {
      saveRemoteHostNames(
        {
          setItem: () => {
            throw new Error('quota');
          },
        },
        {},
      );
    }).not.toThrow();
  });
});
