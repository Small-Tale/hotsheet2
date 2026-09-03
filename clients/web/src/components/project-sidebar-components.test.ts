import { Archive, Plus  } from 'lucide';
import { describe, expect, it } from 'vitest';

import { CommandNavigation } from './command-navigation';
import { DriveControl } from './drive-control';
import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';
import { ProjectSidebar } from './project-sidebar';
import { ProjectSummary } from './project-summary';
import { RepositorySummary } from './repository-summary';
import { ViewNavigation } from './view-navigation';

describe('ProjectSidebar component slice', () => {
  it('uses one stable menu-item grid for icon, label, trailing content, and selection', () => {
    const markup = String(MenuItem({ action: 'select', itemId: 'archive', selected: true, icon: LucideIcon({ icon: Archive, name: 'archive' }), label: 'Archive', trailing: '241' as never }));
    expect(markup).toContain('data-component="menu-item"');
    expect(markup).toContain('data-item-id="archive"');
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('menu-item__icon');
    expect(markup).toContain('menu-item__label');
    expect(markup).toContain('menu-item__trailing');
  });

  it('shares one menu-header contract for action and toggle groups', () => {
    const action = String(MenuHeader({ label: 'Views', action: 'add', actionLabel: 'Add view', actionIcon: Plus, actionIconName: 'plus' }));
    const toggle = String(MenuHeader({ label: 'Commands', action: 'toggle', actionIcon: Plus, actionIconName: 'plus', expanded: false, toggle: true }));
    expect(action).toContain('data-component="menu-header"');
    expect(action).toContain('aria-label="Add view"');
    expect(toggle).toContain('aria-expanded="false"');
  });
  it('derives project progress bars and accessible summary text from props', () => {
    const markup = String(ProjectSummary({ completedToday: 8, inProgress: 2, trend: [1, 4] }));
    expect(markup).not.toContain('coverage');
    expect(markup).not.toContain('project-summary__coverage');
    expect(markup).toContain('Tickets completed over the last 2 days: 1, 4');
    expect(markup).toContain('8 completed today');
    expect(markup).toContain('2 in progress');
    expect(markup.match(/data-bar=/g)).toHaveLength(2);
  });

  it('renders zero-completion days as explicit baseline marks', () => {
    const markup = String(ProjectSummary({ completedToday: 1, inProgress: 0, trend: [0, 1] }));
    expect(markup).toContain('data-zero="true"');
    expect(markup).toContain('data-zero="false"');
  });

  it('renders repository status as one discoverable action', () => {
    const markup = String(RepositorySummary({ branch: 'main', unpushed: 3, uncommitted: 1 }));
    expect(markup).toContain('Repository status for main');
    expect(markup).toContain('data-lucide="git-branch"');
    expect(markup).toContain('repository-summary__branch-name');
    expect(markup).toContain('3 unpushed commits');
    expect(markup).not.toContain('data-lucide="file-pen-line"');
  });

  it('projects current view, counts, and attention', () => {
    const markup = String(ViewNavigation({ selectedId: 'all', items: [{ id: 'all', label: 'All Tickets', count: 4, icon: 'all' }, { id: 'review', label: 'Review', count: 2, attention: true, icon: 'needs-review' }] }));
    expect(markup.match(/class="menu-item__count"/g)).toHaveLength(2);
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('data-attention="true"');
    expect(markup).toContain('aria-label="Add view"');
  });

  it('renders expanded command state and running presentation', () => {
    const markup = String(CommandNavigation({ label: 'Commands', expanded: true, commands: [{ id: 'test', label: 'Test', color: '#3b82f6', icon: 'test', group: 'Checks', running: true, lastRun: 'completed (exit 0)' }] }));
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain('Running Test');
    expect(markup).toContain('data-lucide="test-tube-2"');
    expect(markup).toContain('data-command-color="#3b82f6"');
    expect(markup).toContain('data-command-group="Checks"');
    expect(markup).toContain('Last run: completed (exit 0). Press and hold for output.');
  });

  it('falls back to the HS1 neutral command color with dark contrast', () => {
    const markup = String(CommandNavigation({ label: 'Commands', expanded: true, commands: [{ id: 'custom', label: 'Custom', color: '#123456', icon: 'build' }] }));
    expect(markup).toContain('data-command-color="#e5e7eb"');
    expect(markup).toContain('--command-text-color:#1a1a1a');
  });

  it('changes drive action semantics with running state', () => {
    expect(String(DriveControl({ running: false, tool: 'Codex' }))).toContain('Start Codex');
    const running = String(DriveControl({ running: true, tool: 'Codex' }));
    expect(running).toContain('Stop Codex');
    expect(running).toContain('data-lucide="square"');
  });

  it('composes the five sidebar boundaries without duplicating their markup', () => {
    const markup = String(ProjectSidebar({ completedToday: 1, inProgress: 2, completionTrend: [0, 1], branch: 'main', unpushed: 0, uncommitted: 1, views: [{ id: 'all', label: 'All Tickets', icon: 'all' }], selectedViewId: 'all', commandGroupLabel: 'Commands', commands: [{ id: 'test', label: 'Test', color: '#3b82f6', icon: 'test' }], commandGroupExpanded: true, driveRunning: false, driveTool: 'Codex', openCount: 7, upNextCount: 3 }));
    for (const component of ['project-summary', 'repository-summary', 'view-navigation', 'command-navigation', 'drive-control']) expect(markup).toContain(`data-component="${component}"`);
    expect(markup).toContain('data-component="project-work-summary">7 open, 3 up next');
    expect(markup.indexOf('project-work-summary')).toBeLessThan(markup.indexOf('data-component="drive-control"'));
  });

  it('omits the command section when the project has no commands', () => {
    const markup = String(ProjectSidebar({ completedToday: 0, inProgress: 0, completionTrend: [], branch: 'main', unpushed: 0, uncommitted: 0, views: [], selectedViewId: 'all', commandGroupLabel: 'Project commands', commands: [], commandGroupExpanded: true, driveRunning: false, driveTool: 'Codex', openCount: 0, upNextCount: 0 }));
    expect(markup).not.toContain('data-component="command-navigation"');
    expect(markup).not.toContain('Project commands');
  });

});
