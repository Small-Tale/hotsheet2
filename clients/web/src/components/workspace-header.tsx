import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './workspace-header.css';

import type { IconNode } from 'lucide';
import { ArrowDown, ArrowDownAZ, ArrowUp, Bell, Columns3, List, MoreHorizontal, Search, Settings, Star, X } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { ToolbarControlGroup } from './toolbar-control-group';
import { ToolbarText } from './toolbar-text';

export type WorkspaceViewMode = 'list' | 'board' | 'notifications' | 'settings';
export type WorkspaceSort = 'updated' | 'priority' | 'title' | 'status';
export type WorkspaceSortDirection = 'ascending' | 'descending';

export interface WorkspaceHeaderProps {
  projectName: string;
  mode: WorkspaceViewMode;
  searchOpen?: boolean;
  searchQuery?: string;
  sort?: WorkspaceSort;
  sortDirection?: WorkspaceSortDirection;
  controlsVisible?: boolean;
  notificationCount?: number;
}

export function WorkspaceIdentity({ projectName }: { projectName: string }) {
  return <div class="workspace-header__identity" data-component="workspace-identity"><ToolbarText text={projectName} size="large" /></div>;
}

function ModeButton({ mode, current, label, icon, iconName, badge = 0 }: { mode: WorkspaceViewMode; current: WorkspaceViewMode; label: string; icon: IconNode; iconName: string; badge?:number }) {
  return <button type="button" class="view-mode-switcher__button" data-action="set-view-mode" data-view-mode={mode} aria-label={`${label} view${badge?`, ${badge} pending`:''}`} aria-pressed={String(mode === current)} title={`${label} view`}><LucideIcon icon={icon} name={iconName} />{badge>0&&<span class="view-mode-switcher__badge" aria-hidden="true">{badge>99?'99+':badge}</span>}</button>;
}

const sortOptions: ReadonlyArray<{ value: WorkspaceSort; label: string }> = [
  { value: 'updated', label: 'Recently updated' },
  { value: 'priority', label: 'Priority' },
  { value: 'title', label: 'Title' },
  { value: 'status', label: 'Status' },
];

export function defaultWorkspaceSortDirection(sort: WorkspaceSort): WorkspaceSortDirection {
  return sort === 'updated' ? 'descending' : 'ascending';
}

export function nextWorkspaceSort(current: WorkspaceSort, direction: WorkspaceSortDirection, selected: WorkspaceSort): { sort: WorkspaceSort; direction: WorkspaceSortDirection } {
  if (selected !== current) return { sort: selected, direction: defaultWorkspaceSortDirection(selected) };
  return { sort: current, direction: direction === 'ascending' ? 'descending' : 'ascending' };
}

export function applyWorkspaceSortDirection(comparison: number, direction: WorkspaceSortDirection): number {
  return direction === 'ascending' ? comparison : -comparison;
}

export function WorkspaceControls({ mode, searchOpen = false, searchQuery = '', sort = 'updated', sortDirection = defaultWorkspaceSortDirection(sort),notificationCount=0 }: Omit<WorkspaceHeaderProps, 'projectName' | 'controlsVisible'>) {
  const projectActionsDisabled = mode === 'settings'||mode==='notifications';
  return <div class="workspace-header__actions" data-component="workspace-controls">
      <ToolbarControlGroup className="view-mode-switcher" label="View mode">
        <ModeButton mode="list" current={mode} label="List" icon={List} iconName="list" />
        <ModeButton mode="board" current={mode} label="Columns" icon={Columns3} iconName="columns-3" />
        <ModeButton mode="notifications" current={mode} label="Notifications" icon={Bell} iconName="bell" badge={notificationCount}/>
        <ModeButton mode="settings" current={mode} label="Settings" icon={Settings} iconName="settings" />
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__sort-group" single>
        <wa-dropdown class="workspace-header__sort" placement="bottom-end">
          <wa-button slot="trigger" appearance="plain" with-caret disabled={projectActionsDisabled} aria-label="Sort tickets" title="Sort tickets"><LucideIcon icon={ArrowDownAZ} name="arrow-down-a-z" /></wa-button>
          {sortOptions.map(option => {
            const selected = sort === option.value;
            const directionLabel = sortDirection === 'ascending' ? 'ascending' : 'descending';
            return <wa-dropdown-item aria-current={selected ? 'true' : undefined} aria-label={selected ? `${option.label}, ${directionLabel}` : option.label} data-sort={option.value} value={option.value}>
              {selected && <span slot="icon" class="workspace-header__sort-direction"><LucideIcon icon={sortDirection === 'ascending' ? ArrowUp : ArrowDown} name={sortDirection === 'ascending' ? 'arrow-up' : 'arrow-down'} /></span>}
              {option.label}
            </wa-dropdown-item>;
          })}
        </wa-dropdown>
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__utility-group" label="View actions">
        <wa-button appearance="plain" disabled={projectActionsDisabled} data-action="toggle-favorite" aria-label="Favorite view" title="Favorite view"><LucideIcon icon={Star} name="star" /></wa-button>
        <wa-button appearance="plain" disabled={projectActionsDisabled} data-action="more-workspace-actions" aria-label="More workspace actions" title="More workspace actions"><LucideIcon icon={MoreHorizontal} name="ellipsis" /></wa-button>
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__search-group" expanded={searchOpen} single>
        {searchOpen
          ? <wa-input class="workspace-header__search" name="workspace-search" label="Search tickets" placeholder="Search tickets" value={searchQuery} disabled={projectActionsDisabled} autofocus><span slot="start" class="workspace-header__search-icon"><LucideIcon icon={Search} name="search" /></span>{searchQuery && <button type="button" slot="end" class="workspace-header__search-clear" data-action="clear-workspace-search" aria-label="Clear search" title="Clear search"><LucideIcon icon={X} name="x" /></button>}</wa-input>
          : <wa-button class="workspace-header__search-button" appearance="plain" disabled={projectActionsDisabled} data-action="open-workspace-search" aria-label="Search tickets" title="Search tickets"><LucideIcon icon={Search} name="search" /></wa-button>}
      </ToolbarControlGroup>
    </div>;
}

export function WorkspaceHeader({ projectName, mode, searchOpen = false, searchQuery = '', sort = 'updated', sortDirection = defaultWorkspaceSortDirection(sort), controlsVisible = true }: WorkspaceHeaderProps) {
  return <header class="workspace-header" data-component="workspace-header" data-controls-visible={String(controlsVisible)}>
    <WorkspaceIdentity projectName={projectName} />
    {controlsVisible && <WorkspaceControls mode={mode} searchOpen={searchOpen} searchQuery={searchQuery} sort={sort} sortDirection={sortDirection} />}
  </header>;
}
