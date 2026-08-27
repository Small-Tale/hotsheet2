import { signal } from 'kerfjs';
import { PanelLeftClose, PanelLeftOpen } from 'lucide';
import { AppShell } from '../components/app-shell';
import { ConnectionStateBanner, type ConnectionState } from '../components/connection-state-banner';
import { ProjectSidebar } from '../components/project-sidebar';
import { ProjectTab, type ProjectTabProps } from '../components/project-tab';
import { ProjectTabBar } from '../components/project-tab-bar';
import { ResizableRegion, clampRegionSize } from '../components/resizable-region';
import { LucideIcon } from '../components/lucide-icon';
import { TicketBoard } from '../components/ticket-board';
import { TicketInspector } from '../components/ticket-inspector';
import { TicketList } from '../components/ticket-list';
import { WorkspaceHeader } from '../components/workspace-header';
import { collectionTickets } from './ticket-collections-demo';
import { commandGroupExpanded, driveRunning, runningCommandId, selectedViewId, sidebarCommands, sidebarViews } from './project-sidebar-demo';
import { filteredWorkspaceTickets, inspectorOpen, workspaceColumns, workspaceMode } from './workspace-components-demo';

const initialTabs: ProjectTabProps[] = [
  { id: 'hotsheet', name: 'Hot Sheet 2', location: 'local', selected: true },
  { id: 'website', name: 'Small Tale Website', location: 'remote', busy: true },
  { id: 'api', name: 'Internal API', location: 'remote', attention: true },
  { id: 'archive', name: 'Legacy Archive', location: 'local', disconnected: true },
];

export const projectTabs = signal<ProjectTabProps[]>(initialTabs.map(tab => ({ ...tab })));
export const shellConnectionState = signal<ConnectionState | undefined>('reconnecting');
export const shellSidebarSize = signal(248);
export const shellInspectorSize = signal(352);
export const resizeDemoWidth = signal(260);
export const resizeDemoHeight = signal(180);
export const resizeDemoCollapsed = signal(false);
export const shellEvent = signal('Explore the application shell.');

export const regionBounds: Record<string, { min: number; max: number }> = {
  'resize-demo-horizontal': { min: 180, max: 420 },
  'resize-demo-vertical': { min: 120, max: 260 },
  'app-sidebar': { min: 200, max: 360 },
  'app-inspector': { min: 280, max: 520 },
};

export function regionSize(id: string): number {
  if (id === 'resize-demo-horizontal') return resizeDemoWidth.value;
  if (id === 'resize-demo-vertical') return resizeDemoHeight.value;
  if (id === 'app-sidebar') return shellSidebarSize.value;
  return shellInspectorSize.value;
}

export function setRegionSize(id: string, size: number): void {
  const bounds = regionBounds[id];
  if (!bounds) return;
  const next = clampRegionSize(size, bounds.min, bounds.max);
  if (id === 'resize-demo-horizontal') resizeDemoWidth.value = next;
  else if (id === 'resize-demo-vertical') resizeDemoHeight.value = next;
  else if (id === 'app-sidebar') shellSidebarSize.value = next;
  else shellInspectorSize.value = next;
}

export function selectProjectTab(id: string): void {
  projectTabs.value = projectTabs.value.map(tab => ({ ...tab, selected: tab.id === id }));
  shellEvent.value = `${projectTabs.value.find(tab => tab.id === id)?.name ?? 'Project'} selected.`;
}

export function closeProjectTab(id: string): void {
  const closing = projectTabs.value.find(tab => tab.id === id);
  const remaining = projectTabs.value.filter(tab => tab.id !== id);
  if (closing?.selected && remaining.length > 0) remaining[0] = { ...remaining[0], selected: true };
  projectTabs.value = remaining;
  shellEvent.value = `${closing?.name ?? 'Project'} closed.`;
}

let addedProject = 1;
export function addDemoProject(): void {
  const id = `new-${addedProject}`;
  projectTabs.value = [...projectTabs.value.map(tab => ({ ...tab, selected: false })), { id, name: `New Project ${addedProject++}`, location: 'local', selected: true }];
  shellEvent.value = 'Project added.';
}

export function ProjectTabDemo() {
  return <section class="project-tab-demo" aria-label="ProjectTab demo">
    <div class="project-tab-demo__surface" role="tablist" aria-label="ProjectTab states">
      <ProjectTab id="selected" name="Selected local" location="local" selected />
      <ProjectTab id="remote" name="Remote project" location="remote" />
      <ProjectTab id="busy" name="Busy project" location="local" busy />
      <ProjectTab id="attention" name="Needs attention" location="remote" attention />
      <ProjectTab id="offline" name="Disconnected" location="remote" disconnected />
      <ProjectTab id="fixed" name="Not closable" location="local" closable={false} />
    </div>
    <p class="component-stage__event">Selected, remote, busy, attention, disconnected, and fixed states.</p>
  </section>;
}

