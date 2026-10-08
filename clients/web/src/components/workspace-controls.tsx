import '@awesome.me/webawesome/dist/components/button/button.js';
import './workspace-header.css';

import { foregroundColorVar, px } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { SegmentedControl, type SegmentedControlChoice } from '@kerfjs/ui/segmented-control';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import type { TokenSearchModel } from '@kerfjs/ui/token-search-model';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import type { IconNode } from 'lucide';
import {
  ArrowDownAZ,
  ArrowDownWideNarrow,
  ArrowUpAZ,
  ArrowUpNarrowWide,
  Bell,
  ClockArrowDown,
  ClockArrowUp,
  Columns3,
  List,
  ListSortAscending,
  ListSortDescending,
  MoreHorizontal,
  Search,
  Settings,
  Star,
  Trash2,
} from 'lucide';

import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import { createTicketSearchModel } from '../ticket-search-model';
import { QuickTicketLauncher } from './quick-ticket-composer';
import { TicketSearchField } from './ticket-search-field';

export type WorkspaceViewMode = 'list' | 'board' | 'notifications' | 'settings';
export type WorkspaceControlsPresentation = 'toolbar' | 'rail';
export type WorkspaceSort = 'updated' | 'priority' | 'title' | 'status';
export type WorkspaceSortDirection = 'ascending' | 'descending';
export type WorkspaceUpNextState = 'none' | 'mixed' | 'all';
export const WORKSPACE_SEARCH_INLINE_BREAKPOINT = 1024;

export function workspaceUpNextState(values: readonly boolean[]): WorkspaceUpNextState {
  return values.some(Boolean) ? (values.every(Boolean) ? 'all' : 'mixed') : 'none';
}

function workspaceUpNextDetails(state: WorkspaceUpNextState): string {
  switch (state) {
    case 'none':
      return 'None';
    case 'mixed':
      return 'Some';
    case 'all':
      return 'All';
    default:
      return state satisfies never;
  }
}

function WorkspaceUpNextIcon({ state }: { state: WorkspaceUpNextState }) {
  // Starred selections take the Up Next color; an unstarred one inherits the control's text color.
  const color = state === 'none' ? undefined : foregroundColorVar('--hs-ticket-state-up-next');
  return (
    <span class="workspace-header__up-next-icon" data-up-next-state={state} aria-hidden="true">
      <LucideIcon icon={Star} name="star" appearance={state === 'all' ? 'solid' : 'outline'} color={color} />
      {state === 'mixed' && (
        <span class="workspace-header__up-next-fill">
          <LucideIcon icon={Star} name="star" appearance="solid" color={color} />
        </span>
      )}
    </span>
  );
}

let inertSearchModel: TokenSearchModel | undefined;
/** An empty, tag-less model for previews that render the controls without the app's search state. */
function previewSearchModel(): TokenSearchModel {
  inertSearchModel ??= createTicketSearchModel({ tags: () => [] });
  return inertSearchModel;
}

export interface WorkspaceHeaderProps {
  projectName: string;
  mode: WorkspaceViewMode;
  presentation?: WorkspaceControlsPresentation;
  searchOpen?: boolean;
  /**
   * The Kerf-managed workspace search model the TicketSearchField renders (HS2-5JXBQY). The app
   * always passes its own; a standalone preview without one renders an inert, tag-less model.
   */
  searchModel?: TokenSearchModel;
  searchHelpOpen?: boolean;
  sort?: WorkspaceSort;
  sortDirection?: WorkspaceSortDirection;
  controlsVisible?: boolean;
  notificationCount?: number;
  selectedTicketCount?: number;
  selectedTicketsUpNext?: WorkspaceUpNextState;
  selectedTicketsUpNextEligible?: boolean;
  selectedTicketsMutable?: boolean;
}

/** The project name as Toolbar leading text; it hides at the narrowest toolbar widths. */
export function WorkspaceIdentity({
  projectName,
  id,
  headingLevel,
  searchOpen = false,
}: {
  projectName: string;
  id?: string;
  headingLevel?: 1 | 2 | 3 | 4 | 5 | 6;
  searchOpen?: boolean;
}) {
  return (
    <ToolbarText
      className="workspace-header__identity"
      hideBelow={px(searchOpen ? WORKSPACE_SEARCH_INLINE_BREAKPOINT : 224)}
      text={projectName}
      id={id}
      size="large"
      headingLevel={headingLevel}
    />
  );
}

