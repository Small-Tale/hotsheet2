import '@kerfjs/ui/floating-toolbar.css';
import './app-shell.css';

import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import type { ResizableRegionContentOverflow, ResizableRegionSeparator } from '@kerfjs/ui/resizable-region';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Workbench, type WorkbenchPanel } from '@kerfjs/ui/workbench';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { PanelBottomOpen, PanelRightOpen } from 'lucide';

import { APP_REGION_BOUNDS, TERMINAL_DRAWER_MIN_SIZE } from '../app-region-resize';
import type { ProjectTabProps } from './project-tab';
import type { ProjectTabBarMode } from './project-tab-bar';
import { ProjectTabBar } from './project-tab-bar';
import { type SidebarPanelParts, workbenchSidebarPanel } from './sidebar-panel';

/** The Workbench id; Kerf derives the panel ids `app-left-rail`, `app-right-rail`, and `app-bottom-drawer`. */
export const APP_WORKBENCH_ID = 'app';

export interface AppShellProps {
  tabs: ProjectTabProps[];
  /** The left rail's panel parts; the Workbench composes its toolbar and collapse toggle. */
  sidebar?: SidebarPanelParts;
  header: SafeHtml;
  headerActions?: SafeHtml;
  projectTabAction?: SafeHtml;
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
  /** Foreground anchored to the viewport, outside the mobile side-panel stacking contexts. */
  viewportOverlay?: SafeHtml;
  terminalDrawer?: SafeHtml;
  terminalDrawerVisible?: boolean;
  terminalDrawerSize?: number;
  terminalDrawerMax?: number;
  terminalDrawerTransitioning?: boolean;
  terminalFocusMode?: boolean;
  sidePanelSeparator?: ResizableRegionSeparator;
  terminalDrawerContentOverflow?: ResizableRegionContentOverflow;
}

/**
 * The application shell on Kerf's `Workbench` (HS2-P289N2): the project/operations sidebar is the
 * left rail, the ticket inspector or rail is the right rail, and the terminal drawer is the bottom
 * drawer, each collapsible and resizable through Kerf's own panel contract. The main column (shell
 * toolbar, project tab strip, banners, page header, and the ticket work area) stays app-owned. The
 * app keeps its own mobile presentation: side panels overlay the column when `mobile` is set and the
 * shell's click-away scrim dismisses them, so the Workbench's responsive overlay breakpoints stay off.
 */
