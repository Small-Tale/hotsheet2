import '@awesome.me/webawesome/dist/components/toast/toast.js';

import { type ResizableRegionAxis, type ResizableRegionEdge } from '@kerfjs/ui/resizable-region';
import { wireNavStack } from '@kerfjs/ui/wire-nav-stack';
import { wireWorkbench } from '@kerfjs/ui/wire-workbench';
import { batch, effect, mount, signal } from 'kerfjs';

import { claimEtaPresentation, isTicketActivelyWorkedOn, projectTabTicketState } from '../active-ticket-work';
import { filterAdvancedSearchResults, usesAdvancedSearchExpression } from '../advanced-search';
import {
  Api,
  type Capabilities,
  type CheckoutTicketCounts,
  type CheckoutTicketQuery,
  type CommandDefinition,
  type CorruptTicket,
  type CustomView,
  type DuplicateBacklink,
  type FullTicket,
  type PollResponse,
  type ProviderOutboxOperation,
  type TicketRow as WireTicketRow,
  TurnStreamReplayGuard,
} from '../api';
import {
  type AppRegionId,
  loadAppRegionSize,
  normalizeAppRegionSize,
  saveAppRegionSize,
  terminalDrawerMinimum,
} from '../app-region-resize';
import { type AttachmentLabelEditing } from '../attachment-label-editing';
import { type BoardColumnPage, isPerColumnBoardView } from '../board-pagination';
import { browserRandomId } from '../browser-id';
import { type BulkUpdateProgress, createBulkUpdateProgress } from '../bulk-update-progress';
import { type AiFeedbackDialogState } from '../components/ai-feedback-dialog';
import { APP_WORKBENCH_ID } from '../components/app-shell';
import { type BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import type { ConfidenceCalibrationState } from '../components/confidence-calibration';
import type { MarkdownEditorMode } from '../components/markdown-editor';
import { type NotificationView } from '../components/notification-navigation';
import { type ProjectCloseDialogState } from '../components/project-close-dialog';
import { projectRestoreTabId } from '../components/project-restore-error';
import { type ProjectTabBarMode } from '../components/project-tab-bar';
import { type AppTabKind } from '../components/project-tab-context-menu';
import { focusQuickTicketComposerTitle, showQuickTicketComposer } from '../components/quick-ticket-composer';
import { ServerBusyBars, ServerBusyMessage } from '../components/server-busy-bars';
import { type SettingsCategory } from '../components/settings-navigation';
import type { TicketStatus } from '../components/status-badge';
import {
  type TerminalCopyState,
  type TerminalEditMenuState,
  type TerminalPasteState,
} from '../components/terminal-clipboard-dialogs';
import { type TerminalDashboardGroup } from '../components/terminal-dashboard';
import { type TerminalRenameTarget } from '../components/terminal-rename-dialog';
import { type TerminalVisibilityNamePrompt } from '../components/terminal-visibility-dialog';
import { type TicketCloseDialogState } from '../components/ticket-close-dialog';
import { type InspectorTab } from '../components/ticket-inspector';
import { type TicketLinkChoice } from '../components/ticket-link-choice-dialog';
import { showTicketReaderDialog, type TicketReaderDialogElement } from '../components/ticket-reader';
import type { TicketPriority, TicketRowProps } from '../components/ticket-row';
import { type TrashRetentionDraft } from '../components/trash-settings';
import { SavedViewContextMenu } from '../components/view-navigation';
import {
  type WorkspaceSort,
  type WorkspaceSortDirection,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { withControlledOpen } from '../controlled-open';
import { customAiCommandSignalConnection, customAiCommandTicket, HOTSHEET_SKILL_SIGNAL } from '../custom-ai-command';
import { createAiConfigurationController } from '../features/ai-configuration';
import { createBoardColumnPaging } from '../features/board-column-paging';
import { createClaimClockController } from '../features/claim-clock';
import { createCommandsController } from '../features/commands';
import { createConversationArchiveController } from '../features/conversation-archive';
import { createCorruptTicketRecoveryController } from '../features/corrupt-ticket-recovery';
import { createDriveConversationsController } from '../features/drive-conversations';
import { createGalleryController } from '../features/gallery';
import { createHaltedSessionsController } from '../features/halted-sessions';
import { createHotSheetApp } from '../features/hot-sheet-app';
import { createHotSheetShell } from '../features/hot-sheet-shell';
import { createInspectorSurface } from '../features/inspector-surface';
import { createPermissionsController } from '../features/permissions';
import { createProjectChangeStreamsController } from '../features/project-change-streams';
import { createProjectCloseController } from '../features/project-close';
import { createProjectLifecycleController } from '../features/project-lifecycle';
import {
  type CachedProjectProjection,
  createProjectRefresh,
  type ProjectRefreshOptions,
} from '../features/project-refresh';
import { createProjectTeardown } from '../features/project-teardown';
import { createReaderLayersSurface } from '../features/reader-layers-surface';
import { createRepositoryController } from '../features/repository';
import { createSavedViewsController } from '../features/saved-views';
import { createSettingsSurface } from '../features/settings-surface';
import { createSidebarSurface } from '../features/sidebar-surface';
import { createTerminalDashboardController } from '../features/terminal-dashboard';
import { createTerminalDrawerController } from '../features/terminal-drawer';
import {
  createTerminalDrawerGeometryController,
  currentMobileViewportGeometry,
} from '../features/terminal-drawer-geometry';
import { createTerminalNamesController } from '../features/terminal-names';
import { createTerminalOperationsSurface } from '../features/terminal-operations-surface';
import { createTerminalPresentation } from '../features/terminal-presentation';
import { createTerminalRailSurface } from '../features/terminal-rail-surface';
import { createTerminalViewportsController } from '../features/terminal-viewports';
import { createTicketViewSurface } from '../features/ticket-view-surface';
import { createTicketWorkflows } from '../features/ticket-workflows';
import { createWorkspaceSearchController, type SidebarSearchCounts } from '../features/workspace-search';
import { createWorkspaceSurface } from '../features/workspace-surface';
import { type InlineFeedbackReply } from '../feedback-replies';
import { syncFocusedDraftControl } from '../focused-draft-sync';
import { projectHaltedSessions } from '../halted-sessions';
import { effectiveSearch, type InlineSearchToken, orderedSearchText } from '../inline-search';
import { beginInteractionTiming } from '../interaction-performance';
import type { Control, DetailsFinishTask, NotWorkingTarget, PendingEvidence, Project } from '../interactions/types';
import { stringArray, tryParseJson } from '../json-value';
import { isAppleShortcutPlatform, loadShortcutOverrides, type ShortcutChord } from '../keyboard-shortcuts';
import { localBridge } from '../local-bridge-client';
import { LocalTicketChangeAcknowledgements } from '../local-ticket-changes';
import { isMobileViewport, isSidePanelPortal } from '../mobile-layout';
import { loadMobileTerminalColumns } from '../mobile-terminal-columns';
import { INACTIVE_MOBILE_TERMINAL_FOCUS } from '../mobile-terminal-focus';
import { type ProjectTicketSources, resolveNewTicketSource } from '../new-ticket-source';
import { mergeRetainedCreatedRows, PendingCreatedTickets, prependCreatedTicketRow } from '../pending-created-tickets';
import { priorityFromWire } from '../priority-wire';
import { afterBrowserPaint } from '../project-activation';
import { type DrawerAIChat } from '../project-drive';
import { openProjectFetch, restoreRememberedProjects } from '../project-startup';
import { createProjectTabRefreshCoordinator } from '../project-tab-refresh';
import {
  appendUniqueTicketRows,
  BOARD_COLUMN_PAGE_SIZE,
  isStaleTicketCursorError,
  loadProjectTicketRefresh,
  type ProjectTicketRefresh,
} from '../project-ticket-refresh';
import { createProjectWarmCache } from '../project-warm-cache';
import { createRefreshBarrier } from '../refresh-barrier';
import { createRenderMetrics } from '../render-metrics';
import { computeServerBusyBarCount, serverBusy, serverBusyMessage } from '../server-busy';
import { interleaveByRank } from '../tab-order';
import { TERMINAL_GRID_DEFAULT_ACROSS, TERMINAL_GRID_DEFAULT_HIGH } from '../terminal-grid-layout';
import { consumeTerminalModifiers, NO_TERMINAL_MODIFIERS, type TerminalModifiers } from '../terminal-keys';
import { parseTerminalNames } from '../terminal-names';
import { terminalDrawerActivation } from '../terminal-project-scope';
import {
  parseTerminalVisibilityState,
  TERMINAL_VISIBILITY_STORAGE_KEY,
  TERMINAL_VISIBILITY_TYPES,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import { hasUnresolvedBlocker } from '../ticket-blocking';
import { BulkTicketMutationSequencer } from '../ticket-bulk-operations';
import { loadLastTicketCategory } from '../ticket-category-preference';
import { type DuplicateTarget } from '../ticket-close';
import { ticketCompletionTrend } from '../ticket-completion-trend';
import { parseTicketDeepLink, type TicketDeepLink, ticketDeepLinkRoot } from '../ticket-deep-link';
import { loadTicketEditorSizes } from '../ticket-editor-size';
import {
  reconcileActiveDraft,
  reconcileReaderNoteDraft,
  type TicketFieldConflict,
} from '../ticket-field-reconciliation';
import { parseTicketLinkReference, resolveTicketLink, type TicketLinkMatch } from '../ticket-link-resolution';
import { waitForTicketMotionSettled } from '../ticket-motion';
import { type ClipboardTicket, type TicketHistory, type TicketSnapshot } from '../ticket-operations';
import {
  activeTicketReaderProject,
  popTicketReaderFrame,
  pushTicketReaderFrame,
  ticketReaderEditState,
  type TicketReaderFrame,
} from '../ticket-reader-stack';
import { defersContainedListScrollRestore } from '../ticket-row-containment';
import { TicketScrollMemory } from '../ticket-scroll-state';
import { replaceTicketSearch } from '../ticket-search-model';
import type { TicketTitleEditSurface } from '../ticket-title-editing';
import {
  customTicketViewKey,
  isArchivedTicket,
  isOpenTicket,
  isQueuedTicket,
  isTrashedTicket,
  selectionAfterTicketViewChange,
  ticketsForView,
  type TicketView,
  ticketViewQuery,
} from '../ticket-views';
import { createToastLifetime } from '../toast-lifetime';
import { createToastPresentation } from '../toast-presentation';
import { createTrackedSizeObserver } from '../tracked-size-observer';
import { createTrailingTask } from '../trailing-task';
import {
  loadWorkspacePreferences,
  saveWorkspacePreferences,
  sortableWorkspaceView,
  type SortableWorkspaceViewMode,
} from '../workspace-preferences';
import {
  activeProjectRoot,
  loadDraftFiles,
  loadProjectWorkspaceSession,
  saveActiveProjectRoot,
  saveProjectWorkspaceSession,
} from '../workspace-session';
import { compareWorkspaceTickets } from '../workspace-ticket-sort';
import { wireTicketRailToolbarVisibility, wireWorkspaceToolbarVisibility } from '../workspace-toolbar-visibility';
import { installDevelopmentDiagnostics } from './development-bootstrap';
import { createHotSheetInteractionBindings, type InteractionBindingsPort } from './interaction-bindings';
import { wireHotSheetInteractions } from './wire-interactions';

/**
 * Create and start one Hot Sheet browser application runtime. The returned `disposeInteractions`
 * removes every delegated and native interaction listener the runtime registered.
 */
/** A warm project's list continuation: the cursor and the exact query that issued it (HS2-38MPMC). */
interface TicketCursorPosition {
  cursor?: string;
  query: CheckoutTicketQuery;
}

export async function startHotSheetWebClient() {
  const deferContainedListScrollRestore = defersContainedListScrollRestore(navigator.userAgent);
  await installDevelopmentDiagnostics();
  const turnStreamReplayGuard = new TurnStreamReplayGuard();
  const localTicketChangeAcknowledgements = new LocalTicketChangeAcknowledgements();
  // Retains just-created rows so an eventually-consistent index that hasn't indexed them yet
  // cannot drop them from a background refresh (HS2-Y5PDHW).
  const pendingCreatedTickets = new PendingCreatedTickets();
  const turnStreamEvents = (response: PollResponse) => turnStreamReplayGuard.events(response);

  loadTicketEditorSizes(localStorage, document.documentElement.style);

  const projects = signal<Project[]>([]),
    selectedProjectId = signal(''),
    tickets = signal<WireTicketRow[]>([]),
    ticketRowsByProject = signal<Record<string, WireTicketRow[]>>({}),
    providerOutboxOperations = signal<Record<string, ProviderOutboxOperation | undefined>>({}),
    corruptTickets = signal<CorruptTicket[]>([]),
    selectedTicket = signal<FullTicket | null>(null);
  const { corruptRecovery, revealCorruptTicket, queueCorruptTicketRepair } = createCorruptTicketRecoveryController({
    project: () => project(),
    corruptTickets,
    showToast: (message) => {
      showToast(message);
    },
    refreshProject: () => refreshProject({ showLoading: false }),
  });
  const aiFeedbackDialog = signal<AiFeedbackDialogState | undefined>(undefined);
  const ticketCountsByProject = signal<Record<string, CheckoutTicketCounts>>({});
  const partialSourcesByProject = signal<Record<string, boolean>>({});
  // The server-authoritative 7-day completion trend and today's completion count, retained per project so a
  // local mutation (which drops the exact counts snapshot) does not force the sidebar graph to be re-derived
  // from the partially-loaded rows — which is wrong until the next authoritative refresh (HS2-BRDMBB).
  const ticketTrendByProject = signal<Record<string, { trend: readonly number[]; completedToday: number }>>({});
  function recordAuthoritativeTicketTrend(projectId: string, counts: CheckoutTicketCounts) {
    ticketTrendByProject.value = {
      ...ticketTrendByProject.value,
      [projectId]: { trend: counts.completion_trend ?? [], completedToday: counts.completed_today },
    };
  }
  const sidebarSearchCounts = signal<SidebarSearchCounts | undefined>(undefined);
  const duplicateBacklinkState = signal<{
    key: string;
    backlinks: DuplicateBacklink[];
    inaccessibleProjects: string[];
  }>({
    key: '',
    backlinks: [],
    inaccessibleProjects: [],
  });
  const resolvedDuplicateTargets = signal<Record<string, DuplicateTarget>>({});
  const projectProjectionById = signal<Record<string, CachedProjectProjection>>({});
  // Bounded LRU of projects whose loaded projection stays resident so tab switches paint instantly and
  // revalidate silently; evicted projects fall back to a cold (loading) activation (HS2-AZZ9TF).
  const warmProjects = createProjectWarmCache();
  const projectCloseDialog = signal<ProjectCloseDialogState | undefined>(undefined);
  const ticketLinkChoice = signal<TicketLinkChoice | undefined>(undefined);
  const customViewsByProject = signal<Record<string, CustomView[]>>({});
  const repositoryController = createRepositoryController({ project: () => project(), selectedTicket, showToast });
  const {
    repository,
    repositoryError,
    repositoryView,
    repositoryFileMenu,
    repositorySelectedFiles,
    repositorySetupStep,
    repositorySetupError,
    changeEvidenceView,
    changeEvidenceReader,
    repositoryComparison,
    expandedCodeReviewCommits,
    repositoryDetail,
    codeReview,
    codeReviewLoading,
    codeReviewMessage,
    refreshRepositoryStatus,
    initializeRepository,
    connectRepositoryRemote,
    skipRepositoryRemote,
    loadRepositoryDetail,
    syncRepositoryPaginationObserver,
    refreshCodeReview,
    repositoryStatusSurface,
    changeEvidenceSurfaceProps,
    changeEvidenceSurface,
  } = repositoryController;
  const ticketScrollMemory = new TicketScrollMemory();
  const shellMode = signal<ProjectTabBarMode>('project'),
    statsProjectId = signal<string | undefined>(undefined),
    // Per-project completion-confidence calibration shown in the stats view (HS2-Q1WCCY).
    confidenceReportByProject = signal<Record<string, ConfidenceCalibrationState>>({});
  /** Load a project's calibration report, keeping the last report visible while refreshing. */
  async function loadConfidenceReport(target: Project) {
    const previous = confidenceReportByProject.peek()[target.id] as ConfidenceCalibrationState | undefined;
    if (previous?.status !== 'ready')
      confidenceReportByProject.value = { ...confidenceReportByProject.peek(), [target.id]: { status: 'loading' } };
    let next: ConfidenceCalibrationState;
    try {
      next = { status: 'ready', report: await new Api(target.apiPath).checkoutConfidenceReport(target.id) };
    } catch (error) {
      next = { status: 'error', message: error instanceof Error ? error.message : String(error) };
    }
    confidenceReportByProject.value = { ...confidenceReportByProject.peek(), [target.id]: next };
  }
  effect(() => {
    if (shellMode.value !== 'stats' || !statsProjectId.value) return;
    const target = projects.peek().find((item) => item.id === statsProjectId.value);
    if (target) void loadConfidenceReport(target);
  });
  const terminalRailScreen = signal<'root' | 'ticket'>('root');
  const terminalGridFullscreen = signal(false);
  const terminalGroups = signal<TerminalDashboardGroup[]>([]),
    terminalDashboardLoading = signal(false),
    terminalDashboardMessage = signal('');
  const showLoadingActivity = signal(localStorage.getItem('hotsheet.show-loading-activity') === 'true');
  const inheritGlobalShellHistory = signal(false),
    terminalSettingsMessage = signal(''),
    trashCleanupDaysByProject = signal<Record<string, number>>({}),
    trashSettingsMessagesByProject = signal<Record<string, string>>({}),
    trashRetentionDraftsByProject = signal<Record<string, TrashRetentionDraft>>({});
  const terminalDashboardSize = signal({ width: 1200, height: 601 }),
    terminalFitAcross = signal(
      Number(localStorage.getItem('hotsheet.terminals.fit-across')) || TERMINAL_GRID_DEFAULT_ACROSS,
    ),
    terminalFitHigh = signal(Number(localStorage.getItem('hotsheet.terminals.fit-high')) || TERMINAL_GRID_DEFAULT_HIGH);
  const rememberedRoots = [
      ...new Set(stringArray(tryParseJson(localStorage.getItem('hotsheet.open-projects') || '[]'))),
    ],
    initialProjectRestorePending = signal(rememberedRoots.length > 0),
    // Remembered roots still opening in the background after the active project was revealed (HS2-2BEJXD).
    projectRestorePendingRoots = signal<readonly string[]>(rememberedRoots);
  const pendingRestoreEntries = <T,>(item: (root: string) => T) =>
    projectRestorePendingRoots.value.map((root) => ({ rank: rememberedRoots.indexOf(root), item: item(root) }));
  const initialTerminalDrawerVisible = localStorage.getItem('hotsheet.terminals.drawer-open') === 'true';
  const terminalDrawerVisible = signal(initialTerminalDrawerVisible),
    terminalDrawerMounted = signal(initialTerminalDrawerVisible),
    terminalDrawerTransitioning = signal(false),
    terminalDrawerSize = signal(loadAppRegionSize(localStorage, 'app-bottom-drawer')),
    terminalDrawerMax = signal(520),
    terminalDrawerMin = signal(terminalDrawerMinimum()),
    terminalDrawerMaximized = signal(false),
    terminalDrawerBounds = signal({ width: 900, height: 320 }),
    terminalDrawerFitAcross = signal(Number(localStorage.getItem('hotsheet.terminals.drawer-fit-across')) || 2),
    terminalDrawerFitHigh = signal(Number(localStorage.getItem('hotsheet.terminals.drawer-fit-high')) || 2),
    terminalDrawerSelected = signal('grid'),
    mobileTerminalFocus = signal(INACTIVE_MOBILE_TERMINAL_FOCUS),
    mobileTerminalColumns = signal(loadMobileTerminalColumns(localStorage)),
    // Phone key-bar state (HS2-CKS78M): sticky modifiers and whether the Fn row is showing.
    terminalModifiers = signal<TerminalModifiers>(NO_TERMINAL_MODIFIERS),
    terminalFunctionRow = signal(false),
    mobileViewportGeometry = signal(currentMobileViewportGeometry());
  // Drawer geometry and the phone terminal viewport live in their own owner (HS2-T1DX2V). The
  // mobile-layout signal is created further down, so it is read lazily.
  const {
    syncTerminalDrawerMaximum,
    settleTerminalDrawerGeometry,
    toggleTerminalDrawerMaximized,
    enterMobileTerminalFocus,
    syncMobileTerminalViewport,
    exitMobileTerminalFocus,
    syncMobileViewportGeometry,
    cycleMobileTerminalColumns,
    mobileMagnifiedTerminal,
  } = createTerminalDrawerGeometryController({
    terminalDrawerMin,
    terminalDrawerMax,
    terminalDrawerMaximized,
    mobileTerminalFocus,
    mobileTerminalColumns,
    mobileViewportGeometry,
    terminalModifiers,
    terminalFunctionRow,
    viewportMobile: () => viewportMobile.value,
    updateTerminalDrawerBounds,
    showToast,
  });
  const terminalDrawerChatsByProject = signal<Record<string, DrawerAIChat[]>>({}),
    terminalDrawerOrderByProject = signal<Record<string, string[]>>({});
  const magnifiedTerminalKey = signal<string | undefined>(undefined),
    terminalVisibility = signal(parseTerminalVisibilityState(localStorage.getItem(TERMINAL_VISIBILITY_STORAGE_KEY))),
    terminalVisibilityDialogScope = signal<string | undefined>(undefined),
    terminalVisibilityFilter = signal<readonly TerminalVisibilityType[]>(TERMINAL_VISIBILITY_TYPES),
    terminalVisibilityContextMenu = signal<{ id: string; x: number; y: number } | undefined>(undefined),
    terminalVisibilityNamePrompt = signal<TerminalVisibilityNamePrompt | undefined>(undefined);
  const terminalNames = signal(parseTerminalNames(localStorage.getItem('hotsheet.terminals.names'))),
    terminalContextMenu = signal<{ key: string; x: number; y: number } | undefined>(undefined),
    terminalRename = signal<TerminalRenameTarget | undefined>(undefined),
    // Phone terminal Copy and Paste-fallback sheets (HS2-FRB545).
    terminalCopy = signal<TerminalCopyState | undefined>(undefined),
    terminalPaste = signal<TerminalPasteState | undefined>(undefined),
    // Long-press terminal edit menu (HS2-KKP8YJ).
    terminalEditMenu = signal<TerminalEditMenuState | undefined>(undefined);
  const { saveTerminalName, resetTerminalName, reconcileTerminalNames, applyTerminalRenamed } =
    createTerminalNamesController({
      projects,
      terminalNames,
      terminalGroups,
      showToast: (message) => {
        showToast(message);
      },
      terminalGroupLoaded: (projectId) => terminalGroupLoaded(projectId),
      refreshTerminalDashboard: () => refreshTerminalDashboard(),
    });
  let terminalPreviewClickTimer: number | undefined;
  const { syncTerminalViewportMounts, parkProjectViewports } = createTerminalViewportsController({
    projects,
    mobileTerminalColumns: () => mobileTerminalColumns.peek(),
    terminalModifiers: {
      current: () => terminalModifiers.peek(),
      consume: () => {
        terminalModifiers.value = consumeTerminalModifiers(terminalModifiers.peek());
      },
    },
    get pendingTerminalFocus() {
      return terminalDrawer.pendingTerminalFocus;
    },
    set pendingTerminalFocus(value) {
      terminalDrawer.pendingTerminalFocus = value;
    },
    openTicketReference(element, slug, projectId, preferredProject) {
      ticketLinkReturnFocus = element;
      void selectLinkedTicket(slug, projectId, preferredProject);
    },
  });
  const selectedCorruptKey = signal<string | undefined>(undefined);
  const selectedTicketSlugs = signal<string[]>([]);
  const ticketContextMenu = signal<{ x: number; y: number; ticketSlug: string; hideUpNext?: boolean } | undefined>(
    undefined,
  );
  const ticketCloseDialog = signal<TicketCloseDialogState | undefined>(undefined);
  const appTabContextMenu = signal<
    { x: number; y: number; kind: AppTabKind; id: string; direction: 'left' | 'right' } | undefined
  >(undefined);
  const bulkTicketDialog = signal<BulkTicketDialogState | undefined>(undefined);
  let bulkTicketSlugs: string[] = [];

  const CLOSED_NOT_WORKING_TARGET: NotWorkingTarget = {
    projectId: '',
    apiPath: '',
    ticketId: '',
    slug: '',
    connectionId: '',
    mode: 'not-working',
  };
  const notWorkingTarget = signal<NotWorkingTarget>(CLOSED_NOT_WORKING_TARGET),
    notWorkingNote = signal(''),
    notWorkingFiles = signal<PendingEvidence[]>([]),
    notWorkingSubmitting = signal(false),
    notWorkingError = signal('');
  let ticketSelectionAnchor: string | undefined;
  const histories = new Map<string, TicketHistory>();
  const mutationGenerations = new Map<string, number>();
  // Open local ticket writes; every project refresh waits on it before reading tickets (HS2-RE1PS6).
  const localTicketMutationBarrier = createRefreshBarrier();
  // Per-ticket single-edit mutation sequencing (HS2-K9SG2R). Rapid same-ticket field edits used to read
  // the same pre-first concurrency token and self-conflict ("the ticket was modified"). `committedTickets`
  // holds the latest server-committed full ticket per slug so each queued edit bases off the previous
  // edit's committed token (last-write-wins for the user's own sequential edits), while a genuine external
  // write still fails the token check and surfaces a real conflict. `ticketMutationChains` serializes the
  // network section per slug; the optimistic UI update stays immediate for responsiveness.
  const committedTickets = new Map<string, FullTicket>();
  // Reuse the same per-key serializer the bulk path uses, keyed by ticket slug for single-ticket edits.
  const singleTicketMutationSequencer = new BulkTicketMutationSequencer();
  const bulkTicketMutationSequencer = new BulkTicketMutationSequencer();
  let clipboard: { tickets: ClipboardTicket[]; cut: boolean; source: Project } | undefined;
  const storedWorkspacePreferences = loadWorkspacePreferences(localStorage);
  const loading = signal(false),
    bulkUpdateProgress = signal<BulkUpdateProgress | undefined>(undefined),
    error = signal(''),
    viewMode = signal<WorkspaceViewMode>(storedWorkspacePreferences.viewMode),
    // The list/columns view the user last had open, so Notifications or Settings can hand back to it (HS2-42T028).
    ticketViewMode = signal<SortableWorkspaceViewMode>(storedWorkspacePreferences.ticketViewMode),
    selectedView = signal<TicketView>('all'),
    ticketCollectionState = signal<{ projectId: string; view: TicketView; status: 'loading' | 'error' } | undefined>(
      undefined,
    ),
    searchOpen = signal(false),
    searchQuery = signal(''),
    searchTokens = signal<InlineSearchToken[]>([]),
    searchHelpOpen = signal(false),
    searchMatchKeys = signal<Set<string> | undefined>(undefined),
    workspaceSorts = signal(storedWorkspacePreferences.sorts);
  const bulkUpdateActivities = createBulkUpdateProgress((progress) => {
    bulkUpdateProgress.value = progress;
  });
  const ticketNextCursor = signal<string | undefined>(undefined),
    // A cursor is only valid with the exact query that issued it (the server binds it to sort and filters), so a
    // warm project keeps both together and a switch restores them as one unit (HS2-38MPMC).
    ticketCursorsByProject = signal<Record<string, TicketCursorPosition | undefined>>({}),
    ticketPageLoading = signal(false);
  // Per-column board pagination (HS2-8NBGBX): each status column paginates independently. `boardColumnPages`
  // holds the next-page cursor + loaded count per column id; `boardColumnLoading` is the per-column
  // spinner. Both are scoped to the active project+view and cleared when that changes.
  const boardColumnPages = signal<Record<string, BoardColumnPage>>({}),
    boardColumnLoading = signal<Record<string, boolean>>({});
  function boardPagesWithVisibleLimit(
    pages: Record<string, BoardColumnPage>,
    previous: Record<string, BoardColumnPage> = boardColumnPages.value,
  ) {
    return Object.fromEntries(
      Object.entries(pages).map(([id, page]) => {
        const prior = Object.hasOwn(previous, id) ? previous[id] : undefined;
        return [id, { ...page, visible: prior?.visible ?? BOARD_COLUMN_PAGE_SIZE }];
      }),
    );
  }
  function resetBoardColumnPages() {
    boardColumnPages.value = {};
    boardColumnLoading.value = {};
  }
  // Warm projects keep their board column cursors with their rows, so a warm switch restores columns
  // exactly instead of re-paging from the top (HS2-HNZZHC).
  const boardPagesByProject = signal<Record<string, Record<string, BoardColumnPage>>>({});
  const ticketPageQuery = signal<CheckoutTicketQuery>({ collection: 'queue' });
  const INITIAL_TICKET_RENDER_COUNT = 40,
    TICKET_RENDER_CHUNK = 160;
  const renderedTicketLimit = signal(INITIAL_TICKET_RENDER_COUNT);
  let ticketRenderGeneration = 0,
    ticketRenderScheduled = false,
    skipNextTicketMotion = false;
  function resetProgressiveTicketRendering() {
    ticketRenderGeneration += 1;
    ticketRenderScheduled = false;
    renderedTicketLimit.value = INITIAL_TICKET_RENDER_COUNT;
  }
  function continueProgressiveTicketRendering(total: number) {
    if (renderedTicketLimit.value >= total || ticketRenderScheduled) return;
    ticketRenderScheduled = true;
    const generation = ticketRenderGeneration;
    requestAnimationFrame(() => {
      const renderMore = () => {
        ticketRenderScheduled = false;
        if (generation !== ticketRenderGeneration) return;
        skipNextTicketMotion = true;
        renderedTicketLimit.value = Math.min(total, renderedTicketLimit.value + TICKET_RENDER_CHUNK);
      };
      if (typeof window.requestIdleCallback === 'function') window.requestIdleCallback(renderMore, { timeout: 100 });
      else globalThis.setTimeout(renderMore, 0);
    });
  }
  const sort = {
    get value(): WorkspaceSort {
      return activeWorkspaceSort().sort;
    },
    set value(value: WorkspaceSort) {
      const mode = sortableWorkspaceView(viewMode.value);
      workspaceSorts.value = { ...workspaceSorts.value, [mode]: { ...workspaceSorts.value[mode], sort: value } };
    },
  };
  const sortDirection = {
    get value(): WorkspaceSortDirection {
      return activeWorkspaceSort().sortDirection;
    },
    set value(value: WorkspaceSortDirection) {
      const mode = sortableWorkspaceView(viewMode.value);
      workspaceSorts.value = {
        ...workspaceSorts.value,
        [mode]: { ...workspaceSorts.value[mode], sortDirection: value },
      };
    },
  };
  const toastHost = document.createElement('wa-toast');
  const toastPresentation = createToastPresentation({
    create: (message) => toastHost.create(message, { duration: 0, variant: 'neutral' }),
    clear: () => {
      toastHost.replaceChildren();
    },
    prepare: (item, generation) => {
      item.dataset.toastGeneration = String(generation);
      const text = document.createElement('span');
      text.className = 'app-toast';
      text.textContent = item.textContent;
      item.replaceChildren(text);
    },
  });
  const toastLifetime = createToastLifetime((state) => {
    if (toastHost.isConnected) void toastPresentation.publish(state);
  });
  function showToast(message: string) {
    toastLifetime.show(message);
  }
  const activeTicketCount = signal(0),
    projectTabClaimClock = signal(Date.now()),
    // Local render clock for ETA countdowns (HS2-XQMDQB); its timer never makes network requests.
    claimEtaClock = signal(Date.now());
  const {
    liveClaimNotice,
    scheduleClaimLeaseExpiry,
    dispose: disposeClaimClock,
  } = createClaimClockController({
    projects,
    selectedProjectId,
    ticketCountsByProject,
    activeTicketCount,
    projectTabClaimClock,
    claimEtaClock,
    projectTabTicketRows,
    projectTicketCounts,
    refreshProjectTab: (current) => projectTabRefresh.request(current),
  });
  // The selected settings view is shared across projects: switching project keeps the same
  // settings view rather than resetting per-project (HS2-4J50K3).
  const selectedSettingsCategory = signal<SettingsCategory>('sources');
  const appleShortcutPlatform = isAppleShortcutPlatform();
  const keyboardShortcutOverrides = signal<Record<string, ShortcutChord>>(loadShortcutOverrides(localStorage));
  const capturingShortcutId = signal<string | undefined>(undefined);
  const notificationView = signal<NotificationView>('pending');
  const lastUsedTicketCategory = loadLastTicketCategory(localStorage);
  const composerExpanded = signal(false),
    composerTitle = signal(''),
    composerDetails = signal(''),
    composerCategory = signal(lastUsedTicketCategory),
    composerUpNext = signal(false),
    // The new ticket's target source while the composer is open, and the last source a ticket was
    // created in per project — in memory only, never persisted (HS2-NZMJBJ).
    composerSource = signal<string | undefined>(undefined),
    lastTicketSourceByProject = signal<Record<string, string>>({}),
    inspectorTab = signal<InspectorTab>('info'),
    readerTab = signal<InspectorTab>('info'),
    sidebarVisible = signal(storedWorkspacePreferences.sidebarVisible),
    inspectorVisible = signal(storedWorkspacePreferences.inspectorVisible);
  const composerAttachments = signal<PendingEvidence[]>([]),
    composerAttachmentMessage = signal(''),
    composerAttachmentError = signal(false),
    composerScreening = signal(false),
    composerSubmitting = signal(false);
  let projectSessionTimer: number | undefined,
    restoringProjectSession = false,
    projectSessionInteractionVersion = 0;
  const sidebarSize = signal(loadAppRegionSize(localStorage, 'app-left-rail')),
    inspectorSize = signal(loadAppRegionSize(localStorage, 'app-right-rail'));
  const detailsMode = signal<MarkdownEditorMode>('preview'),
    detailsDraft = signal('');
  let detailsEditGeneration = 0;
  const readerOpen = signal(false),
    linkedReaderStack = signal<TicketReaderFrame[]>([]),
    readerLargeText = signal(localStorage.getItem('hotsheet.reader.large-text') === 'true');
  const readerReturnFocus = new Map<string, HTMLElement>();
  const readerClosing = new Set<string>(),
    readerApprovedClose = new Set<string>();
  let ticketLinkReturnFocus: HTMLElement | undefined;
  function attachmentContext(ticket: FullTicket, current = project()) {
    return ticketAttachmentContext(ticket, current);
  }
  const galleryController = createGalleryController({
    project: () => project(),
    selectedTicket,
    api: () => api(),
    attachmentContext,
    showToast,
    error,
    attachmentsEditable: (ticket) =>
      attachmentsEditableFor(ticket.connection_id) ||
      (providerCapabilities.value[ticket.connection_id] as Capabilities | undefined)?.attachment_crop === true,
    attachmentsCroppable: (ticket) =>
      (providerCapabilities.value[ticket.connection_id] as Capabilities | undefined)?.attachment_crop === true,
  });
  const {
    attachmentGalleryUrl,
    attachmentGalleryGeometry,
    attachmentGalleryScale,
    attachmentGalleryMarkup,
    attachmentGallerySaveState,
    attachmentGalleryDrawMode,
    attachmentGalleryTool,
    attachmentGalleryAnnotations,
    attachmentGalleryCropMode,
    attachmentGalleryCrop,
    attachmentGalleryOriginalSize,
    beginGalleryCrop,
    restoreGalleryCrop,
    finishGalleryCrop,
    attachmentGallerySelectedAnnotation,
    attachmentGalleryPlayhead,
    attachmentGalleryDuration,
    attachmentGalleryPlaying,
    attachmentGalleryVolume,
    attachmentGalleryMuted,
    attachmentGalleryVolumeOpen,
    attachmentMenu,
    galleryImages,
    gallerySurface,
    stopGallerySvgClock,
    updateGalleryPlaybackPresentation,
    gallerySvgClock,
    activeAttachmentGalleryVideo,
    resetAttachmentGallery,
    syncAttachmentGalleryMeasurement,
    attachmentMenuSurfaceProps,
    beginGalleryAnnotationSession,
    finishGalleryAnnotationSession,
    discardGalleryAnnotationSession,
    shiftGallery,
  } = galleryController;
  const readerDetailsMode = signal<MarkdownEditorMode>('preview'),
    readerDetailsDraft = signal('');
  let readerDetailsEditGeneration = 0;
  const attachmentMessage = signal('');
  // The open attachment batch label editor lives in application state so a live rerender keeps it
  // open (HS2-SG0AZY).
  const attachmentLabelEditing = signal<AttachmentLabelEditing | undefined>(undefined);

  const editingNoteId = signal<string | undefined>(undefined),
    noteDraft = signal('');
  const readerEditingNoteId = signal<string | undefined>(undefined),
    readerNoteDraft = signal('');
  const readerInlineFeedbackReplies = signal<Record<string, InlineFeedbackReply[]>>({});
  const readerFeedbackChoiceSelections = signal<Record<string, string[]>>({});
  const readerFeedbackChoiceAnchors = new Map<string, string>();
  const composingNote = signal(false),
    newNoteDraft = signal('');
  const blockedReasonEditing = signal(false),
    blockedReasonDraft = signal('');
  const readerBlockedReasonEditing = signal(false),
    readerBlockedReasonDraft = signal('');
  const titleEditingSurface = signal<TicketTitleEditSurface | undefined>(undefined),
    titleDraft = signal('');
  const fieldConflict = signal<TicketFieldConflict | undefined>(undefined),
    fieldConflictResolution = signal('');
  let detailsDraftBase = '',
    readerDetailsDraftBase = '',
    titleDraftBase = '',
    blockedReasonDraftBase = '',
    readerBlockedReasonDraftBase = '',
    noteDraftBase = '',
    readerNoteDraftBase = '';
  const providerCapabilities = signal<Record<string, Capabilities>>({});
  const defaultProviders = signal<Record<string, ProjectTicketSources | undefined>>({});
  const hideVerifiedByProject = signal<Record<string, boolean>>({});
  const {
    commandDefinitions,
    commandRuns,
    commandGroupExpanded,
    commandGroupsCollapsed,
    commandDialogId,
    commandStopConfirmation,
    commandSettingsEditingId,
    commandSettingsDraftsByProject,
    commandSettingsMessagesByProject,
    commandSettingsSelectedByProject,
    commandSettingsExtraGroupsByProject,
    commandIconSearch,
    commandSettingsMessage,
    setCommandSettingsDraft,
    setCommandSettingsMessage,
    commandSettingsDefinitions,
    commandSelection,
    selectCommandSetting,
    selectCommandRow,
    updateCommandSetting,
    updateCommandAiSelection,
    addCommandSetting,
    deleteCommandSetting,
    commandSettingsExtraGroups,
    loadCommandGroups,
    reorderCommandSettings,
    addCommandGroup,
    deleteCommandGroup,
    flushCommandAutosaves,
    commandRunFor,
    commandIcon,
    commandDialogSurface,
  } = createCommandsController({ projects, selectedProjectId, storedWorkspacePreferences });
  const driveConversations = createDriveConversationsController({
    projects,
    project: () => project(),
    error,
    terminalDrawerChatsByProject,
    createDrawerAIChat: (selection, options) => terminalDrawer.createDrawerAIChat(selection, options),
    selectDrawerItem: (id) => {
      terminalDrawer.selectDrawerItem(id);
    },
    setTerminalDrawerVisible: (visible) => {
      terminalDrawer.setTerminalDrawerVisible(visible);
    },
    aiConfiguration: () => aiConfigurationController,
  });
  const {
    driveConnectionsByProject,
    drivePendingByProject,
    conversationStates,
    conversationDrafts,
    conversationSelections,
    conversationConnectionId,
    conversationOpen,
    conversationRenderRevision,
    conversationPersistence,
    replaceConversationStates,
    updateConversation,
    conversationForActivity,
    beginConversation,
    refreshDriveConnections,
    toggleSidebarDrive,
    openSidebarConversation,
    sendConversationTurn,
    stopConversation,
  } = driveConversations;

  const aiConfigurationController = createAiConfigurationController({
    selectedProjectId,
    project: () => project(),
    conversationConnectionId,
    conversationSelections,
    terminalDrawerChatsByProject,
    driveConnectionsByProject,
    conversationStates,
    conversationOpen,
    createDrawerAIChat: (selection) => terminalDrawer.createDrawerAIChat(selection),
    showToast,
    beginConversation,
    updateConversation,
    error,
    commandSettingsDefinitions,
    commandSettingsEditingId,
  });
  const {
    aiTools,
    aiDefaults,
    aiSettingsLoading,
    aiSettingsMessage,
    driveOptionsOpen,
    driveOptionsAnchor,
    driveOverridesByProject,
    manualModelDialog,
    aiToolLabel,
    normalizedAiSelection,
    effectiveCommandAiSelection,
    effectiveDriveSelection,
    selectDriveModel,
    selectDefaultModel,
    selectDefaultEffort,
    selectDefaultProvider,
    selectConversationModel,
    selectConversationEffort,
    selectConversationProvider,
    openManualModel,
    restoreCommandEditorAfterManualModel,
    aiLaunchConfiguration,
    refreshAiConfiguration,
    saveAiDefaults,
    conversationAiSelection,
  } = aiConfigurationController;
  const {
    conversationExportDialog,
    conversationSelectedMessages,
    pickConversationMessage,
    clearConversationSelection,
    copyConversationSelection,
    updateConversationExportDraft,
    openConversationExport,
    finishConversationExport,
    openSavedConversation,
  } = createConversationArchiveController({
    project: () => project(),
    conversationConnectionId,
    conversationStates,
    driveConnectionsByProject,
    terminalDrawerChatsByProject,
    conversationAiSelection,
    aiToolLabel,
    showToast,
    error,
    terminalVisibility,
    persistTerminalVisibility: (next) => {
      persistTerminalVisibility(next);
    },
    replaceConversationStates,
    selectDrawerItem: (id) => {
      terminalDrawer.selectDrawerItem(id);
    },
    setTerminalDrawerVisible: (visible) => {
      terminalDrawer.setTerminalDrawerVisible(visible);
    },
  });
  let appRegionResizeDrag:
    | {
        id: AppRegionId;
        axis: ResizableRegionAxis;
        edge: ResizableRegionEdge;
        startPoint: number;
        startSize: number;
        pendingSize: number;
        collapseRequested?: boolean;
        region: HTMLElement;
        handle: HTMLElement;
        frame?: number;
      }
    | undefined;
  const permissionsController = createPermissionsController({ projects, selectedProjectId });
  const {
    loadPermissionAutomation,
    permissionRevision,
    permissionInbox,
    permissionTimer,
    permissionAutomationByProject,
    pendingPermissions,
    permissionHistory,
    projectPendingPermissions,
    projectPermissionHistory,
    permissionCount,
    permissionAutomation,
    refreshPermissions,
    startPermissionUpdates,
    resolvePermission,
    serverResolvedPermission,
    updatePermissionTimer,
    permissionPopupSurface,
    notificationsPaused,
    setNotificationsPaused,
  } = permissionsController;
  const haltedSessionsController = createHaltedSessionsController({
    restoring: () => projectRestorePendingRoots.value.length > 0,
    unresolvedProjects: () =>
      projects.value
        .filter(
          (project) =>
            !terminalGroups.value.some((group) => group.projectId === project.id) ||
            !Object.hasOwn(driveConnectionsByProject.value, project.id),
        )
        .map((project) => project.id),
    episodes: () => {
      void conversationRenderRevision.value;
      return projectHaltedSessions(
        projects.value,
        terminalGroups.value,
        terminalDrawerChatsByProject.value,
        driveConnectionsByProject.value,
        conversationStates.peek(),
      );
    },
    paused: () => notificationsPaused.value,
    permissionVisible: () =>
      !notificationsPaused.value &&
      (Boolean(permissionInbox.visible()) ||
        (conversationOpen.value &&
          pendingPermissions().some(
            (item) => item.projectId === selectedProjectId.value && item.connection === conversationConnectionId.value,
          ))),
    open: (episode) => {
      if (episode.kind === 'chat') openGridAIChat(episode.projectId, episode.sessionId);
      else openTerminalInProject(`${episode.projectId}:${episode.sessionId}`);
    },
    answer: async (episode, answers) => {
      const current = projects.peek().find((item) => item.id === episode.projectId);
      if (!current || !episode.question) throw new Error('Question is no longer active.');
      await new Api(current.apiPath).answerTerminalQuestion(episode.sessionId, episode.question, answers);
    },
  });
  const projectLifecycleController = createProjectLifecycleController({
    projects,
    selectedProjectId,
    selectedView,
    loading,
    error,
    hideVerifiedByProject,
    providerCapabilities,
    defaultProviders,
    terminalDrawerSelected,
    terminalDrawerVisible,
    project: () => project(),
    rememberedProjectRoots: () => currentRememberedProjectRoots(),
    activateOpenProject,
    projectSelectionGeneration: () => projectSelectionGeneration,
    loadPermissionAutomation,
    setPermissionAutomation: (projectId, automation) => {
      permissionAutomationByProject.value = { ...permissionAutomationByProject.value, [projectId]: automation };
    },
    startPermissionUpdates,
    // The change-stream controller is created later; resolve it lazily.
    syncProjectChangeStreams: () => {
      syncProjectChangeStreams();
    },
    refreshProject,
    refreshCommands,
    refreshCustomViews,
    refreshDriveConnections,
    refreshTerminalDashboard: () => refreshTerminalDashboard(),
    restoreProjectSession,
    projectSessionInteractionVersion: () => projectSessionInteractionVersion,
    customViewFor,
    restoreCustomView,
    observeTerminalDrawer,
    requestProjectRefresh: (target) => projectTabRefresh.request(target),
    showToast,
  });
  const {
    projectDialogOpen,
    projectDialogError,
    remoteProjectDialogOpen,
    remoteProjectCheckouts,
    remoteProjectLoading,
    remoteProjectError,
    unhealthyServerRecovery,
    unhealthyServerRecoveryBusy,
    hs1MigrationProject,
    hs1MigrationBusy,
    hs1MigrationError,
    migrationJobsByRoot,
    migrationConnectionErrors,
    migrationJobDetails,
    migrationJobs,
    ticketSourceSetupProject,
    ticketSourceSetupError,
    providerConnections,
    providerSetupKind,
    providerEditingId,
    providerAccounts,
    providerAccountsError,
    signingOutAccount,
    unusedAccountSourceChoice,
    removingUnusedAccountSource,
    refreshProviderAccounts,
    signOutProviderAccount,
    requestUnusedAccountSourceRemoval,
    cancelUnusedAccountSourceRemoval,
    removeUnusedAccountSource,
    useGithubAccount,
    providerAccountChoice,
    useProviderAccount,
    requestProjectSourceRemoval,
    setProjectDefaultSource,
    setProjectSourceColor,
    relinkProjectGitSource,
    providerSettingsBusy,
    providerSettingsError,
    providerRemovingId,
    requestProviderRemoval,
    cancelProviderRemoval,
    removeExternalProvider,
    toggleProviderDisabled,
    refreshGitHubRepositories,
    loadEditingGitHubRepositories,
    chooseGitHubEnterprise,
    copyGitHubCode,
    reopenGitHubSignIn,
    githubAuth,
    ticketSourceSetupNavigation,
    createdGitTicketStore,
    ticketSourceRemoteBusy,
    ticketSourceRemoteError,
    projectRestoreFailures,
    selectedProjectRestoreRoot,
    projectsPendingActivation,
    projectRestoreRank,
    hs1SourceIdentity,
    retainProjectRestoreFailure,
    wireOpenedProject,
    presentOpenedProjectSetup,
    activateOpenedProject,
    openProject,
    retryProjectRestore,
    recoverUnhealthyProjectServer,
    chooseHs1TicketStore,
    importHs1Project,
    removeOldHs1Data,
    createProjectGitSource,
    connectCreatedGitRemote,
    refreshProviderConnections,
    saveExternalProvider,
    startGitHubSignIn,
    cancelGitHubSignIn,
    chooseProjectPath,
    chooseAndOpenProject,
    openProjectPicker,
    openRemoteProjectDialog,
    openRemoteCheckout,
  } = projectLifecycleController;
  let projectSelectionGeneration = 0;
  let projectActivationGeneration = 0,
    ticketCollectionGeneration = 0,
    commandRefreshGeneration = 0,
    projectSessionRestoreRun = 0;
  const ticketCollectionRefreshTask = createTrailingTask<TicketView>(250, (view) => {
    const query = sortedTicketQuery(ticketViewQuery(view));
    const boardPagesMissing =
      viewMode.value === 'board' &&
      isPerColumnBoardView(view, workspaceSearchActive()) &&
      Object.keys(boardColumnPages.value).length === 0;
    if (JSON.stringify(query) === JSON.stringify(ticketPageQuery.value) && !boardPagesMissing) {
      ticketCollectionGeneration += 1;
      ticketCollectionState.value = undefined;
      return;
    }
    void refreshTicketCollection(view);
  });
  const project = () => projects.value.find((item) => item.id === selectedProjectId.value);
  // Still-opening remembered roots stay remembered, in place, if the set is saved mid-restore (HS2-2BEJXD).
  const currentRememberedProjectRoots = () => [
    ...new Set([
      ...interleaveByRank(
        projects.value.map((item) => ({ root: item.root, rank: projectRestoreRank(item.id) })),
        (item) => item.rank,
        pendingRestoreEntries((root) => ({ root, rank: undefined as number | undefined })),
      ).map((item) => item.root),
      ...projectRestoreFailures.value.map((item) => item.root),
    ]),
  ];
  const settingsCategory = () => selectedSettingsCategory.value;
  function setSettingsCategory(_projectId: string, category: SettingsCategory) {
    selectedSettingsCategory.value = category;
  }
  const hideVerifiedColumn = () => hideVerifiedByProject.value[selectedProjectId.value] ?? false;
  const canUpdateSelected = () =>
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    selectedTicket.value ? (providerCapabilities.value[selectedTicket.value.connection_id]?.update ?? true) : false;
  const canEditStartedPhaseSelected = () => {
    const ticket = selectedTicket.value;
    const current = project();
    return Boolean(
      ticket &&
      current &&
      canUpdateSelected() &&
      defaultProviders.value[current.id]?.sources.some(
        (source) => source.connectionId === ticket.connection_id && source.provider === 'git',
      ),
    );
  };
  const noteCapabilities = () =>
    selectedTicket.value ? providerCapabilities.value[selectedTicket.value.connection_id] : undefined;
  const canAddNotes = () => noteCapabilities()?.notes ?? true;
  const canEditNotes = () => noteCapabilities()?.note_edit ?? canUpdateSelected();
  const canDeleteNotes = () => noteCapabilities()?.note_delete ?? canUpdateSelected();
  const canUseAttachments = () =>
    selectedTicket.value
      ? // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
        (providerCapabilities.value[selectedTicket.value.connection_id]?.attachments ?? true)
      : false;
  // An append-only provider (GitHub's assets repository) adds and serves files but cannot edit
  // existing ones; servers that predate the capability edit whatever they can attach (HS2-HSA64D).
  const attachmentsEditableFor = (connectionId: string) => {
    const capabilities = providerCapabilities.value[connectionId] as Capabilities | undefined;
    return capabilities?.attachment_edit ?? capabilities?.attachments ?? true;
  };
  const canEditAttachments = () =>
    canUseAttachments() && Boolean(selectedTicket.value && attachmentsEditableFor(selectedTicket.value.connection_id));
  const capabilitiesFor = (connectionId: string) => providerCapabilities.value[connectionId];
  const defaultProvider = () => defaultProviders.value[selectedProjectId.value];
  const newTicketSource = () =>
    resolveNewTicketSource(
      defaultProvider(),
      composerSource.value ?? lastTicketSourceByProject.value[selectedProjectId.value],
    );
  const canStageNewTicketAttachments = () => {
    const capabilities = newTicketSource()?.capabilities;
    return (capabilities?.create ?? true) && (capabilities?.attachments ?? true);
  };
  const tagSuggestions = () => [...new Set(tickets.value.flatMap((ticket) => ticket.tags))];
  const api = () => new Api(project()?.apiPath ?? '');
  const customViewsFor = (projectId = selectedProjectId.value) => customViewsByProject.value[projectId] ?? [];
  function customViewFor(view: TicketView, projectId = selectedProjectId.value) {
    const id = customTicketViewKey(view);
    return id ? customViewsFor(projectId).find((item) => item.id === id) : undefined;
  }
  const {
    savedViewDialogOpen,
    savedViewDialogSession,
    savedViewDialogMode,
    savedViewName,
    savedViewHelpOpen,
    savedViewBusy,
    savedViewError,
    savedViewMenu,
    savedViewDeleteTargetId,
    savedViewDeleteBusy,
    savedViewDeleteError,
    openSavedViewDialog,
    openSavedViewRename,
    closeSavedViewDialog,
    focusSavedViewQuery,
    savedViewSearchModel,
    savedViewSearchTokenOffset,
    saveSavedView,
    openSavedViewDelete,
    closeSavedViewDelete,
    deleteSavedView,
  } = createSavedViewsController({
    project,
    customViewsByProject,
    customViewsFor,
    customViewFor,
    selectedView,
    searchQuery,
    searchTokens,
    // The workspace search controller is created later; resolve it lazily.
    availableSearchTags: () => availableSearchTags(),
    selectTicketView,
    showToast,
  });
  // A shared view's query is an implicit scope (HS2-50R1YQ): it never populates the search bar, and any
  // search-bar query narrows it as `(view query) AND (search-bar query)`.
  function activateCustomView() {
    searchMatchKeys.value = undefined;
    resetProgressiveTicketRendering();
    scheduleTicketSearch();
  }
  /** Restore a selected shared view; sessions saved before HS2-50R1YQ copied its query into the bar, so drop it. */
  function restoreCustomView(view: CustomView) {
    if (orderedSearchText(searchQuery.value, searchTokens.value, () => true).trim() === view.query.trim())
      workspaceSearchModel.clear();
    activateCustomView();
  }
  function selectTicketView(next: TicketView, { refresh = true }: { refresh?: boolean } = {}) {
    ticketCollectionRefreshTask.cancel();
    ticketCollectionGeneration += 1;
    const definition = customViewFor(next);
    if (customTicketViewKey(next) && !definition) return;
    const finishTiming = beginInteractionTiming('ticket-view-change', { view: next }),
      previous = selectedView.value,
      nextSelection = selectionAfterTicketViewChange(previous, next, selectedTicketSlugs.value);
    batch(() => {
      if (nextSelection.length !== selectedTicketSlugs.value.length) {
        cancelTicketDrafts();
        selectedCorruptKey.value = undefined;
        selectedTicketSlugs.value = nextSelection;
        ticketSelectionAnchor = undefined;
        selectedTicket.value = null;
      }
      resetProgressiveTicketRendering();
      resetBoardColumnPages();
      selectedView.value = next;
      if (definition) activateCustomView();
      else if (customTicketViewKey(previous)) searchMatchKeys.value = undefined;
      ticketCollectionState.value =
        refresh && !definition && next !== 'errors' && !workspaceSearchActive()
          ? { projectId: selectedProjectId.value, view: next, status: 'loading' }
          : undefined;
    });
    if (refresh && !definition && next !== 'errors') {
      if (workspaceSearchActive()) void refreshTicketSearch();
      else ticketCollectionRefreshTask.schedule(next);
    }
    scheduleProjectSessionPersistence();
    finishTiming();
  }

  async function refreshTicketCollection(view: TicketView) {
    const current = project(),
      query = sortedTicketQuery(ticketViewQuery(view)),
      generation = ++ticketCollectionGeneration;
    if (!current) return;
    const active = () =>
      generation === ticketCollectionGeneration && project()?.id === current.id && selectedView.value === view;
    try {
      // Lists and board columns both page in the active sort: the server's keyset cursor is bound to that
      // sort, so page one is the head of the order the user sees and Load more appends below it (HS2-9ZZ8QQ).
      const board = boardRefreshSpec(view, current.id),
        index = await loadProjectTicketRefresh(new Api(current.apiPath), current.id, query, board);
      if (!active()) return;
      if (index.tickets)
        partialSourcesByProject.value = {
          ...partialSourcesByProject.value,
          [current.id]: Boolean(index.sourceErrors?.length),
        };
      if (index.tickets) {
        const mergedTickets = mergeRetainedCreatedRows(
          index.tickets,
          pendingCreatedTickets.retain(current.id, index.tickets),
        );
        boardColumnPages.value = boardPagesWithVisibleLimit(index.boardPages ?? {});
        ticketPageQuery.value = query;
        tickets.value = mergedTickets;
        ticketNextCursor.value = index.nextCursor;
        ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: mergedTickets };
        if (index.ticketCounts) {
          ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: index.ticketCounts };
          recordAuthoritativeTicketTrend(current.id, index.ticketCounts);
        }
        scheduleClaimLeaseExpiry();
      }
      ticketCollectionState.value = index.ticketsError ? { projectId: current.id, view, status: 'error' } : undefined;
      corruptTickets.value = index.corruptTickets ?? [];
      error.value = [index.ticketsError, index.corruptTicketsError].filter(Boolean).join(' · ');
    } catch (reason) {
      if (active()) {
        ticketCollectionState.value = { projectId: current.id, view, status: 'error' };
        error.value = reason instanceof Error ? reason.message : String(reason);
      }
    }
  }
  const terminalDashboardController = createTerminalDashboardController({
    projects,
    selectedProjectId: () => selectedProjectId.value,
    terminalGroups,
    terminalDashboardLoading,
    terminalDashboardMessage,
    terminalVisibility,
    terminalVisibilityDialogScope: () => terminalVisibilityDialogScope.value,
    terminalVisibilityFilter: () => terminalVisibilityFilter.value,
    terminalDrawerOrderByProject,
    terminalDrawerChatsByProject: () => terminalDrawerChatsByProject.value,
    terminalNames: () => terminalNames.value,
    hasDriveConnections: (projectId) => Object.hasOwn(driveConnectionsByProject.value, projectId),
    refreshDriveConnections,
    reconcileTerminalNames,
    workspaceTerminalGroups: () => workspaceTerminalGroups(),
    aiToolLabel,
    showToast,
    error,
  });
  const {
    terminalSession,
    terminalHiddenKeys,
    terminalHiddenCount,
    persistTerminalVisibility,
    terminalVisibilityScopeFor,
    terminalKeysForVisibilityDialog,
    drawerTabOrder,
    currentDrawerTabIds,
    persistDrawerTabOrder,
    refreshTerminalDashboard,
    clearTerminalHalt,
  } = terminalDashboardController;
  // The workspace grid and drawer sizes follow whichever element currently renders them: a re-render
  // that replaces the measured node must re-bind the observer, or the size freezes (HS2-0PF13V).
  const terminalDashboardSizeObserver = createTrackedSizeObserver({
    // The drawer's own grid carries the same measure marker; only the workspace dashboard counts here.
    find: () =>
      Array.from(document.querySelectorAll<HTMLElement>('[data-terminal-grid-measure="true"]')).find(
        (element) => !element.closest('[data-component="terminal-drawer"]'),
      ),
    enabled: () => shellMode.value === 'terminals',
    onSize: (next) => {
      const previous = terminalDashboardSize.value;
      if (next.width !== previous.width || next.height !== previous.height) terminalDashboardSize.value = next;
    },
  });
  function observeTerminalDashboard() {
    queueMicrotask(terminalDashboardSizeObserver.sync);
  }
  function updateTerminalDrawerBounds(
    target: HTMLElement,
    rect: Pick<DOMRectReadOnly, 'width' | 'height'> = target.getBoundingClientRect(),
  ) {
    const next = { width: Math.max(1, Math.floor(rect.width)), height: Math.max(1, Math.floor(rect.height)) },
      previous = terminalDrawerBounds.value;
    if (next.width !== previous.width || next.height !== previous.height) terminalDrawerBounds.value = next;
  }
  // The drag owns the drawer size while it runs and applies the final bounds itself on release.
  const terminalDrawerSizeObserver = createTrackedSizeObserver({
    find: () => document.querySelector<HTMLElement>('[data-terminal-drawer-measure="true"] .terminal-drawer__content'),
    enabled: () => terminalDrawerVisible.value,
    paused: () => appRegionResizeDrag?.id === 'app-bottom-drawer',
    onSize: (size, target) => {
      updateTerminalDrawerBounds(target, size);
    },
  });
  function observeTerminalDrawer() {
    queueMicrotask(terminalDrawerSizeObserver.sync);
  }
  const terminalDrawer = createTerminalDrawerController({
    projects,
    selectedProjectId,
    terminalGroups,
    terminalDrawerVisible,
    terminalDrawerMounted,
    terminalDrawerTransitioning,
    terminalDrawerSelected,
    terminalDrawerChatsByProject,
    terminalDashboardLoading,
    terminalDashboardMessage,
    terminalVisibility,
    terminalContextMenu,
    appTabContextMenu,
    error,
    project: () => project(),
    conversationConnectionId,
    driveConnectionsByProject,
    drivePendingByProject,
    aiToolLabel,
    exitMobileTerminalFocus,
    settleTerminalDrawerGeometry,
    observeTerminalDrawer,
    disconnectTerminalDrawerSize: () => {
      terminalDrawerSizeObserver.disconnect();
    },
    refreshTerminalDashboard,
    terminalSession,
    currentDrawerTabIds,
    persistDrawerTabOrder,
    persistTerminalVisibility,
    saveTerminalName,
    activateOpenProject,
    refreshActivatedProject,
    refreshProject,
    refreshCommands,
    setShellMode,
  });
  const {
    focusDrawerTab,
    setTerminalDrawerVisible,
    selectDrawerItem,
    openTerminalInProject,
    openGridAIChat,
    createProjectTerminal,
    createShellCommandTerminal,
    createDrawerAIChat,
    closeDrawerTabIds,
    closeTerminalIds,
    closeDrawerAIChat,
  } = terminalDrawer;
  const {
    requestProjectClose,
    confirmProjectClose,
    closeAllProjectResources,
    restoreBorrowedProjectCloseTerminal,
    cancelProjectClose,
  } = createProjectCloseController({
    projects,
    projectCloseDialog,
    terminalGroups,
    terminalDrawerChatsByProject,
    driveConnectionsByProject,
    conversationStates,
    aiToolLabel,
    closeProjectIds,
  });
  function setShellMode(mode: ProjectTabBarMode) {
    if (terminalPreviewClickTimer !== undefined) {
      window.clearTimeout(terminalPreviewClickTimer);
      terminalPreviewClickTimer = undefined;
    }
    if (mode !== 'stats') statsProjectId.value = undefined;
    if (mode !== 'terminals') terminalGridFullscreen.value = false;
    if (mode === 'terminals' && shellMode.value !== 'terminals') {
      terminalRailScreen.value = 'root';
    }
    shellMode.value = mode;
    if (mode === 'terminals' || mode === 'stats')
      for (const current of projects.value)
        if (current.id !== selectedProjectId.value && !Object.hasOwn(ticketRowsByProject.value, current.id))
          void projectTabRefresh.request(current);
    magnifiedTerminalKey.value = undefined;
    terminalContextMenu.value = undefined;
    if (mode === 'terminals') {
      void refreshTerminalDashboard();
      observeTerminalDashboard();
    } else terminalDashboardSizeObserver.disconnect();
  }
  // Switch the workspace to a list/board/notifications/settings view from anywhere (a keyboard shortcut
  // may fire while a terminal grid or stats overlay is active), so return to the project shell first and
  // mirror the set-view-mode click handler's progressive-render reset + preference persistence (HS2-9SHYWD).
  function switchWorkspaceView(mode: WorkspaceViewMode) {
    setShellMode('project');
    resetProgressiveTicketRendering();
    viewMode.value = mode;
    persistWorkspacePreferences();
    if (mode === 'list' || mode === 'board') void refreshProject({ showLoading: loading.value });
  }
  function selectTerminalRailProject(next: string) {
    if (next === selectedProjectId.value || !projects.value.some((item) => item.id === next)) return;
    const activated = activateOpenProject(next);
    if (!activated) return;
    terminalRailScreen.value = 'root';
    void refreshActivatedProject(activated, false);
  }
  // Switch the active project tab (shared by the desktop project tab click and the mobile project Select —
  // HS2-4C5RM7). `next` is a tab id: a project id, or a restore-failure tab id.
  function selectProjectTab(next: string) {
    projectSelectionGeneration += 1;
    exitMobileTerminalFocus();
    setShellMode('project');
    const failure = projectRestoreFailures.value.find((item) => projectRestoreTabId(item.root) === next);
    if (failure) {
      selectedProjectRestoreRoot.value = failure.root;
      saveActiveProjectRoot(localStorage, failure.root);
      return;
    }
    selectedProjectRestoreRoot.value = '';
    if (next === selectedProjectId.value) {
      // Re-selecting the current project from Notifications or Settings returns to the ticket view the user
      // last had open (list or columns), like a home affordance (HS2-42T028).
      if (viewMode.value === 'settings' || viewMode.value === 'notifications')
        switchWorkspaceView(ticketViewMode.value);
      return;
    }
    const activated = activateOpenProject(next);
    if (activated) void refreshActivatedProject(activated, terminalDrawerVisible.value);
  }
  const status = (value?: string): TicketStatus =>
    ['not_started', 'started', 'completed', 'verified', 'backlog', 'archive', 'deleted'].includes(value ?? '')
      ? (value as TicketStatus)
      : 'not_started';
  const priority = (value?: string): TicketPriority => priorityFromWire(value);
  const appRegionSize = (id: AppRegionId) =>
    id === 'app-left-rail'
      ? sidebarSize.value
      : id === 'app-right-rail'
        ? inspectorSize.value
        : terminalDrawerMaximized.value
          ? terminalDrawerMax.value
          : Math.max(terminalDrawerMin.value, Math.min(terminalDrawerSize.value, terminalDrawerMax.value));
  function setAppRegionSize(id: AppRegionId, size: number) {
    if (id === 'app-bottom-drawer') {
      const next = Math.min(
        terminalDrawerMax.value,
        Math.max(terminalDrawerMin.value, normalizeAppRegionSize(id, size)),
      );
      if (terminalDrawerMaximized.value && next === terminalDrawerMax.value) return;
      terminalDrawerMaximized.value = false;
      terminalDrawerSize.value = saveAppRegionSize(localStorage, id, next);
      return;
    }
    const next = saveAppRegionSize(localStorage, id, size);
    if (id === 'app-left-rail') sidebarSize.value = next;
    else inspectorSize.value = next;
  }
  function activeWorkspaceSort() {
    return workspaceSorts.value[sortableWorkspaceView(viewMode.value)];
  }
  effect(() => {
    const mode = viewMode.value;
    if (mode === 'list' || mode === 'board') ticketViewMode.value = mode;
  });
  function persistWorkspacePreferences() {
    saveWorkspacePreferences(localStorage, {
      viewMode: viewMode.value,
      ticketViewMode: ticketViewMode.value,
      sorts: workspaceSorts.value,
      sidebarVisible: sidebarVisible.value,
      inspectorVisible: inspectorVisible.value,
      commandGroupExpanded: commandGroupExpanded.value,
      commandGroupsCollapsed: commandGroupsCollapsed.value,
    });
  }
  function draftScope(kind: 'composer' | 'not-working', projectId = selectedProjectId.value) {
    return `${kind}:${projectId}`;
  }
  function persistProjectSessionNow() {
    if (initialProjectRestorePending.value) return;
    if (selectedProjectRestoreRoot.value) {
      saveActiveProjectRoot(localStorage, selectedProjectRestoreRoot.value);
      return;
    }
    const current = project();
    if (!current || restoringProjectSession) return;
    const feedbackNote = selectedTicket.value?.notes.find(
      (note) =>
        note.id === readerEditingNoteId.value && (note.kind === 'feedback_needed' || note.kind === 'feedback_draft'),
    );
    saveActiveProjectRoot(localStorage, current.root);
    saveProjectWorkspaceSession(localStorage, current.id, {
      selectedView: selectedView.value,
      selectedTicketSlugs: [...selectedTicketSlugs.value],
      searchOpen: searchOpen.value,
      searchQuery: searchQuery.value,
      inspectorTab: inspectorTab.value,
      readerTab: readerTab.value,
      composer: {
        open: composerExpanded.value,
        title: composerTitle.value,
        details: composerDetails.value,
        category: composerCategory.value,
        upNext: composerUpNext.value,
        attachments: composerAttachments.value.map(({ id, name }) => ({ id, name })),
      },
      composingNote: composingNote.value,
      newNoteDraft: newNoteDraft.value,
      feedbackReplies: readerInlineFeedbackReplies.value,
      feedbackSelections: readerFeedbackChoiceSelections.value,
      feedbackNoteId: feedbackNote?.id,
      feedbackDraft: feedbackNote ? readerNoteDraft.value : '',
      notWorking: notWorkingTarget.value.slug
        ? {
            ticketId: notWorkingTarget.value.ticketId,
            slug: notWorkingTarget.value.slug,
            connectionId: notWorkingTarget.value.connectionId,
            mode: notWorkingTarget.value.mode,
            note: notWorkingNote.value,
            attachments: notWorkingFiles.value.map(({ id, name }) => ({ id, name })),
          }
        : undefined,
    });
  }
  function cacheActiveProjectProjection() {
    const id = selectedProjectId.value;
    if (!id || !projects.value.some((item) => item.id === id)) return;
    // Leaving a cold project before its first load finished must not cache its empty placeholder as
    // a warm projection: the next activation would paint "No tickets" instead of loading (HS2-AZZ9TF).
    if (loading.value && !Object.hasOwn(ticketRowsByProject.value, id)) return;
    ticketRowsByProject.value = { ...ticketRowsByProject.value, [id]: tickets.value };
    ticketCursorsByProject.value = {
      ...ticketCursorsByProject.value,
      [id]: { cursor: ticketNextCursor.value, query: ticketPageQuery.value },
    };
    boardPagesByProject.value = { ...boardPagesByProject.value, [id]: boardColumnPages.value };
    projectProjectionById.value = {
      ...projectProjectionById.value,
      [id]: {
        corruptTickets: corruptTickets.value,
        repository: repository.value,
        repositoryError: repositoryError.value,
        commandDefinitions: commandDefinitions.value,
        commandRuns: commandRuns.value,
        searchMatchKeys: searchMatchKeys.value,
      },
    };
  }
  function activateOpenProject(next: string): { project: Project; generation: number; cached: boolean } | undefined {
    const nextProject = projects.value.find((item) => item.id === next);
    if (!nextProject) return;
    if (next !== selectedProjectId.value) parkProjectViewports(selectedProjectId.value);
    projectTabRefresh.activate(next);
    const finishTiming = beginInteractionTiming('project-change', { project: next });
    persistProjectSessionNow();
    cacheActiveProjectProjection();
    resetTicketComposer(false);
    resetProgressiveTicketRendering();
    resetBoardColumnPages();
    const rows = ticketRowsByProject.value[next],
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      cached = rows !== undefined,
      projection = projectProjectionById.value[next],
      stored = loadProjectWorkspaceSession(localStorage, next),
      generation = ++projectActivationGeneration,
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      live = new Set(rows?.map((ticket) => ticket.slug) ?? []),
      selection = stored?.selectedTicketSlugs.filter((slug) => live.has(slug)) ?? [],
      drawerActivation = terminalDrawerActivation(localStorage, next);
    batch(() => {
      selectedProjectRestoreRoot.value = '';
      selectedProjectId.value = drawerActivation.projectId;
      terminalDrawerSelected.value = drawerActivation.selectedId;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      tickets.value = rows ?? [];
      const position = ticketCursorsByProject.value[next];
      ticketNextCursor.value = position?.cursor;
      // Without the issuing query the cached cursor is unusable; an empty query also forces the next view
      // refresh to reload page one instead of trusting a query it never fetched (HS2-38MPMC).
      ticketPageQuery.value = position?.query ?? {};
      boardColumnPages.value = boardPagesByProject.value[next] ?? {};
      ticketCollectionState.value = undefined;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      corruptTickets.value = projection?.corruptTickets ?? [];
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      repository.value = projection?.repository ?? null;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      repositoryError.value = projection?.repositoryError ?? '';
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      commandDefinitions.value = projection?.commandDefinitions ?? [];
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      commandRuns.value = projection?.commandRuns ?? [];
      commandSettingsEditingId.value = undefined;
      selectedView.value =
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
        stored?.selectedView === 'errors' && !projection?.corruptTickets.length
          ? 'all'
          : (stored?.selectedView ?? 'all');
      searchOpen.value = stored?.searchOpen ?? false;
      searchQuery.value = stored?.searchQuery ?? '';
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      searchMatchKeys.value = projection?.searchMatchKeys;
      selectedCorruptKey.value = undefined;
      selectedTicket.value = null;
      selectedTicketSlugs.value = selection;
      ticketSelectionAnchor = selection[0];
      error.value = '';
      loading.value = !cached;
    });
    markProjectWarm(next);
    aiConfigurationController.restoreAiConfiguration(nextProject);
    scheduleClaimLeaseExpiry();
    saveActiveProjectRoot(localStorage, nextProject.root);
    finishTiming();
    return { project: nextProject, generation, cached };
  }
  /** Mark a project most recently used and drop the resident projection of any LRU-evicted project. */
  function markProjectWarm(id: string) {
    const evicted = warmProjects.touch(id).filter((item) => item !== selectedProjectId.value);
    if (!evicted.length) return;
    const drop = new Set(evicted);
    const keep = <Value,>(record: Record<string, Value>) =>
      Object.fromEntries(Object.entries(record).filter(([key]) => !drop.has(key)));
    batch(() => {
      ticketRowsByProject.value = keep(ticketRowsByProject.value);
      ticketCursorsByProject.value = keep(ticketCursorsByProject.value);
      boardPagesByProject.value = keep(boardPagesByProject.value);
      projectProjectionById.value = keep(projectProjectionById.value);
    });
    for (const item of evicted) aiConfigurationController.forgetAiConfiguration(item);
  }
  /** Whether the all-project terminal dashboard snapshot already holds this project's group. */
  function terminalGroupLoaded(projectId: string) {
    return terminalGroups.value.some((group) => group.projectId === projectId);
  }
  async function refreshActivatedProject(
    activation: { project: Project; generation: number; cached: boolean },
    includeTerminals: boolean,
  ) {
    const { project: current, generation, cached } = activation;
    if (cached) {
      await afterBrowserPaint();
      if (generation !== projectActivationGeneration || project()?.id !== current.id) return;
    }
    const firstActivation = projectsPendingActivation.has(current.id);
    if (firstActivation) presentOpenedProjectSetup(current);
    // Settings projections are single-project signals, so they are always reloaded for the new project;
    // outside the Settings view that reload is invisible background work and must not flash the busy UI.
    const category = settingsCategory(),
      settingsQuiet = cached && viewMode.value !== 'settings',
      settingsRefresh =
        category === 'sources'
          ? refreshProviderConnections(current, settingsQuiet)
          : category === 'terminals'
            ? refreshTerminalSettings(current, settingsQuiet)
            : category === 'lifecycle'
              ? refreshTrashSettings(current, settingsQuiet)
              : Promise.resolve(),
      // A warm project restores its cached AI tool inventory at activation; only a cold project (or the
      // open AI settings page, which wants an authoritative answer) loads it here (HS2-AZZ9TF).
      aiRefresh =
        (category === 'ai' && viewMode.value === 'settings') ||
        !aiConfigurationController.restoreAiConfiguration(current)
          ? refreshAiConfiguration(current)
          : Promise.resolve(),
      // A cached activation revalidates silently: the rows are already painted and kept current by the
      // project's live change stream, so the switch must not show loading or the busy indicator.
      quiet = cached;
    await Promise.all([
      refreshProject({ showLoading: !cached, quiet, refreshRepository: true }),
      refreshCommands(current, quiet),
      refreshCustomViews(current, quiet),
      settingsRefresh,
      aiRefresh,
      ...(firstActivation ? [refreshDriveConnections(current, true, quiet)] : []),
      // The terminal dashboard snapshot spans every open project; switching tabs only needs a load when
      // this project's group has never been loaded (terminal creation/close refresh it explicitly).
      ...(includeTerminals && !terminalGroupLoaded(current.id) ? [refreshTerminalDashboard()] : []),
    ]);
    if (generation !== projectActivationGeneration || project()?.id !== current.id) return;
    await restoreProjectSession(current, generation);
    const restored = customViewFor(selectedView.value, current.id);
    if (restored) restoreCustomView(restored);
    else if (customTicketViewKey(selectedView.value)) selectedView.value = 'all';
    if (includeTerminals) observeTerminalDrawer();
  }
  function scheduleProjectSessionPersistence() {
    if (restoringProjectSession) return;
    projectSessionInteractionVersion += 1;
    if (projectSessionTimer !== undefined) window.clearTimeout(projectSessionTimer);
    projectSessionTimer = window.setTimeout(() => {
      projectSessionTimer = undefined;
      persistProjectSessionNow();
    }, 700);
  }
  let previousSearchOpen = searchOpen.value;
  effect(() => {
    const next = searchOpen.value;
    if (next === previousSearchOpen) return;
    previousSearchOpen = next;
    if (!next) searchHelpOpen.value = false;
    scheduleProjectSessionPersistence();
  });
  async function restoreProjectSession(current: Project, generation = projectActivationGeneration) {
    const stored = loadProjectWorkspaceSession(localStorage, current.id),
      active = () => generation === projectActivationGeneration && project()?.id === current.id;
    if (!stored || !active()) return;
    const run = ++projectSessionRestoreRun,
      restoreComposerOpen =
        stored.composer.open &&
        initialProjectRestoreComplete &&
        !ticketSourceSetupProject.value &&
        !hs1MigrationProject.value;
    restoringProjectSession = true;
    try {
      // If the user opened the new-ticket composer during this activation's async refresh window, leave it
      // alone — a delayed session restore must not close a composer the user just opened (HS2-T1F2VT).
      const composerOpenedByUser = composerExpanded.value;
      selectedView.value =
        stored.selectedView === 'errors' && !corruptTickets.value.length ? 'all' : stored.selectedView;
      searchOpen.value = stored.searchOpen;
      replaceTicketSearch(workspaceSearchModel, stored.searchQuery);
      searchMatchKeys.value = undefined;
      inspectorTab.value = stored.inspectorTab;
      readerTab.value = stored.readerTab;
      if (!composerOpenedByUser) {
        composerExpanded.value = false;
        composerTitle.value = stored.composer.title;
        composerDetails.value = stored.composer.details;
        composerCategory.value = stored.composer.category;
        composerUpNext.value = stored.composer.upNext;
        const attachments = await loadDraftFiles(draftScope('composer', current.id), stored.composer.attachments).catch(
          () => [],
        );
        if (!active()) return;
        composerAttachments.value = attachments;
        if (restoreComposerOpen) {
          showQuickTicketComposer(document);
          composerExpanded.value = true;
          requestAnimationFrame(() => requestAnimationFrame(() => focusQuickTicketComposerTitle(document)));
        }
      }
      const live = new Set(tickets.value.map((ticket) => ticket.slug)),
        selection = stored.selectedTicketSlugs.filter((slug) => live.has(slug));
      selectedTicketSlugs.value = selection;
      ticketSelectionAnchor = selection[0];
      if (selection.length === 1) {
        const row = tickets.value.find((ticket) => ticket.slug === selection[0]);
        if (row) {
          const full = (await new Api(current.apiPath).checkoutTicket(current.id, row.qualified_id)).ticket;
          if (!active()) return;
          presentTicket(full);
        }
      }
      composingNote.value = selection.length === 1 && stored.composingNote;
      newNoteDraft.value = composingNote.value ? stored.newNoteDraft : '';
      readerInlineFeedbackReplies.value = selection.length === 1 ? stored.feedbackReplies : {};
      readerFeedbackChoiceSelections.value = selection.length === 1 ? stored.feedbackSelections : {};
      const feedbackNote = selectedTicket.value?.notes.find(
        (note) =>
          note.id === stored.feedbackNoteId && (note.kind === 'feedback_needed' || note.kind === 'feedback_draft'),
      );
      readerEditingNoteId.value = feedbackNote?.id;
      readerNoteDraft.value = feedbackNote ? stored.feedbackDraft : '';
      if (stored.notWorking) {
        const row = tickets.value.find((ticket) => ticket.id === stored.notWorking!.ticketId);
        if (row) {
          const files = await loadDraftFiles(
            draftScope('not-working', current.id),
            stored.notWorking.attachments,
          ).catch(() => []);
          if (!active()) return;
          notWorkingTarget.value = {
            projectId: current.id,
            apiPath: current.apiPath,
            ticketId: row.id,
            slug: row.slug,
            connectionId: row.connection_id,
            mode: stored.notWorking.mode ?? 'not-working',
          };
          notWorkingNote.value = stored.notWorking.note;
          notWorkingFiles.value = files;
        }
      }
      if (searchQuery.value.trim()) void refreshTicketSearch();
    } finally {
      if (run === projectSessionRestoreRun) restoringProjectSession = false;
    }
  }
  // Mobile single-column layout (HS2-ZK51WP). Below the desktop size floor the sidebar and inspector
  // overlay the single main column instead of taking horizontal space. Each rail's `collapsed` signal
  // is the one state the Workbench renders and `wireWorkbench` drives (HS2-Y1B1Y1): on a phone both
  // start collapsed and Kerf keeps the overlays exclusive, closes them on Escape or an outside press,
  // traps Tab inside the open one, and returns focus to its opener; the persisted desktop
  // sidebar/inspector preferences are untouched by that ephemeral state and come back with the
  // side-by-side layout.
  // A touch-first device (coarse primary pointer) gets terminal Copy/Paste even at desktop widths, where
  // xterm offers no touch selection or Paste callout (HS2-5DHHPV). A trackpad-equipped tablet reports a
  // fine pointer and keeps the plain desktop chrome.
  const coarsePointerQuery = window.matchMedia('(pointer: coarse)'),
    coarsePointer = signal(coarsePointerQuery.matches);
  coarsePointerQuery.addEventListener('change', (event) => {
    coarsePointer.value = event.matches;
  });
  const viewportMobile = signal(isMobileViewport(window.innerWidth)),
    sidebarCollapsed = signal(viewportMobile.value || !sidebarVisible.value),
    inspectorCollapsed = signal(viewportMobile.value || !inspectorVisible.value);
  function setInspectorVisible(visible: boolean) {
    inspectorCollapsed.value = !visible;
    if (viewportMobile.value) return;
    inspectorVisible.value = visible;
    persistWorkspacePreferences();
  }
  function setSidebarVisible(visible: boolean) {
    sidebarCollapsed.value = !visible;
    if (viewportMobile.value) return;
    sidebarVisible.value = visible;
    persistWorkspacePreferences();
  }
  /** Show the inspector for a ticket the user tapped or just created: on a phone the overlay opens (Kerf
   * closes the sidebar); larger layouts keep the inspector the user chose (HS2-N7RPFP, HS2-QFW2A7). */
  function revealInspectorOverlay() {
    if (viewportMobile.value) inspectorCollapsed.value = false;
  }
  window.addEventListener('resize', () => {
    const mobile = isMobileViewport(window.innerWidth);
    if (mobile !== viewportMobile.value) {
      viewportMobile.value = mobile;
      if (mobile) {
        sidebarCollapsed.value = true;
        inspectorCollapsed.value = true;
      } else {
        exitMobileTerminalFocus();
        // Restore the desktop preferences once the rails render inline again: while a rail still
        // presents as an overlay, Kerf's exclusive-overlay rule would close the other one.
        requestAnimationFrame(() => {
          if (viewportMobile.value) return;
          sidebarCollapsed.value = !sidebarVisible.value;
          inspectorCollapsed.value = !inspectorVisible.value;
        });
      }
    }
    if (mobile) {
      syncMobileTerminalViewport();
      syncMobileViewportGeometry();
    }
  });
  window.visualViewport?.addEventListener('resize', syncMobileTerminalViewport);
  window.visualViewport?.addEventListener('scroll', syncMobileTerminalViewport);
  window.visualViewport?.addEventListener('resize', syncMobileViewportGeometry);
  window.visualViewport?.addEventListener('scroll', syncMobileViewportGeometry);
  const ticketSnapshot = (slug: string) =>
    tickets.value.find((item) => item.slug === slug) as TicketSnapshot | undefined;
  const ticketSearchKey = (ticket: WireTicketRow) => `${ticket.connection_id}:${ticket.native_id}`;
  const ago = (value?: string) => {
    if (!value) return 'Recently';
    const seconds = Math.max(0, (Date.now() - Date.parse(value)) / 1000);
    if (seconds < 60) return 'Now';
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    return `${Math.floor(seconds / 86400)}d ago`;
  };
  const pendingOperations = (ids?: string[]) =>
    ids?.map((id) => providerOutboxOperations.value[id]).filter((item) => item !== undefined) ?? [];
  const row = (ticket: WireTicketRow): TicketRowProps => ({
    slug: ticket.slug,
    title: ticket.title,
    status: status(ticket.status),
    startedPhase: ticket.started_phase,
    priority: priority(ticket.priority),
    category: ticket.category ?? 'issue',
    source: defaultProviders.value[project()?.id ?? '']?.sources.find(
      (source) => source.connectionId === ticket.connection_id,
    ),
    showSourceIcon: (defaultProviders.value[project()?.id ?? '']?.sources.length ?? 0) > 1,
    tags: ticket.tags,
    upNext: ticket.up_next,
    upNextEligible: ticket.status === 'not_started' || ticket.status === 'started',
    feedbackNeeded: ticket.feedback_needed,
    blocked: hasUnresolvedBlocker(ticket),
    selected: selectedTicketSlugs.value.includes(ticket.slug),
    cutPending: Boolean(
      clipboard?.cut &&
      clipboard.source.id === project()?.id &&
      clipboard.tickets.some((item) => item.slug === ticket.slug),
    ),
    busy: isTicketActivelyWorkedOn(ticket),
    claimEta: claimEtaPresentation(ticket, claimEtaClock.value),
    agentName: ticket.worker_label || ticket.claimed_by || 'AI',
    updatedLabel: ago(ticket.updated_at),
    latestConfidence: ticket.latest_confidence,
    pendingOperationCount: ticket.pending_operation_ids?.length ?? 0,
    pendingSyncState: pendingOperations(ticket.pending_operation_ids).some((item) => item.state === 'needs_attention')
      ? 'needs_attention'
      : pendingOperations(ticket.pending_operation_ids).some((item) => item.state === 'sending')
        ? 'sending'
        : pendingOperations(ticket.pending_operation_ids).some((item) => item.state === 'rate_limited')
          ? 'rate_limited'
          : 'queued',
  });

  async function refreshProviderOutbox(current: Project) {
    const connections =
      defaultProviders.value[current.id]?.sources
        .filter((source) => source.provider === 'jira')
        .map((source) => source.connectionId) ?? [];
    if (!connections.length) return;
    const client = new Api(current.apiPath);
    const results = await Promise.allSettled(connections.map((connection) => client.providerOutbox(connection)));
    const next = { ...providerOutboxOperations.value };
    for (const result of results)
      if (result.status === 'fulfilled') for (const operation of result.value) next[operation.operation_id] = operation;
    providerOutboxOperations.value = next;
  }

  async function changeProviderOutbox(connectionId: string, operationId: string, action: 'retry' | 'discard') {
    const current = project();
    if (!current) return;
    const client = new Api(current.apiPath);
    try {
      const result =
        action === 'retry'
          ? await client.retryProviderOutbox(connectionId, operationId)
          : await client.discardProviderOutbox(connectionId, operationId);
      providerOutboxOperations.value = { ...providerOutboxOperations.value, [operationId]: result };
      await projectTabRefresh.request(current);
      if (selectedTicket.value?.connection_id === connectionId && selectedTicket.value.native_id === result.native_id)
        selectedTicket.value = (await client.checkoutTicket(current.id, `${connectionId}:${result.native_id}`)).ticket;
      showToast(
        result.state === 'confirmed'
          ? 'Jira already confirmed this edit.'
          : action === 'retry'
            ? 'Jira edit queued for retry.'
            : 'Local edit discarded.',
      );
    } catch (reason) {
      showToast(reason instanceof Error ? reason.message : String(reason));
    }
  }
  const retryProviderOutbox = (connectionId: string, operationId: string) =>
    changeProviderOutbox(connectionId, operationId, 'retry');
  const discardProviderOutbox = (connectionId: string, operationId: string) =>
    changeProviderOutbox(connectionId, operationId, 'discard');

  function projectTabTicketRows(projectId: string) {
    return projectId === selectedProjectId.value ? tickets.value : (ticketRowsByProject.value[projectId] ?? []);
  }
  function projectTicketCounts(projectId: string) {
    const exact = ticketCountsByProject.value[projectId];
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    if (exact) return exact;
    const rows = projectTabTicketRows(projectId),
      work = projectTabTicketState(rows),
      carry = ticketTrendByProject.value[projectId],
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      completionTrend = carry ? [...carry.trend] : ticketCompletionTrend(rows);
    return {
      total: rows.length,
      queued: rows.filter(isQueuedTicket).length,
      backlog: rows.filter((ticket) => ticket.status === 'backlog').length,
      archive: rows.filter(isArchivedTicket).length,
      trash: rows.filter(isTrashedTicket).length,
      open: rows.filter(isOpenTicket).length,
      up_next: work.upNextCount,
      active: work.activeTicketCount,
      started: rows.filter((ticket) => ticket.status === 'started').length,
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      completed_today: carry ? carry.completedToday : (completionTrend.at(-1) ?? 0),
      completion_trend: completionTrend,
    };
  }
  function publishOptimisticTicketRows(projectId: string) {
    ticketRowsByProject.value = { ...ticketRowsByProject.value, [projectId]: tickets.value };
    ticketCountsByProject.value = Object.fromEntries(
      Object.entries(ticketCountsByProject.value).filter(([id]) => id !== projectId),
    );
  }

  function visibleTickets() {
    let result: WireTicketRow[];
    if (workspaceSearchActive()) {
      const matches = searchMatchKeys.value,
        matched = matches ? tickets.value.filter((ticket) => matches.has(ticketSearchKey(ticket))) : [],
        effective = selectedViewSearch(),
        advanced = usesAdvancedSearchExpression(effective.text),
        tags = advanced
          ? []
          : effective.tokens
              .filter((token): token is Extract<InlineSearchToken, { kind: 'tag' }> => token.kind === 'tag')
              .map((token) => token.value.toLowerCase());
      result = filterAdvancedSearchResults(matched, effective.text, 'all', []).filter((ticket) =>
        tags.every((tag) => ticket.tags.some((value) => value.toLowerCase() === tag)),
      );
    } else result = ticketsForView(tickets.value, selectedView.value);
    const active = activeWorkspaceSort();
    return result.slice().sort((a, b) => compareWorkspaceTickets(a, b, active.sort, active.sortDirection));
  }
  const {
    searchBarActive,
    workspaceSearchActive,
    selectedViewSearch,
    workspaceSearchModel,
    workspaceSearchTokenOffset,
    searchSignature,
    sortedTicketQuery,
    refreshTicketSearch,
    availableSearchTags,
    focusWorkspaceSearch,
    scheduleTicketSearch,
  } = createWorkspaceSearchController({
    searchQuery,
    searchTokens,
    searchHelpOpen,
    searchMatchKeys,
    sidebarSearchCounts,
    selectedView,
    error,
    ticketPageQuery,
    ticketNextCursor,
    tickets,
    ticketRowsByProject,
    project,
    customViewFor,
    customViewsFor,
    activeWorkspaceSort,
    ticketSearchKey,
    mergeTicketLinkRows,
    refreshProject,
    resetBoardColumnPages,
    resetProgressiveTicketRendering,
    scheduleProjectSessionPersistence,
  });

  function mergeTicketLinkRows(existing: readonly WireTicketRow[], incoming: readonly WireTicketRow[]) {
    const byId = new Map(existing.map((ticket) => [ticket.qualified_id, ticket]));
    for (const ticket of incoming) byId.set(ticket.qualified_id, ticket);
    return [...byId.values()];
  }
  function focusTopTicketReader() {
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        document
          .querySelector<HTMLElement>(
            '[data-component="ticket-reader"][data-reader-active="true"] [data-action="close-ticket-reader"]',
          )
          ?.focus({ preventScroll: true }),
      ),
    );
  }
  function readerDialog(id: string) {
    return document.querySelector<TicketReaderDialogElement>(
      `[data-component="ticket-reader"][data-reader-frame-id="${CSS.escape(id)}"]`,
    );
  }
  function presentTicketReaderDialog(
    id: string,
    trigger: HTMLElement | undefined,
    onOpen: () => void,
    afterOpen?: () => void,
  ) {
    if (trigger) readerReturnFocus.set(id, trigger);
    if (trigger?.isConnected) trigger.focus({ preventScroll: true });
    onOpen();
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const dialog = readerDialog(id);
        if (!dialog) return;
        showTicketReaderDialog(document, id);
        if (afterOpen) afterOpen();
        else focusTopTicketReader();
      }),
    );
  }
  function focusAfterTicketReader(id: string, slug: string) {
    const trigger = readerReturnFocus.get(id);
    readerReturnFocus.delete(id);
    setTimeout(() => {
      if (trigger?.isConnected) {
        trigger.focus({ preventScroll: true });
        return;
      }
      const row = [
        ...document.querySelectorAll<HTMLElement>(
          `[data-action="select-ticket-row"][data-ticket-slug="${CSS.escape(slug)}"]`,
        ),
      ].find((item) => item.offsetParent !== null);
      if (row) {
        row.focus({ preventScroll: true });
        return;
      }
      const inspector = document.querySelector<HTMLElement>('#app-right-rail [data-action="open-ticket-reader"]');
      if (inspector) {
        inspector.focus({ preventScroll: true });
        return;
      }
      document.querySelector<HTMLElement>('[data-work-area-focus-owner]')?.focus({ preventScroll: true });
    }, 0);
  }
  async function openTicketLinkMatch(match: TicketLinkMatch) {
    const target = projects.value.find((item) => item.id === match.projectId);
    if (!target) {
      showToast(`No open project matches ${match.projectId}.`);
      return;
    }
    ticketLinkChoice.value = undefined;
    const cached = ticketRowsByProject.value[target.id] ?? [],
      row = cached.find((item) => item.qualified_id === match.qualifiedId);
    if (row)
      ticketRowsByProject.value = { ...ticketRowsByProject.value, [target.id]: mergeTicketLinkRows(cached, [row]) };
    try {
      const ticket = (await new Api(target.apiPath).checkoutTicket(target.id, match.qualifiedId)).ticket,
        capabilities = capabilitiesFor(ticket.connection_id);
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      if (!capabilities) throw new Error(`Capabilities unavailable for ${ticket.connection_id}.`);
      const id = browserRandomId(),
        trigger = ticketLinkReturnFocus;
      ticketLinkReturnFocus = undefined;
      linkedReaderStack.value = pushTicketReaderFrame(linkedReaderStack.value, {
        id,
        open: false,
        projectId: target.id,
        projectName: target.name,
        apiPath: target.apiPath,
        ticket,
        activeTab: 'info',
        capabilities,
        edit: ticketReaderEditState(ticket),
      });
      presentTicketReaderDialog(id, trigger, () => {
        replaceLinkedReaderFrame(id, (frame) => ({ ...frame, open: true }));
      });
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  async function selectLinkedTicket(
    slug: string,
    projectId?: string,
    preferredProjectId = activeTicketReaderProject(linkedReaderStack.value, selectedProjectId.value),
  ) {
    const reference = parseTicketLinkReference(projectId ? `@${projectId}/${slug}` : slug);
    if (!reference) {
      showToast(`No exact match for ${slug}.`);
      return;
    }
    const candidates = reference.projectId
        ? projects.value.filter((item) => item.id === reference.projectId)
        : projects.value,
      results = await Promise.allSettled(
        candidates.map(async (item) => ({
          project: item,
          result: await new Api(item.apiPath).checkoutTickets(item.id, {
            text: reference.slug,
            compact: true,
            limit: 500,
          }),
        })),
      ),
      linkProjects = results.flatMap((result) =>
        result.status === 'fulfilled'
          ? [{ id: result.value.project.id, name: result.value.project.name, tickets: result.value.result.items }]
          : [],
      );
    if (
      results.some(
        (result) => result.status === 'rejected' || result.value.result.partial || result.value.result.truncated,
      )
    )
      error.value =
        'Linked-ticket search results may be incomplete because a source failed or reached its result limit.';
    for (const item of linkProjects)
      ticketRowsByProject.value = {
        ...ticketRowsByProject.value,
        [item.id]: mergeTicketLinkRows(ticketRowsByProject.value[item.id] ?? [], item.tickets),
      };
    const resolution = resolveTicketLink(reference, linkProjects, preferredProjectId);
    if (resolution.kind === 'not_found') {
      ticketLinkReturnFocus = undefined;
      showToast(resolution.message);
      return;
    }
    if (resolution.kind === 'choose') {
      ticketLinkChoice.value = resolution;
      return;
    }
    await openTicketLinkMatch(resolution.match);
  }
  async function flushWorkspaceTicketReader() {
    const [details, note, blocked] = await Promise.all([
      finishDetailsEdit(true),
      readerNoteAutosave.flush(),
      readerBlockedReasonAutosave.flush(),
    ]);
    if (note) readerEditingNoteId.value = undefined;
    if (blocked) readerBlockedReasonEditing.value = false;
    return details && note && blocked;
  }
  function approveTicketReaderClose(dialog: TicketReaderDialogElement) {
    const id = dialog.dataset.readerFrameId;
    if (!id || readerClosing.has(id)) return;
    readerClosing.add(id);
    void (id === 'workspace-reader' ? flushWorkspaceTicketReader() : flushLinkedReader(id)).then((saved) => {
      readerClosing.delete(id);
      if (!saved) return;
      readerApprovedClose.add(id);
      dialog.open = false;
    });
  }
  function finishTicketReaderClose(dialog: TicketReaderDialogElement) {
    const id = dialog.dataset.readerFrameId;
    if (!id) return;
    if (id === 'workspace-reader') {
      const slug = selectedTicket.value?.slug ?? '';
      readerOpen.value = false;
      focusAfterTicketReader(id, slug);
      return;
    }
    const top = linkedReaderStack.value.at(-1);
    if (!top || top.id !== id) return;
    const popped = popTicketReaderFrame(linkedReaderStack.value);
    linkedReaderAutosaves.delete(id);
    linkedReaderStack.value = popped.stack;
    focusAfterTicketReader(id, popped.closed?.ticket.slug ?? '');
  }
  function cancelTicketLinkChoice() {
    ticketLinkChoice.value = undefined;
    const focus = ticketLinkReturnFocus;
    ticketLinkReturnFocus = undefined;
    requestAnimationFrame(() => {
      if (focus?.isConnected) focus.focus({ preventScroll: true });
    });
  }

  function showFieldConflict(conflict: TicketFieldConflict) {
    fieldConflict.value = conflict;
    fieldConflictResolution.value = conflict.mine;
    error.value = '';
  }
  function reconcileRefreshedSelected(previous: FullTicket, refreshed: FullTicket) {
    let conflict: TicketFieldConflict | undefined, settledConflictKey: string | undefined;
    if (detailsMode.value === 'write' && !detailsAutosave.pending()) {
      const next = reconcileActiveDraft(detailsDraftBase, detailsDraft.value, refreshed.details);
      detailsDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged' || next.kind === 'merged') {
        detailsDraft.value = next.draft;
        settledConflictKey = 'details';
      }
      // A clean merge of both edits still has to be saved (HS2-A4XCXE).
      if (next.kind === 'merged') {
        syncFocusedDraftControl(document.activeElement, next.mine, next.draft);
        detailsAutosave.schedule(next.draft);
      }
      if (next.kind === 'conflict') {
        detailsAutosave.cancel();
        conflict = {
          key: 'details',
          field: 'details',
          label: 'Details',
          base: previous.details,
          mine: detailsDraft.value,
          theirs: refreshed.details,
        };
      }
    }
    if (readerDetailsMode.value === 'write' && !readerDetailsAutosave.pending()) {
      const next = reconcileActiveDraft(readerDetailsDraftBase, readerDetailsDraft.value, refreshed.details);
      readerDetailsDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged' || next.kind === 'merged') {
        readerDetailsDraft.value = next.draft;
        settledConflictKey = 'details';
      }
      // A clean merge of both edits still has to be saved (HS2-A4XCXE).
      if (next.kind === 'merged') {
        syncFocusedDraftControl(document.activeElement, next.mine, next.draft);
        readerDetailsAutosave.schedule(next.draft);
      }
      if (next.kind === 'conflict' && !conflict) {
        readerDetailsAutosave.cancel();
        conflict = {
          key: 'details',
          field: 'details',
          label: 'Details',
          base: previous.details,
          mine: readerDetailsDraft.value,
          theirs: refreshed.details,
        };
      }
    }
    if (titleEditingSurface.value !== undefined && !titleAutosave.pending()) {
      const next = reconcileActiveDraft(titleDraftBase, titleDraft.value, refreshed.title);
      titleDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged' || next.kind === 'merged') {
        titleDraft.value = next.draft;
        settledConflictKey = 'title';
      }
      // A clean merge of both edits still has to be saved (HS2-A4XCXE).
      if (next.kind === 'merged') {
        syncFocusedDraftControl(document.activeElement, next.mine, next.draft);
        titleAutosave.schedule(next.draft);
      }
      if (next.kind === 'conflict' && !conflict) {
        titleAutosave.cancel();
        conflict = {
          key: 'title',
          field: 'title',
          label: 'Title',
          base: previous.title,
          mine: titleDraft.value,
          theirs: refreshed.title,
        };
      }
    }
    if (blockedReasonEditing.value && !blockedReasonAutosave.pending()) {
      const remote = refreshed.blocked_reason ?? '',
        next = reconcileActiveDraft(blockedReasonDraftBase, blockedReasonDraft.value, remote);
      blockedReasonDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged' || next.kind === 'merged') {
        blockedReasonDraft.value = next.draft;
        settledConflictKey = 'blocked_reason';
      }
      // A clean merge of both edits still has to be saved (HS2-A4XCXE).
      if (next.kind === 'merged') {
        syncFocusedDraftControl(document.activeElement, next.mine, next.draft);
        blockedReasonAutosave.schedule(next.draft);
      }
      if (next.kind === 'conflict' && !conflict) {
        blockedReasonAutosave.cancel();
        conflict = {
          key: 'blocked_reason',
          field: 'blocked_reason',
          label: 'Blocked Reason',
          base: previous.blocked_reason ?? '',
          mine: blockedReasonDraft.value,
          theirs: remote,
        };
      }
    }
    if (readerBlockedReasonEditing.value && !readerBlockedReasonAutosave.pending()) {
      const remote = refreshed.blocked_reason ?? '',
        next = reconcileActiveDraft(readerBlockedReasonDraftBase, readerBlockedReasonDraft.value, remote);
      readerBlockedReasonDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged' || next.kind === 'merged') {
        readerBlockedReasonDraft.value = next.draft;
        settledConflictKey = 'blocked_reason';
      }
      // A clean merge of both edits still has to be saved (HS2-A4XCXE).
      if (next.kind === 'merged') {
        syncFocusedDraftControl(document.activeElement, next.mine, next.draft);
        readerBlockedReasonAutosave.schedule(next.draft);
      }
      if (next.kind === 'conflict' && !conflict) {
        readerBlockedReasonAutosave.cancel();
        conflict = {
          key: 'blocked_reason',
          field: 'blocked_reason',
          label: 'Blocked Reason',
          base: previous.blocked_reason ?? '',
          mine: readerBlockedReasonDraft.value,
          theirs: remote,
        };
      }
    }
    const noteId = editingNoteId.value;
    if (noteId && !noteAutosave.pending()) {
      const remote = refreshed.notes.find((note) => note.id === noteId)?.text ?? '',
        previousNote = previous.notes.find((note) => note.id === noteId)?.text ?? '',
        next = reconcileActiveDraft(noteDraftBase, noteDraft.value, remote);
      noteDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged' || next.kind === 'merged') {
        noteDraft.value = next.draft;
        settledConflictKey = `note:${noteId}`;
      }
      // A clean merge of both edits still has to be saved (HS2-A4XCXE).
      if (next.kind === 'merged') {
        syncFocusedDraftControl(document.activeElement, next.mine, next.draft);
        noteAutosave.schedule({ id: noteId, value: next.draft });
      }
      if (next.kind === 'conflict' && !conflict) {
        noteAutosave.cancel();
        conflict = {
          key: `note:${noteId}`,
          field: 'note',
          label: 'Note',
          base: previousNote,
          mine: noteDraft.value,
          theirs: remote,
        };
      }
    }
    const readerNoteId = readerEditingNoteId.value;
    if (readerNoteId && !readerNoteAutosave.pending()) {
      const remote = refreshed.notes.find((note) => note.id === readerNoteId)?.text ?? '',
        previousNote = previous.notes.find((note) => note.id === readerNoteId)?.text ?? '',
        next = reconcileReaderNoteDraft(previous, refreshed, readerNoteId, readerNoteDraftBase, readerNoteDraft.value);
      if (!next) settledConflictKey = `note:${readerNoteId}`;
      else {
        readerNoteDraftBase = next.base;
        if (next.kind === 'adopt-remote' || next.kind === 'converged' || next.kind === 'merged') {
          readerNoteDraft.value = next.draft;
          settledConflictKey = `note:${readerNoteId}`;
        }
        // A clean merge of both edits still has to be saved (HS2-A4XCXE).
        if (next.kind === 'merged') {
          syncFocusedDraftControl(document.activeElement, next.mine, next.draft);
          readerNoteAutosave.schedule({ id: readerNoteId, value: next.draft });
        }
        if (next.kind === 'conflict' && !conflict) {
          readerNoteAutosave.cancel();
          conflict = {
            key: `note:${readerNoteId}`,
            field: 'note',
            label: 'Note',
            base: previousNote,
            mine: readerNoteDraft.value,
            theirs: remote,
          };
        }
      }
    }
    selectedTicket.value = refreshed;
    committedTickets.set(refreshed.slug, refreshed);
    if (conflict) showFieldConflict(conflict);
    else if (fieldConflict.value?.key === settledConflictKey) {
      fieldConflict.value = undefined;
      fieldConflictResolution.value = '';
    }
  }

  const projectRefresh = createProjectRefresh({
    loading,
    error,
    selectedView,
    tickets,
    ticketRowsByProject,
    ticketNextCursor,
    ticketPageQuery,
    ticketCountsByProject,
    partialSourcesByProject,
    boardColumnPages,
    defaultProviders,
    selectedTicketSlugs,
    selectedTicket,
    corruptTickets,
    selectedCorruptKey,
    repository,
    repositoryError,
    commandDefinitions,
    commandRuns,
    projectProjectionById,
    localTicketMutationBarrier,
    pendingCreatedTickets,
    ticketSelectionAnchor: {
      get: () => ticketSelectionAnchor,
      set: (value) => {
        ticketSelectionAnchor = value;
      },
    },
    project: () => project(),
    sortedTicketQuery: (query) => sortedTicketQuery(query),
    // Built later by the board column paging owner, so it is read through a callback.
    boardRefreshSpec: (view, projectId, loaded) => boardRefreshSpec(view, projectId, loaded),
    boardPagesWithVisibleLimit: (pages) => boardPagesWithVisibleLimit(pages),
    workspaceSearchActive: () => workspaceSearchActive(),
    refreshTicketSearch: () => refreshTicketSearch(),
    markProjectWarm,
    recordAuthoritativeTicketTrend,
    scheduleClaimLeaseExpiry: () => {
      scheduleClaimLeaseExpiry();
    },
    reconcileRefreshedSelected,
  });
  // A hoisted entry point: earlier-built owners and handlers call it before this line runs.
  function refreshProject(options?: ProjectRefreshOptions) {
    return projectRefresh.refreshProject(options);
  }
  async function loadNextTicketPage() {
    const current = project(),
      cursor = ticketNextCursor.value,
      query = ticketPageQuery.value;
    if (!current || !cursor || ticketPageLoading.value) return;
    ticketPageLoading.value = true;
    try {
      const page = await new Api(current.apiPath).checkoutTicketPage(current.id, 200, cursor, query);
      if (project()?.id !== current.id || ticketNextCursor.value !== cursor) return;
      const previous = tickets.value,
        next = appendUniqueTicketRows(previous, page.items),
        rows = next.slice(previous.length);
      tickets.value = next;
      ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: tickets.value };
      ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: page.counts };
      recordAuthoritativeTicketTrend(current.id, page.counts);
      if (page.source_errors?.length) {
        partialSourcesByProject.value = { ...partialSourcesByProject.value, [current.id]: true };
        error.value = page.source_errors.join(' · ');
      }
      ticketNextCursor.value = page.next_cursor;
      if (searchQuery.value.trim() || searchTokens.value.length) {
        const effective = effectiveSearch(searchQuery.value, searchTokens.value),
          matched = usesAdvancedSearchExpression(effective.text)
            ? filterAdvancedSearchResults(rows, effective.text, 'all', [])
            : rows,
          matches = searchMatchKeys.value ?? new Set<string>();
        for (const ticket of matched) matches.add(ticketSearchKey(ticket));
        searchMatchKeys.value = new Set(matches);
      }
      resetProgressiveTicketRendering();
      scheduleClaimLeaseExpiry();
    } catch (reason) {
      const text = reason instanceof Error ? reason.message : String(reason);
      // A cursor the server no longer accepts cannot be repaired by retrying it; restart the visible
      // collection from page one instead of surfacing a dead-end error (HS2-38MPMC).
      if (isStaleTicketCursorError(text) && project()?.id === current.id && ticketNextCursor.value === cursor) {
        ticketNextCursor.value = undefined;
        ticketPageQuery.value = {};
        if (workspaceSearchActive()) void refreshTicketSearch();
        else void refreshTicketCollection(selectedView.value);
      } else error.value = text;
    } finally {
      if (project()?.id === current.id) ticketPageLoading.value = false;
    }
  }
  function suppressBulkTicketMotion(count: number) {
    if (count > 12) skipNextTicketMotion = true;
  }
  const { boardRefreshSpec, beginBulkBoardRefill, finishBulkBoardRefill, loadBoardColumnMore } =
    createBoardColumnPaging({
      viewMode,
      selectedProjectId,
      selectedView,
      hideVerifiedByProject,
      boardColumnPages,
      boardColumnLoading,
      ticketPageQuery,
      tickets,
      ticketRowsByProject,
      ticketCountsByProject,
      partialSourcesByProject,
      error,
      project,
      hideVerifiedColumn,
      workspaceSearchActive,
      visibleTickets: () => visibleTickets(),
      refreshProject: (options) => refreshProject(options),
      loadNextTicketPage: () => loadNextTicketPage(),
      recordAuthoritativeTicketTrend,
      scheduleClaimLeaseExpiry: () => {
        scheduleClaimLeaseExpiry();
      },
    });
  const projectTabRefresh = createProjectTabRefreshCoordinator<Project, ProjectTicketRefresh>({
    waitUntilSafe: async () => {
      for (let open = openMetadataPopup(); open; open = openMetadataPopup())
        await new Promise<void>((resolve) => {
          open.addEventListener(
            'wa-after-hide',
            () => {
              resolve();
            },
            { once: true },
          );
        });
    },
    activeProjectId: () => selectedProjectId.value,
    isOpen: (target) =>
      projects.value.some(
        (item) => item.id === target.id && item.root === target.root && item.apiPath === target.apiPath,
      ),
    refreshActive: async (target) => {
      if (project()?.id === target.id) await refreshProject({ showLoading: false });
    },
    // Background tab refreshes are invisible work: they load the collection the project will show when
    // reactivated (its remembered view) without driving the busy indicator (HS2-AZZ9TF).
    loadBackground: async (target) => {
      const view = loadProjectWorkspaceSession(localStorage, target.id)?.selectedView ?? 'all',
        rows = ticketRowsByProject.value[target.id] as WireTicketRow[] | undefined,
        pages = boardPagesByProject.value[target.id] as Record<string, BoardColumnPage> | undefined;
      const query = sortedTicketQuery(ticketViewQuery(view));
      const snapshot = await loadProjectTicketRefresh(
        new Api(target.apiPath, '', { trackBusy: false }),
        target.id,
        query,
        boardRefreshSpec(view, target.id, rows && pages ? { rows, pages } : undefined),
      );
      return { ...snapshot, nextCursorQuery: query };
    },
    publishBackground: (target, snapshot) => {
      // Rows stay resident only for warm projects (spare LRU capacity admits a live-refreshed background one); counts
      // for tab badges are always kept.
      if (snapshot.tickets && warmProjects.admit(target.id)) {
        ticketRowsByProject.value = {
          ...ticketRowsByProject.value,
          [target.id]: mergeRetainedCreatedRows(
            snapshot.tickets,
            pendingCreatedTickets.retain(target.id, snapshot.tickets),
          ),
        };
        ticketCursorsByProject.value = {
          ...ticketCursorsByProject.value,
          [target.id]: snapshot.nextCursorQuery
            ? { cursor: snapshot.nextCursor, query: snapshot.nextCursorQuery }
            : undefined,
        };
        boardPagesByProject.value = { ...boardPagesByProject.value, [target.id]: snapshot.boardPages ?? {} };
      }
      if (snapshot.ticketCounts) {
        ticketCountsByProject.value = { ...ticketCountsByProject.value, [target.id]: snapshot.ticketCounts };
        recordAuthoritativeTicketTrend(target.id, snapshot.ticketCounts);
      }
      if (snapshot.tickets)
        partialSourcesByProject.value = {
          ...partialSourcesByProject.value,
          [target.id]: Boolean(snapshot.sourceErrors?.length),
        };
      scheduleClaimLeaseExpiry();
    },
  });
  async function refreshCommands(current = project(), quiet = false) {
    const generation = ++commandRefreshGeneration;
    if (!current) return;
    const active = () => generation === commandRefreshGeneration && project()?.id === current.id;
    try {
      const client = new Api(current.apiPath, '', { trackBusy: !quiet }),
        previous = JSON.stringify(commandDefinitions.value, null, 2),
        [definitions, runs, groups] = await Promise.all([
          client.commands(),
          client.commandRuns(),
          /* An older server without kept command groups (HS2-EZ5KMC) must not block commands. */ client
            .commandGroups()
            .catch(() => undefined),
        ]);
      if (!active()) return;
      commandDefinitions.value = definitions;
      commandRuns.value = runs;
      if (groups) loadCommandGroups(current.id, groups);
      const projection = projectProjectionById.value[current.id];
      projectProjectionById.value = {
        ...projectProjectionById.value,
        [current.id]: {
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
          corruptTickets: projection?.corruptTickets ?? corruptTickets.value,
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
          repository: projection?.repository ?? repository.value,
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
          repositoryError: projection?.repositoryError ?? repositoryError.value,
          commandDefinitions: definitions,
          commandRuns: runs,
        },
      };
      const draft = commandSettingsDraftsByProject.value[current.id];
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      if (viewMode.value !== 'settings' || draft === undefined || draft === previous)
        setCommandSettingsDraft(current.id, JSON.stringify(definitions, null, 2));
      setCommandSettingsMessage(current.id, '');
    } catch (reason) {
      if (active()) setCommandSettingsMessage(current.id, reason instanceof Error ? reason.message : String(reason));
    }
  }
  async function refreshCustomViews(current = project(), quiet = false) {
    if (!current) return;
    try {
      const views = await new Api(current.apiPath, '', { trackBusy: !quiet }).customViews();
      if (!projects.value.some((item) => item.id === current.id)) return;
      customViewsByProject.value = { ...customViewsByProject.value, [current.id]: views };
      if (project()?.id !== current.id) return;
      const key = customTicketViewKey(selectedView.value);
      if (!key) return;
      const selected = views.find((view) => view.id === key);
      if (selected) activateCustomView();
      else selectTicketView('all');
    } catch {
      /* older or temporarily unavailable servers simply keep their last known shared view list */
    }
  }
  async function refreshTerminalSettings(current = project(), quiet = false) {
    if (!current) return;
    try {
      const value = await new Api(current.apiPath, '', { trackBusy: !quiet }).terminalSettings();
      if (project()?.id !== current.id) return;
      inheritGlobalShellHistory.value = value.inherit_global_shell_history;
      terminalSettingsMessage.value = '';
    } catch (reason) {
      if (project()?.id === current.id)
        terminalSettingsMessage.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  async function refreshTrashSettings(current = project(), quiet = false) {
    if (!current) return;
    trashSettingsMessagesByProject.value = { ...trashSettingsMessagesByProject.value, [current.id]: 'Loading…' };
    try {
      const value = await new Api(current.apiPath, '', { trackBusy: !quiet }).trashSettings(current.id);
      if (project()?.id !== current.id) return;
      trashCleanupDaysByProject.value = { ...trashCleanupDaysByProject.value, [current.id]: value.trash_cleanup_days };
      trashSettingsMessagesByProject.value = { ...trashSettingsMessagesByProject.value, [current.id]: '' };
    } catch (reason) {
      if (project()?.id === current.id)
        trashSettingsMessagesByProject.value = {
          ...trashSettingsMessagesByProject.value,
          [current.id]: reason instanceof Error ? reason.message : String(reason),
        };
    }
  }
  const openMetadataPopup = () =>
    [...document.querySelectorAll<Control>('wa-select, wa-dropdown[data-inspector-status-menu]')].find(
      (node) => node.open,
    );
  function beginLocalTicketMutation() {
    // A refresh may already have passed the barrier and be waiting on an older
    // ticket snapshot. Invalidate that active-project response before applying
    // optimistic rows; later refreshes still wait for the mutation to settle.
    projectRefresh.invalidate();
    return localTicketMutationBarrier.begin();
  }
  function beginLocalTicketCreation() {
    const release = beginLocalTicketMutation();
    return async () => {
      await new Promise<void>((next) =>
        requestAnimationFrame(() => {
          next();
        }),
      );
      await waitForTicketMotionSettled();
      release();
    };
  }
  const { syncProjectChangeStreams, dispose: disposeProjectChangeStreams } = createProjectChangeStreamsController({
    projects,
    project,
    shellMode,
    statsProjectId,
    localTicketMutationBarrier,
    localTicketChangeAcknowledgements,
    projectTabRefresh,
    turnStreamEvents,
    conversationPersistence,
    updateConversation,
    conversationForActivity,
    refreshDriveConnections,
    refreshPermissions,
    serverResolvedPermission,
    applyTerminalRenamed,
    refreshTerminalDashboard,
    refreshProviderOutbox,
    refreshCommands,
    refreshCustomViews,
    refreshRepositoryStatus,
    loadConfidenceReport,
  });
  const {
    restoreTicketDraft,
    flushTicketDrafts,
    updateSelected,
    history,
    updateSelectedTracked,
    detailsAutosave,
    readerDetailsAutosave,
    noteAutosave,
    readerNoteAutosave,
    blockedReasonAutosave,
    readerBlockedReasonAutosave,
    titleAutosave,
    tagsAutosave,
    linkedReaderFrame,
    linkedReaderAutosaves,
    replaceLinkedReaderFrame,
    linkedReaderSaves,
    flushLinkedReader,
    selectedRows,
    restoreTrashedTickets,
    executeBulkTicketAction,
    openEmptyTrash,
    emptyTrash,
    openBulkTicketDialog,
    copySelection,
    pasteSelection,
    copyDraggedTickets,
    isEditableEvent,
    ticketWorkAreaFocused,
    ordinaryTextSelected,
    timeline,
    notes,
    attachmentContext: ticketAttachmentContext,
    duplicateTargetFor,
    addAttachments,
    selectionOrder,
    presentTicket,
    cancelTicketDrafts,
    selectTickets,
    openTicketReader,
    closeNotWorking,
    presentNotWorkingDialog,
    openNotWorking,
    closeTicketCloseDialog,
    openTicketClose,
    setTicketCloseReason,
    searchTicketCloseTargets,
    submitTicketClose,
    openDuplicateTarget,
    addNotWorkingFiles,
    openTicketComposer,
    resetTicketComposer,
    addNewTicketFiles,
    submitNewTicket,
    submitNotWorking,
  } = createTicketWorkflows({
    state: {
      get blockedReasonDraftBase() {
        return blockedReasonDraftBase;
      },
      set blockedReasonDraftBase(value) {
        blockedReasonDraftBase = value;
      },
      get bulkTicketSlugs() {
        return bulkTicketSlugs;
      },
      set bulkTicketSlugs(value) {
        bulkTicketSlugs = value;
      },
      get clipboard() {
        return clipboard;
      },
      set clipboard(value) {
        clipboard = value;
      },
      get detailsDraftBase() {
        return detailsDraftBase;
      },
      set detailsDraftBase(value) {
        detailsDraftBase = value;
      },
      get detailsEditGeneration() {
        return detailsEditGeneration;
      },
      set detailsEditGeneration(value) {
        detailsEditGeneration = value;
      },
      get noteDraftBase() {
        return noteDraftBase;
      },
      set noteDraftBase(value) {
        noteDraftBase = value;
      },
      get readerBlockedReasonDraftBase() {
        return readerBlockedReasonDraftBase;
      },
      set readerBlockedReasonDraftBase(value) {
        readerBlockedReasonDraftBase = value;
      },
      get readerDetailsDraftBase() {
        return readerDetailsDraftBase;
      },
      set readerDetailsDraftBase(value) {
        readerDetailsDraftBase = value;
      },
      get readerDetailsEditGeneration() {
        return readerDetailsEditGeneration;
      },
      set readerDetailsEditGeneration(value) {
        readerDetailsEditGeneration = value;
      },
      get readerNoteDraftBase() {
        return readerNoteDraftBase;
      },
      set readerNoteDraftBase(value) {
        readerNoteDraftBase = value;
      },
      get ticketSelectionAnchor() {
        return ticketSelectionAnchor;
      },
      set ticketSelectionAnchor(value) {
        ticketSelectionAnchor = value;
      },
      get titleDraftBase() {
        return titleDraftBase;
      },
      set titleDraftBase(value) {
        titleDraftBase = value;
      },
    },
    CLOSED_NOT_WORKING_TARGET,
    projects,
    selectedProjectId,
    tickets,
    ticketRowsByProject,
    ticketCountsByProject,
    selectedTicket,
    selectedTicketSlugs,
    selectedCorruptKey,
    selectedView,
    ticketCollectionState,
    loading,
    error,
    attachmentMessage,
    inspectorTab,
    inspectorVisible,
    readerTab,
    readerOpen,
    linkedReaderStack,
    detailsMode,
    detailsDraft,
    readerDetailsMode,
    readerDetailsDraft,
    titleEditingSurface,
    titleDraft,
    blockedReasonEditing,
    blockedReasonDraft,
    readerBlockedReasonEditing,
    readerBlockedReasonDraft,
    editingNoteId,
    readerEditingNoteId,
    noteDraft,
    readerNoteDraft,
    fieldConflict,
    fieldConflictResolution,
    readerInlineFeedbackReplies,
    readerFeedbackChoiceSelections,
    readerFeedbackChoiceAnchors,
    codeReview,
    codeReviewLoading,
    codeReviewMessage,
    expandedCodeReviewCommits,
    duplicateBacklinkState,
    resolvedDuplicateTargets,
    ticketCloseDialog,
    ticketLinkChoice,
    bulkTicketDialog,
    notWorkingTarget,
    notWorkingNote,
    notWorkingFiles,
    notWorkingSubmitting,
    notWorkingError,
    composerExpanded,
    composerTitle,
    composerDetails,
    composerCategory,
    composerUpNext,
    composerSource,
    lastTicketSourceByProject,
    composerAttachments,
    composerAttachmentMessage,
    composerAttachmentError,
    composerScreening,
    composerSubmitting,
    histories,
    mutationGenerations,
    committedTickets,
    singleTicketMutationSequencer,
    bulkTicketMutationSequencer,
    localTicketChangeAcknowledgements,
    pendingCreatedTickets,
    project,
    api,
    defaultProvider,
    newTicketSource,
    capabilitiesFor,
    canUseAttachments,
    canStageNewTicketAttachments,
    ticketSnapshot,
    visibleTickets,
    projectTabTicketRows,
    projectTicketCounts,
    suppressBulkTicketMotion,
    beginBulkBoardRefill,
    finishBulkBoardRefill,
    publishOptimisticTicketRows,
    beginLocalTicketMutation,
    beginLocalTicketCreation,
    refreshProject,
    refreshTicketCollection,
    refreshCodeReview,
    selectTicketView,
    scheduleClaimLeaseExpiry,
    scheduleProjectSessionPersistence,
    persistWorkspacePreferences,
    showToast,
    showFieldConflict,
    reconcileRefreshedSelected,
    openTicketLinkMatch,
    presentTicketReaderDialog,
    beginDetailsEdit,
    revealTicketInspector: revealInspectorOverlay,
    activeTicketSurface,
    draftScope,
    ago,
    beginBulkUpdateProgress: bulkUpdateActivities.begin,
  });
  const projectTeardown = createProjectTeardown({
    projects,
    selectedProjectId,
    statsProjectId,
    projectRestoreFailures,
    selectedProjectRestoreRoot,
    terminalDrawerVisible,
    commandDialogId,
    commandSettingsEditingId,
    commandDefinitions,
    commandRuns,
    readerOpen,
    linkedReaderStack,
    projectRecords: [
      ticketRowsByProject,
      ticketCursorsByProject,
      boardPagesByProject,
      ticketCountsByProject,
      ticketTrendByProject,
      projectProjectionById,
      customViewsByProject,
      terminalDrawerChatsByProject,
      commandSettingsDraftsByProject,
      commandSettingsMessagesByProject,
      commandSettingsSelectedByProject,
      commandSettingsExtraGroupsByProject,
    ],
    connectionRecords: [defaultProviders, driveConnectionsByProject, drivePendingByProject],
    histories,
    linkedReaderAutosaves,
    readerReturnFocus,
    readerClosing,
    readerApprovedClose,
    readerDetailsAutosave,
    readerNoteAutosave,
    readerBlockedReasonAutosave,
    projectTabRefresh,
    pendingCreatedTickets,
    projectsPendingActivation,
    warmProjects,
    forgetAiConfiguration: (id) => {
      aiConfigurationController.forgetAiConfiguration(id);
    },
    project: () => project(),
    resetTicketComposer: () => {
      resetTicketComposer();
    },
    activateOpenProject,
    refreshActivatedProject,
    currentRememberedProjectRoots,
    syncProjectChangeStreams,
    refreshProject: () => refreshProject(),
    refreshCommands,
    refreshCustomViews,
    refreshDriveConnections,
  });
  // A hoisted entry point: the project-close owner built earlier receives it as its teardown callback.
  function closeProjectIds(ids: readonly string[]) {
    projectTeardown.closeProjectIds(ids);
  }

  async function queueAiCommand(command: CommandDefinition, current: Project) {
    if (!(defaultProvider()?.capabilities.create ?? true)) {
      error.value = 'The default ticket source does not support ticket creation.';
      return;
    }
    const client = new Api(current.apiPath),
      finishLocalCreation = beginLocalTicketCreation();
    try {
      const created = await client.createCheckoutTicket(current.id, customAiCommandTicket(command));
      localTicketChangeAcknowledgements.acknowledge(current.id, {
        store: created.connection_id,
        id: created.id,
        kind: 'created',
      });
      pendingCreatedTickets.register(current.id, created);
      if (project()?.id === current.id) {
        tickets.value = prependCreatedTicketRow(tickets.value, created);
        publishOptimisticTicketRows(current.id);
      }
      const connection = customAiCommandSignalConnection(
        driveConnectionsByProject.value[current.id] ?? [],
        command.tool,
        aiDefaults.value.tool,
      );
      if (!connection) {
        showToast(`Queued ${created.slug}.`);
        return;
      }
      const finishStart = beginConversation(connection.id, HOTSHEET_SKILL_SIGNAL);
      try {
        const updated = await client.sendToolTurn(connection.id, HOTSHEET_SKILL_SIGNAL, connection.session_id, {
          ...(command.model ? { model: command.model } : {}),
          ...(command.effort ? { effort: command.effort } : {}),
        });
        if (project()?.id === current.id)
          driveConnectionsByProject.value = {
            ...driveConnectionsByProject.value,
            [current.id]: (driveConnectionsByProject.value[current.id] ?? [])
              .filter((item) => item.id !== updated.id)
              .concat(updated),
          };
        showToast(`Queued ${created.slug} and notified ${aiToolLabel(connection.tool)}.`);
      } catch (reason) {
        const message = reason instanceof Error ? reason.message : String(reason);
        updateConversation(connection.id, (state) => ({
          ...state,
          activeAssistantId: undefined,
          progress: undefined,
          error: message,
          messages: state.messages.map((item) =>
            item.id === state.activeAssistantId
              ? { ...item, status: 'failed', content: item.content || 'The Hot Sheet signal could not be sent.' }
              : item,
          ),
        }));
        showToast(`Queued ${created.slug}; ${aiToolLabel(connection.tool)} could not be notified.`);
      } finally {
        finishStart();
      }
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      await finishLocalCreation();
    }
  }
  async function runCommand(commandId: string) {
    const command = commandDefinitions.value.find((item) => item.id === commandId),
      current = project();
    if (!command || !current) return;
    const active = commandRunFor(commandId);
    if (active?.state === 'running') {
      commandDialogId.value = commandId;
      commandStopConfirmation.value = true;
      return;
    }
    if (command.confirmation && !window.confirm(command.confirmation)) return;
    if (command.kind === 'shell') {
      await createShellCommandTerminal(command, current);
      return;
    }
    if (command.kind === 'ai') {
      await queueAiCommand(command, current);
      return;
    }
    try {
      const run = await new Api(current.apiPath).runCommand(commandId);
      if (project()?.id === current.id)
        commandRuns.value = [run, ...commandRuns.value.filter((item) => item.id !== run.id)];
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  async function openCommandHistory(commandId: string) {
    const current = project(),
      run = commandRunFor(commandId);
    commandDialogId.value = commandId;
    commandStopConfirmation.value = false;
    if (!current || !run) return;
    try {
      const full = await new Api(current.apiPath).commandRun(run.id);
      if (project()?.id === current.id)
        commandRuns.value = commandRuns.value.map((item) => (item.id === full.id ? full : item));
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  const { sidebarSurfaceProps } = createSidebarSurface({
    viewMode,
    notificationView,
    notificationsPaused,
    partialSourcesByProject,
    sidebarSearchCounts,
    repository,
    repositoryError,
    tickets,
    selectedView,
    corruptTickets,
    driveConnectionsByProject,
    drivePendingByProject,
    commandDefinitions,
    commandGroupExpanded,
    commandGroupsCollapsed,
    driveOptionsOpen,
    driveOptionsAnchor,
    aiTools,
    aiSettingsLoading,
    aiSettingsMessage,
    driveOverridesByProject,
    aiDefaults,
    conversationOpen,
    conversationConnectionId,
    project: () => project(),
    settingsCategory: () => settingsCategory(),
    projectPermissionHistory,
    projectPendingPermissions,
    projectTicketCounts,
    searchBarActive,
    searchSignature,
    customViewsFor,
    effectiveDriveSelection,
    commandRunFor,
    commandIcon,
    aiToolLabel,
  });
  /** This project's own ticket sources — never another project's (HS2-3SCH1K, HS2-SM9PM8). */
  const { settingsWorkspace } = createSettingsSurface({
    settingsCategory: () => settingsCategory(),
    defaultProviders,
    providerConnections,
    providerSettingsError,
    ticketSourceSetupProject,
    providerAccounts,
    providerAccountsError,
    signingOutAccount,
    unusedAccountSourceChoice,
    removingUnusedAccountSource,
    aiTools,
    aiDefaults,
    aiSettingsLoading,
    aiSettingsMessage,
    commandSettingsDefinitions,
    commandSettingsExtraGroups,
    commandSettingsEditingId,
    commandSelection,
    commandIconSearch,
    commandSettingsMessage,
    trashCleanupDaysByProject,
    trashSettingsMessagesByProject,
    trashRetentionDraftsByProject,
    inheritGlobalShellHistory,
    terminalSettingsMessage,
    permissionAutomation,
    hideVerifiedColumn,
    showLoadingActivity,
    keyboardShortcutOverrides,
    capturingShortcutId,
    appleShortcutPlatform,
  });
  const { workspaceTerminalGroups, globalWorkspaceSurfaceProps, projectTerminalDrawerProps, aiConversationSurface } =
    createTerminalPresentation({
      projects,
      project,
      shellMode,
      statsProjectId,
      confidenceReportByProject,
      canGiveFeedback: () => Boolean(selectedTicket.value && noteCapabilities()?.ai_feedback),
      terminals: {
        terminalGroups,
        terminalGridFullscreen,
        drawerTabOrder,
        terminalDashboardSize,
        terminalFitAcross,
        terminalFitHigh,
        magnifiedTerminalKey,
        terminalHiddenKeys,
        terminalDashboardLoading,
        terminalDashboardMessage,
        terminalContextMenu,
        terminalDrawerBounds,
        terminalDrawerFitAcross,
        terminalDrawerFitHigh,
        terminalDrawerSelected,
        terminalDrawerMaximized,
        mobileTerminalFocus,
        mobileMagnifiedTerminal,
        viewportMobile,
        coarsePointer,
      },
      conversations: {
        conversationStates,
        driveConnectionsByProject,
        terminalDrawerChatsByProject,
        conversationDrafts,
        conversationOpen,
        conversationConnectionId,
        conversationSelectedMessages,
      },
      ai: aiConfigurationController,
      permissions: permissionsController,
    });
  const { ticketViewTitle, ticketViewActionSpec, ticketContextMenuSurface } = createTicketViewSurface({
    canStageNewTicketAttachments,
    capabilitiesFor,
    customViewFor,
    priority,
    selectedTicketSlugs,
    status,
    ticketCloseDialog,
    ticketContextMenu,
    tickets,
  });
  const { terminalOperationsSurfaceProps } = createTerminalOperationsSurface({
    partialSourcesByProject,
    projects,
    projectTicketCounts,
    selectedProjectId,
    ticketRowsByProject,
    tickets,
  });
  const { notWorkingSurfaceProps, attachmentItems, selectedInspectorProps, previewInspectorProps } =
    createInspectorSurface({
      ago,
      api,
      attachmentContext,
      attachmentLabelEditing,
      attachmentMessage,
      attachmentsEditableFor,
      blockedReasonDraft,
      blockedReasonEditing,
      canAddNotes,
      canDeleteNotes,
      canEditAttachments,
      canEditNotes,
      canEditStartedPhaseSelected,
      canUpdateSelected,
      canUseAttachments,
      capabilitiesFor,
      codeReview,
      codeReviewLoading,
      codeReviewMessage,
      composingNote,
      defaultProviders,
      detailsDraft,
      detailsMode,
      duplicateBacklinkState,
      duplicateTargetFor,
      editingNoteId,
      expandedCodeReviewCommits,
      fieldConflict,
      fieldConflictResolution,
      inspectorTab,
      liveClaimNotice,
      newNoteDraft,
      noteDraft,
      notes,
      priority,
      project,
      providerOutboxOperations,
      readerOpen,
      selectedTicket,
      status,
      tagSuggestions,
      timeline,
      titleDraft,
      titleEditingSurface,
    });
  const { gallerySourceFor, readerLayersSurfaceProps } = createReaderLayersSurface({
    ago,
    attachmentContext,
    attachmentItems,
    attachmentLabelEditing,
    attachmentMenuSurfaceProps,
    attachmentMessage,
    canAddNotes,
    canDeleteNotes,
    canEditAttachments,
    canEditNotes,
    canUpdateSelected,
    canUseAttachments,
    changeEvidenceSurfaceProps,
    codeReview,
    codeReviewLoading,
    codeReviewMessage,
    composingNote,
    duplicateTargetFor,
    expandedCodeReviewCommits,
    fieldConflict,
    fieldConflictResolution,
    linkedReaderStack,
    liveClaimNotice,
    newNoteDraft,
    notes,
    priority,
    project,
    projects,
    readerBlockedReasonDraft,
    readerBlockedReasonEditing,
    readerDetailsDraft,
    readerDetailsMode,
    readerEditingNoteId,
    readerFeedbackChoiceSelections,
    readerInlineFeedbackReplies,
    readerLargeText,
    readerNoteDraft,
    readerOpen,
    readerTab,
    selectedTicket,
    status,
    tagSuggestions,
    timeline,
    titleDraft,
    titleEditingSurface,
  });
  const { workspaceSurfaceProps } = createWorkspaceSurface({
    boardColumnLoading,
    boardColumnPages,
    continueProgressiveTicketRendering,
    corruptRecovery,
    corruptTickets,
    error,
    hideVerifiedColumn,
    loading,
    notificationView,
    partialSourcesByProject,
    project,
    projectPendingPermissions,
    projectPermissionHistory,
    renderedTicketLimit,
    row,
    searchMatchKeys,
    searchQuery,
    searchTokens,
    selectedCorruptKey,
    selectedProjectId,
    selectedView,
    settingsWorkspace,
    ticketCollectionState,
    ticketCountsByProject,
    ticketNextCursor,
    ticketPageLoading,
    tickets,
    ticketViewTitle,
    viewMode,
    viewportMobile,
    visibleTickets,
    workspaceSearchActive,
  });
  const { terminalRailSurfaceProps } = createTerminalRailSurface({
    activeWorkspaceSort,
    capabilitiesFor,
    customViewsFor,
    hideVerifiedColumn,
    pendingPermissions,
    permissionHistory,
    project,
    projects,
    projectTicketCounts,
    row,
    searchHelpOpen,
    searchMatchKeys,
    searchOpen,
    searchQuery,
    searchTokens,
    selectedInspectorProps,
    selectedRows,
    selectedTicket,
    selectedTicketSlugs,
    selectedView,
    sort,
    sortDirection,
    terminalRailScreen,
    tickets,
    ticketSearchKey,
    ticketViewActionSpec,
    ticketViewTitle,
    viewMode,
    workspaceSearchModel,
  });
  const { HotSheetShell } = createHotSheetShell({
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
    previewInspectorProps,
    selectedCorruptKey,
    selectedInspectorProps,
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
  });

  const appRoot = document.querySelector<HTMLElement>('#app')!;
  // The workspace grid's ticket rail is a Workbench navigation panel (HS2-FY06N4): Kerf's
  // `wireNavStack` animates its push/pop, moves focus into each new view, and reports the back
  // control, which pops the app-owned rail screen. The stack mounts with the terminals shell, so it is
  // (re)wired after any render that mounted a new one.
  let railNavStack: { section: Element; dispose: () => void } | undefined;
  function syncTerminalRailNavStack() {
    const section = appRoot.querySelector(`#app-right-rail [data-component="nav-stack"]`);
    if (section === (railNavStack?.section ?? null)) return;
    railNavStack?.dispose();
    railNavStack = section
      ? {
          section,
          dispose: wireNavStack(section, {
            onBack: () => {
              terminalRailScreen.value = 'root';
            },
          }),
        }
      : undefined;
  }
  // The compact Repository Status drill-down (HS2-3B8345): its back control pops to the list.
  let repositoryNavStack: { section: Element; dispose: () => void } | undefined;
  function syncRepositoryNavStack() {
    const section = appRoot.querySelector('#repository-status-popover [data-component="nav-stack"]');
    if (section === (repositoryNavStack?.section ?? null)) return;
    repositoryNavStack?.dispose();
    repositoryNavStack = section
      ? {
          section,
          dispose: wireNavStack(section, {
            onBack: () => {
              repositoryController.repositoryDetailActive.value = false;
            },
          }),
        }
      : undefined;
  }
  const renderMetrics = import.meta.env.DEV ? createRenderMetrics(appRoot) : undefined;
  const activeTicketCollectionKey = () => `${selectedProjectId.value}:${selectedView.value}`;
  const ticketScrollRoot = () => appRoot.querySelector<HTMLElement>('.app-shell__workspace') ?? appRoot;
  const ticketScrollScope = () => ({
    project: selectedProjectId.value,
    mode: `${shellMode.value}:${viewMode.value}`,
    view:
      viewMode.value === 'notifications'
        ? notificationView.value
        : viewMode.value === 'settings'
          ? settingsCategory()
          : selectedView.value,
  });
  const initialRenderedTicketCollectionKey = activeTicketCollectionKey();
  let initialProjectRestoreComplete = false;
  if (renderMetrics)
    (window as typeof window & { __hotsheetRenderMetrics?: typeof renderMetrics }).__hotsheetRenderMetrics =
      renderMetrics;
  const { HotSheetApp } = createHotSheetApp({
    activeTicketCollectionKey,
    aiConversationSurface,
    aiFeedbackDialog,
    appRoot,
    appTabContextMenu,
    attachmentMenuSurfaceProps,
    bulkTicketDialog,
    bulkUpdateProgress,
    canStageNewTicketAttachments,
    changeEvidenceSurface,
    commandDialogSurface,
    composerAttachmentError,
    composerAttachmentMessage,
    composerAttachments,
    composerCategory,
    composerDetails,
    composerExpanded,
    composerScreening,
    composerSubmitting,
    composerTitle,
    composerUpNext,
    conversationExportDialog,
    conversationOpen,
    conversationRenderRevision,
    corruptTickets,
    createdGitTicketStore,
    customViewsFor,
    defaultProvider,
    defaultProviders,
    deferContainedListScrollRestore,
    error,
    gallerySurface,
    githubAuth,
    HotSheetShell,
    hs1MigrationBusy,
    hs1MigrationError,
    hs1MigrationProject,
    loading,
    manualModelDialog,
    newTicketSource,
    notWorkingError,
    notWorkingFiles,
    notWorkingNote,
    notWorkingSubmitting,
    notWorkingSurfaceProps,
    notWorkingTarget,
    permissionRevision,
    project,
    projectCloseDialog,
    projectDialogError,
    projectDialogOpen,
    providerAccountChoice,
    providerAccounts,
    providerConnections,
    providerEditingId,
    providerRemovingId,
    providerSettingsBusy,
    providerSettingsError,
    providerSetupKind,
    readerLayersSurfaceProps,
    remoteProjectCheckouts,
    remoteProjectDialogOpen,
    remoteProjectError,
    remoteProjectLoading,
    initialRenderedTicketCollectionKey,
    renderMetrics,
    repositoryStatusSurface,
    savedViewBusy,
    savedViewDeleteBusy,
    savedViewDeleteError,
    savedViewDeleteTargetId,
    savedViewDialogMode,
    savedViewDialogOpen,
    savedViewDialogSession,
    savedViewError,
    savedViewHelpOpen,
    savedViewName,
    savedViewSearchModel,
    searchMatchKeys,
    shellMode,
    consumeSkipNextTicketMotion: () => {
      const skip = skipNextTicketMotion;
      skipNextTicketMotion = false;
      return skip;
    },
    syncAttachmentGalleryMeasurement,
    syncRepositoryNavStack,
    syncRepositoryPaginationObserver,
    syncTerminalDrawerMaximum,
    syncTerminalRailNavStack,
    syncTerminalViewportMounts,
    terminalCopy,
    terminalDashboardLoading,
    terminalDashboardSizeObserver,
    terminalDrawerSizeObserver,
    terminalDrawerVisible,
    terminalEditMenu,
    terminalPaste,
    terminalRename,
    terminalVisibility,
    terminalVisibilityContextMenu,
    terminalVisibilityDialogScope,
    terminalVisibilityFilter,
    terminalVisibilityNamePrompt,
    ticketCloseDialog,
    ticketCollectionState,
    ticketLinkChoice,
    ticketRenderScheduled: () => ticketRenderScheduled,
    tickets,
    ticketScrollMemory,
    ticketScrollRoot,
    ticketScrollScope,
    ticketSourceRemoteBusy,
    ticketSourceRemoteError,
    ticketSourceSetupError,
    ticketSourceSetupNavigation,
    ticketSourceSetupProject,
    unhealthyServerRecovery,
    unhealthyServerRecoveryBusy,
    viewMode,
    workspaceSearchActive,
    workspaceTerminalGroups,
  });
  mount(appRoot, withControlledOpen(appRoot, HotSheetApp));
  toastHost.className = 'app-toast-stack';
  (toastHost as HTMLElement & { placement: string }).placement = 'bottom-end';
  document.body.append(toastHost);
  const onToastHide = (event: Event) => {
    const item = event.target;
    if (!(item instanceof HTMLElement) || !item.matches('wa-toast-item[data-toast-generation]')) return;
    const generation = toastPresentation.dismissed(item);
    if (generation !== undefined) toastLifetime.hide(generation);
  };
  toastHost.addEventListener('wa-after-hide', onToastHide);
  // Kerf drives the shell rails' resizing and persistence (HS2-P289N2) and, through their `collapsed`
  // signals, the phone overlays' exclusivity, Escape/outside-press dismissal, focus trap, and focus
  // return (HS2-Y1B1Y1); the terminal drawer keeps the app's own drag for its measured maximum and
  // drag-past-minimum collapse.
  wireWorkbench(appRoot, {
    id: APP_WORKBENCH_ID,
    panels: {
      // Menus and dialogs a phone overlay launches keep it open (HS2-5APX20, Kerf `keepOpenOn`).
      leftRail: {
        size: sidebarSize,
        storageKey: 'hotsheet.layout.app-left-rail.size',
        collapsed: sidebarCollapsed,
        keepOpenOn: isSidePanelPortal,
      },
      rightRail: {
        size: inspectorSize,
        storageKey: 'hotsheet.layout.app-right-rail.size',
        collapsed: inspectorCollapsed,
        keepOpenOn: isSidePanelPortal,
      },
    },
  });
  const disposeToolbarVisibility = wireWorkspaceToolbarVisibility(appRoot);
  const disposeRailToolbarVisibility = wireTicketRailToolbarVisibility(appRoot);

  const savedViewMenuRoot = document.createElement('div');
  document.body.append(savedViewMenuRoot);
  mount(savedViewMenuRoot, () => (savedViewMenu.value ? <SavedViewContextMenu {...savedViewMenu.value} /> : <></>));

  // Decorative top-of-app "server busy" bars (HS2-MW1V3M). Bar count fills the viewport width and
  // is only recomputed on an actual (debounced) window resize; the strip itself is a fixed overlay.
  const serverBusyBarCount = signal(computeServerBusyBarCount(window.innerWidth));
  let serverBusyResizeTimer: number | undefined;
  window.addEventListener('resize', () => {
    if (serverBusyResizeTimer !== undefined) window.clearTimeout(serverBusyResizeTimer);
    serverBusyResizeTimer = window.setTimeout(() => {
      serverBusyResizeTimer = undefined;
      serverBusyBarCount.value = computeServerBusyBarCount(window.innerWidth);
    }, 150);
  });
  const serverBusyRoot = document.createElement('div');
  document.body.append(serverBusyRoot);
  mount(serverBusyRoot, () => (
    <>
      <ServerBusyBars count={serverBusyBarCount.value} busy={serverBusy.value} />
      <ServerBusyMessage message={serverBusyMessage.value} visible={showLoadingActivity.value && serverBusy.value} />
    </>
  ));
  document.addEventListener(
    'pointerdown',
    (event) => {
      if (
        savedViewMenu.value &&
        !(event.target as Element).closest('[data-context-menu="saved-view"], [data-action="open-saved-view-menu"]')
      )
        savedViewMenu.value = undefined;
    },
    { capture: true },
  );
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') savedViewMenu.value = undefined;
  });

  function beginDetailsEdit(reader = false, frame?: TicketReaderFrame) {
    if (frame) {
      if (!frame.capabilities.update) return;
      replaceLinkedReaderFrame(frame.id, (current) => ({
        ...current,
        edit: {
          ...current.edit,
          detailsMode: 'write',
          detailsDraft: current.ticket.details,
          detailsBase: current.ticket.details,
          detailsGeneration: current.edit.detailsGeneration + 1,
        },
      }));
      queueMicrotask(() =>
        document.querySelector<HTMLElement>(`[data-reader-frame-id="${frame.id}"] [name="markdown-source"]`)?.focus(),
      );
      return;
    }
    if (!selectedTicket.value || !canUpdateSelected()) return;
    const mode = reader ? readerDetailsMode : detailsMode,
      draft = reader ? readerDetailsDraft : detailsDraft;
    // A crash or reload mid-edit leaves a local recovery copy; continue it on its original base (HS2-RE1PS6).
    const restored = restoreTicketDraft('details', selectedTicket.value.details),
      base = restored?.base ?? selectedTicket.value.details;
    if (reader) {
      readerDetailsEditGeneration += 1;
      readerDetailsDraftBase = base;
    } else {
      detailsEditGeneration += 1;
      detailsDraftBase = base;
    }
    draft.value = restored?.draft ?? selectedTicket.value.details;
    mode.value = 'write';
    if (restored) (reader ? readerDetailsAutosave : detailsAutosave).schedule(draft.value);
    queueMicrotask(() => activeTicketSurface().querySelector<HTMLElement>('[name="markdown-source"]')?.focus());
  }

  let pointerDetailsReader: boolean | undefined,
    pointerDetailsFinish: DetailsFinishTask | undefined,
    pointerDetailsTimer: number | undefined;
  function beginDetailsFinish(reader = false): DetailsFinishTask {
    return {
      reader,
      ticketId: selectedTicket.value?.id,
      generation: reader ? readerDetailsEditGeneration : detailsEditGeneration,
      saved: (reader ? readerDetailsAutosave : detailsAutosave).flush(),
    };
  }
  async function settleDetailsFinish(task: DetailsFinishTask) {
    const saved = await task.saved,
      mode = task.reader ? readerDetailsMode : detailsMode;
    if (
      saved &&
      selectedTicket.value?.id === task.ticketId &&
      (task.reader ? readerDetailsEditGeneration : detailsEditGeneration) === task.generation
    )
      mode.value = 'preview';
    return saved;
  }
  async function finishDetailsEdit(reader = false) {
    return settleDetailsFinish(beginDetailsFinish(reader));
  }
  function completePointerDetailsFinish() {
    if (pointerDetailsTimer !== undefined) window.clearTimeout(pointerDetailsTimer);
    pointerDetailsTimer = undefined;
    const reader = pointerDetailsReader,
      task = pointerDetailsFinish;
    pointerDetailsReader = undefined;
    pointerDetailsFinish = undefined;
    if (task) void settleDetailsFinish(task);
    else if (reader !== undefined) void finishDetailsEdit(reader);
  }
  function schedulePointerDetailsFinish() {
    if (pointerDetailsReader === undefined) return;
    if (pointerDetailsTimer !== undefined) window.clearTimeout(pointerDetailsTimer);
    pointerDetailsTimer = window.setTimeout(completePointerDetailsFinish, 0);
  }
  function activeTicketSurface(): ParentNode {
    return (readerOpen.value ? document.querySelector('[data-component="ticket-reader"]') : null) ?? document;
  }

  const interactionBindingsPort: InteractionBindingsPort = {
    openProjectPicker,
    openRemoteProjectDialog,
    chooseAndOpenProject,
    unhealthyServerRecovery,
    projectDialogOpen,
    openRemoteCheckout,
    remoteProjectDialogOpen,
    importHs1Project,
    chooseHs1TicketStore,
    hs1MigrationProject,
    hs1MigrationBusy,
    hs1SourceIdentity,
    project,
    migrationJobDetails,
    migrationJobs,
    migrationConnectionErrors,
    migrationJobsByRoot,
    ticketSourceSetupProject,
    createdGitTicketStore,
    ticketSourceSetupNavigation,
    removeOldHs1Data,
    projects,
    providerSetupKind,
    providerEditingId,
    requestProjectSourceRemoval,
    refreshProviderAccounts,
    signOutProviderAccount,
    requestUnusedAccountSourceRemoval,
    cancelUnusedAccountSourceRemoval,
    removeUnusedAccountSource,
    useGithubAccount,
    providerAccountChoice,
    useProviderAccount,
    setProjectDefaultSource,
    setProjectSourceColor,
    relinkProjectGitSource,
    retryProviderOutbox,
    discardProviderOutbox,
    providerSettingsError,
    ticketSourceRemoteError,
    connectCreatedGitRemote,
    createProjectGitSource,
    chooseProjectPath,
    recoverUnhealthyProjectServer,
    repository,
    repositoryView,
    repositoryDetailActive: repositoryController.repositoryDetailActive,
    repositorySetupStep,
    repositorySetupError,
    repositoryFileMenu,
    repositorySelectedFiles,
    repositoryComparison,
    expandedCodeReviewCommits,
    loadRepositoryDetail,
    refreshRepositoryStatus,
    initializeRepository,
    connectRepositoryRemote,
    skipRepositoryRemote,
    repositoryDetail,
    showToast,
    error,
    codeReview,
    changeEvidenceView,
    changeEvidenceReader,
    selectedTicket,
    codeReviewMessage,
    openProject,
    currentRememberedProjectRoots,
    persistDrawerTabOrder,
    currentDrawerTabIds,
    focusDrawerTab,
    revealCorruptTicket,
    queueCorruptTicketRepair,
    corruptTickets,
    selectedCorruptKey,
    selectedTicketSlugs,
    setInspectorVisible,
    statsProjectId,
    setShellMode,
    selectTerminalRailProject,
    selectTicketView,
    terminalRailScreen,
    selectProjectTab,
    retryProjectRestore,
    terminalDrawerBounds,
    terminalDashboardSize,
    terminalGridFullscreen,
    terminalDrawerFitHigh,
    terminalFitAcross,
    terminalFitHigh,
    terminalSession,
    clearTerminalHalt,
    magnifiedTerminalKey,
    openTerminalInProject,
    terminalContextMenu,
    terminalVisibilityScopeFor,
    terminalVisibility,
    persistTerminalVisibility,
    terminalVisibilityFilter,
    terminalVisibilityContextMenu,
    terminalVisibilityDialogScope,
    terminalVisibilityNamePrompt,
    terminalKeysForVisibilityDialog,
    openGridAIChat,
    setTerminalDrawerVisible,
    terminalDrawerVisible,
    toggleTerminalDrawerMaximized,
    selectDrawerItem,
    enterMobileTerminalFocus,
    exitMobileTerminalFocus,
    cycleMobileTerminalColumns,
    terminalModifiers,
    terminalFunctionRow,
    terminalCopy,
    terminalPaste,
    terminalEditMenu,
    createProjectTerminal,
    aiLaunchConfiguration,
    createDrawerAIChat,
    openSavedConversation,
    requestProjectClose,
    projectCloseDialog,
    restoreBorrowedProjectCloseTerminal,
    cancelProjectClose,
    confirmProjectClose,
    closeAllProjectResources,
    closeTerminalIds,
    closeDrawerAIChat,
    appTabContextMenu,
    terminalGroups,
    terminalRename,
    closeDrawerTabIds,
    saveTerminalName,
    resetTerminalName,
    viewportMobile,
    sidebarCollapsed,
    inspectorCollapsed,
    revealInspectorOverlay,
    selectTickets,
    selectionOrder,
    visibleTickets,
    selectedView,
    cancelTicketDrafts,
    openTicketReader,
    ticketContextMenu,
    selectedRows,
    executeBulkTicketAction,
    tickets,
    openNotWorking,
    openTicketClose,
    copySelection,
    pasteSelection,
    openBulkTicketDialog,
    restoreTrashedTickets,
    bulkTicketDialog,
    openEmptyTrash,
    emptyTrash,
    setTicketCloseReason,
    searchTicketCloseTargets,
    ticketCloseDialog,
    submitTicketClose,
    closeTicketCloseDialog,
    openDuplicateTarget,
    notWorkingNote,
    scheduleProjectSessionPersistence,
    presentNotWorkingDialog,
    addNotWorkingFiles,
    draftScope,
    notWorkingTarget,
    notWorkingFiles,
    submitNotWorking,
    closeNotWorking,
    notWorkingSubmitting,
    keyboardShortcutOverrides,
    appleShortcutPlatform,
    isEditableEvent,
    openSavedViewDialog,
    savedViewMenu,
    openSavedViewRename,
    openSavedViewDelete,
    savedViewName,
    savedViewError,
    focusSavedViewQuery,
    savedViewSearchModel,
    savedViewSearchTokenOffset,
    saveSavedView,
    closeSavedViewDialog,
    savedViewBusy,
    deleteSavedView,
    restoreTicketDraft,
    savedViewHelpOpen,
    closeSavedViewDelete,
    savedViewDeleteBusy,
    commandGroupExpanded,
    persistWorkspacePreferences,
    commandGroupsCollapsed,
    toggleSidebarDrive,
    driveOptionsOpen,
    driveOptionsAnchor,
    aiTools,
    aiSettingsLoading,
    refreshAiConfiguration,
    driveOverridesByProject,
    normalizedAiSelection,
    selectDriveModel,
    openManualModel,
    effectiveDriveSelection,
    openSidebarConversation,
    conversationOpen,
    openConversationExport,
    pickConversationMessage,
    copyConversationSelection,
    clearConversationSelection,
    conversationExportDialog,
    finishConversationExport,
    updateConversationExportDraft,
    conversationConnectionId,
    conversationDrafts,
    sendConversationTurn,
    stopConversation,
    selectConversationProvider,
    selectConversationModel,
    selectConversationEffort,
    canAddNotes,
    canGiveFeedback: () => Boolean(noteCapabilities()?.ai_feedback),
    aiFeedbackDialog,
    updateSelected,
    runCommand,
    commandDialogId,
    commandStopConfirmation,
    commandRuns,
    commandSettingsEditingId,
    commandIconSearch,
    addCommandSetting,
    manualModelDialog,
    deleteCommandSetting,
    addCommandGroup,
    deleteCommandGroup,
    selectCommandRow,
    commandSelection,
    commandSettingsDefinitions,
    selectCommandSetting,
    reorderCommandSettings,
    updateCommandSetting,
    updateCommandAiSelection,
    effectiveCommandAiSelection,
    showLoadingActivity,
    inheritGlobalShellHistory,
    terminalSettingsMessage,
    trashSettingsMessagesByProject,
    trashCleanupDaysByProject,
    trashRetentionDraftsByProject,
    resetProgressiveTicketRendering,
    viewMode,
    setSettingsCategory,
    refreshProviderConnections,
    refreshTerminalSettings,
    refreshTrashSettings,
    capturingShortcutId,
    saveAiDefaults,
    selectDefaultModel,
    selectDefaultEffort,
    selectDefaultProvider,
    restoreCommandEditorAfterManualModel,
    aiDefaults,
    providerConnections,
    githubAuth,
    cancelGitHubSignIn,
    startGitHubSignIn,
    saveExternalProvider,
    providerRemovingId,
    requestProviderRemoval,
    cancelProviderRemoval,
    removeExternalProvider,
    toggleProviderDisabled,
    refreshGitHubRepositories,
    loadEditingGitHubRepositories,
    chooseGitHubEnterprise,
    copyGitHubCode,
    reopenGitHubSignIn,
    notificationView,
    permissionTimer,
    permissionAutomationByProject,
    updatePermissionTimer,
    permissionRevision,
    permissionInbox,
    pendingPermissions,
    setNotificationsPaused,
    resolvePermission,
    selectedProjectId,
    hideVerifiedByProject,
    selectLinkedTicket,
    ticketLinkChoice,
    openTicketLinkMatch,
    cancelTicketLinkChoice,
    searchOpen,
    workspaceSearchModel,
    workspaceSearchTokenOffset,
    searchHelpOpen,
    focusWorkspaceSearch,
    searchQuery,
    searchTokens,
    scheduleTicketSearch,
    sort,
    sortDirection,
    openTicketComposer,
    composerSubmitting,
    composerExpanded,
    resetTicketComposer,
    composerTitle,
    composerDetails,
    composerCategory,
    composerUpNext,
    composerSource,
    addNewTicketFiles,
    composerAttachments,
    composerAttachmentMessage,
    composerAttachmentError,
    submitNewTicket,
    history,
    addAttachments,
    api,
    attachmentMessage,
    attachmentLabelEditing,
    refreshProject,
    galleryImages,
    resetAttachmentGallery,
    gallerySourceFor,
    shiftGallery,
    attachmentGalleryGeometry,
    attachmentGalleryScale,
    attachmentGalleryUrl,
    attachmentMenu,
    syncAttachmentGalleryMeasurement,
    activeAttachmentGalleryVideo,
    attachmentGalleryDuration,
    attachmentGalleryMarkup,
    attachmentGallerySaveState,
    finishGalleryAnnotationSession,
    discardGalleryAnnotationSession,
    beginGalleryAnnotationSession,
    attachmentGalleryDrawMode,
    attachmentGalleryTool,
    attachmentGallerySelectedAnnotation,
    attachmentGalleryAnnotations,
    attachmentGalleryCropMode,
    attachmentGalleryCrop,
    attachmentGalleryOriginalSize,
    beginGalleryCrop,
    restoreGalleryCrop,
    finishGalleryCrop,
    updateGalleryPlaybackPresentation,
    attachmentGalleryPlayhead,
    attachmentGalleryPlaying,
    gallerySvgClock,
    stopGallerySvgClock,
    attachmentGalleryVolumeOpen,
    attachmentGalleryMuted,
    attachmentGalleryVolume,
    canUseAttachments,
    canEditAttachments,
    updateSelectedTracked,
    readerOpen,
    readerDetailsDraft,
    detailsDraft,
    titleDraft,
    readerBlockedReasonDraft,
    blockedReasonDraft,
    readerNoteDraft,
    noteDraft,
    fieldConflictResolution,
    fieldConflict,
    canUpdateSelected,
    canEditStartedPhaseSelected,
    titleEditingSurface,
    activeTicketSurface,
    titleAutosave,
    tagsAutosave,
    beginDetailsEdit,
    linkedReaderFrame,
    replaceLinkedReaderFrame,
    linkedReaderSaves,
    readerDetailsAutosave,
    detailsAutosave,
    beginDetailsFinish,
    finishDetailsEdit,
    readerEditingNoteId,
    editingNoteId,
    composingNote,
    newNoteDraft,
    readerNoteAutosave,
    noteAutosave,
    readerInlineFeedbackReplies,
    readerFeedbackChoiceSelections,
    readerFeedbackChoiceAnchors,
    canDeleteNotes,
    workspaceSearchActive,
    loadBoardColumnMore,
    loadNextTicketPage,
    readerBlockedReasonEditing,
    blockedReasonEditing,
    readerBlockedReasonAutosave,
    blockedReasonAutosave,
    presentTicketReaderDialog,
    readerTab,
    codeReviewLoading,
    refreshCodeReview,
    readerDialog,
    readerApprovedClose,
    approveTicketReaderClose,
    finishTicketReaderClose,
    readerLargeText,
    linkedReaderStack,
    inspectorTab,
    setSidebarVisible,
    sidebarVisible,
    appRegionSize,
    setAppRegionSize,
    terminalDrawerMax,
    terminalDrawerMin,
    syncTerminalDrawerMaximum,
    updateTerminalDrawerBounds,
    copyDraggedTickets,
    inspectorVisible,
    switchWorkspaceView,
    shellMode,
    terminalDrawerSelected,
    ticketWorkAreaFocused,
    ordinaryTextSelected,
    openCommandHistory,
    completePointerDetailsFinish,
    schedulePointerDetailsFinish,
    get repositoryFileSelectionAnchor() {
      return repositoryController.repositoryFileSelectionAnchor;
    },
    set repositoryFileSelectionAnchor(value) {
      repositoryController.repositoryFileSelectionAnchor = value;
    },
    get ticketSelectionAnchor() {
      return ticketSelectionAnchor;
    },
    set ticketSelectionAnchor(value) {
      ticketSelectionAnchor = value;
    },
    get terminalPreviewClickTimer() {
      return terminalPreviewClickTimer;
    },
    set terminalPreviewClickTimer(value) {
      terminalPreviewClickTimer = value;
    },
    get pendingTerminalFocus() {
      return terminalDrawer.pendingTerminalFocus;
    },
    set pendingTerminalFocus(value) {
      terminalDrawer.pendingTerminalFocus = value;
    },
    get bulkTicketSlugs() {
      return bulkTicketSlugs;
    },
    set bulkTicketSlugs(value) {
      bulkTicketSlugs = value;
    },
    get ticketLinkReturnFocus() {
      return ticketLinkReturnFocus;
    },
    set ticketLinkReturnFocus(value) {
      ticketLinkReturnFocus = value;
    },
    get manualModelDialogShown() {
      return aiConfigurationController.manualModelDialogShown;
    },
    set manualModelDialogShown(value) {
      aiConfigurationController.manualModelDialogShown = value;
    },
    get permissionCountdown() {
      return permissionsController.permissionCountdown;
    },
    set permissionCountdown(value) {
      permissionsController.permissionCountdown = value;
    },
    get attachmentRangeGesture() {
      return galleryController.attachmentRangeGesture;
    },
    set attachmentRangeGesture(value) {
      galleryController.attachmentRangeGesture = value;
    },
    get attachmentAnnotationGesture() {
      return galleryController.attachmentAnnotationGesture;
    },
    set attachmentAnnotationGesture(value) {
      galleryController.attachmentAnnotationGesture = value;
    },
    get attachmentGalleryLivePlayhead() {
      return galleryController.attachmentGalleryLivePlayhead;
    },
    set attachmentGalleryLivePlayhead(value) {
      galleryController.attachmentGalleryLivePlayhead = value;
    },
    get attachmentGallerySvgPreviousFrame() {
      return galleryController.attachmentGallerySvgPreviousFrame;
    },
    set attachmentGallerySvgPreviousFrame(value) {
      galleryController.attachmentGallerySvgPreviousFrame = value;
    },
    get attachmentGallerySvgFrame() {
      return galleryController.attachmentGallerySvgFrame;
    },
    set attachmentGallerySvgFrame(value) {
      galleryController.attachmentGallerySvgFrame = value;
    },
    get attachmentGalleryLiveVolume() {
      return galleryController.attachmentGalleryLiveVolume;
    },
    set attachmentGalleryLiveVolume(value) {
      galleryController.attachmentGalleryLiveVolume = value;
    },
    get attachmentSwipeGesture() {
      return galleryController.attachmentSwipeGesture;
    },
    set attachmentSwipeGesture(value) {
      galleryController.attachmentSwipeGesture = value;
    },
    get readerDetailsDraftBase() {
      return readerDetailsDraftBase;
    },
    set readerDetailsDraftBase(value) {
      readerDetailsDraftBase = value;
    },
    get detailsDraftBase() {
      return detailsDraftBase;
    },
    set detailsDraftBase(value) {
      detailsDraftBase = value;
    },
    get titleDraftBase() {
      return titleDraftBase;
    },
    set titleDraftBase(value) {
      titleDraftBase = value;
    },
    get readerBlockedReasonDraftBase() {
      return readerBlockedReasonDraftBase;
    },
    set readerBlockedReasonDraftBase(value) {
      readerBlockedReasonDraftBase = value;
    },
    get blockedReasonDraftBase() {
      return blockedReasonDraftBase;
    },
    set blockedReasonDraftBase(value) {
      blockedReasonDraftBase = value;
    },
    get readerNoteDraftBase() {
      return readerNoteDraftBase;
    },
    set readerNoteDraftBase(value) {
      readerNoteDraftBase = value;
    },
    get noteDraftBase() {
      return noteDraftBase;
    },
    set noteDraftBase(value) {
      noteDraftBase = value;
    },
    get pointerDetailsReader() {
      return pointerDetailsReader;
    },
    set pointerDetailsReader(value) {
      pointerDetailsReader = value;
    },
    get pointerDetailsFinish() {
      return pointerDetailsFinish;
    },
    set pointerDetailsFinish(value) {
      pointerDetailsFinish = value;
    },
    get appRegionResizeDrag() {
      return appRegionResizeDrag;
    },
    set appRegionResizeDrag(value) {
      appRegionResizeDrag = value;
    },
    get clipboard() {
      return clipboard;
    },
    set clipboard(value) {
      clipboard = value;
    },
  };
  // One page-lifetime teardown for every delegated and native interaction listener (HS2-NZT3MT).
  const disposeInteractions = wireHotSheetInteractions(createHotSheetInteractionBindings(interactionBindingsPort));
  // Flush the debounced session (including the in-progress new-ticket composer draft) before the page
  // is hidden, reloaded, or restarted, so a background refresh/restart never loses typed text (HS2-D4PB9Y).
  const flushProjectSessionPersistence = () => {
    if (projectSessionTimer !== undefined) {
      window.clearTimeout(projectSessionTimer);
      projectSessionTimer = undefined;
    }
    persistProjectSessionNow();
  };
  // Unsaved text edits reach the server when the page hides; their local copies survive either way.
  const flushOnHide = () => {
    flushProjectSessionPersistence();
    conversationPersistence.flush();
    void flushTicketDrafts();
    flushCommandAutosaves();
  };
  window.addEventListener('pagehide', flushOnHide);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') flushOnHide();
  });
  async function openStartupTicketDeepLink(link: TicketDeepLink) {
    try {
      let target = projects.value.find((item) => item.id === link.project || item.root === link.project);
      if (!target) {
        const root = ticketDeepLinkRoot(link.project, await localBridge.checkouts());
        if (!root) throw new Error(`No registered project matches ${link.project}.`);
        target = projects.value.find((item) => item.root === root);
        if (!target) {
          const opened = await openProject(root, undefined, true, false);
          if (!opened) throw new Error(`Could not open project ${root}.`);
          target = projects.value.find((item) => item.id === opened.id);
        }
      }
      if (!target) throw new Error(`Could not open project ${link.project}.`);
      if (selectedProjectId.value !== target.id) await activateOpenedProject(target);
      setShellMode('project');
      scheduleProjectSessionPersistence();
      if (link.ticket) {
        const ticket = (await new Api(target.apiPath).checkoutTicket(target.id, link.ticket)).ticket;
        selectedTicketSlugs.value = [ticket.slug];
        presentTicket(ticket);
        presentTicketReaderDialog('workspace-reader', undefined, () => {
          readerOpen.value = true;
        });
      }
    } catch (reason) {
      showToast(`Could not open project link: ${reason instanceof Error ? reason.message : String(reason)}`);
    }
  }
  const startupTicketDeepLink = parseTicketDeepLink(window.location.search);
  const rememberedActiveRoot = activeProjectRoot(localStorage);
  void (async () => {
    try {
      await restoreRememberedProjects({
        roots: rememberedRoots,
        activeRoot: rememberedActiveRoot,
        fetch: (root) => openProjectFetch(root),
        wire: wireOpenedProject,
        retainFailure: (root, failure) => {
          retainProjectRestoreFailure(root, failure.error, failure.recovery?.expected.pid);
        },
        activate: activateOpenedProject,
        selectFailure: (root) => {
          selectedProjectRestoreRoot.value = root;
        },
        // Show the active project as soon as it is ready; the others keep restoring behind it
        // (HS2-X74D4B). Both reconciles are idempotent and run again once every project is registered.
        settled: (root) => {
          projectRestorePendingRoots.value = projectRestorePendingRoots.value.filter((item) => item !== root);
        },
        activeReady: () => {
          initialProjectRestorePending.value = false;
          startPermissionUpdates();
          syncProjectChangeStreams();
        },
      });
      if (startupTicketDeepLink) await openStartupTicketDeepLink(startupTicketDeepLink);
      startPermissionUpdates();
      syncProjectChangeStreams();
    } finally {
      initialProjectRestoreComplete = true;
      initialProjectRestorePending.value = false;
      projectRestorePendingRoots.value = [];
    }
  })();
  return {
    appRoot,
    disposeInteractions: () => {
      disposeToolbarVisibility();
      disposeRailToolbarVisibility();
      toastHost.removeEventListener('wa-after-hide', onToastHide);
      toastPresentation.dispose();
      toastHost.remove();
      toastLifetime.dispose();
      disposeInteractions();
      haltedSessionsController.dispose();
      permissionsController.dispose();
      disposeClaimClock();
      disposeProjectChangeStreams();
      terminalDashboardController.dispose();
    },
  };
}
