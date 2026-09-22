import { beforeEach, describe, expect, it, vi } from 'vitest';

import { focusQuickTicketComposerTitle } from '../components/quick-ticket-composer';
import {
  clampProjectSidebarHeight,
  PROJECT_SIDEBAR_MAX_HEIGHT,
  PROJECT_SIDEBAR_MIN_HEIGHT,
} from './project-sidebar-demo';
import { collectionTickets, resetTicketCollections } from './ticket-collections-demo';
import {
  composerCategory,
  composerDetails,
  composerTitle,
  composerUpNext,
  createDemoTicket,
  filteredWorkspaceTickets,
  focusWorkspaceSearch,
  TerminalTicketRailDemo,
  toggleWorkspaceDemoUpNext,
  workspaceColumns,
  workspaceDemoSelection,
  WorkspaceHeaderDemo,
  workspaceMode,
  workspaceSearchHelpOpen,
  workspaceSearchOpen,
  workspaceSearchQuery,
  workspaceSort,
  workspaceSortDirection,
} from './workspace-components-demo';

describe('connected workspace demo state', () => {
  beforeEach(() => {
    resetTicketCollections();
    workspaceMode.value = 'list';
    workspaceSearchQuery.value = '';
    workspaceSearchOpen.value = false;
    workspaceSearchHelpOpen.value = false;
    workspaceSort.value = 'updated';
    workspaceSortDirection.value = 'descending';
    composerTitle.value = '';
    composerDetails.value = '';
    composerCategory.value = 'task';
    composerUpNext.value = false;
  });

  it('projects selection replacement and mixed/all/none Up Next transitions through both workspace demos', () => {
    const select = (slugs: string[]) => {
      collectionTickets.value = collectionTickets.value.map((ticket) => ({
        ...ticket,
        selected: slugs.includes(ticket.slug),
      }));
    };
    const eligible = collectionTickets.value.filter((ticket) => ticket.status === 'started').slice(0, 2);
    collectionTickets.value = collectionTickets.value.map((ticket) => ({
      ...ticket,
      upNext: ticket.slug === eligible[0].slug,
    }));
    select(eligible.map((ticket) => ticket.slug));
    expect(workspaceDemoSelection().selectedTicketsUpNext).toBe('mixed');
    for (const demo of [WorkspaceHeaderDemo, TerminalTicketRailDemo])
      expect(String(demo())).toContain('aria-pressed="mixed"');
    toggleWorkspaceDemoUpNext();
    expect(workspaceDemoSelection().selectedTicketsUpNext).toBe('all');
    toggleWorkspaceDemoUpNext();
    expect(workspaceDemoSelection().selectedTicketsUpNext).toBe('none');
    select([collectionTickets.value.find((ticket) => ticket.status === 'completed')!.slug]);
    expect(workspaceDemoSelection().selectedTicketsUpNextEligible).toBe(false);
    toggleWorkspaceDemoUpNext();
    expect(workspaceDemoSelection().selectedTicketsUpNext).toBe('none');
    select([]);
    expect(workspaceDemoSelection().selectedTicketCount).toBe(0);
    select([eligible[1].slug]);
    toggleWorkspaceDemoUpNext();
    expect(workspaceDemoSelection().selectedTicketsUpNext).toBe('all');
  });

  it('filters across identity, title, and tags and preserves board totals', () => {
    workspaceSearchQuery.value = 'long-tag-example';
    expect(filteredWorkspaceTickets().map((ticket) => ticket.slug)).toEqual(['HS2-SG1BKJ']);
    expect(workspaceColumns().reduce((total, column) => total + column.tickets.length, 0)).toBe(1);
  });

  it('projects controlled search expansion, query, help, and resets through the ticket rail', () => {
    expect(String(TerminalTicketRailDemo())).toContain('data-expanded="false"');
    workspaceSearchOpen.value = true;
    workspaceSearchQuery.value = 'long-tag-example';
    workspaceSearchHelpOpen.value = true;
    const expanded = String(TerminalTicketRailDemo());
    expect(expanded).toContain('data-expanded="true"');
    expect(expanded).toContain('data-token-search-editor="workspace-search"');
    expect(expanded).toContain('long-tag-example');
    expect(expanded).toContain('aria-label="Search syntax"');
    expect(expanded.match(/data-component="ticket-list-row"/g)).toHaveLength(1);
    workspaceSearchOpen.value = false;
    workspaceSearchQuery.value = '';
    workspaceSearchHelpOpen.value = false;
    const reset = String(TerminalTicketRailDemo());
    expect(reset).toContain('data-expanded="false"');
    expect(reset).not.toContain('data-token-search-editor="workspace-search"');
    expect(reset).not.toContain('aria-label="Search syntax"');
    expect(reset.match(/data-component="ticket-list-row"/g)).toHaveLength(7);
  });

  it('projects rail notifications and returns to the list without offering Columns', () => {
    workspaceMode.value = 'notifications';
    const notifications = String(TerminalTicketRailDemo());
    expect(notifications).toContain(
      'data-view-mode="notifications" aria-label="Notifications view" aria-pressed="true"',
    );
    expect(notifications).toContain('data-component="notification-center"');
    expect(notifications).not.toContain('data-component="ticket-list-row"');
    expect(notifications).not.toContain('data-view-mode="board"');
    workspaceMode.value = 'list';
    const list = String(TerminalTicketRailDemo());
    expect(list).toContain('data-view-mode="list" aria-label="List view" aria-pressed="true"');
    expect(list.match(/data-component="ticket-list-row"/g)).toHaveLength(7);
    expect(list).not.toContain('data-component="notification-center"');
    workspaceMode.value = 'board';
    expect(String(TerminalTicketRailDemo())).not.toContain('data-component="ticket-board"');
  });

  it('validates creation and inserts a selected canonical ticket', () => {
    expect(createDemoTicket()).toBe(false);
    composerTitle.value = 'A newly composed ticket';
    composerDetails.value = 'Demo details';
    composerCategory.value = 'bug';
    composerUpNext.value = true;
    expect(createDemoTicket()).toBe(true);
    expect(collectionTickets.value[0]).toMatchObject({
      title: 'A newly composed ticket',
      category: 'bug',
      selected: true,
      upNext: true,
      categoryIcon: 'bug',
      categoryColor: '#ef4444',
    });
    expect(composerDetails.value).toBe('');
    expect(composerUpNext.value).toBe(false);
    expect(collectionTickets.value.slice(1).every((ticket) => !ticket.selected)).toBe(true);
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
    expect(filteredWorkspaceTickets().map((ticket) => ticket.title)).toEqual(
      [...collectionTickets.value].map((ticket) => ticket.title).sort(),
    );
    expect(collectionTickets.value[0].slug).toBe(sourceFirst);
    workspaceSortDirection.value = 'descending';
    expect(filteredWorkspaceTickets().map((ticket) => ticket.title)).toEqual(
      [...collectionTickets.value]
        .map((ticket) => ticket.title)
        .sort()
        .reverse(),
    );
  });

  it('clamps direct sidebar resizing to the reviewable height range', () => {
    expect(clampProjectSidebarHeight(100)).toBe(PROJECT_SIDEBAR_MIN_HEIGHT);
    expect(clampProjectSidebarHeight(420.4)).toBe(420);
    expect(clampProjectSidebarHeight(900)).toBe(PROJECT_SIDEBAR_MAX_HEIGHT);
  });
});
