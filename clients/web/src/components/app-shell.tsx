import '@kerfjs/ui/floating-toolbar.css';
import './app-shell.css';

import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ResizableRegion } from '@kerfjs/ui/resizable-region';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { PanelBottomOpen, PanelLeftOpen, PanelRightOpen } from 'lucide';

import { TERMINAL_DRAWER_MIN_SIZE } from '../app-region-resize';
import type { ProjectTabProps } from './project-tab';
import type { ProjectTabBarMode } from './project-tab-bar';
import { ProjectTabBar } from './project-tab-bar';

export interface AppShellProps {
  tabs: ProjectTabProps[];
  sidebar?: SafeHtml;
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
  /** Mobile single-column layout: sidebar/inspector overlay the main column and a click-away
   * scrim dismisses whichever one is open (only one is ever open at a time — HS2-ZK51WP). */
  mobile?: boolean;
  workspacePresentation?: 'inset' | 'edge-to-edge';
  overlay?: SafeHtml;
  terminalDrawer?: SafeHtml;
  terminalDrawerVisible?: boolean;
  terminalDrawerSize?: number;
  terminalDrawerMax?: number;
  terminalDrawerTransitioning?: boolean;
}

export function AppShell({
  tabs,
  sidebar,
  header,
  headerActions,
  pageHeader,
  workspace,
  composer,
  inspector,
  inspectorVisible = true,
  banner,
  sidebarSize = 272,
  inspectorSize = 352,
  mode = 'project',
  sidebarVisible = true,
  mobile = false,
  workspacePresentation = 'inset',
  overlay,
  terminalDrawer,
  terminalDrawerVisible = false,
  terminalDrawerSize = 320,
  terminalDrawerMax = 520,
  terminalDrawerTransitioning = false,
}: AppShellProps) {
  return (
    <section
      class="app-shell"
      data-component="app-shell"
      data-mode={mode}
      data-mobile={String(mobile)}
      data-sidebar-visible={String(sidebarVisible)}
    >
      {mode !== 'stats' && sidebar && (
        <ResizableRegion
          id="app-sidebar"
          label={mode === 'terminals' ? 'Operations sidebar' : 'Project sidebar'}
          size={sidebarSize}
          min={250}
          max={360}
          collapsed={!sidebarVisible}
        >
          {sidebar}
        </ResizableRegion>
      )}
      <main class="app-shell__main" data-work-area-focus-owner tabIndex={-1}>
        <Toolbar
          divider={false}
          leading={
            <>
              {mode !== 'stats' && sidebar && !sidebarVisible && (
                <ToolbarControlGroup appearance="borderless" single>
                  <button
                    type="button"
                    data-action="toggle-project-sidebar"
                    aria-label={mode === 'terminals' ? 'Show operations sidebar' : 'Show project sidebar'}
                    title={mode === 'terminals' ? 'Show operations sidebar' : 'Show project sidebar'}
                  >
                    <LucideIcon icon={PanelLeftOpen} name="panel-left-open" />
                  </button>
                </ToolbarControlGroup>
              )}
              {header}
            </>
          }
          trailing={
            <>
              {headerActions}
              {mode !== 'stats' && inspector && !inspectorVisible && (
                <ToolbarControlGroup appearance="borderless" single>
                  <button
                    type="button"
                    data-action="open-ticket-inspector"
                    aria-label={mode === 'terminals' ? 'Show ticket rail' : 'Show ticket inspector'}
                    title={mode === 'terminals' ? 'Show ticket rail' : 'Show ticket inspector'}
                  >
                    <LucideIcon icon={PanelRightOpen} name="panel-right-open" />
                  </button>
                </ToolbarControlGroup>
              )}
            </>
          }
        />
        <ProjectTabBar tabs={tabs} mode={mode} mobile={mobile} />
        {overlay}
        {banner}
        {pageHeader}
        {/* Stable data-keys so the morph matches the scroll container by identity, not position.
          The overlay/banner/pageHeader siblings above are conditional (HS2-H4MWDB: opening the
          ticket context menu toggles the overlay); without a key the shift rebuilds this subtree
          and the workspace loses its scrollTop. */}
        <div
          class="app-shell__work-area"
          data-key="app-shell-work-area"
          data-has-composer={String(Boolean(composer))}
          tabIndex={0}
          aria-label="Ticket work area"
        >
          {composer && <div class="app-shell__composer">{composer}</div>}
          <section
            class="app-shell__workspace"
            data-key="app-shell-workspace"
            data-ticket-scroll-owner="workspace"
            data-presentation={workspacePresentation}
            aria-label="Ticket workspace"
          >
            {workspace}
          </section>
        </div>
        {mode === 'project' && terminalDrawer && (
          <ResizableRegion
            id="app-terminal-drawer"
            label="Terminal drawer"
            size={terminalDrawerSize}
            min={TERMINAL_DRAWER_MIN_SIZE}
            max={terminalDrawerMax}
            axis="vertical"
            edge="start"
            collapsed={!terminalDrawerVisible}
            transitioning={terminalDrawerTransitioning}
          >
            {terminalDrawer}
          </ResizableRegion>
        )}
        {mode === 'project' && terminalDrawer && !terminalDrawerVisible && !terminalDrawerTransitioning && (
          <FloatingToolbar
            label="Terminal drawer controls"
            position="bottom-end"
            className="app-shell__terminal-drawer-restore"
          >
            <ToolbarControlGroup single>
              <button
                type="button"
                data-action="toggle-terminal-drawer"
                aria-label="Show terminal drawer"
                title="Show terminal drawer"
              >
                <LucideIcon icon={PanelBottomOpen} name="panel-bottom-open" />
              </button>
            </ToolbarControlGroup>
          </FloatingToolbar>
        )}
      </main>
      {mobile && (sidebarVisible || inspectorVisible) && (
        <div class="app-shell__scrim" data-action="dismiss-mobile-overlays" aria-hidden="true" />
      )}
      {mode !== 'stats' && inspector && (
        <ResizableRegion
          id="app-inspector"
          label={mode === 'terminals' ? 'Ticket rail' : 'Ticket inspector'}
          size={inspectorSize}
          min={280}
          max={520}
          edge="start"
          collapsed={!inspectorVisible}
        >
          {inspector}
        </ResizableRegion>
      )}
    </section>
  );
}
