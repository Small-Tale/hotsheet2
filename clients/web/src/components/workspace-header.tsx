import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import { ArrowDownAZ, Columns3, List, MoreHorizontal, Search, Settings, Star } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './workspace-header.css';

export type WorkspaceViewMode = 'list' | 'board';

export interface WorkspaceHeaderProps {
  projectName: string;
  viewName: string;
  mode: WorkspaceViewMode;
  searchOpen?: boolean;
  searchQuery?: string;
}

function ModeButton({ mode, current, label, icon, iconName }: { mode: WorkspaceViewMode; current: WorkspaceViewMode; label: string; icon: typeof List; iconName: string }) {
  return <button type="button" class="view-mode-switcher__button" data-action="set-view-mode" data-view-mode={mode} aria-label={`${label} view`} aria-pressed={String(mode === current)} title={`${label} view`}><LucideIcon icon={icon} name={iconName} /></button>;
}

export function WorkspaceHeader({ projectName, viewName, mode, searchOpen = false, searchQuery = '' }: WorkspaceHeaderProps) {
  return <header class="workspace-header" data-component="workspace-header">
    <div class="workspace-header__identity"><p>{projectName}</p><h1>{viewName}</h1></div>
    <div class="workspace-header__actions">
      <div class="view-mode-switcher" role="group" aria-label="View mode">
        <ModeButton mode="list" current={mode} label="List" icon={List} iconName="list" />
        <ModeButton mode="board" current={mode} label="Columns" icon={Columns3} iconName="columns-3" />
      </div>
      <wa-button appearance="outlined" data-action="sort-tickets" aria-label="Sort tickets" title="Sort tickets"><LucideIcon icon={ArrowDownAZ} name="arrow-down-a-z" /></wa-button>
      <wa-button appearance="outlined" data-action="toggle-favorite" aria-label="Favorite view" title="Favorite view"><LucideIcon icon={Star} name="star" /></wa-button>
      <wa-button appearance="outlined" data-action="more-workspace-actions" aria-label="More workspace actions" title="More workspace actions"><LucideIcon icon={MoreHorizontal} name="ellipsis" /></wa-button>
      <wa-button appearance="outlined" data-action="toggle-workspace-search" aria-label={searchOpen ? 'Close search' : 'Search tickets'} title={searchOpen ? 'Close search' : 'Search tickets'}><LucideIcon icon={Search} name="search" /></wa-button>
      <wa-button appearance="outlined" data-action="open-workspace-settings" aria-label="Workspace settings" title="Workspace settings"><LucideIcon icon={Settings} name="settings" /></wa-button>
    </div>
    {searchOpen && <wa-input class="workspace-header__search" name="workspace-search" label="Search tickets" value={searchQuery} autofocus clearable></wa-input>}
  </header>;
}
