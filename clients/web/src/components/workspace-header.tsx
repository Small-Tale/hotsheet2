import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './workspace-header.css';

import type { IconNode } from 'lucide';
import { ArrowDown, ArrowDownAZ, ArrowDownWideNarrow, ArrowUp, ArrowUpAZ, ArrowUpNarrowWide, Bell, CircleHelp, ClockArrowDown, ClockArrowUp, Columns3, List, ListSortAscending, ListSortDescending, MoreHorizontal, Search, Settings, Star, X } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { Select, type SelectChoice } from './select';
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
  searchTokens?: readonly {raw:string;label:string}[];
  searchTagSuggestions?: readonly string[];
  searchDatePrefix?: string;
  searchHelpOpen?: boolean;
  sort?: WorkspaceSort;
  sortDirection?: WorkspaceSortDirection;
  controlsVisible?: boolean;
  notificationCount?: number;
  selectedTicketCount?: number;
  selectedTicketsUpNext?: boolean;
  selectedTicketsUpNextEligible?: boolean;
  selectedTicketsMutable?: boolean;
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

const sortTriggerIcons: Record<WorkspaceSort, Record<WorkspaceSortDirection, { icon: IconNode; iconName: string }>> = {
  updated: { ascending: { icon: ClockArrowUp, iconName: 'clock-arrow-up' }, descending: { icon: ClockArrowDown, iconName: 'clock-arrow-down' } },
  priority: { ascending: { icon: ArrowUpNarrowWide, iconName: 'arrow-up-narrow-wide' }, descending: { icon: ArrowDownWideNarrow, iconName: 'arrow-down-wide-narrow' } },
  title: { ascending: { icon: ArrowDownAZ, iconName: 'arrow-down-a-z' }, descending: { icon: ArrowUpAZ, iconName: 'arrow-up-a-z' } },
  status: { ascending: { icon: ListSortAscending, iconName: 'list-sort-ascending' }, descending: { icon: ListSortDescending, iconName: 'list-sort-descending' } },
};

export function workspaceSortTrigger(sort: WorkspaceSort, direction: WorkspaceSortDirection) {
  return sortTriggerIcons[sort][direction];
}

const localSearchDateExample=new Intl.DateTimeFormat(undefined,{dateStyle:'short'}).format(new Date(2026,8,1));
const localSearchDateTimeExample=new Intl.DateTimeFormat(undefined,{dateStyle:'short',timeStyle:'short'}).format(new Date(2026,8,1,11,5));

