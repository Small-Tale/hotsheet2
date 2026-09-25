import { describe, expect, it } from 'vitest';

import { createProjectWarmCache, PROJECT_WARM_CACHE_CAPACITY } from './project-warm-cache';

describe('project warm cache LRU (HS2-AZZ9TF)', () => {
  it('keeps a sensible default bound and clamps invalid capacities to one', () => {
    expect(createProjectWarmCache().capacity).toBe(PROJECT_WARM_CACHE_CAPACITY);
    expect(PROJECT_WARM_CACHE_CAPACITY).toBeGreaterThanOrEqual(4);
    expect(createProjectWarmCache(0).capacity).toBe(1);
    expect(createProjectWarmCache(2.7).capacity).toBe(2);
  });

  it('touch orders by recency and evicts the least recently used beyond the bound', () => {
    const cache = createProjectWarmCache(3);
    expect(cache.touch('a')).toEqual([]);
    expect(cache.touch('b')).toEqual([]);
    expect(cache.touch('c')).toEqual([]);
    expect(cache.ids()).toEqual(['c', 'b', 'a']);
    // Revisiting A (A→B→C→A) refreshes it, so B becomes the eviction candidate.
    expect(cache.touch('a')).toEqual([]);
    expect(cache.ids()).toEqual(['a', 'c', 'b']);
    expect(cache.touch('d')).toEqual(['b']);
    expect(cache.has('b')).toBe(false);
    expect(cache.ids()).toEqual(['d', 'a', 'c']);
  });

  it('repeated touches of the active project are idempotent', () => {
    const cache = createProjectWarmCache(2);
    cache.touch('a');
    for (let index = 0; index < 5; index += 1) expect(cache.touch('a')).toEqual([]);
    expect(cache.ids()).toEqual(['a']);
  });

  it('admit fills spare capacity as least recent without displacing visited projects', () => {
    const cache = createProjectWarmCache(3);
    cache.touch('active');
    expect(cache.admit('bg1')).toBe(true);
    expect(cache.admit('bg2')).toBe(true);
    expect(cache.admit('bg3')).toBe(false);
    expect(cache.ids()).toEqual(['active', 'bg1', 'bg2']);
    // Admitting an already-warm id reports warm without changing recency.
    expect(cache.admit('active')).toBe(true);
    expect(cache.ids()).toEqual(['active', 'bg1', 'bg2']);
    // A real visit to a cold project evicts the oldest prefetched one first.
    expect(cache.touch('bg3')).toEqual(['bg2']);
  });

  it('forget frees capacity, and empty-then-refill sequences behave like a fresh cache', () => {
    const cache = createProjectWarmCache(2);
    cache.touch('a');
    cache.touch('b');
    cache.forget('a');
    cache.forget('missing');
    expect(cache.ids()).toEqual(['b']);
    expect(cache.admit('c')).toBe(true);
    cache.forget('b');
    cache.forget('c');
    expect(cache.ids()).toEqual([]);
    expect(cache.touch('x')).toEqual([]);
    expect(cache.touch('y')).toEqual([]);
    expect(cache.touch('z')).toEqual(['x']);
  });

  it('rapid interleaved switching never exceeds the bound or duplicates ids', () => {
    const cache = createProjectWarmCache(3),
      evicted: string[] = [];
    const sequence = ['a', 'b', 'a', 'c', 'b', 'd', 'a', 'e', 'e', 'b', 'f', 'a'];
    for (const id of sequence) {
      evicted.push(...cache.touch(id));
      const ids = cache.ids();
      expect(ids.length).toBeLessThanOrEqual(3);
      expect(new Set(ids).size).toBe(ids.length);
      expect(ids[0]).toBe(id);
    }
    expect(cache.ids()).toEqual(['a', 'f', 'b']);
    for (const id of cache.ids()) expect(evicted.at(-1)).not.toBe(id);
  });
});
