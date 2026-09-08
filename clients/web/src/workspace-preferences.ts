import type { WorkspaceSort, WorkspaceSortDirection, WorkspaceViewMode } from './components/workspace-header';

export interface WorkspacePreferences {
  viewMode: WorkspaceViewMode;
  sorts: WorkspaceSortPreferences;
  sidebarVisible: boolean;
  inspectorVisible: boolean;
  commandGroupExpanded: boolean;
}

export type SortableWorkspaceViewMode = 'list' | 'board';
export interface WorkspaceSortPreference { sort: WorkspaceSort; sortDirection: WorkspaceSortDirection }
export type WorkspaceSortPreferences = Record<SortableWorkspaceViewMode, WorkspaceSortPreference>;

export const DEFAULT_WORKSPACE_PREFERENCES: WorkspacePreferences = {
  viewMode: 'list',
  sorts: {
    list: { sort: 'updated', sortDirection: 'descending' },
    board: { sort: 'updated', sortDirection: 'descending' },
  },
  sidebarVisible: true,
  inspectorVisible: true,
  commandGroupExpanded: true,
};

const STORAGE_KEY = 'hotsheet.layout.workspace-preferences.v1';
const viewModes: readonly WorkspaceViewMode[] = ['list', 'board', 'notifications', 'settings'];
const sorts: readonly WorkspaceSort[] = ['updated', 'priority', 'title', 'status'];
const directions: readonly WorkspaceSortDirection[] = ['ascending', 'descending'];

export function sortableWorkspaceView(mode: WorkspaceViewMode): SortableWorkspaceViewMode {
  return mode === 'board' ? 'board' : 'list';
}

function validatedSortPreference(value: unknown, fallback: WorkspaceSortPreference): WorkspaceSortPreference {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return fallback;
  const record = value as Record<string, unknown>;
  const sort = sorts.includes(record.sort as WorkspaceSort) ? record.sort as WorkspaceSort : fallback.sort;
  return {
    sort,
    sortDirection: directions.includes(record.sortDirection as WorkspaceSortDirection) ? record.sortDirection as WorkspaceSortDirection : sort === 'updated' ? 'descending' : 'ascending',
  };
}

export function loadWorkspacePreferences(storage: Pick<Storage, 'getItem'>): WorkspacePreferences {
  let value: unknown;
  try {
    value = JSON.parse(storage.getItem(STORAGE_KEY) ?? 'null');
  } catch {
    return { ...DEFAULT_WORKSPACE_PREFERENCES };
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) return { ...DEFAULT_WORKSPACE_PREFERENCES };
  const record = value as Record<string, unknown>;
  const legacy = validatedSortPreference(record, DEFAULT_WORKSPACE_PREFERENCES.sorts.list);
  const storedSorts = record.sorts && typeof record.sorts === 'object' && !Array.isArray(record.sorts) ? record.sorts as Record<string, unknown> : undefined;
  return {
    viewMode: viewModes.includes(record.viewMode as WorkspaceViewMode) ? record.viewMode as WorkspaceViewMode : DEFAULT_WORKSPACE_PREFERENCES.viewMode,
    sorts: {
      list: validatedSortPreference(storedSorts?.list, legacy),
      board: validatedSortPreference(storedSorts?.board, legacy),
    },
    sidebarVisible: typeof record.sidebarVisible === 'boolean' ? record.sidebarVisible : DEFAULT_WORKSPACE_PREFERENCES.sidebarVisible,
    inspectorVisible: typeof record.inspectorVisible === 'boolean' ? record.inspectorVisible : DEFAULT_WORKSPACE_PREFERENCES.inspectorVisible,
    commandGroupExpanded: typeof record.commandGroupExpanded === 'boolean' ? record.commandGroupExpanded : DEFAULT_WORKSPACE_PREFERENCES.commandGroupExpanded,
  };
}

export function saveWorkspacePreferences(storage: Pick<Storage, 'setItem'>, preferences: WorkspacePreferences): void {
  storage.setItem(STORAGE_KEY, JSON.stringify(preferences));
}
