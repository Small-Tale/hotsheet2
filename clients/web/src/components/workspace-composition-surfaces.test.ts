import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import {
  GlobalWorkspaceSurface,
  ProjectTerminalDrawerSurface,
  SidebarSurface,
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
    expect(String(ProjectTerminalDrawerSurface({}))).toBe('');
    expect(String(TerminalOperationsSurface({ projects: [] }))).toContain(
      'class="kui-pane terminal-operations-sidebar"',
    );
  });

  it('keeps the application root focused on deriving typed surface props', () => {
    const source = readFileSync(new URL('../main.tsx', import.meta.url), 'utf8');
    expect(source).not.toMatch(
      /function (Sidebar|Workspace|TerminalRail|GlobalWorkspace|ProjectTerminalDrawer|TerminalOperations)\(/,
    );
    for (const surface of [
      'SidebarSurface',
      'WorkspaceSurface',
      'TerminalRailSurface',
      'GlobalWorkspaceSurface',
      'ProjectTerminalDrawerSurface',
      'TerminalOperationsSurface',
    ])
      expect(source).toContain(`<${surface}`);
  });
});
