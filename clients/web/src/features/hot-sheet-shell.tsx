import type { TokenSearchModel } from '@kerfjs/ui/token-search-model';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import type { Signal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

import {
  type Capabilities,
  type CheckoutTicketCounts,
  type CorruptTicket,
  type CustomView,
  type FullTicket,
  type TicketRow as WireTicketRow,
} from '../api';
import { type AppRegionId } from '../app-region-resize';
import { AppEmptyState, ProjectRestoreState } from '../components/app-empty-state';
import { type AppShellMobileView } from '../components/app-shell';
import { CodexHooksNoticeBanner } from '../components/codex-hooks-notice-banner';
import type { CorruptTicketRecoveryState } from '../components/corrupt-ticket-row';
import { corruptTicketKey } from '../components/corrupt-ticket-row';
import {
  Hs1CleanupBanner,
  Hs1JobBanner,
  Hs1MigrationBanner,
  Hs1RegistrationRepairBanner,
} from '../components/hs1-migration';
import { MainShell } from '../components/main-shell';
import { type NotificationView, notificationViewTitle } from '../components/notification-navigation';
import { NotificationsPausedBanner } from '../components/notifications-paused-banner';
import type { ProjectRestoreFailure } from '../components/project-restore-error';
import { ProjectRestoreError, projectRestoreTabId } from '../components/project-restore-error';
import { ProjectSetupWarningBanner } from '../components/project-setup-warning-banner';
import type { ProjectStripMode } from '../components/project-strip-actions';
import { type ProjectTabProps } from '../components/project-tab';
import { CompatibilityBannerSurface } from '../components/reader-overlay-surfaces';
import { type SettingsCategory, settingsCategoryTitle } from '../components/settings-navigation';
import { TerminalDashboardControls, type TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { TerminalDrawerProps } from '../components/terminal-drawer';
import type { TicketInspectorProps } from '../components/ticket-inspector';
import {
  corruptInspectorPanel,
  inspectorPanel,
  inspectorPlaceholderPanel,
  inspectorSkeletonPanel,
  notificationInspectorSurfacePanel,
} from '../components/ticket-inspector-surface';
import type {
  GlobalWorkspaceSurfaceProps,
  SidebarSurfaceProps,
  TerminalOperationsSurfaceProps,
  TerminalRailSurfaceProps,
  WorkspaceSurfaceProps,
} from '../components/workspace-composition-surfaces';
import {
  GlobalWorkspaceSurface,
  ProjectTerminalDrawerSurface,
  sidebarSurfacePanel,
  statsInspectorPanel,
  statsSidebarPanel,
  terminalOperationsSurfacePanel,
  terminalRailSurfacePanel,
  WorkspaceSurface,
} from '../components/workspace-composition-surfaces';
import type { TicketViewActionSpec } from '../components/workspace-controls';
import {
  TicketViewAction,
  WorkspaceControls,
  WorkspaceIdentity,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  workspaceUpNextState,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { type Project } from '../interactions/types';
import type { MigrationJob } from '../migration-progress';
import { migrationPercent, migrationPhaseLabel } from '../migration-progress';
import type { MobileTerminalFocusState } from '../mobile-terminal-focus';
import { mobileViewChoices } from '../mobile-view-choices';
import type { PermissionItem } from '../permission-notifications';
import { interleaveByRank } from '../tab-order';
import type { TerminalVisibilityState } from '../terminal-visibility';
import { activeTerminalVisibilityGroup, TERMINAL_DASHBOARD_VISIBILITY_SCOPE } from '../terminal-visibility';
import { canBulkUpdate } from '../ticket-bulk-operations';
import { canCreateTicketInView, customTicketViewId, type TicketView } from '../ticket-views';
import { hs1CleanupPromptDismissed } from '../workspace-session';

/** Live application bindings createHotSheetShell reads (HS2-K0VC2C). */
export interface HotSheetShellDependencies {
  appRegionSize: (id: AppRegionId) => number;
  capabilitiesFor: (connectionId: string) => Capabilities;
  conversationOpen: Signal<boolean>;
  corruptRecovery: Signal<Record<string, CorruptTicketRecoveryState>>;
  corruptTickets: Signal<CorruptTicket[]>;
  customViewsFor: (projectId?: string) => CustomView[];
  globalWorkspaceSurfaceProps: () => GlobalWorkspaceSurfaceProps;
  haltedSessionsController: { popup: () => SafeHtml | undefined; waiting: () => number; dispose: () => void };
  hs1MigrationProject: Signal<Project | undefined>;
  hs1SourceIdentity: (value: Project) => string;
  initialProjectRestorePending: Signal<boolean>;
  inspectorCollapsed: Signal<boolean>;
  inspectorSize: Signal<number>;
  magnifiedTerminalKey: Signal<string | undefined>;
  migrationConnectionErrors: Signal<Record<string, string>>;
  migrationJobDetails: Signal<Record<string, boolean>>;
  migrationJobsByRoot: Signal<Partial<Record<string, MigrationJob>>>;
  mobileTerminalFocus: Signal<MobileTerminalFocusState>;
  notificationsPaused: Signal<boolean>;
  notificationView: Signal<NotificationView>;
  partialSourcesByProject: Signal<Record<string, boolean>>;
  pendingPermissions: () => PermissionItem[];
  pendingRestoreEntries: <T>(item: (root: string) => T) => { rank: number; item: T }[];
  permissionCount: (projectId?: string) => number;
  permissionPopupSurface: (layer?: 'flow' | 'top') => SafeHtml | undefined;
  project: () => Project | undefined;
  projectRestoreFailures: Signal<ProjectRestoreFailure[]>;
  projectRestoreRank: (id: string) => number | undefined;
  projects: Signal<Project[]>;
  projectTabClaimClock: Signal<number>;
  projectTerminalDrawerProps: () => TerminalDrawerProps | undefined;
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  searchHelpOpen: Signal<boolean>;
  searchOpen: Signal<boolean>;
  selectedCorruptKey: Signal<string | undefined>;
  selectedInspectorProps: () => TicketInspectorProps | undefined;
  previewInspectorProps: (row: WireTicketRow) => TicketInspectorProps | undefined;
  selectedProjectId: Signal<string>;
  selectedProjectRestoreRoot: Signal<string>;
  selectedRows: () => WireTicketRow[];
  selectedTicket: Signal<FullTicket | null>;
  selectedTicketSlugs: Signal<string[]>;
  selectedView: Signal<TicketView>;
  settingsCategory: () => SettingsCategory;
  shellMode: Signal<ProjectStripMode>;
  sidebarCollapsed: Signal<boolean>;
  sidebarSize: Signal<number>;
  sidebarSurfaceProps: () => SidebarSurfaceProps;
  sort: { value: WorkspaceSort };
  sortDirection: { value: WorkspaceSortDirection };
  terminalDrawerMax: Signal<number>;
  terminalDrawerMin: Signal<number>;
  terminalDrawerMounted: Signal<boolean>;
  terminalDrawerSelected: Signal<string>;
  terminalDrawerTransitioning: Signal<boolean>;
  terminalDrawerVisible: Signal<boolean>;
  terminalGridFullscreen: Signal<boolean>;
  terminalGroups: Signal<TerminalDashboardGroup[]>;
  terminalHiddenCount: (scope: string, projectId?: string) => number;
  terminalOperationsSurfaceProps: () => TerminalOperationsSurfaceProps;
  terminalRailSurfaceProps: () => TerminalRailSurfaceProps;
  terminalVisibility: Signal<TerminalVisibilityState>;
  ticketContextMenuSurface: () => SafeHtml;
  ticketViewActionSpec: (
    view: TicketView,
    canCreate: boolean,
    label?: string,
    size?: 'default' | 'compact',
  ) => TicketViewActionSpec | undefined;
  ticketViewTitle: (view: TicketView) => string;
  viewMode: Signal<WorkspaceViewMode>;
  viewportMobile: Signal<boolean>;
  workspaceSearchModel: TokenSearchModel;
  workspaceSurfaceProps: () => WorkspaceSurfaceProps;
}

/**
 * The application shell: project tabs, sidebar, workspace, terminal rail and drawer, dialogs, and
 * popups composed into one Workbench. Extracted from the application runtime without behavior
 * changes (HS2-K0VC2C).
 */
export function createHotSheetShell(dependencies: HotSheetShellDependencies) {
  const {
    appRegionSize,
    capabilitiesFor,
    conversationOpen,
    corruptRecovery,
    corruptTickets,
    customViewsFor,
    globalWorkspaceSurfaceProps,
    haltedSessionsController,
    hs1MigrationProject,
    hs1SourceIdentity,
    initialProjectRestorePending,
    inspectorCollapsed,
    inspectorSize,
    magnifiedTerminalKey,
    migrationConnectionErrors,
    migrationJobDetails,
    migrationJobsByRoot,
    mobileTerminalFocus,
    notificationsPaused,
    notificationView,
    partialSourcesByProject,
    pendingPermissions,
    pendingRestoreEntries,
    permissionCount,
    permissionPopupSurface,
    project,
    projectRestoreFailures,
    projectRestoreRank,
    projects,
    projectTabClaimClock,
    projectTerminalDrawerProps,
    projectTicketCounts,
    searchHelpOpen,
    searchOpen,
    selectedCorruptKey,
    selectedInspectorProps,
    previewInspectorProps,
    selectedProjectId,
    selectedProjectRestoreRoot,
    selectedRows,
    selectedTicket,
    selectedTicketSlugs,
    selectedView,
    settingsCategory,
    shellMode,
    sidebarCollapsed,
    sidebarSize,
    sidebarSurfaceProps,
    sort,
    sortDirection,
    terminalDrawerMax,
    terminalDrawerMin,
    terminalDrawerMounted,
    terminalDrawerSelected,
    terminalDrawerTransitioning,
    terminalDrawerVisible,
    terminalGridFullscreen,
    terminalGroups,
    terminalHiddenCount,
    terminalOperationsSurfaceProps,
    terminalRailSurfaceProps,
    terminalVisibility,
    ticketContextMenuSurface,
    ticketViewActionSpec,
    ticketViewTitle,
    viewMode,
    viewportMobile,
    workspaceSearchModel,
    workspaceSurfaceProps,
  } = dependencies;
  function HotSheetShell() {
    if (initialProjectRestorePending.value) return <ProjectRestoreState />;
    const current = project(),
      restoreFailure = projectRestoreFailures.value.find((item) => item.root === selectedProjectRestoreRoot.value);
    if (!current && !restoreFailure) return <AppEmptyState />;
    // The shell popup is a top-layer popover so the terminal drawer and the Workbench's main-pane clip
    // can never cover it (HS2-ZESCM2); the conversation's foreground keeps its own inline copy.
    const haltPopup = haltedSessionsController.popup(),
      popup = (conversationOpen.value ? undefined : permissionPopupSurface('top')) ?? haltPopup,
      currentJob = current && migrationJobsByRoot.value[current.root],
      currentBackupUnverified = Boolean(
        currentJob?.kind === 'backup' &&
        currentJob.status === 'succeeded' &&
        current?.hs1DatabasePath &&
        !current.hs1CleanupEligible,
      );
    void projectTabClaimClock.value;
    const projectTabs = projects.value.map((item) => {
        const counts = projectTicketCounts(item.id),
          job = migrationJobsByRoot.value[item.root],
          backupUnverified = Boolean(
            job?.kind === 'backup' && job.status === 'succeeded' && item.hs1DatabasePath && !item.hs1CleanupEligible,
          );
        return {
          id: item.id,
          name: item.name,
          location: 'local' as const,
          selected: !restoreFailure && item.id === selectedProjectId.value,
          notificationCount: permissionCount(item.id),
          // A halted session or unanswered AI question in this project's terminals needs the user.
          ...(terminalGroups.value.some(
            (group) =>
              group.projectId === item.id &&
              group.sessions.some((session) => Boolean(session.halt || session.question)),
          )
            ? { attention: true, attentionLabel: 'An AI session needs your attention' }
            : {}),
          upNextCount: counts.up_next,
          activeTicketCount: counts.active,
          countsPartial: partialSourcesByProject.value[item.id] ?? false,
          operation:
            job && (!(job.kind === 'backup' && job.status === 'succeeded') || backupUnverified)
              ? {
                  label: `${item.name}: ${job.status === 'running' ? migrationPhaseLabel(job.progress) : job.status === 'succeeded' && !backupUnverified ? 'Import complete; backup needed' : 'Migration needs attention'}`,
                  state: backupUnverified ? ('failed' as const) : job.status,
                  percent: job.status === 'running' ? migrationPercent(job.progress) : undefined,
                }
              : undefined,
        };
      }),
      // Still-opening remembered projects sit at their remembered positions as dormant placeholders
      // until they register or fail (HS2-2BEJXD).
      rankedProjectTabs = interleaveByRank<{ tab: ProjectTabProps; rank?: number }>(
        projectTabs.map((tab) => ({ tab, rank: projectRestoreRank(tab.id) })),
        (entry) => entry.rank,
        pendingRestoreEntries((root) => ({
          tab: {
            id: `pending:${root}`,
            name: root.split('/').filter(Boolean).at(-1) ?? root,
            location: 'local' as const,
            pending: true,
          },
        })),
      ).map((entry) => entry.tab);
    const tabs = [
      ...rankedProjectTabs,
      ...projectRestoreFailures.value.map((item) => ({
        id: projectRestoreTabId(item.root),
        name: item.name,
        location: 'local' as const,
        selected: item.root === restoreFailure?.root,
        attention: true,
        restoreFailure: true,
        closable: false,
        draggable: false,
      })),
    ];
    if (shellMode.value !== 'project') {
      const active = activeTerminalVisibilityGroup(terminalVisibility.value, TERMINAL_DASHBOARD_VISIBILITY_SCOPE);
      return (
        <MainShell
          tabs={tabs}
          mode={shellMode.value}
          gridFullscreen={shellMode.value === 'terminals' && terminalGridFullscreen.value}
          mobile={viewportMobile.value}
          sidebar={
            shellMode.value === 'terminals'
              ? terminalOperationsSurfacePanel(terminalOperationsSurfaceProps())
              : statsSidebarPanel()
          }
          sidebarVisible={!sidebarCollapsed.value}
          sidebarSize={sidebarSize.value}
          header={<WorkspaceIdentity projectName={shellMode.value === 'terminals' ? 'Workspace Grid' : 'Stats'} />}
          headerActions={
            shellMode.value === 'terminals' ? (
              <TerminalDashboardControls
                hiddenCount={terminalHiddenCount(TERMINAL_DASHBOARD_VISIBILITY_SCOPE)}
                visibilityGroups={terminalVisibility.value.groups}
                activeVisibilityGroupId={active.id}
                visibilityScope={TERMINAL_DASHBOARD_VISIBILITY_SCOPE}
              />
            ) : undefined
          }
          workspace={<GlobalWorkspaceSurface {...globalWorkspaceSurfaceProps()} />}
          workspacePresentation="edge-to-edge"
          inspector={
            shellMode.value === 'terminals'
              ? terminalRailSurfacePanel(terminalRailSurfaceProps())
              : statsInspectorPanel()
          }
          inspectorVisible={!inspectorCollapsed.value}
          inspectorSize={inspectorSize.value}
          sidePanelSeparator={magnifiedTerminalKey.value ? 'hidden' : 'auto'}
          workAreaFocusRing={!magnifiedTerminalKey.value}
          overlay={viewportMobile.value ? undefined : popup}
          viewportOverlay={viewportMobile.value ? popup : undefined}
        />
      );
    }
    if (restoreFailure)
      return (
        <MainShell
          tabs={tabs}
          mode="project"
          header={<WorkspaceIdentity projectName="Project unavailable" id="workspace-page-title" headingLevel={1} />}
          workspace={<ProjectRestoreError {...restoreFailure} />}
          inspectorVisible={false}
          overlay={viewportMobile.value ? undefined : popup}
          viewportOverlay={viewportMobile.value ? popup : undefined}
        />
      );
    if (!current) return <AppEmptyState />;
    const selection = selectedRows(),
      selectedSlug = selectedTicketSlugs.value.length === 1 ? selectedTicketSlugs.value[0] : undefined,
      selectedReady = Boolean(selectedSlug && selectedTicket.value?.slug === selectedSlug),
      selectedTransitioning = Boolean(selectedSlug && selectedTicket.value && !selectedReady),
      // An uncached ticket renders from its list row while the full detail loads (HS2-15833W).
      previewRow =
        selectedSlug && !selectedReady ? selection.find((ticket) => ticket.slug === selectedSlug) : undefined,
      inspectorProps = selectedReady
        ? selectedInspectorProps()
        : previewRow
          ? previewInspectorProps(previewRow)
          : undefined,
      corruptKey = selectedCorruptKey.value,
      corruptTicket = corruptKey
        ? corruptTickets.value.find((item) => corruptTicketKey(item) === corruptKey)
        : undefined,
      canCreate = !['settings', 'notifications'].includes(viewMode.value) && canCreateTicketInView(selectedView.value),
      // The picker names the visible view, Ticket errors included, mirroring the sidebar (HS2-0VPMFS).
      mobileChoices = mobileViewChoices({
        selectedView: selectedView.value,
        trashCount: projectTicketCounts(current.id).trash ?? 0,
        corruptCount: corruptTickets.value.length,
        customViews: customViewsFor(current.id).map((view) => ({
          value: customTicketViewId(view.id),
          label: view.name,
        })),
      });
    const workspaceTitle = (() => {
      switch (viewMode.value) {
        case 'notifications':
          return notificationViewTitle(notificationView.value);
        case 'settings':
          return settingsCategoryTitle(settingsCategory());
        case 'board':
        case 'list':
          // Searching filters the selected view; the heading keeps that view's title (HS2-18Y67S).
          return ticketViewTitle(selectedView.value);
        default:
          return viewMode.value satisfies never;
      }
    })();
    // Settings pages put their page-level add actions where ticket views put New ticket…
    // (HS2-G9J35B, HS2-YKB7HS).
    const settingsAction: TicketViewActionSpec | undefined =
      viewMode.value !== 'settings'
        ? undefined
        : settingsCategory() === 'sources'
          ? { kind: 'add-data-source' }
          : settingsCategory() === 'commands'
            ? { kind: 'command-settings', size: viewportMobile.value ? 'compact' : 'default' }
            : undefined;
    const headingAction = !['settings', 'notifications'].includes(viewMode.value)
      ? ticketViewActionSpec(selectedView.value, canCreate)
      : settingsAction;
    // Heading Toolbars render the view action as literal `trailing` content: Kerf Toolbar accepts one
    // standalone primary wa-button there (KF-9N7K8S, HS2-F4P7ZA).
    const secondaryPageHeader = (
      <Toolbar
        dividerSides=""
        leading={<ToolbarText text={workspaceTitle} id="workspace-page-title" size="xlarge" headingLevel={1} />}
        trailing={headingAction && <TicketViewAction action={headingAction} />}
      />
    );
    const mobileView: AppShellMobileView | undefined =
        viewportMobile.value && !['settings', 'notifications'].includes(viewMode.value)
          ? {
              value: selectedView.value,
              choices: mobileChoices,
              action: ticketViewActionSpec(selectedView.value, canCreate),
            }
          : undefined,
      pageHeader = viewportMobile.value && !mobileView ? secondaryPageHeader : undefined;
    const drawerViewAllowed = !['settings', 'notifications'].includes(viewMode.value);
    const inspectorContent = (() => {
      switch (viewMode.value) {
        case 'notifications':
          return notificationInspectorSurfacePanel();
        case 'settings':
          return inspectorPlaceholderPanel({ selectionCount: 0 });
        case 'board':
        case 'list':
          if (corruptKey)
            return corruptInspectorPanel({
              ticket: corruptTicket,
              recovery: corruptRecovery.value[corruptKey],
              selectionCount: selectedTicketSlugs.value.length,
            });
          if (inspectorProps) return inspectorPanel(inspectorProps);
          if (selectedTransitioning) return inspectorSkeletonPanel({ slug: selectedSlug });
          return inspectorPlaceholderPanel({ selectionCount: selectedTicketSlugs.value.length });
        default:
          return viewMode.value satisfies never;
      }
    })();
    return (
      <MainShell
        tabs={tabs}
        mode="project"
        workspaceSearchOpen={searchOpen.value && (viewMode.value === 'list' || viewMode.value === 'board')}
        mobile={viewportMobile.value}
        sidebar={sidebarSurfacePanel(sidebarSurfaceProps())}
        sidebarVisible={!sidebarCollapsed.value}
        sidebarSize={sidebarSize.value}
        header={
          viewportMobile.value ? (
            <></>
          ) : (
            <WorkspaceIdentity projectName={workspaceTitle} id="workspace-page-title" headingLevel={1} />
          )
        }
        headerActions={
          <WorkspaceControls
            mode={viewMode.value}
            searchOpen={searchOpen.value && (viewMode.value === 'list' || viewMode.value === 'board')}
            searchModel={workspaceSearchModel}
            searchHelpOpen={searchHelpOpen.value}
            sort={sort.value}
            sortDirection={sortDirection.value}
            notificationCount={permissionCount(current.id)}
            selectedTicketCount={selection.length}
            selectedTicketsUpNext={workspaceUpNextState(selection.map((ticket) => ticket.up_next))}
            selectedTicketsUpNextEligible={
              selection.length > 0 &&
              selection.every((ticket) => ticket.status === 'not_started' || ticket.status === 'started')
            }
            selectedTicketsMutable={canBulkUpdate(selection, capabilitiesFor)}
          />
        }
        projectTabAction={viewportMobile.value ? undefined : headingAction}
        banner={
          <>
            <CompatibilityBannerSurface assessment={current.compatibility} />
            {currentJob &&
              !(
                currentJob.kind === 'backup' &&
                currentJob.status === 'succeeded' &&
                !currentBackupUnverified &&
                !migrationConnectionErrors.value[current.root]
              ) && (
                <Hs1JobBanner
                  job={currentJob}
                  backupVerified={!currentBackupUnverified}
                  details={migrationJobDetails.value[current.root]}
                  connectionError={migrationConnectionErrors.value[current.root]}
                />
              )}
            {current.needsHs1Migration && !currentJob && hs1MigrationProject.value?.id !== current.id && (
              <Hs1MigrationBanner databasePath={current.hs1DatabasePath ?? `${current.root}/.hotsheet/db`} />
            )}{' '}
            {current.hs1CleanupEligible &&
              !hs1CleanupPromptDismissed(localStorage, current.id, hs1SourceIdentity(current)) && <Hs1CleanupBanner />}
            {current.hs1RegistrationRepairAvailable &&
              !hs1CleanupPromptDismissed(localStorage, current.id, hs1SourceIdentity(current)) && (
                <Hs1RegistrationRepairBanner />
              )}
            {current.setupWarning && <ProjectSetupWarningBanner detail={current.setupWarning} />}
            {current.codexHooksChanged && <CodexHooksNoticeBanner path={current.codexHooksChanged} />}
            {notificationsPaused.value && (
              <NotificationsPausedBanner
                waiting={pendingPermissions().length}
                halted={haltedSessionsController.waiting()}
              />
            )}
          </>
        }
        pageHeader={pageHeader}
        mobileView={mobileView}
        workspace={<WorkspaceSurface {...workspaceSurfaceProps()} />}
        workspacePresentation={viewMode.value === 'board' && selectedView.value !== 'errors' ? 'edge-to-edge' : 'inset'}
        terminalDrawer={
          drawerViewAllowed ? (
            terminalDrawerMounted.value ? (
              <ProjectTerminalDrawerSurface drawer={projectTerminalDrawerProps()} />
            ) : (
              <></>
            )
          ) : undefined
        }
        terminalDrawerVisible={terminalDrawerVisible.value && drawerViewAllowed}
        terminalDrawerSize={appRegionSize('app-bottom-drawer')}
        terminalDrawerMax={terminalDrawerMax.value}
        terminalDrawerMin={terminalDrawerMin.value}
        terminalDrawerTransitioning={terminalDrawerTransitioning.value}
        terminalFocusMode={
          mobileTerminalFocus.value.active &&
          mobileTerminalFocus.value.terminalId === terminalDrawerSelected.value &&
          viewportMobile.value &&
          terminalDrawerVisible.value &&
          drawerViewAllowed
        }
        sidePanelSeparator={magnifiedTerminalKey.value ? 'hidden' : 'auto'}
        workAreaFocusRing={!magnifiedTerminalKey.value}
        inspector={inspectorContent}
        inspectorVisible={!inspectorCollapsed.value}
        inspectorSize={inspectorSize.value}
        overlay={
          <>
            {!viewportMobile.value && popup}
            {ticketContextMenuSurface()}
          </>
        }
        viewportOverlay={viewportMobile.value ? popup : undefined}
      />
    );
  }
  return { HotSheetShell };
}
