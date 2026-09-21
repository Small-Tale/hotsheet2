import type { SafeHtml } from 'kerfjs/jsx-runtime';

import { NotificationCenter } from './notification-center';
import { NotificationNavigation, type NotificationView } from './notification-navigation';
import { ProjectSidebar, type ProjectSidebarProps } from './project-sidebar';
import { type SettingsCategory, SettingsNavigation } from './settings-navigation';
import { TerminalDashboard, type TerminalDashboardProps } from './terminal-dashboard';
import { TerminalDrawer, type TerminalDrawerProps } from './terminal-drawer';
import { TerminalOperationsSidebar, type TerminalProjectSummary } from './terminal-operations-sidebar';
import { TerminalTicketRail, type TerminalTicketRailProps } from './terminal-ticket-rail';
import { TicketBoard } from './ticket-board';
import { TicketList } from './ticket-list';

type NotificationCenterProps = Parameters<typeof NotificationCenter>[0];
type TicketBoardProps = Parameters<typeof TicketBoard>[0];
type TicketListProps = Parameters<typeof TicketList>[0];

export type SidebarSurfaceProps =
  | { kind: 'settings'; selected: SettingsCategory }
  | { kind: 'notifications'; selected: NotificationView; counts: { pending: number; day: number; week: number } }
  | { kind: 'project'; sidebar: ProjectSidebarProps };

export function SidebarSurface(props: SidebarSurfaceProps) {
  if (props.kind === 'settings') return <SettingsNavigation selected={props.selected} collapseControl />;
  if (props.kind === 'notifications')
    return <NotificationNavigation selected={props.selected} counts={props.counts} collapseControl />;
  return <ProjectSidebar {...props.sidebar} />;
}

export type WorkspaceSurfaceProps =
  | { kind: 'notifications'; notifications: NotificationCenterProps }
  | { kind: 'settings'; content: SafeHtml }
  | { kind: 'errors'; list: TicketListProps }
  | { kind: 'board'; board: TicketBoardProps }
  | { kind: 'list'; list: TicketListProps; more?: SafeHtml };

export function WorkspaceSurface(props: WorkspaceSurfaceProps) {
  if (props.kind === 'notifications') return <NotificationCenter {...props.notifications} />;
  if (props.kind === 'settings') return props.content;
  if (props.kind === 'errors') return <TicketList {...props.list} />;
  if (props.kind === 'board') return <TicketBoard {...props.board} />;
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

export type GlobalWorkspaceSurfaceProps =
  { kind: 'terminals'; dashboard: TerminalDashboardProps } | { kind: 'stats'; projectName?: string };

export function GlobalWorkspaceSurface(props: GlobalWorkspaceSurfaceProps) {
  if (props.kind === 'terminals') return <TerminalDashboard {...props.dashboard} />;
  if (props.projectName)
    return (
      <section class="app-empty" aria-label={`${props.projectName} project statistics`}>
        <h1>{props.projectName} project statistics</h1>
        <p>Detailed ticket-flow and usage charts are coming in a future Hot Sheet update.</p>
      </section>
    );
  return (
    <section class="app-empty">
      <h1>Cross-project stats</h1>
      <p>This dashboard is still being designed.</p>
    </section>
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
