import '@kerfjs/ui/layout.css';

import { Badge } from '@kerfjs/ui/badge';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PagedView } from '@kerfjs/ui/paged-view';

import { type GridSidebarLive, liveStateTitle, liveWordColor, triageOrder } from '../grid-sidebar-live';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { LIVE_STATE_ICONS } from '../live-state-icons';
import { GridSidebarGauges } from './grid-sidebar-gauges';
import { GridSidebarGroup } from './grid-sidebar-group';
import { GridSidebarTriageRow } from './grid-sidebar-triage-row';
import { aggregateAlignedChartValues, chartDomainMaximum, ProjectSummary } from './project-summary';
import { SidebarPane, type SidebarPanelParts } from './sidebar-panel';

export interface TerminalProjectSummary {
  id: string;
  name: string;
  completedToday: number;
  inProgress: number;
  trend: number[];
  partial?: boolean;
  /** The project's live numbers for the gauge and triage pages (HS2-E1AQE8). */
  live?: GridSidebarLive;
}

export function aggregateTerminalProjectSummaries(projects: readonly TerminalProjectSummary[]): TerminalProjectSummary {
  const trend = aggregateAlignedChartValues(projects.map((project) => project.trend));
  return {
    id: 'all',
    name: 'All projects',
    completedToday: projects.reduce((sum, project) => sum + project.completedToday, 0),
    inProgress: projects.reduce((sum, project) => sum + project.inProgress, 0),
    trend,
    ...(projects.some((project) => project.partial) ? { partial: true } : {}),
  };
}

/** The grid sidebar's pages (HS2-E1AQE8, docs/34-glance-surfaces.md §34.7); Completion is the default. */
export const GRID_SIDEBAR_PAGES = ['completion', 'gauges', 'triage'] as const;
export type GridSidebarPage = (typeof GRID_SIDEBAR_PAGES)[number];
/** The grid sidebar PagedView's DOM id, which `wirePagedViews` reports with each page change. */
export const GRID_SIDEBAR_PAGED_VIEW_ID = 'grid-sidebar-pages';
/** Where the chosen page persists per viewer (HS2-EZSQDF). */
export const GRID_SIDEBAR_PAGE_STORAGE_KEY = 'hotsheet.terminals.sidebar-page';

/** A stored or reported page id, or Completion when it is missing or unknown. */
export function parseGridSidebarPage(value: string | null | undefined): GridSidebarPage {
  return GRID_SIDEBAR_PAGES.find((page) => page === value) ?? 'completion';
}
export const GRID_SIDEBAR_PAGE_LABELS: Readonly<Record<GridSidebarPage, string>> = {
  completion: 'Completion',
  gauges: 'Gauges',
  triage: 'Triage',
};

export interface TerminalOperationsPanelProps {
  projects: readonly TerminalProjectSummary[];
  /** Live numbers across every open project, for the All projects group of the gauge and triage pages. */
  allLive?: GridSidebarLive;
  /** The page to show; Completion by default. */
  page?: GridSidebarPage;
}

/** Today's completion charts: the unchanged first page. */
function completionBody(group: TerminalProjectSummary, aggregate: TerminalProjectSummary | undefined) {
  return (
    <ProjectSummary
      completedToday={group.completedToday}
      inProgress={group.inProgress}
      trend={group.trend}
      partial={group.partial}
      projectId={group.id}
      size="compact"
      chartTone={group.id === 'all' ? 'success' : 'brand'}
      chartMaximum={aggregate ? chartDomainMaximum(aggregate.trend) : undefined}
      backgroundTrend={group.id === 'all' ? undefined : aggregate?.trend}
    />
  );
}

