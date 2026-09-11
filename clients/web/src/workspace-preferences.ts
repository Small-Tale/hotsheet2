import type { WorkspaceSort, WorkspaceSortDirection, WorkspaceViewMode } from './components/workspace-header';

export interface WorkspacePreferences {
  viewMode: WorkspaceViewMode;
  sorts: WorkspaceSortPreferences;
  sidebarVisible: boolean;
  inspectorVisible: boolean;
  commandGroupExpanded: boolean;
  commandGroupsCollapsed: Record<string,string[]>;
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
  commandGroupsCollapsed: {},
};

const STORAGE_KEY = 'hotsheet.layout.workspace-preferences.v1';
const viewModes: readonly WorkspaceViewMode[] = ['list', 'board', 'notifications', 'settings'];
const sorts: readonly WorkspaceSort[] = ['updated', 'priority', 'title', 'status'];
const directions: readonly WorkspaceSortDirection[] = ['ascending', 'descending'];

export function sortableWorkspaceView(mode: WorkspaceViewMode): SortableWorkspaceViewMode {
  return mode === 'board' ? 'board' : 'list';
}

function validatedSortPreference(value: unknown, fallback: WorkspaceSortPreference, allowedSorts: readonly WorkspaceSort[] = sorts): WorkspaceSortPreference {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return fallback;
  const record = value as Record<string, unknown>;
  const requestedSort = record.sort as WorkspaceSort;
  if (!allowedSorts.includes(requestedSort)) return fallback;
  const sort = requestedSort;
  return {
    sort,
    sortDirection: directions.includes(record.sortDirection as WorkspaceSortDirection) ? record.sortDirection as WorkspaceSortDirection : sort === 'updated' ? 'descending' : 'ascending',
  };
}

function validatedCollapsedCommandGroups(value:unknown):Record<string,string[]>{
  if(!value||typeof value!=='object'||Array.isArray(value))return{};
  return Object.fromEntries(Object.entries(value).flatMap(([projectId,groups])=>{
    if(!projectId.trim()||!Array.isArray(groups))return[];
    return[[projectId,[...new Set(groups.flatMap(group=>typeof group==='string'&&group.trim()?[group.trim()]:[]))]]];
  }));
}

export function toggleCollapsedCommandGroup(state:Readonly<Record<string,string[]>>,projectId:string,group:string):Record<string,string[]>{
  const normalized=group.trim();
  if(!normalized)return Object.fromEntries(Object.entries(state).map(([id,groups])=>[id,[...groups]]));
  const current=state[projectId]??[],collapsed=current.includes(normalized)?current.filter(item=>item!==normalized):[...current,normalized];
  return{...state,[projectId]:collapsed};
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
      board: validatedSortPreference(storedSorts?.board, legacy.sort === 'status' ? DEFAULT_WORKSPACE_PREFERENCES.sorts.board : legacy, sorts.filter(sort => sort !== 'status')),
    },
    sidebarVisible: typeof record.sidebarVisible === 'boolean' ? record.sidebarVisible : DEFAULT_WORKSPACE_PREFERENCES.sidebarVisible,
    inspectorVisible: typeof record.inspectorVisible === 'boolean' ? record.inspectorVisible : DEFAULT_WORKSPACE_PREFERENCES.inspectorVisible,
    commandGroupExpanded: typeof record.commandGroupExpanded === 'boolean' ? record.commandGroupExpanded : DEFAULT_WORKSPACE_PREFERENCES.commandGroupExpanded,
    commandGroupsCollapsed: validatedCollapsedCommandGroups(record.commandGroupsCollapsed),
  };
}

export function saveWorkspacePreferences(storage: Pick<Storage, 'setItem'>, preferences: WorkspacePreferences): void {
  storage.setItem(STORAGE_KEY, JSON.stringify(preferences));
}
