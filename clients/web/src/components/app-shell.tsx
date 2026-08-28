import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { ResizableRegion } from './resizable-region';
import type { ProjectTabProps } from './project-tab';
import { ProjectTabBar } from './project-tab-bar';
import type { ProjectTabBarMode } from './project-tab-bar';
import { PanelLeftOpen, PanelRightOpen } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { ToolbarControlGroup } from './toolbar-control-group';
import { Toolbar } from './toolbar';
import './app-shell.css';

export interface AppShellProps {
  tabs: ProjectTabProps[];
  sidebar: SafeHtml;
  header: SafeHtml;
  headerActions?: SafeHtml;
  pageHeader?: SafeHtml;
  workspace: SafeHtml;
  composer?: SafeHtml;
  inspector?: SafeHtml;
  inspectorVisible?: boolean;
  banner?: SafeHtml;
  sidebarSize?: number;
  inspectorSize?: number;
  mode?: ProjectTabBarMode;
  sidebarVisible?: boolean;
  workspacePresentation?: 'inset' | 'edge-to-edge';
}

export function AppShell({ tabs, sidebar, header, headerActions, pageHeader, workspace, composer, inspector, inspectorVisible = true, banner, sidebarSize = 272, inspectorSize = 352, mode = 'project', sidebarVisible = true, workspacePresentation = 'inset' }: AppShellProps) {
  return <section class="app-shell" data-component="app-shell" data-mode={mode} data-sidebar-visible={String(sidebarVisible)}>
    {mode === 'project' && <ResizableRegion id="app-sidebar" label="Project sidebar" size={sidebarSize} min={250} max={360} collapsed={!sidebarVisible}>{sidebar}</ResizableRegion>}
    <main class="app-shell__main">
      <Toolbar
        leading={<>{mode === 'project' && !sidebarVisible && <ToolbarControlGroup appearance="borderless" single><button type="button" data-action="toggle-project-sidebar" aria-label="Show project sidebar" title="Show project sidebar"><LucideIcon icon={PanelLeftOpen} name="panel-left-open" /></button></ToolbarControlGroup>}{header}</>}
        trailing={<>{headerActions}{mode === 'project' && inspector && !inspectorVisible && <ToolbarControlGroup appearance="borderless" single><button type="button" data-action="open-ticket-inspector" aria-label="Show ticket inspector" title="Show ticket inspector"><LucideIcon icon={PanelRightOpen} name="panel-right-open" /></button></ToolbarControlGroup>}</>}
      />
      <ProjectTabBar tabs={tabs} mode={mode} />
      {banner}
      {pageHeader}
      <div class="app-shell__work-area">
        {composer && <div class="app-shell__composer">{composer}</div>}
        <section class="app-shell__workspace" data-presentation={workspacePresentation} aria-label="Ticket workspace">{workspace}</section>
      </div>
    </main>
    {mode === 'project' && inspector && <ResizableRegion id="app-inspector" label="Ticket inspector" size={inspectorSize} min={280} max={520} edge="start" collapsed={!inspectorVisible}>{inspector}</ResizableRegion>}
  </section>;
}
