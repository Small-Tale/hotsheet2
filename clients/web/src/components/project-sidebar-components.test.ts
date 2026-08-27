import { describe, expect, it } from 'vitest';
import { CommandNavigation } from './command-navigation';
import { DriveControl } from './drive-control';
import { ProjectSummary } from './project-summary';
import { RepositorySummary } from './repository-summary';
import { ViewNavigation } from './view-navigation';

describe('ProjectSidebar component slice', () => {
  it('derives project progress bars and accessible summary text from props', () => {
    const markup = String(ProjectSummary({ completed: 8, inProgress: 2, progress: 80, trend: [1, 4] }));
    expect(markup).toContain('80% complete');
    expect(markup).toContain('8 completed');
    expect(markup.match(/data-bar=/g)).toHaveLength(2);
  });

  it('renders repository status as one discoverable action', () => {
    const markup = String(RepositorySummary({ branch: 'main', unpushed: 3, uncommitted: 1 }));
    expect(markup).toContain('Repository status for main');
    expect(markup).toContain('data-lucide="git-branch"');
    expect(markup).toContain('3 unpushed commits');
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
});
