import { describe, expect, it } from 'vitest';

import { announcerDigest, announcerItem, mergeAnnouncerReel, retainedAnnouncerReel } from './announcer-reel';
import type { ActivityEvent } from './api';

const event = (id: string, ts: string, importance: ActivityEvent['importance'] = 'normal'): ActivityEvent => ({
  id,
  ts,
  tool: 'codex',
  kind: 'edit',
  summary: `Changed synthetic ${id}`,
  importance,
});

describe('HS2-5PSQJQ Announcer reel', () => {
  it('merges linked project windows in time order without narrating a shared store twice', () => {
    const first = event('A', '2026-10-01T10:00:00Z');
    const second = event('B', '2026-10-01T10:01:00Z', 'high');
    const reel = retainedAnnouncerReel([
      { id: 'one', name: 'One', events: [{ store_id: 'shared', event: first }] },
      {
        id: 'two',
        name: 'Two',
        events: [
          { store_id: 'second', event: second },
          { store_id: 'shared', event: first },
        ],
      },
    ]);
    expect(reel.map((item) => item.key)).toEqual(['shared:A', 'second:B']);
    expect(announcerDigest(reel).map((item) => item.key)).toEqual(['shared:A', 'second:B']);
  });

  it('deduplicates a replay, caps the reel, and only shows diff detail when the event contains it', () => {
    const before = announcerItem('one', 'One', 'store', event('A', '2026-10-01T10:00:00Z', 'low'));
    const changed = announcerItem('one', 'One', 'store', {
      ...event('B', '2026-10-01T10:01:00Z'),
      detail: { path: 'src/example.ts', before: 'one', after: 'two' },
    });
    expect(before.change).toBeUndefined();
    expect(changed.change).toEqual({ path: 'src/example.ts', before: 'one', after: 'two', diff: undefined });
    const replayed = mergeAnnouncerReel([before, changed], [changed], 1);
    expect(replayed.map((item) => item.key)).toEqual(['store:B']);
    expect(announcerDigest([before, changed])).toEqual([changed]);
  });
});
