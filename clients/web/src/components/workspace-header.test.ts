import { describe, expect, it } from 'vitest';
import { WorkspaceHeader } from './workspace-header';

describe('WorkspaceHeader', () => {
  it('exposes an accessible selected view mode and optional search field', () => {
    const markup = String(WorkspaceHeader({ projectName: 'Hot Sheet 2', viewName: 'All Tickets', mode: 'board', searchOpen: true, searchQuery: 'client' }));
    expect(markup).toContain('aria-label="View mode"');
    expect(markup).toContain('data-view-mode="board" aria-label="Columns view" aria-pressed="true"');
    expect(markup).toContain('name="workspace-search"');
    expect(markup).toContain('value="client"');
  });
});
