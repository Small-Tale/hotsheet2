import { describe, expect, it } from 'vitest';
import { CommandNavigation } from './command-navigation';
import { DriveControl } from './drive-control';
import { ProjectSummary } from './project-summary';
import { ProjectSidebar } from './project-sidebar';
import { RepositorySummary } from './repository-summary';
import { ViewNavigation } from './view-navigation';

describe('ProjectSidebar component slice', () => {
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
    expect(markup).toContain('aria-current="page"');
    expect(markup).toContain('data-attention="true"');
    expect(markup).toContain('aria-label="Add view"');
  });

  it('renders expanded command state and running presentation', () => {
    const markup = String(CommandNavigation({ label: 'Commands', expanded: true, commands: [{ id: 'test', label: 'Test', color: '#2563eb', icon: 'test', running: true }] }));
    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain('Running Test');
    expect(markup).toContain('data-lucide="test-tube-2"');
  });

  it('changes drive action semantics with running state', () => {
    expect(String(DriveControl({ running: false, tool: 'Codex' }))).toContain('Start Codex');
    const running = String(DriveControl({ running: true, tool: 'Codex' }));
    expect(running).toContain('Stop Codex');
    expect(running).toContain('data-lucide="square"');
  });

  it('composes the five sidebar boundaries without duplicating their markup', () => {
    const markup = String(ProjectSidebar({ completedToday: 1, inProgress: 2, completionTrend: [0, 1], branch: 'main', unpushed: 0, uncommitted: 1, views: [{ id: 'all', label: 'All Tickets', icon: 'all' }], selectedViewId: 'all', commandGroupLabel: 'Commands', commands: [], commandGroupExpanded: true, driveRunning: false, driveTool: 'Codex' }));
    for (const component of ['project-summary', 'repository-summary', 'view-navigation', 'command-navigation', 'drive-control']) expect(markup).toContain(`data-component="${component}"`);
  });

});
