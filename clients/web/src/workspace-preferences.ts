import type { WorkspaceSort, WorkspaceSortDirection, WorkspaceViewMode } from './components/workspace-header';

export interface WorkspacePreferences {
  viewMode: WorkspaceViewMode;
  sort: WorkspaceSort;
  sortDirection: WorkspaceSortDirection;
  sidebarVisible: boolean;
  inspectorVisible: boolean;
  commandGroupExpanded: boolean;
}

export const DEFAULT_WORKSPACE_PREFERENCES: WorkspacePreferences = {
  viewMode: 'list',
  sort: 'updated',
  sortDirection: 'descending',
  sidebarVisible: true,
  inspectorVisible: true,
  commandGroupExpanded: true,
};

const STORAGE_KEY = 'hotsheet.layout.workspace-preferences.v1';
const viewModes: readonly WorkspaceViewMode[] = ['list', 'board', 'notifications', 'settings'];
const sorts: readonly WorkspaceSort[] = ['updated', 'priority', 'title', 'status'];
const directions: readonly WorkspaceSortDirection[] = ['ascending', 'descending'];

export function loadWorkspacePreferences(storage: Pick<Storage, 'getItem'>): WorkspacePreferences {
  let value: unknown;
  try {
    value = JSON.parse(storage.getItem(STORAGE_KEY) ?? 'null');
  } catch {
    return { ...DEFAULT_WORKSPACE_PREFERENCES };
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) return { ...DEFAULT_WORKSPACE_PREFERENCES };
  const record = value as Record<string, unknown>;
  const sort = sorts.includes(record.sort as WorkspaceSort) ? record.sort as WorkspaceSort : DEFAULT_WORKSPACE_PREFERENCES.sort;
  return {
    viewMode: viewModes.includes(record.viewMode as WorkspaceViewMode) ? record.viewMode as WorkspaceViewMode : DEFAULT_WORKSPACE_PREFERENCES.viewMode,
    sort,
    sortDirection: directions.includes(record.sortDirection as WorkspaceSortDirection)
      ? record.sortDirection as WorkspaceSortDirection
      : sort === 'updated' ? 'descending' : 'ascending',
    sidebarVisible: typeof record.sidebarVisible === 'boolean' ? record.sidebarVisible : DEFAULT_WORKSPACE_PREFERENCES.sidebarVisible,
    inspectorVisible: typeof record.inspectorVisible === 'boolean' ? record.inspectorVisible : DEFAULT_WORKSPACE_PREFERENCES.inspectorVisible,
    commandGroupExpanded: typeof record.commandGroupExpanded === 'boolean' ? record.commandGroupExpanded : DEFAULT_WORKSPACE_PREFERENCES.commandGroupExpanded,
  };
}

export function saveWorkspacePreferences(storage: Pick<Storage, 'setItem'>, preferences: WorkspacePreferences): void {
  storage.setItem(STORAGE_KEY, JSON.stringify(preferences));
}
