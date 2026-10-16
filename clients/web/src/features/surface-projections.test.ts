import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type { CheckoutTicketCounts, ProviderConnection } from '../api';
import type { WorkspaceViewMode } from '../components/workspace-header';
import type { Project } from '../interactions/types';
import type { ProjectTicketSources } from '../new-ticket-source';
import { createSettingsSurface, type SettingsSurfaceDependencies } from './settings-surface';
import { createSidebarSurface, type SidebarSurfaceDependencies } from './sidebar-surface';

const current = { id: 'p1', apiPath: '/api/p1' } as Project;
const read = <T>(value: T) => ({ value });

function sidebar(mode: WorkspaceViewMode, overrides: Partial<SidebarSurfaceDependencies> = {}) {
  const counts = { queued: 3, backlog: 1, archive: 2, open: 4, completed_today: 1, started: 2 } as CheckoutTicketCounts;
  return createSidebarSurface({
    viewMode: read(mode),
    notificationView: read('pending'),
    notificationsPaused: read(false),
    partialSourcesByProject: read({}),
    sidebarSearchCounts: read(undefined),
    repository: read({ branch: 'main', ahead: 2, behind: 0, staged: 1, unstaged: 1, untracked: 1, conflicted: 0 }),
    repositoryError: read(''),
    tickets: read([]),
    selectedView: read('all'),
    corruptTickets: read([]),
    driveConnectionsByProject: read({}),
    drivePendingByProject: read({}),
    commandDefinitions: read([{ id: 'c1', title: 'Run tests', kind: 'shell', command: 'npm test' }]),
    commandGroupExpanded: read(true),
    commandGroupsCollapsed: read({}),
    driveOptionsOpen: read(false),
    driveOptionsAnchor: read(undefined),
    aiTools: read([]),
    aiSettingsLoading: read(false),
    aiSettingsMessage: read(''),
    driveOverridesByProject: read({}),
    aiDefaults: read({ tool: 'codex' }),
    conversationOpen: read(false),
    conversationConnectionId: read(undefined),
    projectConversationUnread: (projectId) => (projectId === 'p1' ? 3 : 0),
    project: () => current,
    settingsCategory: () => 'ai',
    projectPermissionHistory: () => [{ resolvedAt: Date.now() }, { resolvedAt: 0 }],
    projectPendingPermissions: () => [1],
    projectTicketCounts: () => counts,
    searchBarActive: () => false,
    searchSignature: () => '',
    searchScope: () => 'all',
    customViewsFor: () => [{ id: 'v1', name: 'Mine', query: 'status:started' }],
    effectiveDriveSelection: () => ({ tool: 'codex' }),
    commandRunFor: () => ({ id: 'r1', command_id: 'c1', state: 'failed', exit_code: 2 }) as never,
    commandIcon: () => 'terminal',
    aiToolLabel: (tool) => tool.toUpperCase(),
    ...overrides,
  }).sidebarSurfaceProps();
}

describe('sidebar surface projection (HS2-BSSWGV)', () => {
  it('projects the settings and notifications navigations', () => {
    expect(sidebar('settings')).toEqual({ kind: 'settings', selected: 'ai' });
    expect(sidebar('notifications')).toMatchObject({
      kind: 'notifications',
      selected: 'pending',
      counts: { pending: 1, day: 1, week: 2 },
      paused: false,
    });
  });

  it('projects the project sidebar with repository, views, commands, and Drive', () => {
    const props = sidebar('list');
    if (props.kind !== 'project') throw new Error('expected the project sidebar');
    expect(props.sidebar).toMatchObject({
      branch: 'main',
      unpushed: 2,
      uncommitted: 3,
      driveToolLabel: 'CODEX',
      conversationUnread: 3,
    });
    expect(props.sidebar.views.map((view) => view.label)).toEqual(['Queue', 'Backlog', 'Archive', 'Mine']);
    expect(props.sidebar.commands).toEqual([
      expect.objectContaining({ id: 'c1', label: 'Run tests', lastRun: 'failed (exit 2)', running: false }),
    ]);
  });

  it('adds Ticket errors and a no-branch summary when the repository is unknown', () => {
    const props = sidebar('list', { repository: read(null), corruptTickets: read([{ path: 'x' }] as never) });
    if (props.kind !== 'project') throw new Error('expected the project sidebar');
    expect(props.sidebar.branch).toBe('No git branch');
    expect(props.sidebar.views.at(-1)).toMatchObject({ id: 'errors', count: 1, attention: true });
  });
});

