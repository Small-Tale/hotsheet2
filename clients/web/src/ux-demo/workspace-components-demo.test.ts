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
  ignoreWorkspaceDemoPermission,
  resetWorkspaceDemoNotifications,
  resolveWorkspaceDemoPermission,
  TerminalTicketRailDemo,
  toggleWorkspaceDemoUpNext,
  workspaceColumns,
  workspaceDemoNotifications,
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
    resetWorkspaceDemoNotifications();
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

  it('projects a shared notification count and content through every mode, resolution, empty queue, and reset (HS2-Y70MJY)', () => {
    resetWorkspaceDemoNotifications(Date.now() - 1000);
    for (const mode of ['list', 'notifications', 'board', 'settings', 'notifications'] as const) {
      workspaceMode.value = mode;
      const markup = String(WorkspaceHeaderDemo());
      expect(markup).toContain('Notifications view, 2 pending');
      expect(markup.includes('data-component="notification-center"')).toBe(mode === 'notifications');
      expect(markup.includes('data-component="ticket-board"')).toBe(mode === 'board');
      expect(markup.includes('aria-label="Project settings"')).toBe(mode === 'settings');
      expect(markup.includes('aria-label="Workspace tickets"')).toBe(mode === 'list');
    }
    expect(ignoreWorkspaceDemoPermission('workspace-demo:1')).toBe(true);
    expect(workspaceDemoNotifications.value.pending[0].ignored).toBe(true);
    expect(workspaceDemoNotifications.value.pending).toHaveLength(2);
    expect(resolveWorkspaceDemoPermission('workspace-demo:1', 'allow', 'always')).toBe(false);
    expect(resolveWorkspaceDemoPermission('missing', 'allow', 'once')).toBe(false);
    expect(ignoreWorkspaceDemoPermission('missing')).toBe(false);
    expect(resolveWorkspaceDemoPermission('workspace-demo:1', 'allow', 'once')).toBe(true);
    expect(workspaceDemoNotifications.value.history[0]).toMatchObject({
      key: 'workspace-demo:1',
      decision: 'allow',
      scope: 'once',
    });
    expect(String(WorkspaceHeaderDemo())).toContain('Notifications view, 1 pending');
    expect(resolveWorkspaceDemoPermission('workspace-demo:1', 'deny', 'once')).toBe(false);
    expect(resolveWorkspaceDemoPermission('workspace-demo:2', 'deny', 'once')).toBe(true);
    expect(workspaceDemoNotifications.value.pending).toHaveLength(0);
    expect(workspaceDemoNotifications.value.history).toHaveLength(3);
    expect(String(WorkspaceHeaderDemo())).not.toContain(' pending');
    expect(String(WorkspaceHeaderDemo())).not.toContain('data-action="resolve-permission"');
    workspaceMode.value = 'list';
    resetWorkspaceDemoNotifications();
    expect(String(WorkspaceHeaderDemo())).toContain('Notifications view, 2 pending');
    workspaceMode.value = 'notifications';
    expect(workspaceDemoNotifications.value.history).toHaveLength(1);
    expect(workspaceDemoNotifications.value.pending.every((item) => !item.ignored)).toBe(true);
    expect(resolveWorkspaceDemoPermission('workspace-demo:2', 'allow', 'always')).toBe(true);
    expect(String(WorkspaceHeaderDemo())).toContain('allowed this kind of request');
    expect(String(WorkspaceHeaderDemo())).toContain('Notifications view, 1 pending');
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
      'data-segment-value="notifications" data-selected="true" aria-label="Notifications view" aria-pressed="true"',
    );
    expect(notifications).toContain('data-component="notification-center"');
    expect(notifications).not.toContain('data-component="ticket-list-row"');
    expect(notifications).not.toContain('data-view-mode="board"');
    workspaceMode.value = 'list';
    const list = String(TerminalTicketRailDemo());
    expect(list).toContain('data-segment-value="list" data-selected="true" aria-label="List view" aria-pressed="true"');
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
