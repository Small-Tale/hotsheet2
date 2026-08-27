import { signal } from 'kerfjs';
import { CommandNavigation, type CommandNavigationItem } from '../components/command-navigation';
import { DriveControl } from '../components/drive-control';
import { ProjectSummary } from '../components/project-summary';
import { RepositorySummary } from '../components/repository-summary';
import { ViewNavigation, type ViewNavigationItem } from '../components/view-navigation';

export const selectedViewId = signal('all');
export const commandGroupExpanded = signal(true);
export const runningCommandId = signal<string | undefined>(undefined);
export const driveRunning = signal(false);
export const sidebarEvent = signal('Choose a view or run an action.');

export const sidebarViews: ViewNavigationItem[] = [
  { id: 'needs-review', label: 'Needs Review', count: 3, attention: true, icon: 'needs-review' },
  { id: 'all', label: 'All Tickets', count: 12, icon: 'all' },
  { id: 'backlog', label: 'Backlog', count: 5, icon: 'backlog' },
  { id: 'archive', label: 'Archive', count: 241, icon: 'archive' },
];
export const sidebarCommands: CommandNavigationItem[] = [
  { id: 'verify', label: 'Verify project', color: '#0ea5a8', icon: 'test' },
  { id: 'build', label: 'Build clients', color: '#ca8a04', icon: 'build' },
  { id: 'publish', label: 'Publish preview', color: '#7c3aed', icon: 'send' },
];

function DemoFrame({ children }: { children: unknown }) {
  return <section class="sidebar-component-demo"><div class="sidebar-component-demo__rail">{children}</div><p class="component-stage__event" aria-live="polite">{sidebarEvent.value}</p></section>;
}
export function ProjectSummaryDemo() { return <DemoFrame><ProjectSummary completed={42} inProgress={6} coverage={84} trend={[3, 1, 2, 5, 4, 7, 6]} /></DemoFrame>; }
export function RepositorySummaryDemo() { return <DemoFrame><RepositorySummary branch="feature/client-sidebar" unpushed={6} uncommitted={2} /></DemoFrame>; }
export function ViewNavigationDemo() { return <DemoFrame><ViewNavigation items={sidebarViews} selectedId={selectedViewId.value} /></DemoFrame>; }
export function CommandNavigationDemo() { return <DemoFrame><CommandNavigation label="Project commands" expanded={commandGroupExpanded.value} commands={sidebarCommands.map(command => ({ ...command, running: command.id === runningCommandId.value }))} /></DemoFrame>; }
export function DriveControlDemo() { return <DemoFrame><DriveControl running={driveRunning.value} tool="Codex" /></DemoFrame>; }