export function WorkspaceControls({ mode, searchOpen = false, searchQuery = '', searchTokens=[],searchTagSuggestions=[],searchDatePrefix,searchHelpOpen=false,sort = 'updated', sortDirection = defaultWorkspaceSortDirection(sort),notificationCount=0,selectedTicketCount=0,selectedTicketsUpNext=false,selectedTicketsUpNextEligible=false,selectedTicketsMutable=true }: Omit<WorkspaceHeaderProps, 'projectName' | 'controlsVisible'>) {
  const projectActionsDisabled = mode === 'settings'||mode==='notifications';
  const ticketActionsDisabled=projectActionsDisabled||selectedTicketCount===0||!selectedTicketsMutable;
  const directionIcon=sortDirection==='ascending'?ArrowUp:ArrowDown,directionName=sortDirection==='ascending'?'arrow-up':'arrow-down';
  const sortChoices:ReadonlyArray<SelectChoice<WorkspaceSort>>=sortOptions.map(option=>({...option,...(option.value===sort?{icon:directionIcon,iconName:directionName}:{})}));
  const sortLabel=sortOptions.find(option=>option.value===sort)!.label,trigger=workspaceSortTrigger(sort,sortDirection);
  return <div class="workspace-header__actions" data-component="workspace-controls">
      <ToolbarControlGroup className="view-mode-switcher" label="View mode">
        <ModeButton mode="list" current={mode} label="List" icon={List} iconName="list" />
        <ModeButton mode="board" current={mode} label="Columns" icon={Columns3} iconName="columns-3" />
        <ModeButton mode="notifications" current={mode} label="Notifications" icon={Bell} iconName="bell" badge={notificationCount}/>
        <ModeButton mode="settings" current={mode} label="Settings" icon={Settings} iconName="settings" />
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__sort-group" single>
        <Select className="workspace-header__sort" name="workspace-sort" ariaLabel={`Sort tickets: ${sortLabel}, ${sortDirection}`} value={sort} choices={sortChoices} disabled={projectActionsDisabled} renderSelected={()=> <LucideIcon icon={trigger.icon} name={trigger.iconName} />} />
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__utility-group" label="View actions">
        <wa-button appearance="plain" disabled={ticketActionsDisabled||!selectedTicketsUpNextEligible} data-action="toggle-selected-up-next" aria-label="Toggle Up Next for selected tickets" aria-pressed={String(selectedTicketsUpNext)} title="Toggle Up Next for selected tickets"><LucideIcon icon={Star} name="star" /></wa-button>
        <wa-button appearance="plain" disabled={ticketActionsDisabled} data-action="open-selected-ticket-actions" aria-label="More actions for selected tickets" title="More actions for selected tickets"><LucideIcon icon={MoreHorizontal} name="ellipsis" /></wa-button>
      </ToolbarControlGroup>
      <ToolbarControlGroup className="workspace-header__search-group" expanded={searchOpen} single>
        {searchOpen
          ? <>
            <div class="workspace-header__search-editor">
              <wa-input class="workspace-header__search" name="workspace-search" label="Search tickets" placeholder={searchTokens.length?'Add search…':'Search tickets'} value={searchQuery} disabled={projectActionsDisabled} autofocus>
                <span slot="start" class="workspace-header__search-icon"><LucideIcon icon={Search} name="search" /></span>
                <span slot="end" class="workspace-header__search-end">{(searchQuery||searchTokens.length>0) && <button type="button" class="workspace-header__search-clear" data-action="clear-workspace-search" aria-label="Clear search" title="Clear search"><LucideIcon icon={X} name="x" /></button>}<button type="button" class="workspace-header__search-help-button" data-action="toggle-workspace-search-help" aria-label="Search syntax help" aria-expanded={String(searchHelpOpen)} title="Search syntax help"><LucideIcon icon={CircleHelp} name="circle-help" /></button></span>
              </wa-input>
              {searchTokens.length>0&&<div class="workspace-header__search-tokens">{searchTokens.map(token=><span class="workspace-header__search-token" data-component="filter-chip" data-token-raw={token.raw} title="Double-click to edit"><span>{token.label}</span><button type="button" data-action="remove-workspace-search-token" data-token-raw={token.raw} aria-label={`Remove ${token.label.replace(/^tag:/,'tag ')}`}><LucideIcon icon={X} name="x"/></button></span>)}</div>}
            </div>
            {searchTagSuggestions.length>0&&<div class="workspace-header__search-suggestions" role="listbox" aria-label="Matching tags">{searchTagSuggestions.map(tag=><button type="button" role="option" data-action="select-workspace-search-tag" data-tag={tag}>tag:{tag.includes(' ')?`"${tag}"`:tag}</button>)}</div>}
            {searchDatePrefix&&<div class="workspace-header__search-date" role="group" aria-label="Date and time helper"><label>Date<input name="workspace-search-date" type="date"/></label><label>Time (optional)<input name="workspace-search-time" type="time"/></label><button type="button" data-action="apply-workspace-search-date" data-date-prefix={searchDatePrefix}>Apply</button></div>}
            {searchHelpOpen&&<aside class="workspace-header__search-help" role="dialog" aria-label="Search syntax"><header><strong>Search syntax</strong><p>Type words, then add any filters you need.</p></header><dl>
              <div><dt>Tags</dt><dd><code>tag:client</code><code>tag:&quot;needs design&quot;</code></dd></div>
              <div><dt>Content</dt><dd><code>has:attachment</code><code>has:media-annotation</code><code>has:commit</code><code>attachment:*.png</code></dd></div>
              <div><dt>Workflow</dt><dd><code>is:up-next</code><code>is:active</code><code>is:open</code><code>is:not-started</code><code>is:started</code><code>is:completed</code><code>is:verified</code><code>is:backlog</code><code>is:archived</code></dd></div>
              <div><dt>Dates</dt><dd><code>updated-after:4h ago</code><code>{`created-after:${localSearchDateExample}`}</code><code>{`completed-before:${localSearchDateTimeExample}`}</code><code>updated-after:2026-09-01T11:05</code></dd></div>
            </dl><div class="workspace-header__search-help-notes"><p><strong>Combine filters</strong> with case-insensitive <code>AND</code>, <code>OR</code>, <code>NOT</code>, and parentheses.</p><code>(client OR server) AND NOT is:archived</code><p>NOT binds before AND, and AND before OR.</p><p><strong>Date fields:</strong> created, completed, started, verified, archived, and updated. Add <code>-before</code> or <code>-after</code>; local, relative, and ISO 8601 dates work.</p></div></aside>}
          </>
          : <wa-button class="workspace-header__search-button" appearance="plain" disabled={projectActionsDisabled} data-action="open-workspace-search" aria-label="Search tickets" title="Search tickets"><LucideIcon icon={Search} name="search" /></wa-button>}
      </ToolbarControlGroup>
    </div>;
}

export function WorkspaceHeader({ projectName, mode, searchOpen = false, searchQuery = '',searchTokens=[],searchTagSuggestions=[],searchDatePrefix,searchHelpOpen=false, sort = 'updated', sortDirection = defaultWorkspaceSortDirection(sort), controlsVisible = true, notificationCount = 0,selectedTicketCount=0,selectedTicketsUpNext=false,selectedTicketsUpNextEligible=false,selectedTicketsMutable=true }: WorkspaceHeaderProps) {
  return <header class="workspace-header" data-component="workspace-header" data-controls-visible={String(controlsVisible)}>
    <WorkspaceIdentity projectName={projectName} />
    {controlsVisible && <WorkspaceControls mode={mode} searchOpen={searchOpen} searchQuery={searchQuery} searchTokens={searchTokens} searchTagSuggestions={searchTagSuggestions} searchDatePrefix={searchDatePrefix} searchHelpOpen={searchHelpOpen} sort={sort} sortDirection={sortDirection} notificationCount={notificationCount} selectedTicketCount={selectedTicketCount} selectedTicketsUpNext={selectedTicketsUpNext} selectedTicketsUpNextEligible={selectedTicketsUpNextEligible} selectedTicketsMutable={selectedTicketsMutable} />}
  </header>;
}
