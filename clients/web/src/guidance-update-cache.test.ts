import { describe, expect, it } from 'vitest';

import { guidanceUpdateBuildIdentity, guidanceUpdateCacheIsFresh } from './guidance-update-cache';

describe('guidance update cache freshness', () => {
  const now = 100_000_000;
  const entry = { checkedAt: now - 1_000, buildIdentity: 'build-a', updates: [] };

  it('reuses a same-build result within 24 hours and expires it at the boundary', () => {
    expect(guidanceUpdateCacheIsFresh(entry, 'build-a', now)).toBe(true);
    expect(guidanceUpdateCacheIsFresh({ ...entry, checkedAt: now - 86_400_000 }, 'build-a', now)).toBe(false);
  });

  it('rechecks after an upgrade or when no trustworthy build identity exists', () => {
    expect(guidanceUpdateCacheIsFresh(entry, 'build-b', now)).toBe(false);
    expect(guidanceUpdateCacheIsFresh(entry, undefined, now)).toBe(false);
    expect(guidanceUpdateCacheIsFresh({ ...entry, buildIdentity: undefined }, 'build-a', now)).toBe(false);
  });

  it('tracks host and server revisions independently', () => {
    const base = { kind: 'compatible' as const, revisionMismatch: false, sourceStale: false, canRestartServer: false };
    const first = guidanceUpdateBuildIdentity({
      ...base,
      clientRevision: 'host-a',
      server: { generation: 'hs2', build_revision: 'server-a' },
    });
    const hostUpgrade = guidanceUpdateBuildIdentity({
      ...base,
      clientRevision: 'host-b',
      server: { generation: 'hs2', build_revision: 'server-a' },
    });
    const serverUpgrade = guidanceUpdateBuildIdentity({
      ...base,
      clientRevision: 'host-a',
      server: { generation: 'hs2', build_revision: 'server-b' },
    });
    expect(first).not.toBe(hostUpgrade);
    expect(first).not.toBe(serverUpgrade);
    expect(guidanceUpdateBuildIdentity(base)).toBeUndefined();
  });

  it('rejects future and invalid timestamps', () => {
    expect(guidanceUpdateCacheIsFresh({ ...entry, checkedAt: now + 1 }, 'build-a', now)).toBe(false);
    expect(guidanceUpdateCacheIsFresh({ ...entry, checkedAt: Number.NaN }, 'build-a', now)).toBe(false);
  });
});