export function ProjectTabBarDemo() {
  return <section class="project-tab-bar-demo" aria-label="ProjectTabBar demo"><ProjectTabBar tabs={projectTabs.value} /><p class="component-stage__event" aria-live="polite">{shellEvent.value}</p></section>;
}

export function ResizableRegionDemo() {
  return <section class="resizable-region-demo" aria-label="ResizableRegion demo">
    <div class="resizable-region-demo__controls"><wa-button appearance="outlined" data-action="toggle-resizable-collapse"><LucideIcon icon={resizeDemoCollapsed.value ? PanelLeftOpen : PanelLeftClose} name={resizeDemoCollapsed.value ? 'panel-left-open' : 'panel-left-close'} /><span>{resizeDemoCollapsed.value ? 'Restore horizontal region' : 'Collapse horizontal region'}</span></wa-button></div>
    <div class="resizable-region-demo__horizontal">
      <ResizableRegion id="resize-demo-horizontal" label="Example sidebar" size={resizeDemoWidth.value} min={180} max={420} collapsed={resizeDemoCollapsed.value}><div class="resizable-region-demo__panel"><strong>Horizontal region</strong><span>{resizeDemoWidth.value}px</span></div></ResizableRegion>
      <div class="resizable-region-demo__remainder">Remaining workspace</div>
    </div>
    <div class="resizable-region-demo__vertical">
      <ResizableRegion id="resize-demo-vertical" label="Example drawer" size={resizeDemoHeight.value} min={120} max={260} axis="vertical"><div class="resizable-region-demo__panel"><strong>Vertical region</strong><span>{resizeDemoHeight.value}px</span></div></ResizableRegion>
      <div class="resizable-region-demo__remainder">Remaining height</div>
    </div>
    <p class="component-stage__event" aria-live="polite">Drag either handle or use arrow keys while it is focused.</p>
  </section>;
}

export function ConnectionStateBannerDemo() {
  return <section class="connection-banner-demo" aria-label="ConnectionStateBanner demo">
    <ConnectionStateBanner state="connecting" detail="Establishing a secure session." />
    <ConnectionStateBanner state="reconnecting" detail="Showing the latest cached project state." />
    <ConnectionStateBanner state="offline" detail="Changes will remain local until reconnection." />
    <ConnectionStateBanner state="incompatible" detail="This client requires Hot Sheet Server 2.4 or later." />
    <ConnectionStateBanner state="authentication" detail="Your saved credential is no longer accepted." />
    <p class="component-stage__event" aria-live="polite">{shellEvent.value}</p>
  </section>;
}

function ShellSidebar() {
  return <ProjectSidebar completedToday={6} inProgress={3} completionTrend={[3, 0, 2, 5, 4, 7, 6]} branch="feature/client-shell" unpushed={2} uncommitted={1} views={sidebarViews} selectedViewId={selectedViewId.value} commandGroupLabel="Project commands" commands={sidebarCommands.map(command => ({ ...command, running: command.id === runningCommandId.value }))} commandGroupExpanded={commandGroupExpanded.value} driveRunning={driveRunning.value} driveTool="Codex" />;
}

export function AppShellDemo() {
  const ticket = collectionTickets.value.find(item => item.selected) ?? collectionTickets.value[0];
  const banner = shellConnectionState.value ? <ConnectionStateBanner state={shellConnectionState.value} detail="Showing the latest cached project state." /> : undefined;
  return <section class="app-shell-demo" aria-label="AppShell demo">
    <AppShell tabs={projectTabs.value} sidebar={<ShellSidebar />} banner={banner} sidebarSize={shellSidebarSize.value} header={<WorkspaceHeader projectName="Hot Sheet 2" viewName="All Tickets" mode={workspaceMode.value} />} workspace={workspaceMode.value === 'settings' ? <section class="workspace-settings-preview" aria-label="Project settings"><h2>Project settings</h2><p>Configure ticket providers, project defaults, commands, and checkout behavior.</p></section> : workspaceMode.value === 'board' ? <TicketBoard columns={workspaceColumns()} label="Project board" /> : <TicketList tickets={filteredWorkspaceTickets()} label="All project tickets" />} inspectorSize={shellInspectorSize.value} inspector={inspectorOpen.value ? <TicketInspector slug={ticket.slug} title={ticket.title} status={ticket.status} priority={ticket.priority} category={ticket.category} tags={ticket.tags} details="The inspector remains available beside the workspace on wide displays and yields on compact layouts." activeTab="info" upNext={ticket.upNext} /> : undefined} />
    <p class="component-stage__event" aria-live="polite">{shellEvent.value}</p>
  </section>;
}
