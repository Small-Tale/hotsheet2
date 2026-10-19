import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { px } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { clampRegionSize, ResizableRegion } from '@kerfjs/ui/resizable-region';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { wireResizableRegions } from '@kerfjs/ui/wire-resizable-regions';
import { reorderTabs, wireTabBars } from '@kerfjs/ui/wire-tab-bars';
import { wireWorkbench } from '@kerfjs/ui/wire-workbench';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { PanelLeftClose, PanelLeftOpen } from 'lucide';

import { AppShell } from '../components/app-shell';
import { type ConnectionState, ConnectionStateBanner } from '../components/connection-state-banner';
import { projectSidebarPanel } from '../components/project-sidebar';
import { ProjectTab, type ProjectTabProps } from '../components/project-tab';
import { PROJECT_TAB_BAR_ID } from '../components/project-tab-bar';
import { ProjectTabBar, type ProjectTabBarMode } from '../components/project-tab-bar';
import { QuickTicketComposer } from '../components/quick-ticket-composer';
import { terminalOperationsPanel } from '../components/terminal-operations-sidebar';
import { TicketBoard } from '../components/ticket-board';
import { ticketInspectorPanel } from '../components/ticket-inspector';
import { ticketInspectorPlaceholderPanel } from '../components/ticket-inspector-placeholder';
import { TicketList } from '../components/ticket-list';
import { WorkspaceControls, WorkspaceIdentity } from '../components/workspace-header';
import { DemoEvent } from './components/demo-event';
import { tabContextMenu } from './components/demo-state';
import {
  editingNoteId,
  latestDemoConfidence,
  markdownMode,
  markdownSavedValue,
  markdownValue,
  noteDraft,
  readerNotes,
} from './content-components-demo';
import { DEMO_ACTIONS, DEMO_MARKERS } from './demo-actions';
import { type DemoModule, type FormControl, listen } from './demo-module';
import { DemoSettingsForm, DemoSettingsPreview } from './demo-settings-form';
import {
  collapsedCommandGroups,
  commandGroupExpanded,
  driveRunning,
  PROJECT_SIDEBAR_MAX_HEIGHT,
  PROJECT_SIDEBAR_MIN_HEIGHT,
  PROJECT_SIDEBAR_REGION_ID,
  projectSidebarHeight,
  runningCommandId,
  selectedViewId,
  sidebarCommands,
  sidebarEvent,
  sidebarViews,
} from './project-sidebar-demo';
import { syncSettingsControls } from './settings-controls';
import { collectionTickets } from './ticket-collections-demo';
import {
  composerCategory,
  composerDetails,
  composerExpanded,
  composerTitle,
  composerUpNext,
  filteredWorkspaceTickets,
  inspectorCodeReview,
  inspectorOpen,
  inspectorTab,
  workspaceColumns,
  workspaceMode,
  workspaceSearchHelpOpen,
  workspaceSearchModel,
  workspaceSearchOpen,
  workspaceSort,
} from './workspace-components-demo';

const initialTabs: ProjectTabProps[] = [
  { id: 'hotsheet', name: 'Hot Sheet 2', location: 'local', selected: true },
  { id: 'website', name: 'Small Tale Website', location: 'remote', upNextCount: 3, activeTicketCount: 1 },
  {
    id: 'api',
    name: 'Internal API',
    location: 'remote',
    attention: true,
    attentionLabel: 'An AI session stopped on an error',
    upNextCount: 128,
  },
  { id: 'archive', name: 'Legacy Archive', location: 'local', disconnected: true },
];

export const projectTabs = signal<ProjectTabProps[]>(initialTabs.map((tab) => ({ ...tab })));
export const shellConnectionState = signal<ConnectionState | undefined>('reconnecting');
export const shellMode = signal<ProjectTabBarMode>('project');
export const shellStatsProjectName = signal<string | undefined>(undefined);
export const shellSidebarSize = signal(272);
export const shellSidebarVisible = signal(true);
export const shellInspectorSize = signal(352);
export const shellTerminalDrawerVisible = signal(false);
export const resizeDemoWidth = signal(260);
export const resizeDemoHeight = signal(180);
export const resizeDemoCollapsed = signal(false);
export const shellEvent = signal('Explore the application shell.');
/** AppShell's public presentation props (HS2-DR549A), adjustable in the demo settings (HS2-8ZJMCE). */
export const appShellSettings = {
  presentation: signal<'framed' | 'viewport'>('framed'),
  /** Simulates a top-layer overlay hosted by the work area owning focus: the work-area ring turns off. */
  overlayOpen: signal(false),
  /** The phone page header (`mobileView`): a ticket-view Select beside the view's action (HS2-T35VN7). */
  mobileView: signal(false),
  mobileViewValue: signal('all'),
};

