import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { addDemoProject, closeProjectTab, projectTabs, resizeDemoCollapsed, resizeDemoWidth, selectProjectTab, setRegionSize, shellMode, shellStatsProjectName } from '../ux-demo/app-shell-demo';
import { AppShell } from './app-shell';
import { ConnectionStateBanner } from './connection-state-banner';
import { PageHeader } from './page-header';
import { ProjectTab,projectTabActivityDash } from './project-tab';
import { ProjectTabBar } from './project-tab-bar';
import { AppTabContextMenu } from './project-tab-context-menu';
import { clampRegionSize, ResizableRegion,resizeRegionFromPointer } from './resizable-region';

describe('application shell components', () => {
  it('defines the supported application floor as 1024 by 600 CSS pixels', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    const productionCss=readFileSync(new URL('../style.css',import.meta.url),'utf8');
    expect(css).toMatch(/\.app-shell \{[^}]*min-width: 64rem/);
    expect(css).toMatch(/\.app-shell \{[^}]*min-height: 37\.5rem/);
    expect(productionCss).not.toMatch(/\.app-shell\[data-component="app-shell"\] \{[^}]*(?:min-width|min-height):/);
    expect(css).not.toMatch(/@media[^{}]*max-width[^{}]*\{[^{}]*\.app-shell > \.resizable-region[^{}]*display: none/);
  });

  it('gives an expanded narrow search its own row below the project identity', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    expect(css).toMatch(/@media \(max-width: 48rem\) \{[\s\S]*toolbar:has\(\.workspace-header__search-group\[data-expanded="true"\]\) \{ grid-template-columns: minmax\(0, 1fr\); row-gap: \.4rem;/);
    expect(css).toMatch(/toolbar:has\(\.workspace-header__search-group\[data-expanded="true"\]\) > \.toolbar__leading \{ padding-inline: \.5rem 0;/);
    expect(css).toMatch(/toolbar:has\(\.workspace-header__search-group\[data-expanded="true"\]\) > \.toolbar__trailing \{ grid-column: 1; width: 100%; padding-inline: \.75rem 0; justify-content: stretch;/);
    expect(css).toMatch(/\.workspace-header__search-group \{ width: auto; min-width: 11rem; flex: 1 1 auto;/);
  });

  it('separates the terminal header from its lowered dashboard surface', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    expect(css).toMatch(/\.app-shell\[data-mode="terminals"\][^{]*\.project-tab-bar \{[^}]*border-bottom: 1px solid var\(--wa-color-surface-border\)/);
  });
  it('draws one continuous focus outline around the ticket work area', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    expect(css).toContain('.app-shell__work-area:focus, .app-shell__work-area:focus-within');
    expect(css).toMatch(/app-shell__work-area:focus-within \{[^}]*outline: 2px solid var\(--wa-color-focus\)/);
    expect(css).toMatch(/app-shell__work-area::after \{[^}]*z-index: 20[^}]*border: 2px solid transparent[^}]*pointer-events: none/);
    expect(css).toMatch(/app-shell__work-area:focus-within::after \{[^}]*border-color: var\(--wa-color-focus\)/);
    expect(css).toContain('.app-shell__work-area:has(.terminal-dashboard__magnified) { z-index: 3; outline-color: transparent; transition: none; }');
    expect(css).toContain('.app-shell__work-area:has(.terminal-dashboard__magnified)::after { border-color: transparent; transition: none; }');
  });
  it('lets the composer own the workspace top rhythm without removing spacing when absent', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    const withComposer=String(AppShell({ tabs: [], sidebar: 'side' as never, header: 'head' as never, composer: 'compose' as never, workspace: 'work' as never }));
    const withoutComposer=String(AppShell({ tabs: [], sidebar: 'side' as never, header: 'head' as never, workspace: 'work' as never }));
    expect(withComposer).toContain('data-has-composer="true"');
    expect(withoutComposer).toContain('data-has-composer="false"');
    expect(css).toMatch(/\.app-shell__composer \{[^}]*padding: \.75rem 1rem;/);
    expect(css).toMatch(/data-has-composer="true"[^}]*app-shell__workspace \{[^}]*padding-top: 0;/);
    expect(css).toMatch(/@media \(max-width: 42rem\)[\s\S]*\.app-shell__composer \{ padding: \.7rem; \}/);
  });
  it('projects every ProjectTab state without nesting actions', () => {
    const markup = String(ProjectTab({ id: 'one', name: 'One', location: 'remote', selected: true, busy: true, disconnected: true, attention: true }));
    expect(markup).toContain('role="tab"');
    expect(markup).toContain('aria-selected="true"');
    expect(markup).toContain('data-location="remote"');
    expect(markup).toContain('data-ticket-drop-project="one"');
    expect(markup).toContain('data-tab-kind="project"');
    expect(markup).toContain('data-tab-id="one"');
    expect(markup).toContain('draggable="true"');
    expect(markup).toContain('tabindex="0"');
    expect(markup).toContain('aria-keyshortcuts="Delete Backspace"');
    expect(markup).toMatch(/app-tab__close[^>]*tabindex="-1"|tabindex="-1"[^>]*app-tab__close/);
    expect(markup).toContain('data-lucide="cloud"');
    expect(markup).toContain('aria-label="Project busy"');
    expect(markup).not.toContain('data-lucide="loader-circle"');
    expect(markup).not.toContain('data-lucide="wifi-off"');
    expect(markup.indexOf('</button><button')).toBeGreaterThan(0);
    const notificationMarkup = String(ProjectTab({ id: 'one', name: 'One', location: 'local', notificationCount: 2 }));
    expect(notificationMarkup).not.toContain('data-lucide="folder-git-2"');
    expect(notificationMarkup).toContain('aria-label="2 pending notifications"');
    expect(notificationMarkup).toContain('data-lucide="bell"');
    expect(String(ProjectTab({ id: 'empty', name: 'Empty', location: 'local' }))).not.toContain('project-tab__work');
    const queuedMarkup=String(ProjectTab({ id: 'queued', name: 'Queued', location: 'local', upNextCount: 99 }));
    expect(queuedMarkup).toContain('aria-label="99 Up Next tickets"');
    expect(queuedMarkup).toContain('>99</span>');
    expect(queuedMarkup).not.toContain('project-tab__activity-ring');
    const cappedActiveMarkup=String(ProjectTab({ id: 'active', name: 'Active', location: 'local', notificationCount: 1, upNextCount: 125, activeTicketCount: 2 }));
    expect(cappedActiveMarkup).toContain('aria-label="125 Up Next tickets, 2 active tickets"');
    expect(cappedActiveMarkup).toContain('>2</span>');
    expect(cappedActiveMarkup).toContain('project-tab__activity-ring');
    expect(cappedActiveMarkup).toContain('data-segments="2"');
    expect(cappedActiveMarkup).toContain('stroke-dasharray="21.2058 7.0686"');
    expect(cappedActiveMarkup).toContain('aria-label="1 pending notification"');
    const activeOnlyMarkup=String(ProjectTab({id:'working',name:'Working',location:'local',activeTicketCount:1}));
    expect(activeOnlyMarkup).toContain('aria-label="1 active ticket"');
    expect(activeOnlyMarkup).toContain('project-tab__activity-ring');
    expect(activeOnlyMarkup).toContain('data-segments="1"');
    expect(activeOnlyMarkup).toContain('>1</span>');
    expect(projectTabActivityDash(1)).toBe('42.4115 14.1372');
    expect(projectTabActivityDash(3)).toBe('14.1372 4.7124');
  });

  it('draws tab-selection focus around the complete compound pill', () => {
    const css=readFileSync(new URL('./app-tab.css',import.meta.url),'utf8'),barCss=readFileSync(new URL('./project-tab-bar.css',import.meta.url),'utf8');
    expect(css).toContain('.app-tab:has(.app-tab__select:focus-visible) { outline: var(--wa-focus-ring); outline-offset: -2px; }');
    expect(css).toContain('.app-tab__select:focus-visible { outline: none; }');
    expect(css).toContain('.app-tab__close:focus-visible { border-radius: var(--wa-border-radius-pill); outline: var(--wa-focus-ring); outline-offset: -2px; }');
    expect(css).not.toContain('.app-tab__select:focus-visible, .app-tab__close:focus-visible');
    expect(barCss).toMatch(/\.project-tab-bar__tabs \{[^}]*margin: calc\(var\(--wa-space-2xs\) \* -1\);[^}]*padding: var\(--wa-space-2xs\);/);
  });

  it('composes tabs with add and overflow actions', () => {
    const markup = String(ProjectTabBar({ tabs: [{ id: 'one', name: 'One', location: 'local', selected: true }] }));
    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('aria-label="Add project"');
    expect(markup).toContain('data-action="choose-project"');
    expect(markup).not.toContain('data-action="add-project"');
    expect(markup).toContain('aria-label="Workspace grid"');
    expect(markup).toContain('data-lucide="grid-3x3"');
    expect(markup).toContain('aria-label="Cross-project stats"');
    expect(markup.match(/tabindex="0"/g)?.length).toBeGreaterThanOrEqual(3);
    expect(markup).not.toContain('aria-label="Hide project sidebar"');
    expect(markup).toContain('data-project-id="one"');
    expect(markup.indexOf('Global dashboards')).toBeLessThan(markup.indexOf('role="tablist"'));
  });

  it('shares context-menu actions across project and terminal tabs with Option reversing direction',()=>{
    const project=String(AppTabContextMenu({kind:'project',id:'one',x:10,y:20}));
    const terminal=String(AppTabContextMenu({kind:'terminal',id:'term',x:10,y:20,direction:'left'}));
    const chat=String(AppTabContextMenu({kind:'ai-chat',id:'chat',x:10,y:20}));
    for(const label of ['Close Tab','Close Other Tabs','Close Tabs to the Right','Close All Tabs'])expect(project).toContain(label);
    expect(project).toContain('aria-label="Project tab actions"');expect(project).toContain('data-action="project-tab-context-action"');
    expect(project).not.toContain('Rename…');expect(terminal).toContain('aria-label="Terminal tab actions"');expect(terminal).toContain('Rename…');expect(terminal).toContain('data-tab-action="rename"');expect(terminal).toContain('Close Tabs to the Left');expect(terminal).toContain('data-action="terminal-tab-context-action"');
    expect(chat).toContain('aria-label="AI chat tab actions"');expect(chat).toContain('data-chat-id="chat"');expect(chat).toContain('data-action="terminal-tab-context-action"');expect(chat).not.toContain('Rename…');
  });

  it('clamps and projects accessible splitters in both axes', () => {
    expect(clampRegionSize(100, 180, 420)).toBe(180);
    expect(clampRegionSize(300.6, 180, 420)).toBe(301);
    expect(clampRegionSize(500, 180, 420)).toBe(420);
    expect(resizeRegionFromPointer(300, 20, 'end')).toBe(320);
    expect(resizeRegionFromPointer(300, 20, 'start')).toBe(280);
    expect(resizeRegionFromPointer(300, -20, 'start')).toBe(320);
    const markup = String(ResizableRegion({ id: 'left', label: 'Sidebar', size: 240, min: 180, max: 420, children: 'content' as never }));
    expect(markup).toContain('aria-orientation="vertical"');
    expect(markup).toContain('aria-valuenow="240"');
    expect(markup).toContain('data-lucide="grip-vertical"');
  });

  it('composes the terminal drawer as a center-only vertical region with collapsed recovery',()=>{
    const open=String(AppShell({tabs:[],sidebar:'side' as never,header:'head' as never,workspace:'work' as never,terminalDrawer:'drawer' as never,terminalDrawerVisible:true,terminalDrawerSize:340}));
    expect(open).toContain('data-region-id="app-terminal-drawer"');expect(open).toContain('data-axis="vertical"');expect(open).toContain('data-edge="start"');expect(open).toContain('aria-valuenow="340"');expect(open.indexOf('data-region-id="app-terminal-drawer"')).toBeLessThan(open.indexOf('</main>'));
    const tall=String(AppShell({tabs:[],sidebar:'side' as never,header:'head' as never,workspace:'work' as never,terminalDrawer:'drawer' as never,terminalDrawerVisible:true,terminalDrawerSize:700,terminalDrawerMax:760,terminalDrawerTransitioning:true}));expect(tall).toContain('aria-valuemax="760"');expect(tall).toContain('aria-valuenow="700"');expect(tall).toContain('data-transitioning="true"');
    const collapsed=String(AppShell({tabs:[],sidebar:'side' as never,header:'head' as never,workspace:'work' as never,terminalDrawer:'drawer' as never}));expect(collapsed).toContain('data-collapsed="true"');expect(collapsed).toContain('aria-label="Show terminal drawer"');
  });

  it('keeps the ticket rail beside the terminal dashboard',()=>{
    const markup=String(AppShell({tabs:[],mode:'terminals',sidebar:'side' as never,header:'Terminals' as never,workspace:'dashboard' as never,inspector:'ticket rail' as never}));
    expect(markup).toContain('data-region-id="app-sidebar"');
    expect(markup).toContain('aria-label="Operations sidebar"');
    expect(markup).toContain('data-region-id="app-inspector"');
    expect(markup).toContain('aria-label="Ticket rail"');
    const hidden=String(AppShell({tabs:[],mode:'terminals',sidebar:'side' as never,header:'Terminals' as never,workspace:'dashboard' as never,inspector:'ticket rail' as never,inspectorVisible:false}));
    expect(hidden).toContain('aria-label="Show ticket rail"');
    expect(hidden).toContain('data-collapsed="true"');
  });

  it('renders all connection semantics and only valid actions', () => {
    expect(String(ConnectionStateBanner({ state: 'connecting' }))).toContain('role="status"');
    expect(String(ConnectionStateBanner({ state: 'connecting' }))).not.toContain('<button');
    expect(String(ConnectionStateBanner({ state: 'offline' }))).toContain('data-action="retry-connection"');
    expect(String(ConnectionStateBanner({ state: 'authentication' }))).toContain('data-action="authenticate-connection"');
    expect(String(ConnectionStateBanner({ state: 'client-too-old' }))).toContain('data-action="reload-client"');
    expect(String(ConnectionStateBanner({ state: 'server-too-old' }))).toContain('Server update required');
    expect(String(ConnectionStateBanner({ state: 'compatibility-unknown' }))).toContain('Server compatibility unknown');
    expect(String(ConnectionStateBanner({ state: 'revision-mismatch' }))).toContain('Different server build is running');
  });

  it('composes all top-level regions', () => {
    const markup = String(AppShell({ tabs: [], sidebar: 'side' as never, header: 'head' as never, headerActions: 'actions' as never, pageHeader: PageHeader({ title: 'All Tickets' }), composer: 'compose' as never, workspace: 'work' as never, inspector: 'inspect' as never, banner: 'banner' as never, overlay: 'overlay' as never }));
    expect(markup).toContain('data-component="app-shell"');
    expect(markup).toContain('data-region-id="app-sidebar"');
    expect(markup).toContain('aria-valuemin="250"');
    expect(markup).toContain('data-region-id="app-inspector"');
    expect(markup).toContain('Ticket workspace');
    // HS2-H4MWDB: stable data-keys let the morph match the scroll-container chain by identity so
    // toggling the conditional overlay/banner siblings above it never rebuilds it (which would
    // reset the workspace scrollTop, e.g. when the ticket context menu opens).
    expect(markup).toContain('class="app-shell__work-area" data-key="app-shell-work-area" data-has-composer="true" tabindex="0" aria-label="Ticket work area"');
    expect(markup).toContain('data-key="app-shell-workspace"');
    expect(markup).toContain('data-ticket-scroll-owner="workspace"');
    expect(markup).toContain('class="app-shell__composer">compose');
    expect(markup.indexOf('app-shell__composer')).toBeLessThan(markup.indexOf('Ticket workspace'));
    expect(markup).toContain('data-component="page-header"');
    expect(markup).toContain('class="toolbar__leading">head');
    expect(markup).toContain('class="toolbar__trailing">actions');
    expect(markup).toContain('data-component="toolbar" data-divider="false"');
    expect(markup.indexOf('data-component="project-tab-bar"')).toBeLessThan(markup.indexOf('overlay'));
    expect(markup.indexOf('overlay')).toBeLessThan(markup.indexOf('data-region-id="app-inspector"'));
    expect(markup.indexOf('head')).toBeLessThan(markup.indexOf('data-component="project-tab-bar"'));
    expect(markup.indexOf('data-component="project-tab-bar"')).toBeLessThan(markup.indexOf('data-component="page-header"'));
    const globalMarkup = String(AppShell({ mode: 'stats', tabs: [], sidebar: 'side' as never, header: 'head' as never, workspace: 'work' as never, inspector: 'inspect' as never }));
    expect(globalMarkup).toContain('data-mode="stats"');
    expect(globalMarkup).not.toContain('data-region-id="app-sidebar"');
    expect(globalMarkup).not.toContain('data-region-id="app-inspector"');
    const terminalMarkup = String(AppShell({ mode: 'terminals', tabs: [], sidebar: 'operations' as never, header: 'head' as never, workspace: 'terminals' as never, inspector: 'tickets' as never }));
    expect(terminalMarkup).toContain('aria-label="Operations sidebar"');
    expect(terminalMarkup).toContain('data-region-id="app-sidebar"');
    const collapsedTerminalMarkup = String(AppShell({ mode: 'terminals', tabs: [], sidebar: 'operations' as never, sidebarVisible: false, header: 'head' as never, workspace: 'terminals' as never }));
    expect(collapsedTerminalMarkup).toContain('aria-label="Show operations sidebar"');
    const collapsedMarkup = String(AppShell({ tabs: [], sidebar: 'side' as never, sidebarVisible: false, header: 'head' as never, workspace: 'work' as never }));
    expect(collapsedMarkup).toContain('data-region-id="app-sidebar"');
    expect(collapsedMarkup).toContain('data-collapsed="true"');
    expect(collapsedMarkup).toContain('aria-label="Show project sidebar"');
    expect(collapsedMarkup.indexOf('aria-label="Show project sidebar"')).toBeLessThan(collapsedMarkup.indexOf('data-component="project-tab-bar"'));
    const hiddenInspectorMarkup = String(AppShell({ tabs: [], sidebar: 'side' as never, header: 'head' as never, workspace: 'work' as never, inspector: 'inspect' as never, inspectorVisible: false }));
    expect(hiddenInspectorMarkup).toContain('aria-label="Show ticket inspector"');
    expect(hiddenInspectorMarkup).toContain('data-region-id="app-inspector"');
    expect(hiddenInspectorMarkup).toContain('data-collapsed="true"');
  });

  it('walks select, close, add, resize, and post-resize transitions', () => {
    projectTabs.value = [
      { id: 'one', name: 'One', location: 'local', selected: true },
      { id: 'two', name: 'Two', location: 'remote' },
    ];
    shellMode.value = 'stats';
    selectProjectTab('two');
    expect(shellMode.value).toBe('project');
    expect(shellStatsProjectName.value).toBeUndefined();
    expect(projectTabs.value.map(tab => [tab.id, tab.selected])).toEqual([['one', false], ['two', true]]);
    closeProjectTab('two');
    expect(projectTabs.value).toMatchObject([{ id: 'one', selected: true }]);
    addDemoProject();
    expect(projectTabs.value.at(-1)).toMatchObject({ location: 'local', selected: true });
    expect(projectTabs.value.slice(0, -1).every(tab => !tab.selected)).toBe(true);
    resizeDemoWidth.value = 260;
    setRegionSize('resize-demo-horizontal', 1000);
    expect(resizeDemoWidth.value).toBe(420);
    setRegionSize('resize-demo-horizontal', 240);
    expect(resizeDemoWidth.value).toBe(250);
    resizeDemoCollapsed.value = true;
    expect(resizeDemoWidth.value).toBe(250);
    resizeDemoCollapsed.value = false;
    expect(resizeDemoWidth.value).toBe(250);
  });
});