/** The triage page's All projects body: Needs you only when non-zero, then workers live. */
function triageSummaryBody(live: GridSidebarLive | undefined) {
  if (!live) return <GridSidebarTriageRow projectId="all" name="All projects" />;
  const action = NAVIGATION_AND_TABS_ACTIONS.openProjectStats.value;
  const title = liveStateTitle('All projects', live);
  return (
    <>
      {live.waiting > 0 ? (
        <ListItem
          action={action}
          rootAttributes={{ 'data-project-id': 'all', 'data-triage-summary': 'waiting' }}
          icon={
            <LucideIcon
              icon={LIVE_STATE_ICONS.waiting_on_you.icon}
              name={LIVE_STATE_ICONS.waiting_on_you.name}
              color={liveWordColor('waiting_on_you')}
            />
          }
          label="Needs you"
          trailing={
            <Badge tone="danger" appearance="solid" label={`${live.waiting} waiting on you`}>
              {live.waiting}
            </Badge>
          }
          title={`${live.waiting} ticket${live.waiting === 1 ? '' : 's'} waiting on you`}
        />
      ) : (
        <></>
      )}
      <ListItem
        action={action}
        rootAttributes={{ 'data-project-id': 'all', 'data-triage-summary': 'workers' }}
        icon={
          <LucideIcon
            icon={LIVE_STATE_ICONS.active.icon}
            name={LIVE_STATE_ICONS.active.name}
            color={liveWordColor(live.workersLive > 0 ? 'active' : 'stopped')}
          />
        }
        label={`${live.workersLive} of ${live.workersExpected}`}
        title={title}
        accessibleLabel={`Open project statistics. ${title}`}
      />
    </>
  );
}

/**
 * One page's groups (HS2-E1AQE8). Every page renders the same `GridSidebarGroup` per project (header,
 * divider under All projects, body), so only the body differs: a completion chart, a gauge pair, or a
 * triage row. Triage orders projects by need; the other pages keep the open-project order.
 */
export function gridSidebarPageContent(page: GridSidebarPage, { projects, allLive }: TerminalOperationsPanelProps) {
  const aggregate = projects.length > 1 ? aggregateTerminalProjectSummaries(projects) : undefined;
  const ordered = page === 'triage' ? triageOrder(projects) : projects;
  const groups: TerminalProjectSummary[] = aggregate ? [{ ...aggregate, live: allLive }, ...ordered] : [...ordered];
  // Project gauges read against the All projects scale so their needles compare (HS2-VFQM4D), as the
  // completion charts share the aggregate's maximum (HS2-C9JM65).
  const sharedScale =
    aggregate && allLive
      ? { tokensPeakPerMinute: allLive.tokensPeakPerMinute, ticketsPeakPerHour: allLive.ticketsPeakPerHour }
      : undefined;
  return (
    <List gap="m" controlInsets="trbl" rootAttributes={{ 'data-grid-sidebar-page': page }}>
      {groups.map((group) => (
        <GridSidebarGroup projectId={group.id} name={group.name} divider={group.id === 'all'}>
          {page === 'completion' ? (
            completionBody(group, aggregate)
          ) : page === 'gauges' ? (
            <GridSidebarGauges
              projectId={group.id}
              name={group.name}
              live={group.live}
              showState={group.id !== 'all'}
              scale={group.id === 'all' ? undefined : sharedScale}
            />
          ) : group.id === 'all' ? (
            triageSummaryBody(group.live)
          ) : (
            <GridSidebarTriageRow projectId={group.id} name={group.name} live={group.live} />
          )}
        </GridSidebarGroup>
      ))}
    </List>
  );
}

/** The operations sidebar's panel parts for the Workbench's left rail (HS2-RWGQWN). */
export function terminalOperationsPanel(props: TerminalOperationsPanelProps): SidebarPanelParts {
  return {
    label: 'Terminal operations sidebar',
    toolbar: { label: 'Operations sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'Operations Sidebar' },
    // The pages fill the pane; each scrolls under Kerf's bottom-floating page dots (HS2-EZSQDF).
    content: (
      <PagedView
        id={GRID_SIDEBAR_PAGED_VIEW_ID}
        label="Sidebar pages"
        page={props.page ?? 'completion'}
        pages={GRID_SIDEBAR_PAGES.map((page) => ({
          id: page,
          label: GRID_SIDEBAR_PAGE_LABELS[page],
          content: gridSidebarPageContent(page, props),
        }))}
      />
    ),
    pane: { contentElement: 'section', contentLabel: 'Terminal operations sidebar' },
  };
}

export function TerminalOperationsSidebar(props: TerminalOperationsPanelProps) {
  return <SidebarPane parts={terminalOperationsPanel(props)} className="terminal-operations-sidebar" collapseControl />;
}
