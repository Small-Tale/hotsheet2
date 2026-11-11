import { beforeEach, describe, expect, it } from 'vitest';

import {
  collapsedTicketGroupKeys,
  collapsedTicketGroups,
  readCollapsedTicketGroups,
  reportedTicketGroups,
  resetTicketGroupSurfaces,
  ticketGroupCollapsed,
  ticketGroupDialog,
  ticketGroupDropTarget,
  toggleTicketGroupCollapsed,
} from './ticket-group-state';

function memoryStorage(initial?: string) {
  const values = new Map<string, string>(initial === undefined ? [] : [['hotsheet.ticket-groups.collapsed', initial]]);
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    values,
  };
}

describe('ticket group collapse preference (HS2-WZ3JGZ)', () => {
  beforeEach(() => {
    collapsedTicketGroups.value = new Set();
  });

  it('toggles per project, view, and group key, and persists each change', () => {
    const storage = memoryStorage();
    expect(ticketGroupCollapsed('p', 'all', 'Board')).toBe(false);
    toggleTicketGroupCollapsed('p', 'all', 'Board', storage);
    expect(ticketGroupCollapsed('p', 'all', ' board ')).toBe(true);
    expect(ticketGroupCollapsed('p', 'backlog', 'Board')).toBe(false);
    expect(ticketGroupCollapsed('q', 'all', 'Board')).toBe(false);
    expect(readCollapsedTicketGroups(storage)).toEqual(collapsedTicketGroups.value);
    // Repeated, interleaved toggles: collapse a second group, expand the first, collapse it again.
    toggleTicketGroupCollapsed('p', 'all', 'Search', storage);
    toggleTicketGroupCollapsed('p', 'all', 'BOARD', storage);
    expect(collapsedTicketGroupKeys('p', 'all', ['Board', 'Search', 'Other'])).toEqual(['search']);
    toggleTicketGroupCollapsed('p', 'all', 'Board', storage);
    expect(collapsedTicketGroupKeys('p', 'all', ['Board', 'Search'])).toEqual(['board', 'search']);
    expect(readCollapsedTicketGroups(storage).size).toBe(2);
    toggleTicketGroupCollapsed('p', 'all', 'Board', storage);
    toggleTicketGroupCollapsed('p', 'all', 'Search', storage);
    expect(readCollapsedTicketGroups(storage).size).toBe(0);
  });

  it('reads missing, malformed, and mixed stored values as a clean set', () => {
    expect(readCollapsedTicketGroups(undefined).size).toBe(0);
    expect(readCollapsedTicketGroups(memoryStorage()).size).toBe(0);
    expect(readCollapsedTicketGroups(memoryStorage('{not json')).size).toBe(0);
    expect(readCollapsedTicketGroups(memoryStorage('{"a":1}')).size).toBe(0);
    expect([...readCollapsedTicketGroups(memoryStorage('["a",1,null,"b"]'))]).toEqual(['a', 'b']);
    expect(
      readCollapsedTicketGroups({
        getItem: () => {
          throw new Error('blocked');
        },
      }).size,
    ).toBe(0);
  });

  it('keeps the choice for this page when storage refuses the write', () => {
    toggleTicketGroupCollapsed('p', 'all', 'Board', {
      setItem: () => {
        throw new Error('quota');
      },
    });
    expect(ticketGroupCollapsed('p', 'all', 'Board')).toBe(true);
    toggleTicketGroupCollapsed('p', 'all', 'Board', undefined);
    expect(ticketGroupCollapsed('p', 'all', 'Board')).toBe(false);
  });

  it('clears transient surfaces without touching preferences or reported groups', () => {
    toggleTicketGroupCollapsed('p', 'all', 'Board', undefined);
    reportedTicketGroups.value = { p: ['Board'] };
    ticketGroupDropTarget.value = { scope: 'list', key: 'board' };
    ticketGroupDialog.value = { mode: 'new', name: '', groups: [], session: 1, projectId: 'p', slugs: ['HS2-A'] };
    resetTicketGroupSurfaces();
    expect(ticketGroupDropTarget.value).toBeUndefined();
    expect(ticketGroupDialog.value).toBeUndefined();
    expect(ticketGroupCollapsed('p', 'all', 'Board')).toBe(true);
    expect(reportedTicketGroups.value).toEqual({ p: ['Board'] });
    reportedTicketGroups.value = {};
  });
});
