import { describe, expect, it } from 'vitest';

import {
  drawerInputFocusRequestStillOwned,
  drawerSelectionView,
  drawerTabCloseIds,
  drawerTabFocusRequestStillOwned,
  drawerTabOrderStorageKey,
  drawerTabSelectionAfterClose,
  isPendingDrawerTabId,
  loadDrawerTabOrder,
  orderedDrawerTabIds,
  parseDrawerTabOrder,
  type PendingDrawerTab,
  reorderDrawerTabIds,
  saveDrawerTabOrder,
  selectedDrawerInput,
  visiblePendingDrawerTabs,
} from './drawer-tab-order';

describe('drawer tab ordering', () => {
  it('applies one remembered order across terminal and AI-chat tabs and appends new tabs', () => {
    expect(
      orderedDrawerTabIds(
        ['terminal-a', 'terminal-b', 'terminal-c'],
        ['chat-a'],
        ['terminal-b', 'chat-a', 'terminal-a'],
      ),
    ).toEqual(['terminal-b', 'chat-a', 'terminal-a', 'terminal-c']);
  });

  it('reorders across kinds for the shared tab-bar pointer and keyboard contract', () => {
    const ids = ['terminal-a', 'terminal-b', 'chat-a'];
    expect(reorderDrawerTabIds(ids, 'chat-a', 'terminal-a', 'before')).toEqual(['chat-a', 'terminal-a', 'terminal-b']);
    expect(reorderDrawerTabIds(ids, 'chat-a', 'terminal-b', 'before')).toEqual(['terminal-a', 'chat-a', 'terminal-b']);
    expect(reorderDrawerTabIds(ids, 'terminal-a', 'missing', 'before')).toEqual(ids);
  });

  it('does not let deferred reorder focus steal a newer user focus', () => {
    const body = {},
      scheduled = { isConnected: true },
      newFocus = {};
    expect(drawerTabFocusRequestStillOwned(scheduled, scheduled, body)).toBe(true);
    expect(drawerTabFocusRequestStillOwned(scheduled, body, body)).toBe(true);
    expect(drawerTabFocusRequestStillOwned(scheduled, newFocus, body)).toBe(false);
  });

  it('cancels an older input-focus frame after keyboard close while allowing the newest request', () => {
    const body = {},
      oldTab = {},
      newTab = {};
    expect(drawerInputFocusRequestStillOwned(1, 1, oldTab, body, body)).toBe(true);
    expect(drawerInputFocusRequestStillOwned(1, 2, oldTab, body, body)).toBe(false);
    expect(drawerInputFocusRequestStillOwned(1, 2, oldTab, newTab, body)).toBe(false);
    expect(drawerInputFocusRequestStillOwned(3, 3, newTab, newTab, body)).toBe(true);
  });

  it('finds the selected terminal or writable chat input without focusing inactive surfaces', () => {
    const terminal = {} as HTMLElement,
      chat = {} as HTMLElement,
      terminalDrawer = {
        querySelector: (selector: string) => (selector.includes('terminal-session') ? terminal : null),
      } as unknown as ParentNode,
      chatDrawer = {
        querySelector: (selector: string) => (selector.includes('ai-conversation') ? chat : null),
      } as unknown as ParentNode,
      gridDrawer = { querySelector: () => null } as unknown as ParentNode;

    expect(selectedDrawerInput(terminalDrawer)).toBe(terminal);
    expect(selectedDrawerInput(chatDrawer)).toBe(chat);
    expect(selectedDrawerInput(gridDrawer)).toBeUndefined();
  });

  it('selects the nearest live tab after a close, preferring the right neighbor', () => {
    const ids = ['terminal-a', 'chat-a', 'terminal-b', 'chat-b'];
    expect(drawerTabSelectionAfterClose(ids, 'chat-a', ['chat-a'])).toBe('terminal-b');
    expect(drawerTabSelectionAfterClose(ids, 'terminal-b', ['terminal-b', 'chat-b'])).toBe('chat-a');
    expect(drawerTabSelectionAfterClose(ids, 'chat-a', ['terminal-a', 'chat-a', 'terminal-b', 'chat-b'])).toBe('grid');
    expect(drawerTabSelectionAfterClose(ids, 'terminal-a', ['chat-a'])).toBe('terminal-a');
  });

  it('targets every relative close action across one mixed terminal and chat order', () => {
    const ids = ['terminal-a', 'chat-a', 'terminal-b', 'chat-b'];
    expect(drawerTabCloseIds(ids, 'chat-a', 'close')).toEqual(['chat-a']);
    expect(drawerTabCloseIds(ids, 'chat-a', 'close-others')).toEqual(['terminal-a', 'terminal-b', 'chat-b']);
    expect(drawerTabCloseIds(ids, 'chat-a', 'close-left')).toEqual(['terminal-a']);
    expect(drawerTabCloseIds(ids, 'chat-a', 'close-right')).toEqual(['terminal-b', 'chat-b']);
    expect(drawerTabCloseIds(ids, 'chat-a', 'close-all')).toEqual(ids);
    expect(drawerTabCloseIds(ids, 'missing', 'close-all')).toEqual([]);
  });

  it('persists a deduplicated device-local order and tolerates invalid data', () => {
    const values = new Map<string, string>(),
      storage = {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => {
          values.set(key, value);
        },
      };
    expect(saveDrawerTabOrder(storage, 'project', ['terminal-a', 'chat-a', 'terminal-a'])).toEqual([
      'terminal-a',
      'chat-a',
    ]);
    expect(values.get(drawerTabOrderStorageKey('project'))).toBe('["terminal-a","chat-a"]');
    expect(loadDrawerTabOrder(storage, 'project')).toEqual(['terminal-a', 'chat-a']);
    expect(parseDrawerTabOrder('{')).toEqual([]);
    expect(parseDrawerTabOrder('["one",1,"one",""]')).toEqual(['one']);
  });
});

