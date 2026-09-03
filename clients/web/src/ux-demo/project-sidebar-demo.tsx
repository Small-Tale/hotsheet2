import { signal } from 'kerfjs';
import { GripHorizontal } from 'lucide';

import { CommandNavigation, type CommandNavigationItem } from '../components/command-navigation';
import { DriveControl } from '../components/drive-control';
import { LucideIcon } from '../components/lucide-icon';
import { ProjectSidebar } from '../components/project-sidebar';
import { ProjectSummary } from '../components/project-summary';
import { RepositorySummary } from '../components/repository-summary';
import { ViewNavigation, type ViewNavigationItem } from '../components/view-navigation';

export const selectedViewId = signal('all');
export const commandGroupExpanded = signal(true);
export const runningCommandId = signal<string | undefined>(undefined);
export const driveRunning = signal(false);
export const sidebarEvent = signal('Choose a view or run an action.');
export const projectSidebarHeight = signal(640);
export const PROJECT_SIDEBAR_MIN_HEIGHT = 288;
export const PROJECT_SIDEBAR_MAX_HEIGHT = 768;
export const clampProjectSidebarHeight = (height: number) => Math.min(PROJECT_SIDEBAR_MAX_HEIGHT, Math.max(PROJECT_SIDEBAR_MIN_HEIGHT, Math.round(height)));

export const sidebarViews: ViewNavigationItem[] = [
  { id: 'needs-review', label: 'Needs Review', count: 3, attention: true, icon: 'needs-review' },
  { id: 'all', label: 'Queue', count: 12, icon: 'all' },
  { id: 'backlog', label: 'Backlog', count: 5, icon: 'backlog' },
  { id: 'archive', label: 'Archive', count: 241, icon: 'archive' },
];
export const sidebarCommands: CommandNavigationItem[] = [
  { id: 'verify', label: 'Verify project', color: '#14b8a6', icon: 'test' },
  { id: 'build', label: 'Build clients', color: '#f97316', icon: 'build' },
  { id: 'publish', label: 'Publish preview', color: '#8b5cf6', icon: 'send' },
];

function DemoFrame({ children }: { children: unknown }) {
  return <section class="sidebar-component-demo"><div class="sidebar-component-demo__rail">{children}</div><p class="component-stage__event" aria-live="polite">{sidebarEvent.value}</p></section>;
}
const completionTrend = [3, 0, 2, 5, 4, 7, 6];
export function ProjectSummaryDemo() { return <DemoFrame><ProjectSummary completedToday={6} inProgress={3} trend={completionTrend} /></DemoFrame>; }
export function RepositorySummaryDemo() { return <DemoFrame><RepositorySummary branch="feature/client-sidebar" unpushed={6} uncommitted={2} /></DemoFrame>; }
export function ViewNavigationDemo() { return <DemoFrame><ViewNavigation items={sidebarViews} selectedId={selectedViewId.value} /></DemoFrame>; }
export function CommandNavigationDemo() { return <DemoFrame><CommandNavigation label="Project commands" expanded={commandGroupExpanded.value} commands={sidebarCommands.map(command => ({ ...command, running: command.id === runningCommandId.value }))} /></DemoFrame>; }
export function DriveControlDemo() { return <DemoFrame><DriveControl running={driveRunning.value} tool="Codex" /></DemoFrame>; }
export function ProjectSidebarDemo() {
  return <section class="project-sidebar-demo"><div class="project-sidebar-demo__resizer" style={`--project-sidebar-demo-height:${projectSidebarHeight.value}px`}><ProjectSidebar completedToday={6} inProgress={3} completionTrend={completionTrend} branch="feature/client-sidebar" unpushed={6} uncommitted={2} views={sidebarViews} selectedViewId={selectedViewId.value} commandGroupLabel="Project commands" commands={sidebarCommands.map(command => ({ ...command, running: command.id === runningCommandId.value }))} commandGroupExpanded={commandGroupExpanded.value} driveRunning={driveRunning.value} driveTool="Codex" openCount={17} upNextCount={4} activeCount={2} /><div class="project-sidebar-demo__resize-handle" data-action="resize-project-sidebar" role="separator" aria-label="Resize project sidebar" aria-orientation="horizontal" aria-valuemin={PROJECT_SIDEBAR_MIN_HEIGHT} aria-valuemax={PROJECT_SIDEBAR_MAX_HEIGHT} aria-valuenow={projectSidebarHeight.value} tabindex="0"><LucideIcon icon={GripHorizontal} name="grip-horizontal" /></div></div><p class="component-stage__event" aria-live="polite">{sidebarEvent.value}</p></section>;
}
