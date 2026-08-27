import { beforeEach, describe, expect, it, vi } from 'vitest';
import { collectionTickets, resetTicketCollections } from './ticket-collections-demo';
import { composerCategory, composerTitle, createDemoTicket, filteredWorkspaceTickets, focusComposerTitle, workspaceColumns, workspaceSearchQuery } from './workspace-components-demo';

describe('connected workspace demo state', () => {
  beforeEach(() => { resetTicketCollections(); workspaceSearchQuery.value = ''; composerTitle.value = ''; composerCategory.value = 'task'; });

  it('filters across identity, title, and tags and preserves board totals', () => {
    workspaceSearchQuery.value = 'long-tag-example';
    expect(filteredWorkspaceTickets().map(ticket => ticket.slug)).toEqual(['HS2-SG1BKJ']);
    expect(workspaceColumns().reduce((total, column) => total + column.tickets.length, 0)).toBe(1);
  });

  it('validates creation and inserts a selected canonical ticket', () => {
    expect(createDemoTicket()).toBe(false);
    composerTitle.value = 'A newly composed ticket';
    composerCategory.value = 'bug';
    expect(createDemoTicket()).toBe(true);
    expect(collectionTickets.value[0]).toMatchObject({ title: 'A newly composed ticket', category: 'bug', selected: true });
    expect(collectionTickets.value.slice(1).every(ticket => !ticket.selected)).toBe(true);
  });

  it('focuses the live title control after every composer expansion', () => {
    const focus = vi.fn();
    const root = { querySelector: () => ({ focus }) } as unknown as ParentNode;
    expect(focusComposerTitle(root)).toBe(true);
    expect(focus).toHaveBeenCalledOnce();
    expect(focusComposerTitle({ querySelector: () => null } as unknown as ParentNode)).toBe(false);
  });
});
