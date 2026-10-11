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
    project: () => current,
    settingsCategory: () => 'ai',
    projectPermissionHistory: () => [{ resolvedAt: Date.now() }, { resolvedAt: 0 }],
    projectPendingPermissions: () => [1],
    projectTicketCounts: () => counts,
    searchBarActive: () => false,
    searchSignature: () => '',
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
    expect(props.sidebar).toMatchObject({ branch: 'main', unpushed: 2, uncommitted: 3, driveToolLabel: 'CODEX' });
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