/** The current ticket view's primary action: create a ticket, or empty the trash in the Trash view. It is
 * data so each placement renders it as literal zone JSX that Kerf's composition rule can check
 * (HS2-PNCDAE). */
export type TicketViewActionSpec =
  | { kind: 'new-ticket'; attachmentsEnabled?: boolean; label?: string; size?: 'default' | 'compact' }
  | { kind: 'empty-trash' };

/** The Trash view's standalone outlined danger `wa-button` (declared `rendersAs @kerfjs/ui:wa-button`). */
export function EmptyTrashAction() {
  return (
    <wa-button
      class="workspace-header__text-action"
      appearance="outlined"
      variant="danger"
      {...TICKET_SELECTION_ACTIONS.openEmptyTrash.attrs}
    >
      <span class="workspace-header__text-action-label">
        <LucideIcon icon={Trash2} name="trash-2" />
        <span>Empty Trash</span>
      </span>
    </wa-button>
  );
}

/** Renders a {@link TicketViewActionSpec} as its single standalone `wa-button` (HS2-PNCDAE). */
export function TicketViewAction({ action }: { action: TicketViewActionSpec }) {
  return action.kind === 'empty-trash' ? (
    <EmptyTrashAction />
  ) : (
    <QuickTicketLauncher attachmentsEnabled={action.attachmentsEnabled} label={action.label} size={action.size} />
  );
}

