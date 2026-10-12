import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  type TicketCollectionBindings,
  type TicketCollectionMenu,
  wireTicketCollection,
} from './ticket-collection-interactions';

interface Registration {
  root: unknown;
  event: string;
  selector: string;
  handle: (event: Event, target: Element) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ root, event, selector, handle });
    return () => undefined;
  },
}));
const rowSelector = '[data-action="select-ticket-row"]';
function handler(event: string, selector: string) {
  const found = registrations.find((item) => item.event === event && item.selector === selector);
  expect(found, `${event} ${selector}`).toBeDefined();
  return found!.handle;
}
const rows = new Map<string, HTMLElement>();
function row(slug: string) {
  const element = {
    dataset: { ticketSlug: slug },
    closest: () => null,
    focus: () => focused.push(slug),
  } as unknown as HTMLElement;
  rows.set(slug, element);
  return element;
}
const root = {
  querySelector: (selector: string) => {
    const slug = /data-ticket-slug="([^"]+)"/.exec(selector)?.[1];
    return slug ? (rows.get(slug) ?? null) : null;
  },
} as unknown as HTMLElement;
/** A plain event record; `insideControl` makes its target sit inside a nested control. */
const event = (type: string, init: Record<string, unknown> = {}, insideControl = false) =>
  ({
    type,
    target: { closest: () => (insideControl ? {} : null) },
    shiftKey: false,
    metaKey: false,
    ctrlKey: false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    stopImmediatePropagation: vi.fn(),
    ...init,
  }) as unknown as Event;
const focused: string[] = [];

function bindings(selected: string[] = []) {
  let menu: TicketCollectionMenu | undefined;
  const value = {
    selectedSlugs: () => selected,
    selectionOrder: () => ['A', 'B', 'C'],
    select: vi.fn(),
    isSelectAll: (keyboard: KeyboardEvent) => keyboard.metaKey && keyboard.key === 'a',
    selectAll: vi.fn(),
    selectColumn: vi.fn(),
    openReader: vi.fn(),
    contextMenu: () => menu,
    setContextMenu: vi.fn((next: TicketCollectionMenu | undefined) => {
      menu = next;
    }),
    onContextField: vi.fn(),
    onContextAction: vi.fn(),
  } satisfies TicketCollectionBindings;
  return value;
}

beforeEach(() => {
  registrations.length = 0;
  rows.clear();
  focused.length = 0;
  vi.stubGlobal('window', { innerWidth: 1440, innerHeight: 900 });
  vi.stubGlobal('CSS', { escape: (value: string) => value });
});
afterEach(() => vi.unstubAllGlobals());

describe('wireTicketCollection (HS2-RWY78R)', () => {
  it('selects with click modifiers but ignores the row Up Next star', () => {
    const state = bindings();
    wireTicketCollection(root, state);
    const click = handler('click', rowSelector);
    click(event('click', { shiftKey: true }), row('B'));
    expect(state.select).toHaveBeenCalledWith(
      'B',
      { range: true, toggle: false },
      ['A', 'B', 'C'],
      rows.get('B'),
      expect.anything(),
    );
    click(event('click', {}, true), row('C'));
    expect(state.select).toHaveBeenCalledOnce();
  });

  it('walks arrows to the adjacent row, select-all, and Enter toggle', () => {
    const state = bindings();
    wireTicketCollection(root, state);
    const keydown = handler('keydown', rowSelector);
    row('A');
    row('C');
    keydown(event('keydown', { key: 'ArrowDown', shiftKey: true }), row('B'));
    expect(focused).toEqual(['C']);
    expect(state.select).toHaveBeenLastCalledWith(
      'C',
      { range: true },
      ['A', 'B', 'C'],
      rows.get('C'),
      expect.anything(),
    );
    keydown(event('keydown', { key: 'ArrowUp' }), rows.get('A')!);
    // The top row clamps to itself.
    expect(state.select).toHaveBeenLastCalledWith(
      'A',
      { range: false },
      ['A', 'B', 'C'],
      rows.get('A'),
      expect.anything(),
    );
    keydown(event('keydown', { key: 'a', metaKey: true }), rows.get('A')!);
    expect(state.selectAll).toHaveBeenCalledOnce();
    keydown(event('keydown', { key: 'Enter', metaKey: true }), rows.get('A')!);
    expect(state.select).toHaveBeenLastCalledWith(
      'A',
      { range: false, toggle: true },
      ['A', 'B', 'C'],
      rows.get('A'),
      expect.anything(),
    );
  });

  it('opens a context menu, selecting an unselected row first, then routes field and action choices', () => {
    const state = bindings(['A', 'B']);
    wireTicketCollection(root, state);
    const contextmenu = handler('contextmenu', rowSelector);
    contextmenu(event('contextmenu', { clientX: 10, clientY: 20 }), row('A'));
    expect(state.select).not.toHaveBeenCalled();
    expect(state.contextMenu()).toMatchObject({ ticketSlug: 'A' });
    const field = { dataset: { contextField: 'priority', contextValue: 'high' } } as unknown as Element;
    handler('click', '[data-context-field]')(event('click'), field);
    expect(state.onContextField).toHaveBeenCalledWith('priority', 'high', ['A', 'B']);
    expect(state.contextMenu()).toBeUndefined();
    // A stale action after the menu closed does nothing.
    const action = { dataset: { contextAction: 'Archive ticket' } } as unknown as Element;
    handler('click', '[data-context-action]')(event('click'), action);
    expect(state.onContextAction).not.toHaveBeenCalled();
    contextmenu(event('contextmenu', { clientX: 10, clientY: 20 }), row('C'));
    expect(state.select).toHaveBeenCalledWith('C', {}, ['A', 'B', 'C'], rows.get('C'), expect.anything());
    handler('click', '[data-context-action]')(event('click'), action);
    expect(state.onContextAction).toHaveBeenCalledWith('Archive ticket', ['A', 'B'], 'C');
  });

  it('selects a whole column and opens the reader on double-click outside controls', () => {
    const state = bindings();
    wireTicketCollection(root, state);
    const column = {
      dataset: { columnId: 'done' },
      querySelectorAll: () => [row('A'), row('B')],
    };
    handler('click', '[data-action="select-ticket-column"]')(event('click'), {
      closest: () => column,
    } as unknown as Element);
    expect(state.selectColumn).toHaveBeenCalledWith(['A', 'B']);
    const dblclick = handler('dblclick', rowSelector);
    dblclick(event('dblclick'), row('B'));
    expect(state.openReader).toHaveBeenCalledWith('B', ['A', 'B', 'C'], rows.get('B'));
    dblclick(event('dblclick', {}, true), row('C'));
    expect(state.openReader).toHaveBeenCalledOnce();
  });
});
