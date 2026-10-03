import '@kerfjs/ui/floating-toolbar.css';
import './app-shell.css';

import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import type { ResizableRegionContentOverflow, ResizableRegionSeparator } from '@kerfjs/ui/resizable-region';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Workbench, type WorkbenchPanel, type WorkbenchStaticPanel } from '@kerfjs/ui/workbench';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { PanelBottomOpen } from 'lucide';

import { APP_REGION_BOUNDS, TERMINAL_DRAWER_MIN_SIZE } from '../app-region-resize';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import type { ProjectTabProps } from './project-tab';
import type { ProjectTabBarMode } from './project-tab-bar';
import { ProjectTabBar } from './project-tab-bar';
import {
  type RailPanelParts,
  type SidebarPanelParts,
  workbenchRailPanel,
  workbenchSidebarPanel,
} from './sidebar-panel';
import type { TicketViewActionSpec } from './workspace-controls';

/** The Workbench id; Kerf derives the panel ids `app-left-rail`, `app-right-rail`, and `app-bottom-drawer`. */
export const APP_WORKBENCH_ID = 'app';

export interface AppShellProps {
  tabs: ProjectTabProps[];
  /** The left rail's panel parts; the Workbench composes its toolbar and collapse toggle. */
  sidebar?: SidebarPanelParts;
  header: SafeHtml;
  headerActions?: SafeHtml;
  /** The current ticket view's primary action, rendered in the project strip's far-edge zone (HS2-PNCDAE). */
  projectTabAction?: TicketViewActionSpec;
  pageHeader?: SafeHtml;
  workspace: SafeHtml;
  composer?: SafeHtml;
  /** The right rail's panel parts; the Workbench composes its toolbar, header, and collapse toggle and
   * relocates the toggle to the trailing edge of the workspace toolbar while the rail is collapsed. */
  inspector?: RailPanelParts;
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
  /** The drawer's usable minimum; a phone grows it by the home-indicator inset it pads (HS2-ZEC4QV). */
  terminalDrawerMin?: number;
  terminalDrawerTransitioning?: boolean;
  terminalFocusMode?: boolean;
  sidePanelSeparator?: ResizableRegionSeparator;
  terminalDrawerContentOverflow?: ResizableRegionContentOverflow;
  /** `framed` (default) draws the shell as a bordered window at a desktop size floor, as the UX demo
   * stages it; `viewport` fills its container edge to edge, as the application root renders it. */
  presentation?: 'framed' | 'viewport';
  /** Draw the work area's focus ring. Off while a top-layer overlay that the work area hosts (the
   * magnified terminal) has focus, so the ring never shows through that overlay's scrim. */
  workAreaFocusRing?: boolean;
}

/**
 * The application shell on Kerf's `Workbench` (HS2-P289N2): the project/operations sidebar is the
 * left rail, the ticket inspector or rail is the right rail, and the terminal drawer is the bottom
 * drawer, each collapsible and resizable through Kerf's own panel contract. The main column (shell
 * toolbar, project tab strip, banners, page header, and the ticket work area) stays app-owned. The
 * app keeps its own mobile breakpoint (1024px of viewport, wider than the Workbench's 704px/448px
 * container breakpoints), so side panels present as overlays when `mobile` is set and the Workbench's
 * responsive breakpoints stay off; `wireWorkbench` owns the overlays' exclusivity, Escape and
 * outside-press dismissal, focus trap, and focus return through the rails' `collapsed` signals
 * (HS2-Y1B1Y1). The shell's backdrop only dims the column and swallows the tap, which Kerf reads as an
 * outside press.
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
  terminalDrawerMin = TERMINAL_DRAWER_MIN_SIZE,
  terminalDrawerTransitioning = false,
  terminalFocusMode = false,
  sidePanelSeparator = 'auto',
  terminalDrawerContentOverflow = 'clip',
  presentation = 'framed',
  workAreaFocusRing = true,
}: AppShellProps) {
  const sidePanelPresentation = mobile ? 'overlay' : 'inline';
  const terminalDrawerExpanded = mode === 'project' && Boolean(terminalDrawer) && terminalDrawerVisible;
  const leftRail: WorkbenchStaticPanel | undefined =
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
          // Every right-rail surface exposes panel parts (HS2-QQW6CT): the Workbench composes the
          // toolbar, the fixed header, and the scrolling content in one Pane, which takes the
          // safe-area insets the rail routes to its content. The terminal ticket rail navigates: its
          // NavStack's active view supplies that toolbar row, header, and content (HS2-FY06N4).
          ...workbenchRailPanel(inspector),
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
  const bottomDrawer: WorkbenchStaticPanel | undefined =
    mode === 'project' && terminalDrawer
      ? {
          content: terminalDrawer,
          label: 'Terminal drawer',
          collapsed: !terminalDrawerVisible,
          size: terminalDrawerSize,
          resizable: { min: terminalDrawerMin, max: Math.max(terminalDrawerMin, terminalDrawerMax) },
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
                  {...TERMINALS_ACTIONS.toggleTerminalDrawer.attrs}
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
      data-presentation={presentation}
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
          // The collapsed left rail's toggle leads this zone and the collapsed right rail's toggle
          // trails it; the Workbench relocates both here.
          leading: header,
          trailing: headerActions,
        }}
        main={
          <main class="app-shell__main" data-work-area-focus-owner tabIndex={-1}>
            {/* The tab strip shares the main column's surface; in terminals mode no page header sits
                between it and the work area, so it draws the separator itself (HS2-WH6CCR). */}
            <ProjectTabBar
              tabs={tabs}
              mode={mode}
              workspaceAction={projectTabAction}
              mobile={mobile}
              surface="default"
              divider={mode === 'terminals'}
            />
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
              data-focus-ring={String(workAreaFocusRing)}
              tabIndex={0}
              aria-label="Ticket work area"
            >
              {composer && <div class="app-shell__composer">{composer}</div>}
              <section
                class="app-shell__workspace"
                data-key="app-shell-workspace"
                data-ticket-scroll-owner="workspace"
                data-presentation={workspacePresentation}
                // A phone column reaches the screen's bottom edge unless the expanded terminal drawer
                // owns that edge; its ticket scrollers then inset their content (HS2-4A29RR).
                data-bottom-edge={String(mobile && !terminalDrawerExpanded)}
                aria-label="Ticket workspace"
              >
                {workspace}
              </section>
            </div>
            {/* Passive backdrop inside the Workbench's stacking context, so it paints under its overlay
            rails (z 41) and over everything else in the main column; Kerf's Workbench overlays have no
            backdrop of their own, so this dims the column and keeps a tap from reaching the control
            beneath it while `wireWorkbench` treats that tap as the outside press that closes the rail. */}
            {mobile && (sidebarVisible || inspectorVisible) && <div class="app-shell__scrim" aria-hidden="true" />}
          </main>
        }
      />
      {viewportOverlay}
    </section>
  );
}
