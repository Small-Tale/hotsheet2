import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import { nextProjectAfterClose, pruneProjectRecords } from './project-teardown';

describe('pruneProjectRecords (HS2-DCTS07)', () => {
  it('drops only the closing ids from every record and keeps the rest intact', () => {
    const rows = signal<Record<string, number[]>>({ a: [1], b: [2], c: [3] }),
      flags = signal<Record<string, boolean>>({ b: true, c: false });
    pruneProjectRecords([rows, flags], new Set(['b']));
    expect(rows.value).toEqual({ a: [1], c: [3] });
    expect(flags.value).toEqual({ c: false });
  });

  it('replaces each record with a new object even when nothing closes, and handles empty records', () => {
    const before = { a: 1 },
      record = signal<Record<string, number>>(before),
      empty = signal<Record<string, number>>({});
    pruneProjectRecords([record, empty], new Set());
    expect(record.value).toEqual(before);
    expect(record.value).not.toBe(before);
    expect(empty.value).toEqual({});
  });
});

describe('nextProjectAfterClose (HS2-DCTS07)', () => {
  const tabs = ['a', 'b', 'c', 'd'].map((id) => ({ id }));
  const after = (...closing: string[]) => tabs.filter((tab) => !closing.includes(tab.id));

  it('prefers the nearest surviving tab to the right of the closed selection', () => {
    expect(nextProjectAfterClose(tabs, after('b'), 1)).toBe('c');
    expect(nextProjectAfterClose(tabs, after('b', 'c'), 1)).toBe('d');
  });

  it('falls back to the nearest tab on the left when nothing survives to the right', () => {
    expect(nextProjectAfterClose(tabs, after('d'), 3)).toBe('c');
    expect(nextProjectAfterClose(tabs, after('c', 'd'), 2)).toBe('b');
  });

  it('uses the first remaining tab when the selection was not found, and none when all close', () => {
    expect(nextProjectAfterClose(tabs, after('a'), -1)).toBe('b');
    expect(nextProjectAfterClose(tabs, [], 0)).toBe('');
  });
});
