import { delegate, delegateCapture, mount, signal } from 'kerfjs';
import '@awesome.me/webawesome/dist/styles/webawesome.css';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import './style.css';
import { demosUsing, demoCatalog, findDemo, type DemoCategory, type DemoDefinition } from './catalog';
import { resetStatusBadgeDemo, StatusBadgeDemo, StatusBadgeSettings, statusBadgeSettings } from './status-badge-demo';
import { resetTagChipDemo, TagChipDemo, TagChipSettings, tagChipSettings } from './tag-chip-demo';
import { resetTicketRowDemo, TicketRowDemo, TicketRowSettings, ticketRowSettings } from './ticket-row-demo';
import { TicketRowContextMenu } from '../components/ticket-row-context-menu';
import { LucideIcon } from '../components/lucide-icon';
import { Network } from 'lucide';
import { collectionTickets, recordCollectionEvent, selectCollectionTicket, TicketBoardColumnDemo, TicketBoardDemo, TicketListDemo, toggleCollectionTicketUpNext } from './ticket-collections-demo';
import { composerCategory, composerExpanded, composerTitle, createDemoTicket, focusComposerTitle, focusWorkspaceSearch, inspectorCategory, inspectorOpen, inspectorPriority, inspectorStatus, inspectorTab, PageHeaderDemo, QuickTicketComposerDemo, TicketInspectorDemo, workspaceMode, workspaceSearchOpen, workspaceSearchQuery, workspaceSort, WorkspaceHeaderDemo } from './workspace-components-demo';
import { ToolbarControlGroupDemo } from './toolbar-control-group-demo';
import { MenuItemDemo } from './menu-item-demo';
import { MenuHeaderDemo } from './menu-header-demo';
import { TicketAttachmentsDemo, TicketCategorySelectDemo, TicketInfoPanelDemo, TicketPrioritySelectDemo, TicketStatusMenuDemo, TicketTimelineDemo } from './ticket-metadata-demo';
import { clampProjectSidebarHeight, commandGroupExpanded, CommandNavigationDemo, driveRunning, DriveControlDemo, projectSidebarHeight, ProjectSidebarDemo, ProjectSummaryDemo, RepositorySummaryDemo, runningCommandId, selectedViewId, sidebarCommands, sidebarEvent, sidebarViews, ViewNavigationDemo } from './project-sidebar-demo';
import { addDemoProject, AppShellDemo, closeAllProjectTabs, closeOtherProjectTabs, closeProjectTab, closeProjectTabsToRight, ConnectionStateBannerDemo, projectTabs, ProjectTabBarDemo, ProjectTabDemo, regionSize, resizeDemoCollapsed, ResizableRegionDemo, selectProjectTab, setRegionSize, shellEvent, shellMode, shellSidebarVisible } from './app-shell-demo';
import { ProjectTabContextMenu } from '../components/project-tab-context-menu';
import { observeProjectTabBarOverflow } from '../components/project-tab-bar';
import { resizeRegionFromPointer, type ResizableRegionEdge } from '../components/resizable-region';

type FormControl = HTMLElement & { checked: boolean; value: string };
const defaultDemo = 'tag-chip';
const fromUrl = () => new URL(location.href).searchParams.get('component') ?? defaultDemo;
const selectedId = signal(findDemo(fromUrl())?.id ?? defaultDemo);
const settingsOpen = signal(false);
const contextMenu = signal<{ x: number; y: number; ticketSlug?: string } | undefined>(undefined);
const tabContextMenu = signal<{ x: number; y: number; projectId: string } | undefined>(undefined);
let sidebarResizeDrag: { startY: number; startHeight: number } | undefined;
let regionResizeDrag: { id: string; axis: 'horizontal' | 'vertical'; edge: ResizableRegionEdge; startPoint: number; startSize: number } | undefined;
const usesCollectionState = () => ['ticket-list', 'ticket-board', 'workspace-header', 'quick-ticket-composer', 'app-shell'].includes(selectedId.value);

