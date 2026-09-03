import { describe, expect, it } from 'vitest';

import { DEFAULT_WORKSPACE_PREFERENCES, loadWorkspacePreferences, saveWorkspacePreferences } from './workspace-preferences';

describe('workspace preferences', () => {
  it('loads defaults when storage is missing or malformed', () => {
    expect(loadWorkspacePreferences({ getItem: () => null })).toEqual(DEFAULT_WORKSPACE_PREFERENCES);
    expect(loadWorkspacePreferences({ getItem: () => '{bad json' })).toEqual(DEFAULT_WORKSPACE_PREFERENCES);
  });

  it('validates each persisted field and derives a missing sort direction', () => {
    const stored = JSON.stringify({ viewMode: 'board', sort: 'priority', sidebarVisible: false, inspectorVisible: false, commandGroupExpanded: false });
    expect(loadWorkspacePreferences({ getItem: () => stored })).toEqual({
      viewMode: 'board',
      sort: 'priority',
      sortDirection: 'ascending',
      sidebarVisible: false,
      inspectorVisible: false,
      commandGroupExpanded: false,
    });
    const invalid = JSON.stringify({ viewMode: 'grid', sort: 'random', sortDirection: 'sideways', sidebarVisible: 'no' });
    expect(loadWorkspacePreferences({ getItem: () => invalid })).toEqual(DEFAULT_WORKSPACE_PREFERENCES);
  });

  it('persists one complete versioned preference record', () => {
    const values = new Map<string, string>();
    saveWorkspacePreferences({ setItem: (key, value) => values.set(key, value) }, {
      viewMode: 'settings',
      sort: 'title',
      sortDirection: 'descending',
      sidebarVisible: false,
      inspectorVisible: true,
      commandGroupExpanded: false,
    });
    expect(loadWorkspacePreferences({ getItem: key => values.get(key) ?? null })).toEqual({
      viewMode: 'settings',
      sort: 'title',
      sortDirection: 'descending',
      sidebarVisible: false,
      inspectorVisible: true,
      commandGroupExpanded: false,
    });
  });
});