describe('pending drawer tabs (HS2-AQ40G7)', () => {
  const live = new Set(['t1', 'ai-chat:c']);
  const pending: PendingDrawerTab = { id: 'pending:1', name: 'Claude shell', kind: 'terminal' };

  it('shows a pending placeholder, never the grid, from request through arrival of the created item', () => {
    // Requested: the pending tab itself is selected.
    expect(drawerSelectionView('pending:1', live, [pending])).toEqual({ kind: 'pending', tab: pending });
    expect(visiblePendingDrawerTabs([pending], live)).toEqual([pending]);
    // Created but not yet in the snapshot: the selection names the new id and still shows the placeholder.
    const resolved = { ...pending, resolvedId: 't2' };
    expect(drawerSelectionView('t2', live, [resolved])).toEqual({ kind: 'pending', tab: resolved });
    expect(visiblePendingDrawerTabs([resolved], live)).toEqual([resolved]);
    // Arrived: the live item replaces the placeholder in place.
    const arrived = new Set([...live, 't2']);
    expect(drawerSelectionView('t2', arrived, [resolved])).toEqual({ kind: 'live', id: 't2' });
    expect(visiblePendingDrawerTabs([resolved], arrived)).toEqual([]);
  });

  it('keeps live selections and the explicit grid, and falls back to the grid for an unknown id', () => {
    expect(drawerSelectionView('ai-chat:c', live, [pending])).toEqual({ kind: 'live', id: 'ai-chat:c' });
    expect(drawerSelectionView('grid', live, [pending])).toEqual({ kind: 'grid' });
    expect(drawerSelectionView('gone', live, [])).toEqual({ kind: 'grid' });
    // Removing the placeholder (a failed create) leaves its id unknown.
    expect(drawerSelectionView('pending:1', live, [])).toEqual({ kind: 'grid' });
    expect(isPendingDrawerTabId('pending:1')).toBe(true);
    expect(isPendingDrawerTabId('t1')).toBe(false);
  });
});
