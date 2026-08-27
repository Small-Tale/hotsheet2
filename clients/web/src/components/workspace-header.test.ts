import { describe, expect, it } from 'vitest';
import { WorkspaceHeader } from './workspace-header';

describe('WorkspaceHeader', () => {
  it('exposes an accessible selected view mode and optional search field', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', viewName: 'All Tickets', mode: 'settings', searchOpen: true, searchQuery: 'client', sort: 'priority' }));
    expect(markup).toContain('aria-label="View mode"');
    expect(markup).toContain('data-view-mode="settings" aria-label="Settings view" aria-pressed="true"');
    expect(markup).toContain('name="workspace-search"');
    expect(markup).toContain('value="client"');
    expect(markup).toContain('slot="trigger" appearance="plain" with-caret');
    expect(markup).toContain('checked data-sort="priority" value="priority"');
    expect(markup).toContain('workspace-header__search-group" data-expanded="true"');
    expect(markup).toContain('slot="start"');
    expect(markup).not.toContain('data-action="open-workspace-search"');
    expect(markup.indexOf('workspace-header__utility-group')).toBeLessThan(markup.indexOf('workspace-header__search'));
  });

  it('renders the collapsed find state as a single magnifier button', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', viewName: 'All Tickets', mode: 'list' }));
    expect(markup).toContain('workspace-header__search-group" data-expanded="false"');
    expect(markup).toContain('data-action="open-workspace-search" aria-label="Search tickets"');
    expect(markup).not.toContain('name="workspace-search"');
  });
});