function demoLink(item: DemoDefinition) {
  const selected = item.id === selectedId.value;
  return <li><a class={item.implemented ? 'catalog-link' : 'catalog-link catalog-link--planned'} href={`/ux-demo?component=${encodeURIComponent(item.id)}`} data-demo-id={item.id} data-implemented={String(Boolean(item.implemented))} aria-current={selected ? 'page' : undefined}><span>{item.name}</span><small>{item.implemented ? 'Demo' : item.phase.replace('-', ' ')}</small></a></li>;
}

function demoNavigation(category: DemoCategory) {
  return <section class="catalog-group"><h2>{category.name}</h2>{category.demos && <ul>{category.demos.map(demoLink)}</ul>}{category.children?.map(child => <section class="catalog-subgroup"><h3>{child.name}</h3><ul>{child.demos?.map(demoLink)}</ul></section>)}</section>;
}

function demoContent(item: DemoDefinition) {
  if (item.id === 'status-badge') return <StatusBadgeDemo />;
  if (item.id === 'tag-chip') return <TagChipDemo />;
  if (item.id === 'ticket-row') return <TicketRowDemo />;
  if (item.id === 'ticket-list') return <TicketListDemo />;
  if (item.id === 'ticket-board') return <TicketBoardDemo />;
  if (item.id === 'ticket-board-column') return <TicketBoardColumnDemo />;
  if (item.id === 'workspace-header') return <WorkspaceHeaderDemo />;
  if (item.id === 'page-header') return <PageHeaderDemo />;
  if (item.id === 'quick-ticket-composer') return <QuickTicketComposerDemo />;
  if (item.id === 'ticket-inspector') return <TicketInspectorDemo />;
  if (item.id === 'toolbar-control-group') return <ToolbarControlGroupDemo />;
  if (item.id === 'menu-item') return <MenuItemDemo />;
  if (item.id === 'menu-header') return <MenuHeaderDemo />;
  if (item.id === 'ticket-category-select') return <TicketCategorySelectDemo />;
  if (item.id === 'ticket-priority-select') return <TicketPrioritySelectDemo />;
  if (item.id === 'ticket-status-menu') return <TicketStatusMenuDemo />;
  if (item.id === 'ticket-info-panel') return <TicketInfoPanelDemo />;
  if (item.id === 'ticket-timeline') return <TicketTimelineDemo />;
  if (item.id === 'ticket-attachments') return <TicketAttachmentsDemo />;
  if (item.id === 'project-summary') return <ProjectSummaryDemo />;
  if (item.id === 'project-sidebar') return <ProjectSidebarDemo />;
  if (item.id === 'repository-summary') return <RepositorySummaryDemo />;
  if (item.id === 'view-navigation') return <ViewNavigationDemo />;
  if (item.id === 'command-navigation') return <CommandNavigationDemo />;
  if (item.id === 'drive-control') return <DriveControlDemo />;
  if (item.id === 'project-tab') return <ProjectTabDemo />;
  if (item.id === 'project-tabs') return <ProjectTabBarDemo />;
  if (item.id === 'resizable-region') return <ResizableRegionDemo />;
  if (item.id === 'connection-state-banner') return <ConnectionStateBannerDemo />;
  if (item.id === 'app-shell') return <AppShellDemo />;
  return <section class="planned-demo" aria-label={`${item.name} planned demo`}><span>Planned component</span><p>The catalog entry and navigation are ready. Its real component demo will be added in a later slice.</p></section>;
}

