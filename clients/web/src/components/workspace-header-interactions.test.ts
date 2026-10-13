import { describe, expect, it, vi } from 'vitest';

import type { WorkspaceSort, WorkspaceSortDirection } from './workspace-controls';
import { wireWorkspaceHeaderActions } from './workspace-header-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
}));
const handler = (event: string) => registrations.find((item) => item.event === event)!.handle;
const overflow = (dataset: Record<string, string>) => {
  const item = { dataset };
  handler('wa-select')({ detail: { item } }, {});
  return item;
};

describe('wireWorkspaceHeaderActions (HS2-AAX2B5)', () => {
  let current: { sort: WorkspaceSort; direction: WorkspaceSortDirection } = {
    sort: 'updated',
    direction: 'descending',
  };
  const bindings = {
    currentSort: () => current,
    sort: vi.fn((next: typeof current) => {
      current = next;
    }),
    toggleSelectedUpNext: vi.fn(),
    openSelectedTicketActions: vi.fn(),
    openSearch: vi.fn(),
    setViewMode: vi.fn(),
  };
  wireWorkspaceHeaderActions({} as HTMLElement, bindings);

  it('cycles the sort from the menu and the overflow, flipping direction on a repeated choice', () => {
    handler('click')({}, { value: 'priority' });
    const first = current;
    overflow({ workspaceOverflowAction: 'set-workspace-sort', workspaceSort: 'priority' });
    expect(current.sort).toBe('priority');
    expect(current.direction).not.toBe(first.direction);
    overflow({ workspaceOverflowAction: 'set-workspace-sort' });
    expect(bindings.sort).toHaveBeenCalledTimes(2);
  });

  it('routes each overflow action and ignores unknown or incomplete ones', () => {
    overflow({ workspaceOverflowAction: 'toggle-selected-up-next' });
    const anchor = overflow({ workspaceOverflowAction: 'open-selected-ticket-actions' });
    overflow({ workspaceOverflowAction: 'open-workspace-search' });
    overflow({ workspaceOverflowAction: 'set-view-mode', viewMode: 'board' });
    overflow({ workspaceOverflowAction: 'set-view-mode' });
    overflow({ workspaceOverflowAction: 'unknown' });
    expect(bindings.toggleSelectedUpNext).toHaveBeenCalledTimes(1);
    expect(bindings.openSelectedTicketActions).toHaveBeenCalledWith(anchor);
    expect(bindings.openSearch).toHaveBeenCalledTimes(1);
    expect(bindings.setViewMode.mock.calls).toEqual([['board']]);
  });
});
