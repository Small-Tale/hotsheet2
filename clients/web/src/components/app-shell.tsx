import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { ResizableRegion } from './resizable-region';
import type { ProjectTabProps } from './project-tab';
import { ProjectTabBar } from './project-tab-bar';
import type { ProjectTabBarMode } from './project-tab-bar';
import './app-shell.css';

export interface AppShellProps {
  tabs: ProjectTabProps[];
  sidebar: SafeHtml;
  header: SafeHtml;
  pageHeader?: SafeHtml;
  workspace: SafeHtml;
  inspector?: SafeHtml;
  banner?: SafeHtml;
  sidebarSize?: number;
  inspectorSize?: number;
  mode?: ProjectTabBarMode;
  sidebarVisible?: boolean;
  workspacePresentation?: 'inset' | 'edge-to-edge';
}

export function AppShell({ tabs, sidebar, header, pageHeader, workspace, inspector, banner, sidebarSize = 272, inspectorSize = 352, mode = 'project', sidebarVisible = true, workspacePresentation = 'inset' }: AppShellProps) {
  return <section class="app-shell" data-component="app-shell" data-mode={mode} data-sidebar-visible={String(sidebarVisible)}>
    {mode === 'project' && <ResizableRegion id="app-sidebar" label="Project sidebar" size={sidebarSize} min={250} max={360} collapsed={!sidebarVisible}>{sidebar}</ResizableRegion>}
    <main class="app-shell__main">
      {header}
      <ProjectTabBar tabs={tabs} mode={mode} sidebarVisible={sidebarVisible} />
      {banner}
      {pageHeader}
      <div class="app-shell__work-area">
        <section class="app-shell__workspace" data-presentation={workspacePresentation} aria-label="Ticket workspace">{workspace}</section>
      </div>
    </main>
    {mode === 'project' && inspector && <ResizableRegion id="app-inspector" label="Ticket inspector" size={inspectorSize} min={280} max={520} edge="start">{inspector}</ResizableRegion>}
  </section>;
}