function DemoRelationships({ item }: { item: DemoDefinition }) {
  const uses = (item.uses ?? []).map(findDemo).filter((demo): demo is DemoDefinition => Boolean(demo));
  const usedBy = demosUsing(item.id);
  if (uses.length === 0 && usedBy.length === 0) return null;
  const items = (demos: DemoDefinition[]) => demos.map(demo =>
    <wa-dropdown-item data-demo-id={demo.id} value={demo.id}>
      {demo.name}<small slot="details">{demo.implemented ? 'Demo' : demo.phase.replace('-', ' ')}</small>
    </wa-dropdown-item>
  );
  return <wa-dropdown class="demo-relationships" placement="top-start">
    <wa-button slot="trigger" appearance="outlined" with-caret>
      <span slot="start" class="demo-relationships__icon"><LucideIcon icon={Network} name="network" /></span>
      Related components
    </wa-button>
    {uses.length > 0 && <h2>Uses</h2>}
    {items(uses)}
    {uses.length > 0 && usedBy.length > 0 && <wa-divider></wa-divider>}
    {usedBy.length > 0 && <h2>Used by</h2>}
    {items(usedBy)}
  </wa-dropdown>;
}

function DemoApp() {
  const selected = findDemo(selectedId.value) ?? findDemo(defaultDemo)!;
  const hasSettings = selected.id === 'tag-chip' || selected.id === 'status-badge' || selected.id === 'ticket-row';
  return (
    <main class={settingsOpen.value ? 'demo-shell demo-shell--settings-open' : 'demo-shell'}>
      <aside class="demo-master" aria-label="Component catalog">
        <header><p class="eyebrow">Hot Sheet</p><h1>UX components</h1><p>Production components with deterministic development support.</p></header>
        <nav>{demoCatalog.map(demoNavigation)}</nav>
      </aside>
      <article class="demo-detail">
        <header class="demo-detail__header"><div><p class="eyebrow">{selected.phase.replace('-', ' ')}</p><h1>{selected.name}</h1><p>{selected.description}</p></div></header>
        {demoContent(selected)}
        <footer class="demo-detail__footer"><DemoRelationships item={selected} /></footer>
      </article>
      {settingsOpen.value && <aside class="settings-inspector" aria-label={`${selected.name} settings`}>
        <header><div><p class="eyebrow">Demo settings</p><h2>{selected.name}</h2></div></header>
        {selected.id === 'tag-chip' ? <TagChipSettings /> : selected.id === 'status-badge' ? <StatusBadgeSettings /> : selected.id === 'ticket-row' ? <TicketRowSettings /> : <p>This demo has no adjustable settings.</p>}
      </aside>}
      {hasSettings && <wa-button class="settings-toggle" data-action="toggle-settings" aria-expanded={settingsOpen.value ? 'true' : 'false'}>{settingsOpen.value ? 'Close settings' : 'Settings'}</wa-button>}
      {contextMenu.value && <TicketRowContextMenu x={contextMenu.value.x} y={contextMenu.value.y} />}
      {tabContextMenu.value && <ProjectTabContextMenu {...tabContextMenu.value} />}
    </main>
  );
}

const root = document.querySelector<HTMLElement>('#ux-demo')!;
mount(root, DemoApp);
observeProjectTabBarOverflow(root);
if (import.meta.env.DEV && new URL(location.href).searchParams.get('dev-review') === '1') {
  void import('../dev-review').then(({ installDevReview }) => installDevReview({
    submit: async submission => {
      const response = await fetch('/__hotsheet/dev-review/tickets', { method: 'POST', headers: { 'content-type': 'application/json', 'x-hotsheet-dev-review': '1' }, body: JSON.stringify(submission) });
      const result = await response.json() as { slug?: string; error?: string };
      if (!response.ok || !result.slug) throw new Error(result.error ?? 'Ticket creation failed.');
      return { slug: result.slug };
    },
  }));
}

function selectDemo(id: string, push = true): void {
  if (!findDemo(id)) return;
  selectedId.value = id;
  settingsOpen.value = false;
  contextMenu.value = undefined;
  if (push) history.pushState(null, '', `/ux-demo?component=${encodeURIComponent(id)}`);
}

