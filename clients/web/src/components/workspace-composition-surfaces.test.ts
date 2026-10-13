import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import {
  GlobalWorkspaceSurface,
  ProjectTerminalDrawerSurface,
  SidebarSurface,
  statsInspectorPanel,
  statsSidebarPanel,
  TerminalOperationsSurface,
  WorkspaceSurface,
} from './workspace-composition-surfaces';

describe('workspace composition surfaces', () => {
  it('owns settings, notification, and project sidebar routing', () => {
    expect(String(SidebarSurface({ kind: 'settings', selected: 'sources' }))).toContain(
      'class="kui-pane settings-navigation"',
    );
    expect(
      String(SidebarSurface({ kind: 'notifications', selected: 'day', counts: { pending: 1, day: 2, week: 3 } })),
    ).toContain('aria-label="Notification views"');
  });

  it('owns workspace list, board-independent settings, and notification routing', () => {
    expect(String(WorkspaceSurface({ kind: 'settings', content: 'settings' as never }))).toBe('settings');
    expect(
      String(
        WorkspaceSurface({
          kind: 'notifications',
          notifications: { title: 'Notifications', pending: [], history: [] },
        }),
      ),
    ).toContain('data-component="notification-center"');
    expect(String(WorkspaceSurface({ kind: 'list', list: { tickets: [], label: 'Project tickets' } }))).toContain(
      'data-component="ticket-list"',
    );
  });

  it('owns global stats, optional drawers, and terminal summaries', () => {
    expect(String(GlobalWorkspaceSurface({ kind: 'stats', projectName: 'Demo' }))).toContain('Demo project statistics');
    // Stats rails show empty states rather than being hidden (HS2-MGQDY2).
    expect(statsSidebarPanel().toggle.action).toBe('toggle-project-sidebar');
    expect(String(statsSidebarPanel().content)).toContain('Stats filters will appear here');
    expect(statsInspectorPanel().toggle.action).toBe('toggle-ticket-inspector');
    expect(String(statsInspectorPanel().content)).toContain('data-stats-placeholder="true"');
    expect(String(ProjectTerminalDrawerSurface({}))).toBe('');
    expect(String(TerminalOperationsSurface({ projects: [] }))).toContain(
      'class="kui-pane terminal-operations-sidebar"',
    );
  });

  it('keeps the application root focused on deriving typed surface props', () => {
    const source = ['../app/runtime.tsx', '../features/hot-sheet-shell.tsx', '../features/hot-sheet-app.tsx']
      .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
      .join('\n');
    expect(source).not.toMatch(
      /function (Sidebar|Workspace|TerminalRail|GlobalWorkspace|ProjectTerminalDrawer|TerminalOperations)\(/,
    );
    for (const surface of ['WorkspaceSurface', 'GlobalWorkspaceSurface', 'ProjectTerminalDrawerSurface'])
      expect(source).toContain(`<${surface}`);
    // Both rails take panel parts the Workbench composes (HS2-RWGQWN, HS2-QQW6CT).
    for (const panel of [
      'sidebarSurfacePanel',
      'terminalOperationsSurfacePanel',
      'terminalRailSurfacePanel',
      'inspectorPanel',
      'inspectorPlaceholderPanel',
      'inspectorSkeletonPanel',
      'notificationInspectorSurfacePanel',
      'corruptInspectorPanel',
    ])
      expect(source).toContain(`${panel}(`);
  });
});
