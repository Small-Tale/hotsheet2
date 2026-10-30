import '@kerfjs/ui/layout.css';

import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { Pane } from '@kerfjs/ui/pane';
import { Text } from '@kerfjs/ui/text';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

import { AppMessageState } from './app-empty-state';
import { ConfidenceCalibration, type ConfidenceCalibrationState } from './confidence-calibration';
import { NotificationCenter } from './notification-center';
import { NotificationNavigation, notificationNavigationPanel, type NotificationView } from './notification-navigation';
import { ProjectSidebar, projectSidebarPanel, type ProjectSidebarProps } from './project-sidebar';
import { type CompactionPreviewState, RepositorySize, type RepositorySizeState } from './repository-size';
import { type SettingsCategory, SettingsNavigation, settingsNavigationPanel } from './settings-navigation';
import { inspectorToggle, type NavigationPanelParts, type SidebarPanelParts } from './sidebar-panel';
import { TerminalDashboard, type TerminalDashboardProps } from './terminal-dashboard';
import { TerminalDrawer, type TerminalDrawerProps } from './terminal-drawer';
import {
  terminalOperationsPanel,
  TerminalOperationsSidebar,
  type TerminalProjectSummary,
} from './terminal-operations-sidebar';
import { TerminalTicketRail, terminalTicketRailPanel, type TerminalTicketRailProps } from './terminal-ticket-rail';
import { TicketBoard } from './ticket-board';
import { TicketList } from './ticket-list';
import { WorkerColumns, type WorkerColumnsProps } from './worker-columns';

type NotificationCenterProps = Parameters<typeof NotificationCenter>[0];
type TicketBoardProps = Parameters<typeof TicketBoard>[0];
type TicketListProps = Parameters<typeof TicketList>[0];

export type SidebarSurfaceProps =
  | { kind: 'settings'; selected: SettingsCategory }
  | {
      kind: 'notifications';
      selected: NotificationView;
      counts: { pending: number; day: number; week: number };
      /** The app-wide notification pause (HS2-QYA9SC). */
      paused?: boolean;
    }
  | { kind: 'project'; sidebar: ProjectSidebarProps };

export function SidebarSurface(props: SidebarSurfaceProps) {
  if (props.kind === 'settings') return <SettingsNavigation selected={props.selected} collapseControl />;
  if (props.kind === 'notifications')
    return (
      <NotificationNavigation selected={props.selected} counts={props.counts} paused={props.paused} collapseControl />
    );
  return <ProjectSidebar {...props.sidebar} />;
}

/** The shell's left-rail panel for the same routing, composed by the Workbench (HS2-RWGQWN). */
export function sidebarSurfacePanel(props: SidebarSurfaceProps): SidebarPanelParts {
  if (props.kind === 'settings') return settingsNavigationPanel({ selected: props.selected });
  if (props.kind === 'notifications')
    return notificationNavigationPanel({ selected: props.selected, counts: props.counts, paused: props.paused });
  return projectSidebarPanel(props.sidebar);
}

export type WorkspaceSurfaceProps =
  | { kind: 'notifications'; notifications: NotificationCenterProps }
  | { kind: 'settings'; content: SafeHtml }
  | { kind: 'errors'; list: TicketListProps }
  | { kind: 'board'; board: TicketBoardProps }
  | { kind: 'workers'; workers: WorkerColumnsProps }
  | { kind: 'list'; list: TicketListProps; more?: SafeHtml };

export function WorkspaceSurface(props: WorkspaceSurfaceProps) {
  if (props.kind === 'notifications') return <NotificationCenter {...props.notifications} />;
  if (props.kind === 'settings') return props.content;
  if (props.kind === 'errors') return <TicketList {...props.list} />;
  if (props.kind === 'board') return <TicketBoard {...props.board} />;
  if (props.kind === 'workers') return <WorkerColumns {...props.workers} />;
  return (
    <>
      <TicketList {...props.list} />
      {props.more}
    </>
  );
}

export interface TerminalRailSurfaceProps {
  rail: TerminalTicketRailProps;
}
export function TerminalRailSurface({ rail }: TerminalRailSurfaceProps) {
  return <TerminalTicketRail {...rail} />;
}
/** The workspace grid's right-rail panel for the same routing, composed by the Workbench (HS2-QQW6CT). */
export function terminalRailSurfacePanel({ rail }: TerminalRailSurfaceProps): NavigationPanelParts {
  return terminalTicketRailPanel(rail);
}

export type GlobalWorkspaceSurfaceProps =
  | { kind: 'terminals'; dashboard: TerminalDashboardProps }
  | {
      kind: 'stats';
      projectName?: string;
      calibration?: ConfidenceCalibrationState;
      /** Git object and attachment sizes against GitHub's limits (HS2-QF44T7). */
      repositorySize?: RepositorySizeState;
      compaction?: CompactionPreviewState;
    };

export function GlobalWorkspaceSurface(props: GlobalWorkspaceSurfaceProps) {
  if (props.kind === 'terminals') return <TerminalDashboard {...props.dashboard} />;
  if (props.projectName)
    return (
      // Per-project statistics (HS2-Q1WCCY): Kerf's Pane owns the scroll and outer inset, and a
      // layout content stack separates the title from the report (HS2-3V3JY6).
      <Pane element="section" label={`${props.projectName} Project Statistics`} deepInset>
        <div class="kui-content">
          <Text variant="h1" size="xlarge" flush>
            {props.projectName} project statistics
          </Text>
          <ConfidenceCalibration state={props.calibration ?? { status: 'loading' }} />
          <RepositorySize state={props.repositorySize ?? { status: 'loading' }} compaction={props.compaction} />
        </div>
      </Pane>
    );
  return <AppMessageState title="Cross-Project Stats" message="This dashboard is still being designed." />;
}

function statsPlaceholderPanel(label: string, toggle: SidebarPanelParts['toggle'], message: string): SidebarPanelParts {
  return {
    label,
    toolbar: { label: `${label} toolbar`, dividerSides: '' },
    toggle,
    content: (
      <List fill hAlign="center" vAlign="middle" rootAttributes={{ 'data-stats-placeholder': 'true' }}>
        <EmptyState title={message} />
      </List>
    ),
    pane: {},
  };
}

/**
 * The cross-project stats view keeps both rails where the user left them (HS2-MGQDY2): switching to
 * Stats never hides or shows a sidebar on its own, so each rail shows an empty state until the
 * dashboard supplies its own rail content.
 */
export function statsSidebarPanel(): SidebarPanelParts {
  return statsPlaceholderPanel(
    'Stats sidebar',
    { action: 'toggle-project-sidebar', name: 'stats sidebar' },
    'Stats filters will appear here',
  );
}
export function statsInspectorPanel(): SidebarPanelParts {
  return statsPlaceholderPanel(
    'Stats details',
    inspectorToggle('stats details'),
    'Select a statistic to see its details',
  );
}

export interface ProjectTerminalDrawerSurfaceProps {
  drawer?: TerminalDrawerProps;
}
export function ProjectTerminalDrawerSurface({ drawer }: ProjectTerminalDrawerSurfaceProps) {
  return drawer ? <TerminalDrawer {...drawer} /> : <></>;
}

export interface TerminalOperationsSurfaceProps {
  projects: readonly TerminalProjectSummary[];
}
export function TerminalOperationsSurface({ projects }: TerminalOperationsSurfaceProps) {
  return <TerminalOperationsSidebar projects={projects} />;
}
export function terminalOperationsSurfacePanel({ projects }: TerminalOperationsSurfaceProps): SidebarPanelParts {
  return terminalOperationsPanel({ projects });
}