delegate(root, 'click', '[data-demo-id]', (event, target) => { event.preventDefault(); selectDemo((target as HTMLElement).dataset.demoId!); });
delegate(root, 'click', '[data-action="toggle-settings"]', () => { settingsOpen.value = !settingsOpen.value; });
delegate(root, 'click', '[data-action="open-repository-status"]', () => { sidebarEvent.value = 'Repository status requested.'; });
delegate(root, 'click', '[data-action="add-view"]', () => { sidebarEvent.value = 'New view editor requested.'; });
delegate(root, 'click', '[data-action="select-view"]', (_event, target) => { const id = (target as HTMLElement).dataset.itemId!; selectedViewId.value = id; sidebarEvent.value = `${sidebarViews.find(view => view.id === id)?.label ?? 'View'} selected.`; });
delegate(root, 'click', '[data-action="toggle-command-group"]', () => { commandGroupExpanded.value = !commandGroupExpanded.value; sidebarEvent.value = commandGroupExpanded.value ? 'Command group expanded.' : 'Command group collapsed.'; });
delegate(root, 'click', '[data-action="run-command"]', (_event, target) => { const id = (target as HTMLElement).dataset.itemId!; runningCommandId.value = runningCommandId.value === id ? undefined : id; sidebarEvent.value = runningCommandId.value ? `${sidebarCommands.find(command => command.id === id)?.label ?? 'Command'} started.` : 'Command stopped.'; });
delegate(root, 'click', '[data-action="toggle-drive"]', () => { driveRunning.value = !driveRunning.value; sidebarEvent.value = driveRunning.value ? 'Codex drive started.' : 'Codex drive stopped.'; });
delegate(root, 'click', '[data-action="select-project-tab"]', (_event, target) => { selectProjectTab((target as HTMLElement).dataset.projectId!); });
delegate(root, 'click', '[data-action="close-project-tab"]', (event, target) => { event.stopPropagation(); closeProjectTab((target as HTMLElement).dataset.projectId!); });
delegate(root, 'contextmenu', '[data-component="project-tab"]', (event, target) => { event.preventDefault(); const pointer = event as MouseEvent; tabContextMenu.value = { x: pointer.clientX, y: pointer.clientY, projectId: (target as HTMLElement).dataset.projectId! }; });
delegate(root, 'click', '[data-action="project-tab-context-action"]', (_event, target) => { const element = target as HTMLElement; const id = element.dataset.projectId!; if (element.dataset.tabAction === 'close') closeProjectTab(id); if (element.dataset.tabAction === 'close-others') closeOtherProjectTabs(id); if (element.dataset.tabAction === 'close-right') closeProjectTabsToRight(id); if (element.dataset.tabAction === 'close-all') closeAllProjectTabs(); tabContextMenu.value = undefined; });
delegate(root, 'click', '[data-action="add-project"]', () => { addDemoProject(); });
delegate(root, 'click', '[data-action="toggle-project-sidebar"]', () => { shellSidebarVisible.value = !shellSidebarVisible.value; shellEvent.value = shellSidebarVisible.value ? 'Project sidebar shown.' : 'Project sidebar hidden.'; });
delegate(root, 'click', '[data-action="set-shell-mode"]', (_event, target) => {
  shellMode.value = (target as HTMLElement).dataset.shellMode as typeof shellMode.value;
  workspaceSearchOpen.value = false;
  workspaceSearchQuery.value = '';
  shellEvent.value = shellMode.value === 'terminals' ? 'Terminal dashboard selected.' : 'Cross-project stats selected.';
});
delegate(root, 'click', '[data-action="toggle-resizable-collapse"]', () => { resizeDemoCollapsed.value = !resizeDemoCollapsed.value; shellEvent.value = resizeDemoCollapsed.value ? 'Horizontal region collapsed.' : 'Horizontal region restored.'; });
delegate(root, 'click', '[data-action="retry-connection"]', () => { shellEvent.value = 'Connection retry requested.'; });
delegate(root, 'click', '[data-action="show-connection-details"]', () => { shellEvent.value = 'Connection details requested.'; });
delegate(root, 'click', '[data-action="authenticate-connection"]', () => { shellEvent.value = 'Authentication requested.'; });
delegate(root, 'pointerdown', '[data-action="resize-region"]', (event, target) => {
  event.preventDefault();
  const handle = target as HTMLElement;
  const region = handle.closest<HTMLElement>('[data-component="resizable-region"]')!;
  const axis = region.dataset.axis as 'horizontal' | 'vertical';
  const id = handle.dataset.regionId!;
  regionResizeDrag = { id, axis, edge: region.dataset.edge as ResizableRegionEdge, startPoint: axis === 'horizontal' ? (event as PointerEvent).clientX : (event as PointerEvent).clientY, startSize: regionSize(id) };
  document.body.dataset.resizingRegion = axis;
});
delegate(root, 'keydown', '[data-action="resize-region"]', (event, target) => {
  const handle = target as HTMLElement;
  const region = handle.closest<HTMLElement>('[data-component="resizable-region"]')!;
  const axis = region.dataset.axis as 'horizontal' | 'vertical';
  const key = (event as KeyboardEvent).key;
  if ((axis === 'horizontal' && key !== 'ArrowLeft' && key !== 'ArrowRight') || (axis === 'vertical' && key !== 'ArrowUp' && key !== 'ArrowDown')) return;
  event.preventDefault();
  const direction = key === 'ArrowRight' || key === 'ArrowDown' ? 1 : -1;
  const edge = region.dataset.edge as ResizableRegionEdge;
  setRegionSize(handle.dataset.regionId!, resizeRegionFromPointer(regionSize(handle.dataset.regionId!), direction * 16, edge));
  shellEvent.value = `${region.getAttribute('aria-label')} resized.`;
});
delegate(root, 'keydown', '[data-action="select-project-tab"]', (event, target) => {
  const key = (event as KeyboardEvent).key;
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(key)) return;
  event.preventDefault();
  const tabs = projectTabs.value;
  const current = tabs.findIndex(tab => tab.id === (target as HTMLElement).dataset.projectId);
  const next = key === 'Home' ? 0 : key === 'End' ? tabs.length - 1 : (current + (key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
  const id = tabs[next]?.id;
  if (!id) return;
  selectProjectTab(id);
  queueMicrotask(() => root.querySelector<HTMLElement>(`[role="tab"][data-project-id="${id}"]`)?.focus());
});
delegate(root, 'pointerdown', '[data-action="resize-project-sidebar"]', (event) => {
  event.preventDefault();
  sidebarResizeDrag = { startY: (event as PointerEvent).clientY, startHeight: projectSidebarHeight.value };
  document.body.dataset.resizingProjectSidebar = 'true';
});
delegate(root, 'keydown', '[data-action="resize-project-sidebar"]', (event) => {
  if ((event as KeyboardEvent).key !== 'ArrowUp' && (event as KeyboardEvent).key !== 'ArrowDown') return;
  event.preventDefault();
  projectSidebarHeight.value = clampProjectSidebarHeight(projectSidebarHeight.value + ((event as KeyboardEvent).key === 'ArrowDown' ? 24 : -24));
  sidebarEvent.value = `Sidebar height ${projectSidebarHeight.value} pixels.`;
});
window.addEventListener('pointermove', event => {
  if (!sidebarResizeDrag) return;
  projectSidebarHeight.value = clampProjectSidebarHeight(sidebarResizeDrag.startHeight + event.clientY - sidebarResizeDrag.startY);
});
window.addEventListener('pointermove', event => {
  if (!regionResizeDrag) return;
  const point = regionResizeDrag.axis === 'horizontal' ? event.clientX : event.clientY;
  setRegionSize(regionResizeDrag.id, resizeRegionFromPointer(regionResizeDrag.startSize, point - regionResizeDrag.startPoint, regionResizeDrag.edge));
});
window.addEventListener('pointerup', () => {
  if (!sidebarResizeDrag) return;
  sidebarResizeDrag = undefined;
  delete document.body.dataset.resizingProjectSidebar;
  sidebarEvent.value = `Sidebar height ${projectSidebarHeight.value} pixels.`;
});
window.addEventListener('pointerup', () => {
  if (!regionResizeDrag) return;
  shellEvent.value = `Region resized to ${regionSize(regionResizeDrag.id)} pixels.`;
  regionResizeDrag = undefined;
  delete document.body.dataset.resizingRegion;
});
window.addEventListener('pointerdown', event => { if (tabContextMenu.value && !(event.target as HTMLElement).closest('.project-tab-context-menu,[data-component="project-tab"]')) tabContextMenu.value = undefined; });
window.addEventListener('keydown', event => { if (event.key === 'Escape') tabContextMenu.value = undefined; });
delegate(root, 'click', '[data-action="reset-settings"]', () => {
  if (selectedId.value === 'tag-chip') resetTagChipDemo(root);
  if (selectedId.value === 'status-badge') resetStatusBadgeDemo(root);
  if (selectedId.value === 'ticket-row') resetTicketRowDemo(root);
});
delegate(root, 'input', '[data-settings="tag-chip"] [name="label"]', (_event, target) => { tagChipSettings.label.value = (target as FormControl).value; });
delegate(root, 'change', '[data-settings="tag-chip"] [name]', (_event, target) => {
  const control = target as FormControl;
  switch (control.getAttribute('name')) {
    case 'variant': tagChipSettings.variant.value = control.value as typeof tagChipSettings.variant.value; break;
    case 'appearance': tagChipSettings.appearance.value = control.value as typeof tagChipSettings.appearance.value; break;
    case 'size': tagChipSettings.size.value = control.value as typeof tagChipSettings.size.value; break;
    case 'removable': tagChipSettings.removable.value = control.checked; break;
    case 'pill': tagChipSettings.pill.value = control.checked; break;
    case 'disabled': tagChipSettings.disabled.value = control.checked; break;
  }
});
delegate(root, 'wa-remove', '[data-component="tag-chip"]', (_event, target) => {
  if ((target as HTMLElement).dataset.disabled !== 'true') tagChipSettings.event.value = `Remove requested for ${(target as HTMLElement).dataset.tagId}`;
});
delegate(root, 'change', '[data-settings="status-badge"] [name]', (_event, target) => {
  const control = target as FormControl;
  if (control.getAttribute('name') === 'status') statusBadgeSettings.status.value = control.value as typeof statusBadgeSettings.status.value;
  if (control.getAttribute('name') === 'appearance') statusBadgeSettings.appearance.value = control.value as typeof statusBadgeSettings.appearance.value;
  if (control.getAttribute('name') === 'show-icon') statusBadgeSettings.showIcon.value = control.checked;
  if (control.getAttribute('name') === 'compact') statusBadgeSettings.compact.value = control.checked;
});
delegate(root, 'click', '[data-action="set-view-mode"]', (_event, target) => {
  workspaceMode.value = (target as HTMLElement).dataset.viewMode as typeof workspaceMode.value;
  if (workspaceMode.value === 'settings') {
    workspaceSearchOpen.value = false;
    workspaceSearchQuery.value = '';
  }
  recordCollectionEvent(`${workspaceMode.value === 'list' ? 'List' : workspaceMode.value === 'board' ? 'Columns' : 'Settings'} view selected`);
});
delegate(root, 'click', '[data-action="open-workspace-search"]', () => {
  workspaceSearchOpen.value = true;
  queueMicrotask(() => { focusWorkspaceSearch(root); });
});
delegate(root, 'input', '[name="workspace-search"]', (_event, target) => { workspaceSearchQuery.value = (target as FormControl).value; });
delegate(root, 'focusout', '[name="workspace-search"]', () => {
  queueMicrotask(() => {
    if (workspaceSearchQuery.value === '') workspaceSearchOpen.value = false;
  });
});
delegate(root, 'click', '[data-sort]', (_event, target) => {
  workspaceSort.value = (target as HTMLElement).dataset.sort as typeof workspaceSort.value;
  recordCollectionEvent(`Sorted by ${workspaceSort.value}`);
});
delegate(root, 'click', '[data-action="toggle-favorite"]', () => { recordCollectionEvent('View favorite toggled'); });
delegate(root, 'click', '[data-action="more-workspace-actions"]', () => { recordCollectionEvent('Workspace actions requested'); });
delegate(root, 'click', '[data-action="expand-ticket-composer"]', () => {
  composerExpanded.value = true;
  queueMicrotask(() => { focusComposerTitle(root); });
});
delegate(root, 'click', '[data-action="cancel-ticket-composer"]', () => { composerExpanded.value = false; composerTitle.value = ''; recordCollectionEvent('Ticket creation cancelled'); });
delegate(root, 'input', '[name="new-ticket-title"]', (_event, target) => { composerTitle.value = (target as FormControl).value; });
delegate(root, 'change', '[name="new-ticket-category"]', (_event, target) => { composerCategory.value = (target as FormControl).value; });
delegate(root, 'submit', '[data-action="create-ticket-form"]', (event) => { event.preventDefault(); if (!createDemoTicket()) recordCollectionEvent('Enter a ticket title'); });
delegate(root, 'click', '[data-action="set-inspector-tab"]', (_event, target) => { inspectorTab.value = (target as HTMLElement).dataset.inspectorTab as typeof inspectorTab.value; });
delegate(root, 'click', '[data-action="close-ticket-inspector"]', () => { inspectorOpen.value = false; recordCollectionEvent('Inspector closed'); });
delegate(root, 'click', '[data-action="open-ticket-inspector"]', () => { inspectorOpen.value = true; recordCollectionEvent('Inspector opened'); });
delegate(root, 'change', '[name="inspector-category"]', (_event, target) => { inspectorCategory.value = (target as FormControl).value; });
delegate(root, 'change', '[name="inspector-priority"]', (_event, target) => { inspectorPriority.value = (target as FormControl).value as typeof inspectorPriority.value; });
delegate(root, 'click', '[data-inspector-status]', (_event, target) => { inspectorStatus.value = (target as HTMLElement).dataset.inspectorStatus as typeof inspectorStatus.value; });
delegate(root, 'click', '[data-action="toggle-inspector-up-next"]', () => {
  const ticket = collectionTickets.value.find(item => item.selected) ?? collectionTickets.value[0];
  toggleCollectionTicketUpNext(ticket.slug);
});
delegate(root, 'click', '[data-action="open-ticket-reader"]', () => { recordCollectionEvent('Ticket reader requested'); });
delegate(root, 'input', '[data-settings="ticket-list-row"] wa-input', (_event, target) => {
  const control = target as FormControl;
  if (control.getAttribute('name') === 'title') ticketRowSettings.title.value = control.value;
  if (control.getAttribute('name') === 'category') ticketRowSettings.category.value = control.value;
  if (control.getAttribute('name') === 'tags') ticketRowSettings.tags.value = control.value;
  if (control.getAttribute('name') === 'agent') ticketRowSettings.agentName.value = control.value;
  if (control.getAttribute('name') === 'updated') ticketRowSettings.updatedLabel.value = control.value;
});
delegate(root, 'change', '[data-settings="ticket-list-row"] [name]', (_event, target) => {
  const control = target as FormControl;
  switch (control.getAttribute('name')) {
    case 'status': ticketRowSettings.status.value = control.value as typeof ticketRowSettings.status.value; break;
    case 'priority': ticketRowSettings.priority.value = control.value as typeof ticketRowSettings.priority.value; break;
    case 'category-icon': ticketRowSettings.categoryIcon.value = control.value; break;
    case 'category-color': ticketRowSettings.categoryColor.value = control.value; break;
    case 'up-next': ticketRowSettings.upNext.value = control.checked; break;
    case 'blocked': ticketRowSettings.blocked.value = control.checked; break;
    case 'needs-review': ticketRowSettings.needsReview.value = control.checked; break;
    case 'selected': ticketRowSettings.selected.value = control.checked; break;
    case 'busy': ticketRowSettings.busy.value = control.checked; break;
  }
});
delegate(root, 'click', '[data-action="select-ticket-row"]', (event, target) => {
  if ((event.target as Element).closest('[data-action="toggle-row-up-next"]')) return;
  const row = target as HTMLElement;
  if (usesCollectionState()) {
    selectCollectionTicket(row.dataset.ticketSlug!);
    return;
  }
  ticketRowSettings.selected.value = !ticketRowSettings.selected.value;
  ticketRowSettings.event.value = ticketRowSettings.selected.value ? 'Ticket selected' : 'Ticket deselected';
  const selected = root.querySelector('[data-settings="ticket-list-row"] [name="selected"]') as FormControl | null;
  if (selected) selected.checked = ticketRowSettings.selected.value;
});
function toggleRowUpNext(target?: Element): void {
  if (usesCollectionState()) {
    const row = target?.closest('[data-component="ticket-list-row"]') as HTMLElement | null;
    if (row) toggleCollectionTicketUpNext(row.dataset.ticketSlug!);
    return;
  }
  ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
  ticketRowSettings.event.value = ticketRowSettings.upNext.value ? 'Added to Up Next' : 'Removed from Up Next';
  const control = root.querySelector('[data-settings="ticket-list-row"] [name="up-next"]') as FormControl | null;
  if (control) control.checked = ticketRowSettings.upNext.value;
}
delegateCapture(root, 'click', '[data-action="toggle-row-up-next"]', (event) => {
  event.stopPropagation();
  toggleRowUpNext(event.target as Element);
});
delegateCapture(root, 'keydown', '[data-action="toggle-row-up-next"]', (event) => {
  const key = (event as KeyboardEvent).key;
  if (key !== 'Enter' && key !== ' ') return;
  event.preventDefault();
  event.stopPropagation();
  toggleRowUpNext(event.target as Element);
});
delegate(root, 'keydown', '[data-action="select-ticket-row"]', (event, target) => {
  const key = (event as KeyboardEvent).key;
  if (key !== 'Enter' && key !== ' ') return;
  event.preventDefault();
  (target as HTMLElement).click();
});
delegate(root, 'contextmenu', '[data-action="select-ticket-row"]', (event, target) => {
  event.preventDefault();
  const pointer = event as MouseEvent;
  const row = target as HTMLElement;
  if (usesCollectionState()) {
    selectCollectionTicket(row.dataset.ticketSlug!, true);
    recordCollectionEvent(`Context menu opened for ${row.dataset.ticketSlug}`);
    contextMenu.value = { x: pointer.clientX, y: pointer.clientY, ticketSlug: row.dataset.ticketSlug };
    return;
  }
  ticketRowSettings.selected.value = true;
  ticketRowSettings.event.value = 'Context menu opened';
  const selected = root.querySelector('[data-settings="ticket-list-row"] [name="selected"]') as FormControl | null;
  if (selected) selected.checked = true;
  contextMenu.value = { x: pointer.clientX, y: pointer.clientY };
});
delegate(root, 'click', '[data-context-action]', (_event, target) => {
  const action = (target as HTMLElement).dataset.contextAction!;
  if (usesCollectionState() && contextMenu.value?.ticketSlug) {
    const slug = contextMenu.value.ticketSlug;
    if (action === 'Toggle Up Next') toggleCollectionTicketUpNext(slug);
    recordCollectionEvent(`${action} selected for ${slug}`);
    contextMenu.value = undefined;
    return;
  }
  if (action === 'Toggle Up Next') {
    ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
    const control = root.querySelector('[data-settings="ticket-list-row"] [name="up-next"]') as FormControl | null;
    if (control) control.checked = ticketRowSettings.upNext.value;
  }
  ticketRowSettings.event.value = `${action} selected`;
  contextMenu.value = undefined;
});
addEventListener('pointerdown', event => {
  if (contextMenu.value && !(event.target as Element).closest('.ticket-context-menu')) contextMenu.value = undefined;
});
addEventListener('keydown', event => { if (event.key === 'Escape') contextMenu.value = undefined; });
addEventListener('popstate', () => selectDemo(fromUrl(), false));
