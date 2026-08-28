import { describe, expect, it } from 'vitest';
import { AppShell } from './app-shell';
import { ConnectionStateBanner } from './connection-state-banner';
import { ProjectTab } from './project-tab';
import { ProjectTabBar } from './project-tab-bar';
import { clampRegionSize, resizeRegionFromPointer, ResizableRegion } from './resizable-region';
import { addDemoProject, closeProjectTab, projectTabs, resizeDemoCollapsed, selectProjectTab, setRegionSize, resizeDemoWidth, shellMode } from '../ux-demo/app-shell-demo';

describe('application shell components', () => {
  it('projects every ProjectTab state without nesting actions', () => {
    const markup = String(ProjectTab({ id: 'one', name: 'One', location: 'remote', selected: true, busy: true, disconnected: true, attention: true }));
    expect(markup).toContain('role="tab"');
    expect(markup).toContain('aria-selected="true"');
    expect(markup).toContain('data-location="remote"');
    expect(markup).toContain('data-lucide="cloud"');
    expect(markup).toContain('data-lucide="loader-circle"');
    expect(markup).not.toContain('data-lucide="wifi-off"');
    expect(markup.indexOf('</button><button')).toBeGreaterThan(0);
  });

  it('composes tabs with add and overflow actions', () => {
    const markup = String(ProjectTabBar({ tabs: [{ id: 'one', name: 'One', location: 'local', selected: true }], projectsOverflowing: true }));
    expect(markup).toContain('role="tablist"');
    expect(markup).toContain('aria-label="Add project"');
    expect(markup).toContain('aria-label="More projects"');
    expect(markup).toContain('aria-label="Terminal dashboard"');
    expect(markup).toContain('aria-label="Cross-project stats"');
    expect(markup).toContain('aria-label="Hide project sidebar"');
    expect(markup).toContain('data-lucide="chevrons-right"');
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

  it('renders all connection semantics and only valid actions', () => {
    expect(String(ConnectionStateBanner({ state: 'connecting' }))).toContain('role="status"');
    expect(String(ConnectionStateBanner({ state: 'connecting' }))).not.toContain('<button');
    expect(String(ConnectionStateBanner({ state: 'offline' }))).toContain('data-action="retry-connection"');
    expect(String(ConnectionStateBanner({ state: 'authentication' }))).toContain('data-action="authenticate-connection"');
  });

  it('composes all top-level regions', () => {
    const markup = String(AppShell({ tabs: [], sidebar: 'side' as never, header: 'head' as never, workspace: 'work' as never, inspector: 'inspect' as never, banner: 'banner' as never }));
    expect(markup).toContain('data-component="app-shell"');
    expect(markup).toContain('data-region-id="app-sidebar"');
    expect(markup).toContain('aria-valuemin="250"');
    expect(markup).toContain('data-region-id="app-inspector"');
    expect(markup).toContain('Ticket workspace');
    const globalMarkup = String(AppShell({ mode: 'stats', tabs: [], sidebar: 'side' as never, header: 'head' as never, workspace: 'work' as never, inspector: 'inspect' as never }));
    expect(globalMarkup).toContain('data-mode="stats"');
    expect(globalMarkup).not.toContain('data-region-id="app-sidebar"');
    expect(globalMarkup).not.toContain('data-region-id="app-inspector"');
    const collapsedMarkup = String(AppShell({ tabs: [], sidebar: 'side' as never, sidebarVisible: false, header: 'head' as never, workspace: 'work' as never }));
    expect(collapsedMarkup).not.toContain('data-region-id="app-sidebar"');
    expect(collapsedMarkup).toContain('aria-label="Show project sidebar"');
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
