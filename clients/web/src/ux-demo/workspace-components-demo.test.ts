import { beforeEach, describe, expect, it, vi } from 'vitest';

import { focusQuickTicketComposerTitle } from '../components/quick-ticket-composer';
import { clampProjectSidebarHeight, PROJECT_SIDEBAR_MAX_HEIGHT, PROJECT_SIDEBAR_MIN_HEIGHT } from './project-sidebar-demo';
import { collectionTickets, resetTicketCollections } from './ticket-collections-demo';
import { composerCategory, composerDetails, composerTitle, composerUpNext, createDemoTicket, filteredWorkspaceTickets, focusWorkspaceSearch, workspaceColumns, workspaceSearchQuery, workspaceSort, workspaceSortDirection } from './workspace-components-demo';

describe('connected workspace demo state', () => {
  beforeEach(() => { resetTicketCollections(); workspaceSearchQuery.value = ''; workspaceSort.value = 'updated'; workspaceSortDirection.value = 'descending'; composerTitle.value = ''; composerDetails.value = ''; composerCategory.value = 'task'; composerUpNext.value = false; });

  it('filters across identity, title, and tags and preserves board totals', () => {
    workspaceSearchQuery.value = 'long-tag-example';
    expect(filteredWorkspaceTickets().map(ticket => ticket.slug)).toEqual(['HS2-SG1BKJ']);
    expect(workspaceColumns().reduce((total, column) => total + column.tickets.length, 0)).toBe(1);
  });

  it('validates creation and inserts a selected canonical ticket', () => {
    expect(createDemoTicket()).toBe(false);
    composerTitle.value = 'A newly composed ticket';
    composerDetails.value = 'Demo details';
    composerCategory.value = 'bug';
    composerUpNext.value = true;
    expect(createDemoTicket()).toBe(true);
    expect(collectionTickets.value[0]).toMatchObject({ title: 'A newly composed ticket', category: 'bug', selected: true, upNext: true, categoryIcon: 'bug', categoryColor: '#ef4444' });
    expect(composerDetails.value).toBe('');
    expect(composerUpNext.value).toBe(false);
    expect(collectionTickets.value.slice(1).every(ticket => !ticket.selected)).toBe(true);
  });

  it('focuses the live title control after every composer expansion', () => {
    const focus = vi.fn();
    const root = { querySelector: () => ({ focus }) } as unknown as ParentNode;
    expect(focusQuickTicketComposerTitle(root)).toBe(true);
    expect(focus).toHaveBeenLastCalledWith({ preventScroll: true });
    expect(focusQuickTicketComposerTitle({ querySelector: () => null } as unknown as ParentNode)).toBe(false);
    expect(focusWorkspaceSearch(root)).toBe(true);
    expect(focus).toHaveBeenCalledTimes(2);
    expect(focus).toHaveBeenLastCalledWith({ preventScroll: true });
  });

  it('sorts the visible ticket projection without mutating source order', () => {
    const sourceFirst = collectionTickets.value[0].slug;
    workspaceSort.value = 'title';
    workspaceSortDirection.value = 'ascending';
    expect(filteredWorkspaceTickets().map(ticket => ticket.title)).toEqual([...collectionTickets.value].map(ticket => ticket.title).sort());
    expect(collectionTickets.value[0].slug).toBe(sourceFirst);
    workspaceSortDirection.value = 'descending';
    expect(filteredWorkspaceTickets().map(ticket => ticket.title)).toEqual([...collectionTickets.value].map(ticket => ticket.title).sort().reverse());
  });

  it('clamps direct sidebar resizing to the reviewable height range', () => {
    expect(clampProjectSidebarHeight(100)).toBe(PROJECT_SIDEBAR_MIN_HEIGHT);
    expect(clampProjectSidebarHeight(420.4)).toBe(420);
    expect(clampProjectSidebarHeight(900)).toBe(PROJECT_SIDEBAR_MAX_HEIGHT);
  });
});
