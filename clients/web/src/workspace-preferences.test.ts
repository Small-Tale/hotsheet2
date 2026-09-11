import { describe, expect, it } from 'vitest';

import { DEFAULT_WORKSPACE_PREFERENCES, loadWorkspacePreferences, saveWorkspacePreferences,toggleCollapsedCommandGroup } from './workspace-preferences';

describe('workspace preferences', () => {
  it('loads defaults when storage is missing or malformed', () => {
    expect(loadWorkspacePreferences({ getItem: () => null })).toEqual(DEFAULT_WORKSPACE_PREFERENCES);
    expect(loadWorkspacePreferences({ getItem: () => '{bad json' })).toEqual(DEFAULT_WORKSPACE_PREFERENCES);
  });

  it('validates each persisted field and derives a missing sort direction', () => {
    const stored = JSON.stringify({ viewMode: 'board', sort: 'priority', sidebarVisible: false, inspectorVisible: false, commandGroupExpanded: false });
    expect(loadWorkspacePreferences({ getItem: () => stored })).toEqual({
      viewMode: 'board',
      sorts: {
        list: { sort: 'priority', sortDirection: 'ascending' },
        board: { sort: 'priority', sortDirection: 'ascending' },
      },
      sidebarVisible: false,
      inspectorVisible: false,
      commandGroupExpanded: false,
      commandGroupsCollapsed: {},
    });
    const invalid = JSON.stringify({ viewMode: 'grid', sort: 'random', sortDirection: 'sideways', sidebarVisible: 'no' });
    expect(loadWorkspacePreferences({ getItem: () => invalid })).toEqual(DEFAULT_WORKSPACE_PREFERENCES);
  });

  it('persists one complete versioned preference record', () => {
    const values = new Map<string, string>();
    saveWorkspacePreferences({ setItem: (key, value) => values.set(key, value) }, {
      viewMode: 'settings',
      sorts: {
        list: { sort: 'title', sortDirection: 'descending' },
        board: { sort: 'priority', sortDirection: 'ascending' },
      },
      sidebarVisible: false,
      inspectorVisible: true,
      commandGroupExpanded: false,
      commandGroupsCollapsed: {project:['Quality','Git']},
    });
    expect(loadWorkspacePreferences({ getItem: key => values.get(key) ?? null })).toEqual({
      viewMode: 'settings',
      sorts: {
        list: { sort: 'title', sortDirection: 'descending' },
        board: { sort: 'priority', sortDirection: 'ascending' },
      },
      sidebarVisible: false,
      inspectorVisible: true,
      commandGroupExpanded: false,
      commandGroupsCollapsed: {project:['Quality','Git']},
    });
  });

  it('validates and independently toggles remembered named command groups per project',()=>{
    const stored=JSON.stringify({commandGroupsCollapsed:{alpha:[' Quality ','Quality',3,''],beta:['Git'],empty:'bad'}});
    const loaded=loadWorkspacePreferences({getItem:()=>stored}).commandGroupsCollapsed;
    expect(loaded).toEqual({alpha:['Quality'],beta:['Git']});
    const collapsed=toggleCollapsedCommandGroup(loaded,'alpha','Release');
    expect(collapsed).toEqual({alpha:['Quality','Release'],beta:['Git']});
    expect(toggleCollapsedCommandGroup(collapsed,'alpha','Quality')).toEqual({alpha:['Release'],beta:['Git']});
    expect(toggleCollapsedCommandGroup(collapsed,'beta','Git')).toEqual({alpha:['Quality','Release'],beta:[]});
  });

  it('replaces obsolete status sorting for columns without changing list sorting', () => {
    const stored = JSON.stringify({
      viewMode: 'board',
      sorts: {
        list: { sort: 'status', sortDirection: 'descending' },
        board: { sort: 'status', sortDirection: 'ascending' },
      },
    });
    expect(loadWorkspacePreferences({ getItem: () => stored }).sorts).toEqual({
      list: { sort: 'status', sortDirection: 'descending' },
      board: { sort: 'updated', sortDirection: 'descending' },
    });
  });
});
