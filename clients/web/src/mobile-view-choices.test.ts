import { describe, expect, it } from 'vitest';

import { mobileViewChoices } from './mobile-view-choices';

const base = { selectedView: 'all', trashCount: 0, corruptCount: 0, customViews: [] };
const values = (input: Parameters<typeof mobileViewChoices>[0]) => mobileViewChoices(input).map((c) => c.value);

describe('narrow-layout view picker choices (HS2-0VPMFS)', () => {
  it('lists the standard views, then Trash, custom views, and Ticket errors like the sidebar', () => {
    expect(values(base)).toEqual(['all', 'backlog', 'archive']);
    expect(
      mobileViewChoices({
        ...base,
        trashCount: 2,
        corruptCount: 1,
        customViews: [{ value: 'custom:mine', label: 'Mine' }],
      }),
    ).toEqual([
      { value: 'all', label: 'Queue' },
      { value: 'backlog', label: 'Backlog' },
      { value: 'archive', label: 'Archive' },
      { value: 'trash', label: 'Trash' },
      { value: 'custom:mine', label: 'Mine' },
      { value: 'errors', label: 'Ticket errors' },
    ]);
  });

  it('keeps an open Trash or Ticket errors view selectable after its count drops to zero', () => {
    expect(values({ ...base, selectedView: 'trash' })).toContain('trash');
    expect(values({ ...base, selectedView: 'errors' })).toContain('errors');
    expect(values({ ...base, selectedView: 'backlog' })).not.toContain('errors');
  });
});