const mobileViewChoices = [
  { value: 'all', label: 'Queue' },
  { value: 'backlog', label: 'Backlog' },
  { value: 'archive', label: 'Archive' },
  { value: 'trash', label: 'Trash' },
];

export function resetAppShellDemo(root?: ParentNode): void {
  appShellSettings.presentation.value = 'framed';
  appShellSettings.overlayOpen.value = false;
  appShellSettings.mobileView.value = false;
  appShellSettings.mobileViewValue.value = 'all';
  if (root)
    syncSettingsControls(root, 'app-shell', {
      values: { presentation: appShellSettings.presentation.value },
      checked: {
        'overlay-open': appShellSettings.overlayOpen.value,
        'mobile-view-header': appShellSettings.mobileView.value,
      },
    });
}

export function AppShellSettings() {
  return (
    <DemoSettingsForm settings="app-shell">
      <Select
        name="presentation"
        label="Presentation"
        value={appShellSettings.presentation.value}
        choices={[
          { value: 'framed', label: 'Framed (UX Catalog Window)' },
          { value: 'viewport', label: 'Viewport (Application Root)' },
        ]}
      />
      <Checkbox
        name="overlay-open"
        checked={appShellSettings.overlayOpen.value}
        label="Top-layer overlay open (work-area focus ring off)"
      />
      <Checkbox
        name="mobile-view-header"
        checked={appShellSettings.mobileView.value}
        label="Phone view header (view Select and action)"
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

export const regionBounds: Record<string, { min: number; max: number }> = {
  'resize-demo-horizontal': { min: 250, max: 420 },
  'resize-demo-vertical': { min: 120, max: 260 },
  'app-left-rail': { min: 250, max: 360 },
  'app-right-rail': { min: 280, max: 520 },
  [PROJECT_SIDEBAR_REGION_ID]: { min: PROJECT_SIDEBAR_MIN_HEIGHT, max: PROJECT_SIDEBAR_MAX_HEIGHT },
};

export function regionSize(id: string): number {
  if (id === 'resize-demo-horizontal') return resizeDemoWidth.value;
  if (id === 'resize-demo-vertical') return resizeDemoHeight.value;
  if (id === 'app-left-rail') return shellSidebarSize.value;
  if (id === PROJECT_SIDEBAR_REGION_ID) return projectSidebarHeight.value;
  return shellInspectorSize.value;
}

export function setRegionSize(id: string, size: number): void {
  // `id` is an arbitrary string, so the lookup can miss even though the index type omits undefined.
  const bounds = regionBounds[id] as { min: number; max: number } | undefined;
  if (!bounds) return;
  const next = clampRegionSize(size, bounds.min, bounds.max);
  if (id === 'resize-demo-horizontal') resizeDemoWidth.value = next;
  else if (id === 'resize-demo-vertical') resizeDemoHeight.value = next;
  else if (id === 'app-left-rail') shellSidebarSize.value = next;
  else if (id === PROJECT_SIDEBAR_REGION_ID) projectSidebarHeight.value = next;
  else shellInspectorSize.value = next;
}

export function selectProjectTab(id: string): void {
  shellMode.value = 'project';
  shellStatsProjectName.value = undefined;
  projectTabs.value = projectTabs.value.map((tab) => ({ ...tab, selected: tab.id === id }));
  shellEvent.value = `${projectTabs.value.find((tab) => tab.id === id)?.name ?? 'Project'} selected.`;
}

export function closeProjectTab(id: string): void {
  const closing = projectTabs.value.find((tab) => tab.id === id);
  const remaining = projectTabs.value.filter((tab) => tab.id !== id);
  if (closing?.selected && remaining.length > 0) remaining[0] = { ...remaining[0], selected: true };
  projectTabs.value = remaining;
  shellEvent.value = `${closing?.name ?? 'Project'} closed.`;
}

export function closeOtherProjectTabs(id: string): void {
  projectTabs.value = projectTabs.value.filter((tab) => tab.id === id).map((tab) => ({ ...tab, selected: true }));
  shellMode.value = 'project';
  shellEvent.value = 'Other project tabs closed.';
}

export function closeProjectTabsToRight(id: string): void {
  const index = projectTabs.value.findIndex((tab) => tab.id === id);
  if (index < 0) return;
  projectTabs.value = projectTabs.value.slice(0, index + 1);
  shellEvent.value = 'Project tabs to the right closed.';
}

export function closeAllProjectTabs(): void {
  projectTabs.value = [];
  shellEvent.value = 'All project tabs closed.';
}

let addedProject = 1;
export function addDemoProject(): void {
  shellMode.value = 'project';
  const id = `new-${addedProject}`;
  projectTabs.value = [
    ...projectTabs.value.map((tab) => ({ ...tab, selected: false })),
    { id, name: `New Project ${addedProject++}`, location: 'local', selected: true },
  ];
  shellEvent.value = 'Project added.';
}

export function ProjectTabDemo() {
  return (
    <CatalogExample
      label="States"
      viewport={{ width: 'full', frame: 'solid', surface: 'lowered' }}
      note="Selected, remote, busy, still opening, active queue, active work, attention, disconnected, and fixed states."
    >
      <div role="tablist" aria-label="ProjectTab states">
        <Grid minColumnWidth={px(240)} gap="xs" controlInsets="trbl">
          <ProjectTab id="selected" name="Selected local" location="local" selected />
          <ProjectTab id="remote" name="Remote project" location="remote" />
          <ProjectTab id="busy" name="Busy project" location="local" busy />
          <ProjectTab id="pending" name="Still opening" location="local" pending />
          <ProjectTab id="active" name="Active queue" location="local" upNextCount={3} activeTicketCount={2} />
          <ProjectTab id="working" name="Active work" location="local" activeTicketCount={1} />
          <ProjectTab id="three" name="Three active" location="local" activeTicketCount={3} />
          <ProjectTab id="four" name="Four active" location="local" activeTicketCount={4} />
          <ProjectTab id="capped" name="Capped active" location="local" upNextCount={7} activeTicketCount={12} />
          <ProjectTab
            id="migration"
            name="Copying database"
            location="local"
            upNextCount={3}
            operation={{ label: 'Copying database, 50 percent', state: 'running', percent: 50 }}
          />
          <ProjectTab
            id="migration-unknown"
            name="Opening database"
            location="local"
            operation={{ label: 'Opening database', state: 'running' }}
          />
          <ProjectTab
            id="migration-retry"
            name="Import failed"
            location="local"
            operation={{ label: 'Import needs attention', state: 'failed' }}
          />
          <ProjectTab
            id="migration-complete"
            name="Imported"
            location="local"
            operation={{ label: 'Import complete, connect backup', state: 'succeeded' }}
          />
          <ProjectTab id="attention" name="Needs attention" location="remote" attention />
          <ProjectTab id="offline" name="Disconnected" location="remote" disconnected />
          <ProjectTab id="fixed" name="Not closable" location="local" closable={false} upNextCount={100} />
        </Grid>
      </div>
    </CatalogExample>
  );
}

export function ProjectTabBarDemo() {
  return (
    <>
      <CatalogExample label="Default" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <ProjectTabBar tabs={projectTabs.value} mode={shellMode.value} />
      </CatalogExample>
      <CatalogExample label="With Workspace Action" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <ProjectTabBar
          tabs={projectTabs.value}
          label="Open Projects with Workspace Action"
          mode={shellMode.value}
          workspaceAction={{ kind: 'new-ticket' }}
        />
      </CatalogExample>
      {/* The Trash view swaps the far-edge action for the outlined danger Empty Trash button (HS2-PNCDAE). */}
      <CatalogExample label="Trash View Action" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <ProjectTabBar
          tabs={projectTabs.value}
          label="Open Projects with the Trash View Action"
          mode={shellMode.value}
          workspaceAction={{ kind: 'empty-trash' }}
        />
      </CatalogExample>
      {/* The shell-column presentation (HS2-DR549A): the strip shares the column surface and leaves the
          separator to the content below it, as AppShell composes it in project mode. */}
      <CatalogExample label="Shell Column" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <ProjectTabBar
          tabs={projectTabs.value}
          label="Open Projects in the Shell Column"
          mode={shellMode.value}
          surface="default"
          divider={false}
        />
      </CatalogExample>
      <CatalogExample label="Phone Project Picker" viewport={{ width: 'full', frame: 'solid', overflow: 'hidden' }}>
        <ProjectTabBar tabs={projectTabs.value} mode={shellMode.value} mobile />
        <DemoEvent>{shellEvent.value}</DemoEvent>
      </CatalogExample>
    </>
  );
}

export function ResizableRegionDemo() {
  const tokens = { '--kui-resizable-region-background': 'var(--wa-color-neutral-fill-quiet)' };
  const panel = (title: string, size: number) => (
    <List fill hAlign="center" vAlign="middle" gap="xs">
      <Text variant="span">
        <strong>{title}</strong>
      </Text>
      <Text variant="span" size="compact" tone="quiet">{`${size}px`}</Text>
    </List>
  );
  const remainder = (copy: string) => (
    <Row flex hAlign="center" vAlign="middle">
      <Text variant="span" tone="quiet">
        {copy}
      </Text>
    </Row>
  );
  return (
    <>
      <CatalogExample
        label="Horizontal, Collapsible"
        note="Drag either handle or use arrow keys while it is focused."
        align="inline-control"
      >
        <Button
          emphasis="secondary"
          action={'toggle-resizable-collapse'}
          icon={
            <LucideIcon
              size="s"
              icon={resizeDemoCollapsed.value ? PanelLeftOpen : PanelLeftClose}
              name={resizeDemoCollapsed.value ? 'panel-left-open' : 'panel-left-close'}
            />
          }
        >
          {resizeDemoCollapsed.value ? 'Restore horizontal region' : 'Collapse horizontal region'}
        </Button>
      </CatalogExample>
      <CatalogExample
        viewport={{ width: 'full', height: 'short', layout: 'flex', frame: 'solid', overflow: 'hidden', tokens }}
      >
        <ResizableRegion
          id="resize-demo-horizontal"
          label="Example Sidebar"
          size={resizeDemoWidth.value}
          min={250}
          max={420}
          collapsed={resizeDemoCollapsed.value}
        >
          {panel('Horizontal region', resizeDemoWidth.value)}
        </ResizableRegion>
        {remainder('Remaining workspace')}
      </CatalogExample>
      <CatalogExample
        label="Vertical"
        viewport={{ width: 'full', height: 'tall', layout: 'flex-column', frame: 'solid', overflow: 'hidden', tokens }}
      >
        <ResizableRegion
          id="resize-demo-vertical"
          label="Example Drawer"
          size={resizeDemoHeight.value}
          min={120}
          max={260}
          axis="vertical"
        >
          {panel('Vertical region', resizeDemoHeight.value)}
        </ResizableRegion>
        {remainder('Remaining height')}
      </CatalogExample>
    </>
  );
}

export function ConnectionStateBannerDemo() {
  return (
    <>
      <CatalogExample label="Connecting">
        <ConnectionStateBanner state="connecting" detail="Establishing a secure session." />
      </CatalogExample>
      <CatalogExample label="Reconnecting">
        <ConnectionStateBanner state="reconnecting" detail="Showing the latest cached project state." />
      </CatalogExample>
      <CatalogExample label="Offline">
        <ConnectionStateBanner state="offline" detail="Changes will remain local until reconnection." />
      </CatalogExample>
      <CatalogExample label="Incompatible">
        <ConnectionStateBanner state="incompatible" detail="This client requires Hot Sheet Server 2.4 or later." />
      </CatalogExample>
      <CatalogExample label="Authentication">
        <ConnectionStateBanner state="authentication" detail="Your saved credential is no longer accepted." />
        <DemoEvent>{shellEvent.value}</DemoEvent>
      </CatalogExample>
    </>
  );
}

function shellSidebarPanel() {
  return projectSidebarPanel({
    completedToday: 6,
    inProgress: 3,
    completionTrend: [3, 0, 2, 5, 4, 7, 6],
    branch: 'feature/client-shell',
    unpushed: 2,
    uncommitted: 1,
    views: sidebarViews,
    selectedViewId: selectedViewId.value,
    commandGroupLabel: 'Project commands',
    commands: sidebarCommands.map((command) => ({ ...command, running: command.id === runningCommandId.value })),
    commandGroupExpanded: commandGroupExpanded.value,
    collapsedCommandGroups: collapsedCommandGroups.value,
    driveRunning: driveRunning.value,
    driveTool: 'codex',
    openCount: 17,
    upNextCount: 4,
    activeCount: 2,
  });
}

function shellTerminalOperationsPanel() {
  return terminalOperationsPanel({
    projects: [
      { id: 'hotsheet', name: 'Hot Sheet 2', completedToday: 6, inProgress: 3, trend: [3, 0, 2, 5, 4, 7, 6] },
      { id: 'website', name: 'Small Tale Website', completedToday: 2, inProgress: 1, trend: [0, 1, 0, 2, 1, 0, 2] },
    ],
  });
}

export function AppShellDemo() {
  const activeShellMode = shellMode.value;
  const ticket = collectionTickets.value.find((item) => item.selected) ?? collectionTickets.value[0];
  const banner = shellConnectionState.value ? (
    <ConnectionStateBanner state={shellConnectionState.value} detail="Showing the latest cached project state." />
  ) : undefined;
  const globalMode = activeShellMode !== 'project';
  const projectSettings = workspaceMode.value === 'settings';
  const projectWorkspace = (() => {
    switch (workspaceMode.value) {
      case 'settings':
        return (
          <DemoSettingsPreview description="Configure ticket providers, project defaults, commands, and checkout behavior." />
        );
      case 'board':
        return <TicketBoard columns={workspaceColumns()} label="Project board" />;
      case 'notifications':
      case 'list':
        return <TicketList tickets={filteredWorkspaceTickets()} label="All project tickets" />;
      default:
        return workspaceMode.value satisfies never;
    }
  })();
  const workspace = (() => {
    switch (activeShellMode) {
      case 'terminals':
        return (
          <section aria-label="Workspace Grid Workspace">
            <EmptyState
              title="Workspace Grid"
              detail="Monitor and arrange terminals, AI chats, and future workspace tools across connected projects."
            />
          </section>
        );
      case 'stats':
        return shellStatsProjectName.value ? (
          <section aria-label={`${shellStatsProjectName.value} Project Statistics`}>
            <EmptyState
              title={`${shellStatsProjectName.value} Project Statistics`}
              detail="Detailed ticket-flow and usage charts are coming in a future Hot Sheet update."
            />
          </section>
        ) : (
          <section aria-label="Cross-Project Stats Workspace">
            <EmptyState
              title="Cross-Project Stats"
              detail="Compare ticket flow and activity across connected projects."
            />
          </section>
        );
      case 'project':
        return projectWorkspace;
      default:
        return activeShellMode satisfies never;
    }
  })();
  const { projectName, viewName } = (() => {
    switch (activeShellMode) {
      case 'terminals':
        return { projectName: 'Workspace Grid', viewName: 'Workspace Grid' };
      case 'stats':
        return { projectName: 'Stats', viewName: 'Cross-project Stats' };
      case 'project':
        return { projectName: 'Hot Sheet 2', viewName: 'Queue' };
      default:
        return activeShellMode satisfies never;
    }
  })();
  const toolbarTitle = globalMode ? projectName : projectSettings ? 'Project Settings' : viewName;
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'full' }} rootAttributes={{ 'data-app-shell-demo': '' }}>
      <AppShell
        presentation={appShellSettings.presentation.value}
        workAreaFocusRing={!appShellSettings.overlayOpen.value}
        mode={shellMode.value}
        tabs={projectTabs.value}
        sidebar={shellMode.value === 'terminals' ? shellTerminalOperationsPanel() : shellSidebarPanel()}
        sidebarVisible={shellSidebarVisible.value}
        banner={banner}
        sidebarSize={shellSidebarSize.value}
        header={<WorkspaceIdentity projectName={toolbarTitle} id="app-shell-demo-page-title" headingLevel={1} />}
        headerActions={
          !globalMode ? (
            <WorkspaceControls
              mode={workspaceMode.value}
              searchOpen={workspaceSearchOpen.value}
              searchModel={workspaceSearchModel}
              sort={workspaceSort.value}
            />
          ) : undefined
        }
        projectTabAction={!globalMode && !projectSettings ? { kind: 'new-ticket' } : undefined}
        mobileView={
          appShellSettings.mobileView.value && !globalMode
            ? {
                value: appShellSettings.mobileViewValue.value,
                choices: mobileViewChoices,
                action:
                  appShellSettings.mobileViewValue.value === 'trash' ? { kind: 'empty-trash' } : { kind: 'new-ticket' },
              }
            : undefined
        }
        workspace={workspace}
        workspacePresentation={workspaceMode.value === 'board' && !globalMode ? 'edge-to-edge' : 'inset'}
        terminalDrawer={
          <section aria-label="Example Terminal Drawer">
            <EmptyState
              title="Terminal Drawer"
              action={
                <Button size="compact" action={'toggle-app-shell-demo-terminal-drawer'}>
                  Hide Terminal Drawer
                </Button>
              }
            />
          </section>
        }
        terminalDrawerVisible={shellTerminalDrawerVisible.value}
        inspectorSize={shellInspectorSize.value}
        inspector={
          projectSettings
            ? ticketInspectorPlaceholderPanel({ selectionCount: 0 })
            : ticketInspectorPanel({
                slug: ticket.slug,
                title: ticket.title,
                status: ticket.status,
                priority: ticket.priority,
                category: ticket.category,
                tags: ticket.tags,
                details: markdownValue.value,
                detailsMode: markdownMode.value,
                detailsDirty: markdownValue.value !== markdownSavedValue.value,
                notes: readerNotes.value,
                editingNoteId: editingNoteId.value,
                noteDraft: noteDraft.value,
                providerName: 'Hot Sheet Git',
                updatedLabel: ticket.updatedLabel,
                activeTab: inspectorTab.value,
                upNext: ticket.upNext,
                codeReview: inspectorCodeReview,
                latestConfidence: latestDemoConfidence(readerNotes.value),
              })
        }
        inspectorVisible={inspectorOpen.value}
      />
      <QuickTicketComposer
        expanded={composerExpanded.value}
        title={composerTitle.value}
        details={composerDetails.value}
        category={composerCategory.value}
        upNext={composerUpNext.value}
        providerName="Hot Sheet Git"
      />
      <DemoEvent>{shellEvent.value}</DemoEvent>
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireAppShellDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // The AppShell demo's rails resize through Kerf's Workbench wiring, as in the app (HS2-P289N2).
  wireWorkbench(root, {
    id: 'app',
    panels: { leftRail: { size: shellSidebarSize }, rightRail: { size: shellInspectorSize } },
  });
  // Tab strips reorder, roam, and edge-autoscroll through the same Kerf wiring the application installs on
  // its body (HS2-KB5PJQ); a reorder only rewrites the controlled demo state that owns that strip.
  demoListeners.add(
    wireTabBars(root, {
      activation: 'manual',
      onReorder: ({ barId, sourceId, targetId, position, source }) => {
        if (barId === PROJECT_TAB_BAR_ID)
          projectTabs.value = reorderTabs(projectTabs.value, (tab) => tab.id, sourceId, targetId, position);
        shellEvent.value = `Moved ${sourceId} ${position} ${targetId} by ${source}.`;
      },
    }),
  );
  // ResizableRegion demos resize through Kerf's pointer and separator-keyboard wiring; the commit only
  // mirrors the settled size into the demo signal that renders it (HS2-KB5PJQ).
  demoListeners.add(
    wireResizableRegions(root, {
      onCommit: ({ id, size }) => {
        setRegionSize(id, size);
      },
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectProjectTab.selector, (_event, target) => {
      selectProjectTab(target.closest<HTMLElement>('[data-tab-kind="project"]')!.dataset.projectId!);
    }),
  );
  demoListeners.add(
    delegate(root, 'change', 'kui-select[name="mobile-project"]', (_event, target) => {
      selectProjectTab((target as HTMLInputElement).value);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.closeProjectTab.selector, (event, target) => {
      event.stopPropagation();
      closeProjectTab(target.closest<HTMLElement>('[data-tab-kind="project"]')!.dataset.projectId!);
    }),
  );
  demoListeners.add(
    delegate(root, 'contextmenu', DEMO_MARKERS.tabKindProject.selector, (event, target) => {
      event.preventDefault();
      const pointer = event as MouseEvent;
      tabContextMenu.value = {
        x: pointer.clientX,
        y: pointer.clientY,
        projectId: (target as HTMLElement).dataset.projectId!,
      };
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.projectTabContextAction.selector, (_event, target) => {
      const element = target as HTMLElement;
      const id = element.dataset.projectId!;
      if (element.dataset.tabAction === 'close') closeProjectTab(id);
      if (element.dataset.tabAction === 'close-others') closeOtherProjectTabs(id);
      if (element.dataset.tabAction === 'close-right') closeProjectTabsToRight(id);
      if (element.dataset.tabAction === 'close-all') closeAllProjectTabs();
      tabContextMenu.value = undefined;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-action="add-project"], [data-action="choose-project"]', () => {
      addDemoProject();
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleProjectSidebar.selector, () => {
      shellSidebarVisible.value = !shellSidebarVisible.value;
      shellEvent.value = shellSidebarVisible.value ? 'Project sidebar shown.' : 'Project sidebar hidden.';
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'click',
      '[data-action="toggle-terminal-drawer"], [data-action="toggle-app-shell-demo-terminal-drawer"]',
      () => {
        shellTerminalDrawerVisible.value = !shellTerminalDrawerVisible.value;
        shellEvent.value = shellTerminalDrawerVisible.value ? 'Terminal drawer shown.' : 'Terminal drawer hidden.';
      },
    ),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.setShellMode.selector, (_event, target) => {
      shellStatsProjectName.value = undefined;
      shellMode.value = (target as HTMLElement).dataset.shellMode as typeof shellMode.value;
      workspaceSearchOpen.value = false;
      workspaceSearchHelpOpen.value = false;
      workspaceSearchModel.clear();
      shellEvent.value = shellMode.value === 'terminals' ? 'Workspace grid selected.' : 'Cross-project stats selected.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openProjectStats.selector, () => {
      const name = projectTabs.value.find((tab) => tab.selected)?.name ?? 'Project';
      shellStatsProjectName.value = name;
      shellMode.value = 'stats';
      workspaceSearchOpen.value = false;
      workspaceSearchHelpOpen.value = false;
      workspaceSearchModel.clear();
      sidebarEvent.value = `${name} project statistics requested.`;
      shellEvent.value = `${name} project statistics selected.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleResizableCollapse.selector, () => {
      resizeDemoCollapsed.value = !resizeDemoCollapsed.value;
      shellEvent.value = resizeDemoCollapsed.value ? 'Horizontal region collapsed.' : 'Horizontal region restored.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.retryConnection.selector, () => {
      shellEvent.value = 'Connection retry requested.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.showConnectionDetails.selector, () => {
      shellEvent.value = 'Connection details requested.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.authenticateConnection.selector, () => {
      shellEvent.value = 'Authentication requested.';
    }),
  );
  demoListeners.add(
    listen(window, 'keydown', (event) => {
      if (event.key === 'Escape') tabContextMenu.value = undefined;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="app-shell"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'presentation')
        appShellSettings.presentation.value = control.value as typeof appShellSettings.presentation.value;
      if (control.getAttribute('name') === 'overlay-open') appShellSettings.overlayOpen.value = control.checked;
      if (control.getAttribute('name') === 'mobile-view-header') appShellSettings.mobileView.value = control.checked;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-app-shell-demo] kui-select[name="mobile-view"]', (_event, target) => {
      const value = (target as FormControl).value;
      appShellSettings.mobileViewValue.value = value;
      shellEvent.value = `${value === 'all' ? 'Queue' : value.charAt(0).toUpperCase() + value.slice(1)} view selected.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-app-shell-demo] [data-action="open-empty-trash"]', () => {
      shellEvent.value = 'Empty Trash confirmation requested.';
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'project-tab': { catalogExamples: true, stackWidth: 'full', Demo: ProjectTabDemo },
    'project-tabs': { catalogExamples: true, stackWidth: 'full', Demo: ProjectTabBarDemo },
    'resizable-region': { catalogExamples: true, stackWidth: 'full', Demo: ResizableRegionDemo },
    'connection-state-banner': { catalogExamples: true, Demo: ConnectionStateBannerDemo },
    'app-shell': { catalogExamples: true, Demo: AppShellDemo, Settings: AppShellSettings, reset: resetAppShellDemo },
  },
  wire: wireAppShellDemo,
};
