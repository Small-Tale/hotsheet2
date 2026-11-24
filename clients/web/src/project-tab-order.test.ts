import { describe, expect, it } from 'vitest';

import { loadProjectTabOrder, PROJECT_TAB_ORDER_KEY, projectTabRanks, saveProjectTabOrder } from './project-tab-order';
import { REMOTE_TAB_RANK_BASE } from './remote-projects/reconnect';

const memory = (initial?: string) => {
  const values = new Map<string, string>(initial === undefined ? [] : [[PROJECT_TAB_ORDER_KEY, initial]]);
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
};

describe('remembered project tab order (HS2-NRBWGF)', () => {
  it('reads nothing, malformed, or non-string entries as an empty or cleaned order', () => {
    expect(loadProjectTabOrder(memory())).toEqual([]);
    expect(loadProjectTabOrder(memory('{bad'))).toEqual([]);
    expect(loadProjectTabOrder(memory('{"a":1}'))).toEqual([]);
    expect(loadProjectTabOrder(memory('["/a", 3, "", "remote:b", "/a"]'))).toEqual(['/a', 'remote:b']);
  });

  it('saves a de-duplicated order and survives a storage that throws', () => {
    const storage = memory();
    saveProjectTabOrder(storage, ['/a', 'remote:b', '/a']);
    expect(loadProjectTabOrder(storage)).toEqual(['/a', 'remote:b']);
    expect(() => {
      saveProjectTabOrder(
        {
          setItem: () => {
            throw new Error('quota');
          },
        },
        ['/a'],
      );
    }).not.toThrow();
  });

  it('ranks a mixed order exactly, and unnamed tabs after it in their old relative order', () => {
    const rank = projectTabRanks(['/local-b', 'remote:x', '/local-a']);
    // A remote tab dragged between two local tabs keeps that place on reload.
    const tabs = [
      { key: '/local-a', fallback: 0 },
      { key: '/local-b', fallback: 1 },
      { key: '/local-new', fallback: 2 },
      { key: 'remote:x', fallback: REMOTE_TAB_RANK_BASE },
      { key: 'remote:new', fallback: REMOTE_TAB_RANK_BASE + 1 },
    ];
    const ordered = [...tabs].sort((left, right) => rank(left.key, left.fallback) - rank(right.key, right.fallback));
    expect(ordered.map((tab) => tab.key)).toEqual(['/local-b', 'remote:x', '/local-a', '/local-new', 'remote:new']);
  });

  it('keeps the old ranks when nothing was saved, so remote tabs still follow local ones', () => {
    const rank = projectTabRanks([]);
    expect(rank('/a', 0)).toBe(0);
    expect(rank('/b', 1)).toBe(1);
    expect(rank('remote:x', REMOTE_TAB_RANK_BASE)).toBe(REMOTE_TAB_RANK_BASE);
  });
});
