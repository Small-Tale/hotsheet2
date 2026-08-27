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
  workspace: SafeHtml;
  inspector?: SafeHtml;
  banner?: SafeHtml;
  sidebarSize?: number;
  inspectorSize?: number;
  mode?: ProjectTabBarMode;
}

export function AppShell({ tabs, sidebar, header, workspace, inspector, banner, sidebarSize = 272, inspectorSize = 352, mode = 'project' }: AppShellProps) {
  return <section class="app-shell" data-component="app-shell" data-mode={mode}>
    {mode === 'project' && <ResizableRegion id="app-sidebar" label="Project sidebar" size={sidebarSize} min={250} max={360}>{sidebar}</ResizableRegion>}
    <main class="app-shell__main">
      <ProjectTabBar tabs={tabs} mode={mode} />
      {banner}
      {header}
      <div class="app-shell__work-area">
        <section class="app-shell__workspace" aria-label="Ticket workspace">{workspace}</section>
        {mode === 'project' && inspector && <ResizableRegion id="app-inspector" label="Ticket inspector" size={inspectorSize} min={280} max={520} edge="start">{inspector}</ResizableRegion>}
      </div>
    </main>
  </section>;
}
