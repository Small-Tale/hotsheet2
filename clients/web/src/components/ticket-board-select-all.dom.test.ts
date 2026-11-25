// HS2-3SJ5G8: a column header's Select All, composed through the shipped column and its delegated
// wiring, selects the members of collapsed groups along with the drawn rows.
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ticketGroupKey } from '../ticket-groups';
import { TicketBoard } from './ticket-board';
import type { TicketBoardColumnProps } from './ticket-board-column';
import {
  collectionSelectAll,
  type TicketCollectionBindings,
  wireTicketCollection,
} from './ticket-collection-interactions';
import type { TicketRowProps } from './ticket-row';

afterEach(() => {
  document.body.replaceChildren();
});

const ticket = (slug: string, group?: string): TicketRowProps => ({
  slug,
  title: `Synthetic ticket ${slug}`,
  status: 'not_started',
  priority: 'default',
  category: 'task',
  tags: [],
  group,
});

function mount(columns: TicketBoardColumnProps[]) {
  const host = document.createElement('main');
  host.innerHTML = String(TicketBoard({ columns, label: 'Board' }));
  document.body.append(host);
  return host;
}

function bindings() {
  return {
    selectedSlugs: () => [],
    selectionOrder: () => [],
    select: vi.fn(),
    isSelectAll: () => false,
    selectAll: vi.fn(),
    selectColumn: vi.fn(),
    openReader: vi.fn(),
    contextMenu: () => undefined,
    setContextMenu: vi.fn(),
    onContextField: vi.fn(),
    onContextAction: vi.fn(),
  } satisfies TicketCollectionBindings;
}

const clickHeader = (host: HTMLElement, title: string) => {
  host.querySelector<HTMLElement>(`[aria-label="Select all ${title} tickets"]`)!.click();
};

describe('HS2-3SJ5G8 column Select All with collapsed groups', () => {
  const tickets = [
    ticket('HS2-A'),
    ticket('HS2-B', 'Alpha'),
    ticket('HS2-C', 'Alpha'),
    ticket('HS2-D', 'Beta'),
    ticket('HS2-E'),
    ticket('HS2-F', 'Gamma'),
  ];

  it('selects drawn rows and collapsed members in drawn order, mixing collapsed and expanded groups', () => {
    const host = mount([
      {
        id: 'not-started',
        title: 'Not Started',
        tickets,
        collapsedGroups: [ticketGroupKey('Alpha'), ticketGroupKey('Gamma')],
      },
    ]);
    // The collapsed groups draw no rows.
    expect(
      [...host.querySelectorAll('[data-ticket-slug]')].map((row) => (row as HTMLElement).dataset.ticketSlug),
    ).toEqual(['HS2-A', 'HS2-D', 'HS2-E']);
    const state = bindings();
    const dispose = wireTicketCollection(host, state);
    clickHeader(host, 'Not Started');
    expect(state.selectColumn).toHaveBeenCalledWith(
      ['HS2-A', 'HS2-B', 'HS2-C', 'HS2-D', 'HS2-E', 'HS2-F'],
      ['HS2-A', 'HS2-D', 'HS2-E'],
    );
    dispose();
  });

  it('selects every ticket of an all-collapsed column, only from that column', () => {
    const host = mount([
      {
        id: 'not-started',
        title: 'Not Started',
        tickets: [ticket('HS2-B', 'Alpha'), ticket('HS2-C', 'Alpha'), ticket('HS2-D', 'Beta')],
        collapsedGroups: [ticketGroupKey('Alpha'), ticketGroupKey('Beta')],
      },
      {
        id: 'started',
        title: 'Started',
        tickets: [ticket('HS2-X', 'Alpha'), ticket('HS2-Y')],
        collapsedGroups: [ticketGroupKey('Alpha'), ticketGroupKey('Beta')],
      },
    ]);
    const state = bindings();
    const dispose = wireTicketCollection(host, state);
    clickHeader(host, 'Not Started');
    expect(state.selectColumn).toHaveBeenLastCalledWith(['HS2-B', 'HS2-C', 'HS2-D'], []);
    clickHeader(host, 'Started');
    expect(state.selectColumn).toHaveBeenLastCalledWith(['HS2-X', 'HS2-Y'], ['HS2-Y']);
    // ⌘A on the board takes every column, collapsed members included.
    expect(collectionSelectAll(host.querySelector('[data-component="ticket-board"]')!)).toEqual({
      slugs: ['HS2-B', 'HS2-C', 'HS2-D', 'HS2-X', 'HS2-Y'],
      reachable: ['HS2-Y'],
    });
    dispose();
  });

  it('selects nothing in an empty column and only drawn rows when every group is expanded', () => {
    const host = mount([
      { id: 'completed', title: 'Completed', tickets: [] },
      { id: 'started', title: 'Started', tickets },
    ]);
    const state = bindings();
    const dispose = wireTicketCollection(host, state);
    clickHeader(host, 'Completed');
    expect(state.selectColumn).toHaveBeenLastCalledWith([], []);
    clickHeader(host, 'Started');
    const all = ['HS2-A', 'HS2-B', 'HS2-C', 'HS2-D', 'HS2-E', 'HS2-F'];
    expect(state.selectColumn).toHaveBeenLastCalledWith(all, all);
    expect(host.querySelector('[data-ticket-group-hidden-slugs]')).toBeNull();
    dispose();
  });
});
