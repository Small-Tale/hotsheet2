import { describe, expect, it } from 'vitest';
import { WorkspaceHeader } from './workspace-header';

describe('WorkspaceHeader', () => {
  it('exposes an accessible selected view mode and optional search field', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', viewName: 'All Tickets', mode: 'settings', searchOpen: true, searchQuery: 'client', sort: 'priority' }));
    expect(markup).toContain('aria-label="View mode"');
    expect(markup).toContain('data-view-mode="settings" aria-label="Settings view" aria-pressed="true"');
    expect(markup).toContain('name="workspace-search"');
    expect(markup).toContain('value="client"');
    expect(markup).toContain('slot="trigger" appearance="outlined" with-caret');
    expect(markup).toContain('checked data-sort="priority" value="priority"');
    expect(markup.indexOf('workspace-header__utility-group')).toBeLessThan(markup.indexOf('workspace-header__search'));
    expect(markup.indexOf('workspace-header__search')).toBeLessThan(markup.indexOf('workspace-header__search-button'));
  });
});