export function AppShell({
  tabs,
  sidebar,
  header,
  headerActions,
  projectTabAction,
  pageHeader,
  workspace,
  composer,
  inspector,
  inspectorVisible = true,
  banner,
  sidebarSize = APP_REGION_BOUNDS['app-left-rail'].fallback,
  inspectorSize = APP_REGION_BOUNDS['app-right-rail'].fallback,
  mode = 'project',
  sidebarVisible = true,
  mobile = false,
  workspacePresentation = 'inset',
  overlay,
  viewportOverlay,
  terminalDrawer,
  terminalDrawerVisible = false,
  terminalDrawerSize = APP_REGION_BOUNDS['app-bottom-drawer'].fallback,
  terminalDrawerMax = 520,
  terminalDrawerTransitioning = false,
  terminalFocusMode = false,
  sidePanelSeparator = 'auto',
  terminalDrawerContentOverflow = 'clip',
}: AppShellProps) {
  const sidePanelPresentation = mobile ? 'overlay' : 'inline';
  const leftRail: WorkbenchPanel | undefined =
    mode !== 'stats' && sidebar
      ? {
          ...workbenchSidebarPanel(sidebar),
          // Rail names stay distinct from the landmarks inside them (the panel's content, the inspector).
          label: mode === 'terminals' ? 'Operations rail' : 'Sidebar rail',
          collapsed: !sidebarVisible,
          size: sidebarSize,
          resizable: { min: APP_REGION_BOUNDS['app-left-rail'].min, max: APP_REGION_BOUNDS['app-left-rail'].max },
          separator: sidePanelSeparator,
          collapseMotion: 'slide',
          presentation: sidePanelPresentation,
          responsiveOverlayAt: 'never',
        }
      : undefined;
  const rightRail: WorkbenchPanel | undefined =
    mode !== 'stats' && inspector
      ? {
          // The inspector surfaces render a Kerf Pane, which takes the safe-area insets the rail routes
          // to its content (HS2-RWGQWN).
          content: inspector,
          label: mode === 'terminals' ? 'Tickets rail' : 'Inspector rail',
          collapsed: !inspectorVisible,
          size: inspectorSize,
          resizable: { min: APP_REGION_BOUNDS['app-right-rail'].min, max: APP_REGION_BOUNDS['app-right-rail'].max },
          separator: sidePanelSeparator,
          collapseMotion: 'slide',
          presentation: sidePanelPresentation,
          responsiveOverlayAt: 'never',
        }
      : undefined;
  const bottomDrawer: WorkbenchPanel | undefined =
    mode === 'project' && terminalDrawer
      ? {
          content: terminalDrawer,
          label: 'Terminal drawer',
          collapsed: !terminalDrawerVisible,
          size: terminalDrawerSize,
          resizable: { min: TERMINAL_DRAWER_MIN_SIZE, max: Math.max(TERMINAL_DRAWER_MIN_SIZE, terminalDrawerMax) },
          separator: terminalFocusMode ? 'hidden' : 'auto',
          collapseMotion: 'fade-slide',
          contentOverflow: terminalFocusMode ? 'visible' : terminalDrawerContentOverflow,
          presentation: 'inline',
          responsiveOverlayAt: 'never',
          restorePosition: 'bottom-end',
          // The restore corner is Kerf's; the toolbar inside it is the app's control (no inset: the
          // corner owns it). It is withheld while the drawer animates so it never flashes mid-motion.
          restoreControl: !terminalDrawerTransitioning ? (
            <FloatingToolbar label="Terminal drawer controls" position="bottom-end">
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
          ) : undefined,
        }
      : undefined;
  return (
    <section
      class="app-shell"
      data-component="app-shell"
      data-mode={mode}
      data-mobile={String(mobile)}
      data-sidebar-visible={String(sidebarVisible)}
      data-terminal-focus-mode={String(terminalFocusMode)}
      data-terminal-drawer-transitioning={String(terminalDrawerTransitioning)}
    >
      <Workbench
        id={APP_WORKBENCH_ID}
        label="Hot Sheet workspace"
        leftRail={leftRail}
        rightRail={rightRail}
        bottomDrawer={bottomDrawer}
        // The app sizes its panels itself (the rails through their bounds, the drawer through its
        // measured maximum), so the work area keeps no Workbench-imposed minimum of its own.
        mainMinSize={0}
        mainMinHeight={0}
        // The shell toolbar is the Workbench's main toolbar, so the work area renders in a Kerf Pane
        // whose safe-area edges the app claims itself: on mobile the toolbar sits directly under the
        // (translucent) status bar and claims the top and inline screen edges (its controls clear the
        // unsafe area while its surface reaches the edge), and the ticket scrollers deeper in the
        // column carry the bottom inset as padding inside themselves (HS2-4A29RR). On desktop the
        // shell sits inside a window, where the insets are zero.
        mainPane={{ safeAreaEdges: ['block-start', 'inline-start', 'inline-end'], contentLabel: 'Workspace' }}
        mainToolbar={{
          label: 'Workspace toolbar',
          dividerSides: '',
          // An expanded search takes a full second row below the identity once the toolbar is narrow
          // (Kerf beta.60 trailing priority); wide toolbars reserve a bounded trailing track for it.
          responsive: 'trailing-priority',
          responsiveAt: 'narrow',
          safeAreaEdges: mobile ? ['block-start', 'inline-start', 'inline-end'] : undefined,
          // The collapsed left rail's toggle leads this zone; the Workbench relocates it here.
          leading: header,
          trailing: (
            <>
              {headerActions}
              {mode !== 'stats' && inspector && !inspectorVisible && (
                <ToolbarControlGroup appearance="borderless" single>
                  <button
                    type="button"
                    data-action="open-ticket-inspector"
                    aria-controls={`${APP_WORKBENCH_ID}-right-rail`}
                    aria-label={mode === 'terminals' ? 'Show ticket rail' : 'Show ticket inspector'}
                    title={mode === 'terminals' ? 'Show ticket rail' : 'Show ticket inspector'}
                  >
                    <LucideIcon icon={PanelRightOpen} name="panel-right-open" />
                  </button>
                </ToolbarControlGroup>
              )}
            </>
          ),
        }}
        main={
          <main class="app-shell__main" data-work-area-focus-owner tabIndex={-1}>
            <ProjectTabBar tabs={tabs} mode={mode} workspaceAction={projectTabAction} mobile={mobile} />
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
            {/* Inside the Workbench's stacking context, so it paints under its overlay rails (z 41) and
            over everything else in the main column. */}
            {mobile && (sidebarVisible || inspectorVisible) && (
              <div class="app-shell__scrim" data-action="dismiss-mobile-overlays" aria-hidden="true" />
            )}
          </main>
        }
      />
      {viewportOverlay}
    </section>
  );
}
