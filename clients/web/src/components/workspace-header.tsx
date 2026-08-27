import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import { ArrowDownAZ, Columns3, List, MoreHorizontal, Search, Settings, Star } from 'lucide';
import type { IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { ToolbarControlGroup } from './toolbar-control-group';
import './workspace-header.css';

export type WorkspaceViewMode = 'list' | 'board' | 'settings';
export type WorkspaceSort = 'updated' | 'priority' | 'title' | 'status';

export interface WorkspaceHeaderProps {
  projectName: string;
  viewName: string;
  mode: WorkspaceViewMode;
  searchOpen?: boolean;
  searchQuery?: string;
  sort?: WorkspaceSort;
}

function ModeButton({ mode, current, label, icon, iconName }: { mode: WorkspaceViewMode; current: WorkspaceViewMode; label: string; icon: IconNode; iconName: string }) {
  return <button type="button" class="view-mode-switcher__button" data-action="set-view-mode" data-view-mode={mode} aria-label={`${label} view`} aria-pressed={String(mode === current)} title={`${label} view`}><LucideIcon icon={icon} name={iconName} /></button>;
}

const sortOptions: ReadonlyArray<{ value: WorkspaceSort; label: string }> = [
  { value: 'updated', label: 'Recently updated' },
  { value: 'priority', label: 'Priority' },
  { value: 'title', label: 'Title' },
  { value: 'status', label: 'Status' },
];

export function WorkspaceHeader({ projectName, viewName, mode, searchOpen = false, searchQuery = '', sort = 'updated' }: WorkspaceHeaderProps) {
  return <header class="workspace-header" data-component="workspace-header">
    <div class="workspace-header__identity"><p>{projectName}</p><h1>{viewName}</h1></div>
    <div class="workspace-header__actions">
      <ToolbarControlGroup className="view-mode-switcher" label="View mode">
        <ModeButton mode="list" current={mode} label="List" icon={List} iconName="list" />
        <ModeButton mode="board" current={mode} label="Columns" icon={Columns3} iconName="columns-3" />
        <ModeButton mode="settings" current={mode} label="Settings" icon={Settings} iconName="settings" />
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__sort-group">
        <wa-dropdown class="workspace-header__sort" placement="bottom-end">
          <wa-button slot="trigger" appearance="plain" with-caret aria-label="Sort tickets" title="Sort tickets"><LucideIcon icon={ArrowDownAZ} name="arrow-down-a-z" /></wa-button>
          {sortOptions.map(option => <wa-dropdown-item type="checkbox" checked={sort === option.value} data-sort={option.value} value={option.value}>{option.label}</wa-dropdown-item>)}
        </wa-dropdown>
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__utility-group" label="View actions">
        <wa-button appearance="plain" data-action="toggle-favorite" aria-label="Favorite view" title="Favorite view"><LucideIcon icon={Star} name="star" /></wa-button>
        <wa-button appearance="plain" data-action="more-workspace-actions" aria-label="More workspace actions" title="More workspace actions"><LucideIcon icon={MoreHorizontal} name="ellipsis" /></wa-button>
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__search-group" expanded={searchOpen}>
        {searchOpen
          ? <wa-input class="workspace-header__search" name="workspace-search" label="Search tickets" placeholder="Search tickets" value={searchQuery} autofocus clearable><span slot="start" class="workspace-header__search-icon"><LucideIcon icon={Search} name="search" /></span></wa-input>
          : <wa-button class="workspace-header__search-button" appearance="plain" data-action="open-workspace-search" aria-label="Search tickets" title="Search tickets"><LucideIcon icon={Search} name="search" /></wa-button>}
      </ToolbarControlGroup>
    </div>
  </header>;
}
