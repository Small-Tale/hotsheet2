import { readFileSync } from 'node:fs';

import { PanelHeader } from '@kerfjs/ui/panel-header';
import { clampRegionSize, ResizableRegion,resizeRegionFromPointer } from '@kerfjs/ui/resizable-region';
import { describe, expect, it } from 'vitest';

import { addDemoProject, closeProjectTab, projectTabs, resizeDemoCollapsed, resizeDemoWidth, selectProjectTab, setRegionSize, shellMode, shellStatsProjectName } from '../ux-demo/app-shell-demo';
import { AppShell } from './app-shell';
import { ConnectionStateBanner } from './connection-state-banner';
import { ProjectTab,projectTabActivityDash,projectTabActivitySegments } from './project-tab';
import { ProjectTabBar } from './project-tab-bar';
import { AppTabContextMenu } from './project-tab-context-menu';

describe('application shell components', () => {
  it('uses canonical spacing for terminal-drawer motion and restore placement', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    const productionCss=readFileSync(new URL('../style.css',import.meta.url),'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/data-collapsed="true"[^}]*__content \{[^}]*translateY\(var\(--kui-space-l\)\)/);
    expect(css).toMatch(/\.app-shell__terminal-drawer-restore \{[^}]*right: calc\(var\(--kui-space-m\) \+ var\(--hotsheet-safe-area-right\)\); bottom: calc\(var\(--kui-space-m\) \+ var\(--hotsheet-safe-area-bottom\)\)/);
    expect(productionCss).toMatch(/html, body, #app \{[^}]*height: 100%; height: 100dvh;/);
    expect(productionCss).toContain('--hotsheet-safe-area-bottom: env(safe-area-inset-bottom, 0px)');
  });

  it('defines the supported application floor as 1024 by 600 CSS pixels', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    const productionCss=readFileSync(new URL('../style.css',import.meta.url),'utf8');
    expect(css).toMatch(/\.app-shell \{[^}]*min-width: remify\(1024px\)/);
    expect(css).toMatch(/\.app-shell \{[^}]*min-height: remify\(600px\)/);
    expect(productionCss).not.toMatch(/\.app-shell\[data-component="app-shell"\] \{[^}]*(?:min-width|min-height):/);
    expect(css).not.toMatch(/@media[^{}]*max-width[^{}]*\{[^{}]*\.app-shell > \.kui-resizable-region[^{}]*display: none/);
  });

  it('gives an expanded narrow search its own row below the project identity', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    expect(css).toMatch(/@media \(max-width: remify\(768px\)\) \{[\s\S]*toolbar:has\(\.workspace-header__search-group\[data-expanded="true"\]\) \{ grid-template-columns: minmax\(0, 1fr\); row-gap: remify\(6\.4px\);/);
    expect(css).toMatch(/toolbar:has\(\.workspace-header__search-group\[data-expanded="true"\]\) > \.kui-toolbar__leading \{ padding-inline: remify\(8px\) 0;/);
    expect(css).toMatch(/toolbar:has\(\.workspace-header__search-group\[data-expanded="true"\]\) > \.kui-toolbar__trailing \{ grid-column: 1; width: 100%; padding-inline: remify\(12px\) 0; justify-content: stretch;/);
    expect(css).toMatch(/\.workspace-header__search-group \{ width: auto; min-width: remify\(176px\); flex: 1 1 auto;/);
  });

  it('separates the terminal header from its lowered dashboard surface', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    expect(css).toMatch(/\.app-shell\[data-mode="terminals"\][^{]*\.project-tab-bar \{[^}]*border-bottom: 1px solid var\(--wa-color-surface-border\)/);
  });
  it('draws a border between the white header chrome and the lowered work area (HS2-WH6CCR)', () => {
    const css=readFileSync(new URL('./app-shell.css',import.meta.url),'utf8');
    // The gray content region carries a top border so it reads as separate from the white
    // toolbar/tab-bar/page-header above it; terminals mode uses the tab-bar border instead.
    expect(css).toMatch(/\.app-shell__work-area \{[^}]*border-top: 1px solid var\(--wa-color-surface-border\)/);
    expect(css).toContain('.app-shell[data-mode="terminals"] .app-shell__work-area { border-top: 0; }');
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
    const withoutSidebar=String(AppShell({tabs:[],header:'head' as never,workspace:'work' as never,sidebarVisible:false}));
    expect(withComposer).toContain('data-has-composer="true"');
    expect(withoutComposer).toContain('data-has-composer="false"');
    expect(withoutSidebar).not.toContain('data-region-id="app-sidebar"');
    expect(withoutSidebar).not.toContain('aria-label="Show project sidebar"');
    expect(css).toMatch(/\.app-shell__composer \{[^}]*padding: remify\(12px\) remify\(16px\);/);
    expect(css).toMatch(/data-has-composer="true"[^}]*app-shell__workspace \{[^}]*padding-top: 0;/);
    expect(css).toMatch(/@media \(max-width: remify\(672px\)\)[\s\S]*\.app-shell__composer \{ padding: remify\(11\.2px\); \}/);
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
    expect(markup).toContain('aria-keyshortcuts="Delete Backspace Alt+Shift+ArrowLeft Alt+Shift+ArrowRight"');
    expect(markup).toMatch(/app-tab__close[^>]*tabindex="-1"|tabindex="-1"[^>]*app-tab__close/);
    expect(markup).toContain('data-lucide="cloud"');
    expect(markup).toContain('aria-label="Project busy"');
    expect(markup).not.toContain('data-lucide="loader-circle"');
    expect(markup).not.toContain('data-lucide="wifi-off"');
    expect(markup.indexOf('</button><button')).toBeGreaterThan(0);
    const restoreFailure=String(ProjectTab({id:'failed',name:'Failed',location:'local',attention:true,restoreFailure:true,closable:false,draggable:false}));
    expect(restoreFailure).toContain('data-restore-failure="true"');
    expect(restoreFailure).toContain('draggable="false"');
    expect(restoreFailure).toContain('data-lucide="circle-alert"');
    expect(restoreFailure).not.toContain('data-action="close-project-tab"');
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
    // The label always projects the Up Next count (segments project the active count); 125 clamps to 99+.
    expect(cappedActiveMarkup).toContain('>99+</span>');
    expect(cappedActiveMarkup).toContain('project-tab__activity-ring');
    expect(cappedActiveMarkup).toContain('data-segments="2"');
    expect(cappedActiveMarkup).toContain('stroke-dasharray="21.2058 7.0686"');
    expect(cappedActiveMarkup).toContain('aria-label="1 pending notification"');
    const activeOnlyMarkup=String(ProjectTab({id:'working',name:'Working',location:'local',activeTicketCount:1}));
    expect(activeOnlyMarkup).toContain('aria-label="1 active ticket"');
    expect(activeOnlyMarkup).toContain('project-tab__activity-ring');
    const projectTabCss=readFileSync(new URL('./project-tab.css',import.meta.url),'utf8');
    // Dark ink keeps the up-next count legible on the filled yellow badge in dark mode (HS2-SXXK5Q);
    // the active (transparent, activity-ring) state uses primary text — black in light, white in dark —
    // for the count itself so it is not an unreadable yellow-on-tab number (HS2-TF43Y0).
    expect(projectTabCss).toMatch(/\.project-tab__work-count\s*\{[^}]*color:\s*var\(--hs-ticket-state-up-next-on\)/);
    expect(projectTabCss).not.toMatch(/\.project-tab__work-count\s*\{[^}]*color:\s*var\(--wa-color-warning-on-normal\)/);
    expect(projectTabCss).toMatch(/\.project-tab__work\[data-active="true"\]\s+\.project-tab__work-count\s*\{[^}]*color:\s*var\(--wa-color-text-normal\)/);
    expect(projectTabCss).toContain('[data-attention="true"] .kui-app-tab__name { color: var(--wa-color-danger-on-quiet); }');
    expect(projectTabCss).toContain('animation:project-tab-activity-rotate 1.7s linear infinite');
    expect(projectTabCss).toContain('@media (prefers-reduced-motion:reduce)');
    expect(activeOnlyMarkup).toContain('data-segments="1"');
    // Up Next is 0 here, so the label reads 0 even though one ticket is active.
    expect(activeOnlyMarkup).toContain('>0</span>');
    expect(projectTabActivityDash(1)).toBe('42.4115 14.1372');
    expect(projectTabActivityDash(3)).toBe('14.1372 4.7124');
    // The ring caps its drawn segments at 8 so a large active count stays legible (HS2-7XHZY1).
    expect(projectTabActivitySegments(1)).toBe(1);
    expect(projectTabActivitySegments(8)).toBe(8);
    expect(projectTabActivitySegments(12)).toBe(8);
    expect(projectTabActivityDash(8)).toBe('5.3014 1.7671');
    expect(projectTabActivityDash(12)).toBe(projectTabActivityDash(8));
    const cappedRingMarkup=String(ProjectTab({id:'capped',name:'Capped',location:'local',upNextCount:7,activeTicketCount:12}));
    // Segments cap at 8 while the accessible label still reports the true active count and the center shows Up Next.
    expect(cappedRingMarkup).toContain('data-segments="8"');
    expect(cappedRingMarkup).toContain('stroke-dasharray="5.3014 1.7671"');
    expect(cappedRingMarkup).toContain('aria-label="7 Up Next tickets, 12 active tickets"');
    expect(cappedRingMarkup).toContain('>7</span>');
  });

  it('draws tab-selection focus around the complete compound pill', () => {
    const projectCss=readFileSync(new URL('./project-tab.css',import.meta.url),'utf8'),barCss=readFileSync(new URL('./project-tab-bar.css',import.meta.url),'utf8');
    expect(String(ProjectTab({id:'focus',name:'Focus',location:'local'}))).toContain('class="kui-app-tab project-tab"');
    expect(projectCss).not.toContain('.project-tab:has(.kui-app-tab__close)');
    expect(barCss).toMatch(/\.project-tab-bar \.kui-tab-bar__tabs \{[^}]*margin-block: calc\(var\(--kui-space-2xs\) \* -1\);[^}]*padding: var\(--kui-space-2xs\);/);
    // The tabs strip does not stretch, so the trailing Add-project (+) follows the tabs rather than
    // sitting far-right (HS2-HV52WR).
    expect(barCss).toMatch(/\.project-tab-bar \.kui-tab-bar__tabs \{[^}]*flex: 0 1 auto/);
  });

  it('composes tabs with add and overflow actions', () => {
    const markup = String(ProjectTabBar({ tabs: [{ id: 'one', name: 'One', location: 'local', selected: true }] }));
    // Built on kerf's TabBar (HS2-Q6P9P0): the strip is the kerf tab-bar with a project className and a
    // stable bar id, the modes are its leading slot and add-project its trailing slot.
    expect(markup).toContain('data-component="tab-bar"');
    expect(markup).toContain('data-tab-bar-id="projects"');
    expect(markup).toContain('class="kui-tab-bar project-tab-bar"');
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

  it('renders a project Select instead of the tab strip on mobile, keeping the mode switcher and add action (HS2-4C5RM7)', () => {
    const markup = String(ProjectTabBar({ mobile: true, tabs: [{ id: 'one', name: 'One', location: 'local', selected: false }, { id: 'two', name: 'Two', location: 'local', selected: true }] }));
    // No kerf TabBar / tablist on mobile — the tabs become a project Select whose value is the active tab.
    expect(markup).not.toContain('role="tablist"');
    expect(markup).not.toContain('data-tab-kind="project"');
    expect(markup).toContain('class="project-tab-bar project-tab-bar--mobile"');
    expect(markup).toContain('name="mobile-project"');
    expect(markup).toContain('value="two"');
    expect(markup).toContain('<wa-option value="one"');
    expect(markup).toContain('<wa-option value="two"');
    // The dashboard mode switcher and Add-project action remain.
    expect(markup).toContain('aria-label="Workspace grid"');
    expect(markup).toContain('data-action="choose-project"');
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
    expect(markup).toContain('data-kui-resize-handle');
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
    expect(String(ConnectionStateBanner({ state: 'connecting' }))).toContain('data-component="state-banner"');
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
    const markup = String(AppShell({ tabs: [], sidebar: 'side' as never, header: 'head' as never, headerActions: 'actions' as never, pageHeader: PanelHeader({ title: 'All Tickets', titleId: 'all-tickets-title' }), composer: 'compose' as never, workspace: 'work' as never, inspector: 'inspect' as never, banner: 'banner' as never, overlay: 'overlay' as never }));
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
    expect(markup).toContain('data-component="panel-header"');
    expect(markup).toContain('class="kui-toolbar__leading">head');
    expect(markup).toContain('class="kui-toolbar__trailing">actions');
    expect(markup).toContain('data-component="toolbar" data-divider="false"');
    expect(markup.indexOf('data-component="tab-bar"')).toBeLessThan(markup.indexOf('overlay'));
    expect(markup.indexOf('overlay')).toBeLessThan(markup.indexOf('data-region-id="app-inspector"'));
    expect(markup.indexOf('head')).toBeLessThan(markup.indexOf('data-component="tab-bar"'));
    expect(markup.indexOf('data-component="tab-bar"')).toBeLessThan(markup.indexOf('data-component="panel-header"'));
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
    expect(collapsedMarkup.indexOf('aria-label="Show project sidebar"')).toBeLessThan(collapsedMarkup.indexOf('data-component="tab-bar"'));
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
