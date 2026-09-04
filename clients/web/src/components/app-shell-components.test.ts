import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { addDemoProject, closeProjectTab, projectTabs, resizeDemoCollapsed, resizeDemoWidth, selectProjectTab, setRegionSize, shellMode } from '../ux-demo/app-shell-demo';
import { AppShell } from './app-shell';
import { ConnectionStateBanner } from './connection-state-banner';
import { PageHeader } from './page-header';
import { ProjectTab } from './project-tab';
import { ProjectTabBar } from './project-tab-bar';
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
  it('draws one continuous focus outline around the ticket work area', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    expect(css).toContain('.app-shell__work-area:focus, .app-shell__work-area:focus-within');
    expect(css).toMatch(/app-shell__work-area:focus-within \{[^}]*outline: 2px solid var\(--wa-color-focus\)/);
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
    expect(markup).toContain('data-lucide="cloud"');
    expect(markup).toContain('aria-label="Project busy"');
    expect(markup).not.toContain('data-lucide="loader-circle"');
    expect(markup).not.toContain('data-lucide="wifi-off"');
    expect(markup.indexOf('</button><button')).toBeGreaterThan(0);
    const notificationMarkup = String(ProjectTab({ id: 'one', name: 'One', location: 'local', notificationCount: 2 }));
    expect(notificationMarkup).not.toContain('data-lucide="folder-git-2"');
    expect(notificationMarkup).toContain('aria-label="2 pending notifications"');
    expect(notificationMarkup).toContain('data-lucide="bell"');
  });

  it('composes tabs with add and overflow actions', () => {
    const markup = String(ProjectTabBar({ tabs: [{ id: 'one', name: 'One', location: 'local', selected: true }] }));
    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('aria-label="Add project"');
    expect(markup).toContain('aria-label="Terminal dashboard"');
    expect(markup).toContain('aria-label="Cross-project stats"');
    expect(markup).not.toContain('aria-label="Hide project sidebar"');
    expect(markup).toContain('data-project-id="one"');
    expect(markup.indexOf('Global dashboards')).toBeLessThan(markup.indexOf('role="tablist"'));
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