function workspaceModeChoices(
  count: number,
  presentation: WorkspaceControlsPresentation,
): ReadonlyArray<SegmentedControlChoice<WorkspaceViewMode>> {
  const modes: ReadonlyArray<{ value: WorkspaceViewMode; label: string; icon: IconNode; iconName: string }> = [
    { value: 'list', label: 'List', icon: List, iconName: 'list' },
    { value: 'board', label: 'Columns', icon: Columns3, iconName: 'columns-3' },
    { value: 'notifications', label: 'Notifications', icon: Bell, iconName: 'bell' },
    { value: 'settings', label: 'Settings', icon: Settings, iconName: 'settings' },
  ];
  return (
    modes
      // The rail offers list, columns (paged), and notifications; settings stay in the main workspace (HS2-656Q43).
      .filter(({ value }) => presentation !== 'rail' || value !== 'settings')
      .map(({ value, label, icon, iconName }) => {
        const badge = value === 'notifications' ? count : 0;
        return {
          value,
          label: `${label} view${badge ? `, ${badge} pending` : ''}`,
          title: `${label} view`,
          content: (
            <span class="view-mode-switcher__content">
              <LucideIcon icon={icon} name={iconName} />
              {badge > 0 && (
                <span class="view-mode-switcher__badge" aria-hidden="true">
                  {badge > 99 ? '99+' : badge}
                </span>
              )}
            </span>
          ),
        };
      })
  );
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

export function nextWorkspaceSort(
  current: WorkspaceSort,
  direction: WorkspaceSortDirection,
  selected: WorkspaceSort,
): { sort: WorkspaceSort; direction: WorkspaceSortDirection } {
  if (selected !== current) return { sort: selected, direction: defaultWorkspaceSortDirection(selected) };
  return { sort: current, direction: direction === 'ascending' ? 'descending' : 'ascending' };
}

export function applyWorkspaceSortDirection(comparison: number, direction: WorkspaceSortDirection): number {
  return direction === 'ascending' ? comparison : -comparison;
}

const sortTriggerIcons: Record<WorkspaceSort, Record<WorkspaceSortDirection, { icon: IconNode; iconName: string }>> = {
  updated: {
    ascending: { icon: ClockArrowUp, iconName: 'clock-arrow-up' },
    descending: { icon: ClockArrowDown, iconName: 'clock-arrow-down' },
  },
  priority: {
    ascending: { icon: ArrowUpNarrowWide, iconName: 'arrow-up-narrow-wide' },
    descending: { icon: ArrowDownWideNarrow, iconName: 'arrow-down-wide-narrow' },
  },
  title: {
    ascending: { icon: ArrowDownAZ, iconName: 'arrow-down-a-z' },
    descending: { icon: ArrowUpAZ, iconName: 'arrow-up-a-z' },
  },
  status: {
    ascending: { icon: ListSortAscending, iconName: 'list-sort-ascending' },
    descending: { icon: ListSortDescending, iconName: 'list-sort-descending' },
  },
};

export function workspaceSortTrigger(sort: WorkspaceSort, direction: WorkspaceSortDirection) {
  return sortTriggerIcons[sort][direction];
}

export function wireWorkspaceOverflowKeyboard(root: Document | HTMLElement): () => void {
  const onKeydown = (event: Event) => {
    const keyboard = event as KeyboardEvent;
    if (!['Enter', ' ', 'ArrowDown'].includes(keyboard.key)) return;
    const origin = keyboard.target;
    if (!(origin instanceof Element)) return;
    const trigger = origin.closest<HTMLElement>('[data-workspace-overflow] > [slot="trigger"]');
    if (!trigger) return;
    const dropdown = trigger.closest<HTMLElement & { open: boolean }>('[data-workspace-overflow]');
    if (!dropdown || dropdown.open) return;
    keyboard.preventDefault();
    keyboard.stopPropagation();
    dropdown.open = true;
  };
  const onAfterShow = (event: Event) => {
    const dropdown = event.target;
    if (!(dropdown instanceof HTMLElement) || !dropdown.matches('[data-workspace-overflow]')) return;
    requestAnimationFrame(() => {
      const first = dropdown.querySelector<HTMLElement & { active?: boolean }>('wa-dropdown-item:not([disabled])');
      if (!first) return;
      first.active = true;
      first.tabIndex = 0;
      first.focus({ preventScroll: true });
    });
  };
  root.addEventListener('keydown', onKeydown);
  root.addEventListener('wa-after-show', onAfterShow);
  return () => {
    root.removeEventListener('keydown', onKeydown);
    root.removeEventListener('wa-after-show', onAfterShow);
  };
}

function WorkspaceOverflowControls({
  mode,
  projectActionsDisabled,
  ticketActionsDisabled,
  searchOpen,
  sort,
  sortDirection,
  visibleSortOptions,
  notificationCount,
  selectedTicketsUpNext,
  selectedTicketsUpNextEligible,
  presentation,
}: {
  mode: WorkspaceViewMode;
  projectActionsDisabled: boolean;
  ticketActionsDisabled: boolean;
  searchOpen: boolean;
  sort: WorkspaceSort;
  sortDirection: WorkspaceSortDirection;
  visibleSortOptions: ReadonlyArray<{ value: WorkspaceSort; label: string }>;
  notificationCount: number;
  selectedTicketsUpNext: WorkspaceUpNextState;
  selectedTicketsUpNextEligible: boolean;
  presentation: WorkspaceControlsPresentation;
}) {
  // The rail keeps its controls on wrapped rows; its overflow menu was never shown.
  if (presentation === 'rail') return <></>;
  const modes: ReadonlyArray<{ value: WorkspaceViewMode; label: string; icon: IconNode; iconName: string }> = [
    { value: 'list', label: 'Show List View', icon: List, iconName: 'list' },
    { value: 'board', label: 'Show Columns View', icon: Columns3, iconName: 'columns-3' },
    {
      value: 'notifications',
      label: `Show Notifications${notificationCount ? ` (${notificationCount} pending)` : ''}`,
      icon: Bell,
      iconName: 'bell',
    },
    { value: 'settings', label: 'Show Settings', icon: Settings, iconName: 'settings' },
  ];
  const overflowIcon = (icon: IconNode, name: string) => <LucideIcon icon={icon} name={name} />;
  const menu = (
    <PopupMenu
      label="More workspace controls"
      icon={<LucideIcon icon={MoreHorizontal} name="ellipsis" />}
      caret={false}
      placement="bottom-end"
      rootAttributes={{ 'data-workspace-overflow': 'true', 'data-token-search-keep-open': 'true' }}
      items={[
        {
          label: 'Toggle Up Next',
          disabled: ticketActionsDisabled || !selectedTicketsUpNextEligible,
          icon: <WorkspaceUpNextIcon state={selectedTicketsUpNext} />,
          details: <>{workspaceUpNextDetails(selectedTicketsUpNext)}</>,
          attributes: {
            'data-workspace-overflow-kind': 'utility',
            'data-workspace-overflow-action': 'toggle-selected-up-next',
            'data-workspace-overflow-state': selectedTicketsUpNext,
          },
        },
        {
          label: 'Show Selected Ticket Actions…',
          disabled: ticketActionsDisabled,
          icon: overflowIcon(MoreHorizontal, 'ellipsis'),
          attributes: {
            'data-workspace-overflow-kind': 'utility',
            'data-workspace-overflow-action': 'open-selected-ticket-actions',
          },
        },
        { kind: 'divider' },
        ...visibleSortOptions.map((option): PopupMenuItem => {
          const direction = option.value === sort ? sortDirection : defaultWorkspaceSortDirection(option.value),
            icon = workspaceSortTrigger(option.value, direction);
          return {
            label: `Sort by ${option.label}${option.value === sort ? `, ${direction}` : ''}`,
            checked: option.value === sort,
            disabled: projectActionsDisabled,
            icon: overflowIcon(icon.icon, icon.iconName),
            attributes: {
              'data-workspace-overflow-kind': 'sort',
              'data-workspace-overflow-action': 'set-workspace-sort',
              'data-workspace-sort': option.value,
            },
          };
        }),
        { kind: 'divider' },
        {
          label: searchOpen ? 'Focus Search' : 'Search Tickets',
          disabled: projectActionsDisabled,
          icon: overflowIcon(Search, 'search'),
          attributes: {
            'data-workspace-overflow-kind': 'search',
            'data-workspace-overflow-action': 'open-workspace-search',
          },
        },
        { kind: 'divider' },
        ...modes.map((option): PopupMenuItem => ({
          label: option.label,
          checked: option.value === mode,
          icon: overflowIcon(option.icon, option.iconName),
          attributes: {
            'data-workspace-overflow-kind': 'view',
            'data-workspace-overflow-action': 'set-view-mode',
            'data-view-mode': option.value,
          },
        })),
      ]}
    />
  );
  return (
    // Keep the narrow toolbar's actions reachable while search is expanded (HS2-NZK4KA).
    <ToolbarControlGroup
      className="workspace-header__overflow-group"
      single
      appearance="borderless"
      nestedDropdown
      showBelow={px(searchOpen ? WORKSPACE_SEARCH_INLINE_BREAKPOINT : 480)}
    >
      {menu}
    </ToolbarControlGroup>
  );
}

/**
 * The workspace controls as Toolbar zone content: view-mode switcher, sort, selection
 * actions, the collapsible ticket search, and the narrow-width overflow menu, each a
 * cataloged ToolbarControlGroup (or the TicketSearchField that renders one). There is no
 * wrapper element; the enclosing Toolbar zone (or the workspace-grid rail's own grid) owns
 * layout (HS2-EZ1N7Z). The groups yield to the open search and the search field sizes itself through
 * Kerf ToolbarControlGroup props, chosen by `presentation` (HS2-8FS5BJ, HS2-DAMHD1).
 */
export function WorkspaceControls({
  mode,
  presentation = 'toolbar',
  searchOpen = false,
  searchModel = previewSearchModel(),
  searchHelpOpen = false,
  sort = 'updated',
  sortDirection = defaultWorkspaceSortDirection(sort),
  notificationCount = 0,
  selectedTicketCount = 0,
  selectedTicketsUpNext = 'none',
  selectedTicketsUpNextEligible = false,
  selectedTicketsMutable = true,
}: Omit<WorkspaceHeaderProps, 'projectName' | 'controlsVisible'>) {
  const projectActionsDisabled = mode === 'settings' || mode === 'notifications';
  const ticketActionsDisabled = projectActionsDisabled || selectedTicketCount === 0 || !selectedTicketsMutable;
  const visibleSortOptions = mode === 'board' ? sortOptions.filter((option) => option.value !== 'status') : sortOptions;
  const sortChoices: ReadonlyArray<SelectChoice<WorkspaceSort>> = visibleSortOptions.map((option) => {
    const choiceIcon = workspaceSortTrigger(
      option.value,
      option.value === sort ? sortDirection : defaultWorkspaceSortDirection(option.value),
    );
    return { ...option, icon: choiceIcon.icon, iconName: choiceIcon.iconName };
  });
  const sortLabel = sortOptions.find((option) => option.value === sort)!.label,
    trigger = workspaceSortTrigger(sort, sortDirection);
  // The rail's groups carry a `--rail` modifier so the header's responsive overflow queries (which
  // fire in any narrow Kerf toolbar) leave them in place; the rail's toolbar wraps them onto rows
  // instead of yielding them to the overflow menu (HS2-K9KWJJ). Kerf's analyzer classifies only
  // literal class names on its components, so each rail variant is spelled out.
  const rail = presentation === 'rail',
    viewSwitcher = (
      <SegmentedControl
        id="workspace-view-mode"
        label="View mode"
        value={mode}
        choices={workspaceModeChoices(notificationCount, presentation)}
        action="set-view-mode"
        appearance="toolbar"
        shape={rail ? 'rounded' : 'pill'}
        layout={rail ? 'equal' : 'content'}
      />
    ),
    sortSelect = (
      <Select
        name="workspace-sort"
        ariaLabel={`Sort tickets: ${sortLabel}, ${sortDirection}`}
        value={sort}
        choices={sortChoices}
        disabled={projectActionsDisabled}
        selectedPresentation="icon-only"
        presentation="toolbar-borderless"
        // A caret-free icon trigger (Kerf beta.62) fills its single pill group like a round action.
        caret={false}
        focusRingOwner="group"
        renderSelected={() => <LucideIcon icon={trigger.icon} name={trigger.iconName} />}
      />
    ),
    utilityButtons = (
      <>
        <button
          type="button"
          class="workspace-header__up-next-button"
          disabled={ticketActionsDisabled || !selectedTicketsUpNextEligible}
          {...TICKET_SELECTION_ACTIONS.toggleSelectedUpNext.attrs}
          aria-label="Toggle Up Next for selected tickets"
          aria-pressed={selectedTicketsUpNext === 'mixed' ? 'mixed' : String(selectedTicketsUpNext === 'all')}
          title="Toggle Up Next for selected tickets"
        >
          <WorkspaceUpNextIcon state={selectedTicketsUpNext} />
        </button>
        <button
          type="button"
          disabled={ticketActionsDisabled}
          {...TICKET_SELECTION_ACTIONS.openSelectedTicketActions.attrs}
          aria-label="More actions for selected tickets"
          title="More actions for selected tickets"
        >
          <LucideIcon icon={MoreHorizontal} name="ellipsis" />
        </button>
      </>
    );
  // The compact toolbar gives search and More one app-owned flex slot; the other groups yield
  // below the width needed to show them together. The rail keeps its own wrapped rows.
  const searchField = (
    <TicketSearchField
      id="workspace-search"
      label="Search tickets"
      model={searchModel}
      disabled={projectActionsDisabled}
      autofocus
      collapsible
      expanded={searchOpen}
      helpOpen={searchHelpOpen}
      layout={rail ? 'row' : 'inline'}
    />
  );
  const overflowControls = (
    <WorkspaceOverflowControls
      mode={mode}
      projectActionsDisabled={projectActionsDisabled}
      ticketActionsDisabled={ticketActionsDisabled}
      searchOpen={searchOpen}
      sort={sort}
      sortDirection={sortDirection}
      visibleSortOptions={visibleSortOptions}
      notificationCount={notificationCount}
      selectedTicketsUpNext={selectedTicketsUpNext}
      selectedTicketsUpNextEligible={selectedTicketsUpNextEligible}
      presentation={presentation}
    />
  );
  return (
    <>
      {rail ? (
        <ToolbarControlGroup className="view-mode-switcher view-mode-switcher--rail" shape="rounded" sizing="fill">
          {viewSwitcher}
        </ToolbarControlGroup>
      ) : (
        <ToolbarControlGroup
          className="view-mode-switcher"
          shape="pill"
          visibility="yield-to-expanded-sibling"
          hideBelow={px(searchOpen ? WORKSPACE_SEARCH_INLINE_BREAKPOINT : 176)}
        >
          {viewSwitcher}
        </ToolbarControlGroup>
      )}
      {rail ? (
        <ToolbarControlGroup
          className="workspace-header__sort-group workspace-header__sort-group--rail"
          single
          focusRing="outline"
        >
          {sortSelect}
        </ToolbarControlGroup>
      ) : (
        <ToolbarControlGroup
          className="workspace-header__sort-group"
          single
          shape="pill"
          focusRing="outline"
          visibility="yield-to-expanded-sibling"
          hideBelow={px(searchOpen ? WORKSPACE_SEARCH_INLINE_BREAKPOINT : 416)}
        >
          {sortSelect}
        </ToolbarControlGroup>
      )}
      {rail ? (
        <ToolbarControlGroup
          className="workspace-header__utility-group workspace-header__utility-group--rail"
          label="View actions"
          selectedChrome="outline"
          selectedTone="pop"
        >
          {utilityButtons}
        </ToolbarControlGroup>
      ) : (
        <ToolbarControlGroup
          className="workspace-header__utility-group"
          label="View actions"
          selectedChrome="outline"
          selectedTone="pop"
          visibility="yield-to-expanded-sibling"
          hideBelow={px(searchOpen ? WORKSPACE_SEARCH_INLINE_BREAKPOINT : 480)}
        >
          {utilityButtons}
        </ToolbarControlGroup>
      )}
      {rail ? (
        searchField
      ) : (
        <div class="workspace-header__search-actions" data-search-open={String(searchOpen)}>
          {searchField}
          {overflowControls}
        </div>
      )}
    </>
  );
}