describe('sidebar search-count scope (HS2-S6TXSW)', () => {
  const searching = (selected: string, values: Record<string, number>, pending: string[] = []) => {
    const props = sidebar('list', {
      selectedView: read(selected as never),
      searchScope: () => selected as never,
      searchBarActive: () => true,
      searchSignature: () => 'sig',
      projectTicketCounts: () =>
        ({ queued: 3, backlog: 1, archive: 2, trash: 13, open: 4, completed_today: 1, started: 2 }) as never,
      sidebarSearchCounts: read({ projectId: 'p1', signature: 'sig', generation: 1, values, pending }),
    });
    if (props.kind !== 'project') throw new Error('expected the project sidebar');
    return Object.fromEntries(props.sidebar.views.map((view) => [view.id, view]));
  };

  it('searching Trash counts only the Trash row; other rows keep their normal totals', () => {
    const views = searching('trash', { trash: 1 });
    expect(views.trash).toMatchObject({ count: 1, searchCount: true, countLoading: false });
    expect(views.all).toMatchObject({ count: 3, searchCount: false, countLoading: false });
    expect(views.backlog).toMatchObject({ count: 1, searchCount: false });
    expect(views.archive).toMatchObject({ count: 2, searchCount: false });
    expect(views['custom:v1']).toMatchObject({ searchCount: false, countLoading: false });
  });

  it('shows the Trash row loading while its search count is pending', () => {
    expect(searching('trash', {}, ['trash']).trash).toMatchObject({ countLoading: true, searchCount: false });
  });

  it('searching another view never counts Trash, which keeps its plain total', () => {
    const views = searching('all', { all: 2, backlog: 0, archive: 5, 'custom:v1': 1 });
    expect(views.trash).toMatchObject({ count: 13, searchCount: false });
    expect(views.all).toMatchObject({ count: 2, searchCount: true });
    expect(views.backlog).toMatchObject({ count: 0, searchCount: true });
    expect(views['custom:v1']).toMatchObject({ count: 1, searchCount: true });
  });

  it('counts every sidebar view and selects none while the search covers All Tickets (HS2-4J6764)', () => {
    const props = sidebar('list', {
      selectedView: read('backlog'),
      searchScope: () => 'all-tickets',
      searchBarActive: () => true,
      searchSignature: () => 'sig',
      projectTicketCounts: () => ({ queued: 3, backlog: 1, archive: 2, trash: 13 }) as never,
      sidebarSearchCounts: read({
        projectId: 'p1',
        signature: 'sig',
        generation: 1,
        values: { all: 2, backlog: 1, archive: 4, 'custom:v1': 0 },
        pending: [],
      }),
    });
    if (props.kind !== 'project') throw new Error('expected the project sidebar');
    const views = Object.fromEntries(props.sidebar.views.map((view) => [view.id, view]));
    expect(props.sidebar.selectedViewId).toBe('all-tickets');
    expect(views.all).toMatchObject({ count: 2, searchCount: true });
    expect(views.backlog).toMatchObject({ count: 1, searchCount: true });
    expect(views.archive).toMatchObject({ count: 4, searchCount: true });
    expect(views.trash).toMatchObject({ count: 13, searchCount: false });
  });

  it('drops search counts again once the search ends', () => {
    const props = sidebar('list', {
      selectedView: read('trash'),
      searchScope: () => 'trash',
      projectTicketCounts: () => ({ queued: 3, backlog: 1, archive: 2, trash: 13 }) as never,
      sidebarSearchCounts: read({
        projectId: 'p1',
        signature: 'sig',
        generation: 1,
        values: { trash: 1 },
        pending: [],
      }),
    });
    if (props.kind !== 'project') throw new Error('expected the project sidebar');
    expect(props.sidebar.views.find((view) => view.id === 'trash')).toMatchObject({ count: 13, searchCount: false });
  });
});

describe('settings surface projection (HS2-BSSWGV)', () => {
  it('lists each source with the other projects that share its connection', () => {
    const dependencies = {
      defaultProviders: signal<Record<string, ProjectTicketSources | undefined>>({
        p1: {
          sources: [{ connectionId: 'gh', name: 'GitHub', provider: 'github', locator: 'o/r', default: true }],
        } as unknown as ProjectTicketSources,
      }),
      providerConnections: signal<ProviderConnection[]>([
        {
          id: 'gh',
          disabled: true,
          projects: [
            { id: 'p1', alias: 'self' },
            { id: 'p2', alias: 'other' },
          ],
        } as unknown as ProviderConnection,
      ]),
      providerSettingsError: signal('bad token'),
      ticketSourceSetupProject: signal<unknown>(undefined),
    } as unknown as SettingsSurfaceDependencies;
    const settings = createSettingsSurface(dependencies).projectSourcesSettings(current);
    expect(settings).toEqual({
      sources: [expect.objectContaining({ connectionId: 'gh', disabled: true, default: true, sharedWith: ['other'] })],
      error: 'bad token',
      setupOpen: false,
    });
  });
});
