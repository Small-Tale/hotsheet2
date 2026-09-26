import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { type ResizableRegionAxis, type ResizableRegionEdge } from '@kerfjs/ui/resizable-region';
import { Select } from '@kerfjs/ui/select';
import { readTokenSearchField } from '@kerfjs/ui/token-search-field';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { batch, effect, mount, signal } from 'kerfjs';
import { ChevronLeft, Trash2 } from 'lucide';

import {
  applyKnownActiveTicketExpiries,
  isTicketActivelyWorkedOn,
  nextActiveTicketExpiry,
  projectTabTicketState,
} from '../active-ticket-work';
import {
  collectMatchingSearchPages,
  filterAdvancedSearchResults,
  usesAdvancedSearchExpression,
  usesBooleanSearchExpression,
} from '../advanced-search';
import {
  applyConversationActivity,
  applyConversationEvent,
  beginConversationTurn,
  type ConversationState,
  conversationUsage,
  EMPTY_CONVERSATION,
} from '../ai-conversation';
import {
  type AiToolDefaults,
  Api,
  type Capabilities,
  type CheckoutTicketCounts,
  type CheckoutTicketQuery,
  type CommandDefinition,
  type CommandRun,
  type CorruptTicket,
  type CustomView,
  type DuplicateBacklink,
  type FullTicket,
  type PollResponse,
  type RepositoryStatus,
  revealCorruptTicketFile,
  type TicketRow as WireTicketRow,
  type ToolConnection,
  TurnStreamReplayGuard,
} from '../api';
import {
  type AppRegionId,
  loadAppRegionSize,
  normalizeAppRegionSize,
  saveAppRegionSize,
  terminalDrawerMaximum,
} from '../app-region-resize';
import { attachmentRoundNumbers } from '../attachment-grouping';
import {
  applyBoardColumnFetch,
  boardColumnHasMore,
  type BoardColumnPage,
  boardColumnStatus,
  boardColumnStatuses,
  isPerColumnBoardView,
  nextBoardColumnFetch,
} from '../board-pagination';
import { browserRandomId } from '../browser-id';
import { AppEmptyState, ProjectRestoreState } from '../components/app-empty-state';
import { AppError } from '../components/app-error';
import { BulkTicketDialog, type BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import { ConversationExportDialog } from '../components/conversation-export-dialog';
import { corruptTicketKey, type CorruptTicketRecoveryState } from '../components/corrupt-ticket-row';
import { Hs1CleanupBanner, Hs1JobBanner, Hs1MigrationBanner, Hs1MigrationDialog } from '../components/hs1-migration';
import { MainShell } from '../components/main-shell';
import { ManualModelDialog } from '../components/manual-model-dialog';
import type { MarkdownEditorMode } from '../components/markdown-editor';
import { NotificationCenter } from '../components/notification-center';
import { NotificationInspector } from '../components/notification-inspector';
import { type NotificationView, notificationViewTitle } from '../components/notification-navigation';
import {
  ProjectCloseDialog,
  type ProjectCloseDialogState,
  type ProjectCloseResource,
  projectCloseResourceKey,
  selectedProjectCloseResource,
} from '../components/project-close-dialog';
import { ProjectDialog, projectDialogRoot, RemoteProjectDialog } from '../components/project-dialog';
import { ProjectRestoreError, projectRestoreTabId } from '../components/project-restore-error';
import { type ProjectTabProps } from '../components/project-tab';
import { type ProjectTabBarMode } from '../components/project-tab-bar';
import { type AppTabKind } from '../components/project-tab-context-menu';
import {
  focusQuickTicketComposerTitle,
  QuickTicketComposer,
  QuickTicketLauncher,
  showQuickTicketComposer,
} from '../components/quick-ticket-composer';
import {
  AppTabMenuSurface,
  AttachmentContextMenuSurface,
  CompatibilityBannerSurface,
  ConnectionDetailsSurface,
  NotWorkingSurface,
  ReaderLayersSurface,
  ReaderLayerSurface,
  TicketContextMenuSurface,
  type TicketContextMenuSurfaceProps,
} from '../components/reader-overlay-surfaces';
import { SavedViewDeleteDialog, SavedViewDialog } from '../components/saved-view-dialog';
import { ServerBusyBars, ServerBusyMessage } from '../components/server-busy-bars';
import { type SettingsCategory, settingsCategoryTitle } from '../components/settings-navigation';
import { SettingsWorkspace } from '../components/settings-workspace';
import type { TicketStatus } from '../components/status-badge';
import { TerminalDashboardControls, type TerminalDashboardGroup } from '../components/terminal-dashboard';
import { TerminalRenameDialog } from '../components/terminal-rename-dialog';
import {
  TerminalVisibilityDialog,
  TerminalVisibilityNameDialog,
  type TerminalVisibilityNamePrompt,
} from '../components/terminal-visibility-dialog';
import { TicketCloseDialog, type TicketCloseDialogState } from '../components/ticket-close-dialog';
import type { TicketEmptyStateProps } from '../components/ticket-empty-state';
import { type InspectorTab, type TicketInspectorProps } from '../components/ticket-inspector';
import { TicketInspectorSkeleton } from '../components/ticket-inspector-skeleton';
import { CorruptInspector, Inspector, InspectorPlaceholder } from '../components/ticket-inspector-surface';
import { type TicketLinkChoice, TicketLinkChoiceDialog } from '../components/ticket-link-choice-dialog';
import { TicketList } from '../components/ticket-list';
import { showTicketReaderDialog, type TicketReaderDialogElement } from '../components/ticket-reader';
import type { TicketPriority, TicketRowProps } from '../components/ticket-row';
import { TicketSourceSetupDialog } from '../components/ticket-source-setup-dialog';
import { SavedViewContextMenu } from '../components/view-navigation';
import {
  GlobalWorkspaceSurface,
  ProjectTerminalDrawerSurface,
  SidebarSurface,
  type SidebarSurfaceProps,
  TerminalOperationsSurface,
  type TerminalOperationsSurfaceProps,
  TerminalRailSurface,
  type TerminalRailSurfaceProps,
  WorkspaceSurface,
  type WorkspaceSurfaceProps,
} from '../components/workspace-composition-surfaces';
import {
  WorkspaceControls,
  WorkspaceIdentity,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  workspaceUpNextState,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { withControlledOpen } from '../controlled-open';
import { loadConversationStates, saveConversationStates } from '../conversation-persistence';
import { syncConversationScroll } from '../conversation-scroll';
import { customAiCommandSignalConnection, customAiCommandTicket, HOTSHEET_SKILL_SIGNAL } from '../custom-ai-command';
import {
  drawerTabFocusRequestStillOwned,
  drawerTabSelectionAfterClose,
  loadDrawerTabOrder,
  orderedDrawerTabIds,
  saveDrawerTabOrder,
  selectedDrawerInput,
} from '../drawer-tab-order';
import { createAiConfigurationController } from '../features/ai-configuration';
import { createCommandsController } from '../features/commands';
import { createConversationArchiveController } from '../features/conversation-archive';
import { createGalleryController } from '../features/gallery';
import { createPermissionsController } from '../features/permissions';
import { createProjectLifecycleController } from '../features/project-lifecycle';
import { createRepositoryController } from '../features/repository';
import { createSavedViewsController } from '../features/saved-views';
import { createTerminalPresentation } from '../features/terminal-presentation';
import { createTerminalViewportsController } from '../features/terminal-viewports';
import { createTicketWorkflows } from '../features/ticket-workflows';
import { fullTicketFeedbackNeeded } from '../feedback-needed';
import { type InlineFeedbackReply } from '../feedback-replies';
import {
  activeDatePrefix,
  activeTagPrefix,
  consumeSearchTokens,
  effectiveSearch,
  fromTokenSearchTokens,
  type InlineSearchToken,
  orderedSearchText,
  sameInlineSearchState,
  tokenFromRaw,
  tokenQuery,
  toTokenSearchToken,
} from '../inline-search';
import { restoreInlineSearchCaret } from '../inline-search-caret';
import { beginInteractionTiming } from '../interaction-performance';
import { data } from '../interactions/dom';
import type { Control, DetailsFinishTask, NotWorkingTarget, PendingEvidence, Project } from '../interactions/types';
import { isAppleShortcutPlatform, loadShortcutOverrides, type ShortcutChord } from '../keyboard-shortcuts';
import { LocalTicketChangeAcknowledgements } from '../local-ticket-changes';
import { migrationPercent, migrationPhaseLabel } from '../migration-progress';
import { isMobileViewport, MOBILE_OVERLAYS_CLOSED, type MobileOverlayState } from '../mobile-layout';
import {
  loadMobileTerminalColumns,
  MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT,
  nextMobileTerminalColumns,
  saveMobileTerminalColumns,
} from '../mobile-terminal-columns';
import {
  INACTIVE_MOBILE_TERMINAL_FOCUS,
  mobileTerminalViewport,
  mobileVirtualKeyboardVisible,
  transitionMobileTerminalFocus,
} from '../mobile-terminal-focus';
import { mergeRetainedCreatedRows, PendingCreatedTickets, prependCreatedTicketRow } from '../pending-created-tickets';
import { parsePermissionResolution, PERMISSION_DELAYS } from '../permission-notifications';
import { priorityFromWire } from '../priority-wire';
import { afterBrowserPaint } from '../project-activation';
import { containsRepositoryChange, containsTicketChange, startProjectChangeStream } from '../project-change-poll';
import {
  type DrawerAIChat,
  prepareProjectConversation,
  projectChatConnectionId,
  projectDriveControlState,
  recoverProjectConnections,
  restoreDrawerAIChats,
  runProjectDrive,
  SIDEBAR_DRIVE_PROMPT,
  sidebarDriveConnectionId,
} from '../project-drive';
import { openProjectFetch, restoreRememberedProjects } from '../project-startup';
import { createProjectTabRefreshCoordinator } from '../project-tab-refresh';
import {
  appendUniqueTicketRows,
  BOARD_COLUMN_PAGE_SIZE,
  type BoardRefreshSpec,
  loadProjectTicketRefresh,
  type ProjectTicketRefresh,
} from '../project-ticket-refresh';
import { createProjectWarmCache } from '../project-warm-cache';
import { createRefreshBarrier } from '../refresh-barrier';
import { createRenderMetrics } from '../render-metrics';
import { computeServerBusyBarCount, serverBusy, serverBusyMessage } from '../server-busy';
import { applyRememberedTabOrder, interleaveByRank } from '../tab-order';
import { TERMINAL_GRID_DEFAULT_ACROSS, TERMINAL_GRID_DEFAULT_HIGH } from '../terminal-grid-layout';
import { defaultTerminalName, parseTerminalNames, terminalNameKey } from '../terminal-names';
import { terminalDrawerActivation, terminalProjectOwner } from '../terminal-project-scope';
import { TERMINAL_DRAWER_RESIZE_END_EVENT, type TerminalFocusRequest } from '../terminal-viewport';
import {
  activeTerminalVisibilityGroup,
  hideNewTerminalInNamedGroups,
  parseTerminalVisibilityState,
  TERMINAL_DASHBOARD_VISIBILITY_SCOPE,
  TERMINAL_VISIBILITY_STORAGE_KEY,
  TERMINAL_VISIBILITY_TYPES,
  terminalProjectVisibilityScope,
  terminalVisibilityItems,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import { hasUnresolvedBlocker } from '../ticket-blocking';
import { ticketBoardGroups, ticketBoardGroupTotal } from '../ticket-board-layout';
import { BulkTicketMutationSequencer, canBulkUpdate } from '../ticket-bulk-operations';
import { loadLastTicketCategory } from '../ticket-category-preference';
import { type DuplicateTarget } from '../ticket-close';
import { ticketCompletionTrend } from '../ticket-completion-trend';
import { loadTicketEditorSizes } from '../ticket-editor-size';
import { reconcileActiveDraft, type TicketFieldConflict } from '../ticket-field-reconciliation';
import { parseTicketLinkReference, resolveTicketLink, type TicketLinkMatch } from '../ticket-link-resolution';
import { animateTicketMotion, captureTicketMotion, waitForTicketMotionSettled } from '../ticket-motion';
import { type ClipboardTicket, type TicketHistory, type TicketSnapshot } from '../ticket-operations';
import {
  activeTicketReaderProject,
  disposeTicketReaderFrames,
  popTicketReaderFrame,
  pushTicketReaderFrame,
  ticketReaderEditState,
  type TicketReaderFrame,
} from '../ticket-reader-stack';
import { TicketScrollMemory } from '../ticket-scroll-state';
import {
  canCreateTicketInView,
  customTicketViewId,
  customTicketViewKey,
  isArchivedTicket,
  isOpenTicket,
  isQueuedTicket,
  isTrashedTicket,
  selectionAfterTicketViewChange,
  ticketSearchCountViews,
  ticketsForView,
  type TicketView,
  ticketViewQuery,
} from '../ticket-views';
import { createTrailingTask } from '../trailing-task';
import { renderStormSuppressionReason } from '../ui-stability-diagnostics';
import { syncVideoPosters } from '../video-posters';
import { loadWorkspacePreferences, saveWorkspacePreferences, sortableWorkspaceView } from '../workspace-preferences';
import {
  activeProjectRoot,
  hs1CleanupPromptDismissed,
  loadDraftFiles,
  loadProjectWorkspaceSession,
  saveActiveProjectRoot,
  saveProjectWorkspaceSession,
} from '../workspace-session';
import { compareWorkspaceTickets } from '../workspace-ticket-sort';
import { installDevelopmentDiagnostics } from './development-bootstrap';
import { createHotSheetInteractionBindings, type InteractionBindingsPort } from './interaction-bindings';
import { wireHotSheetInteractions } from './wire-interactions';

/** Create and start one Hot Sheet browser application runtime. */
export async function startHotSheetWebClient() {
  const uiStabilityDiagnostics = await installDevelopmentDiagnostics();
  const turnStreamReplayGuard = new TurnStreamReplayGuard();
  const localTicketChangeAcknowledgements = new LocalTicketChangeAcknowledgements();
  // Retains just-created rows so an eventually-consistent index that hasn't indexed them yet
  // cannot drop them from a background refresh (HS2-Y5PDHW).
  const pendingCreatedTickets = new PendingCreatedTickets();
  const turnStreamEvents = (response: PollResponse) => turnStreamReplayGuard.events(response);
  let backgroundProjectRefresh = false;

  loadTicketEditorSizes(localStorage, document.documentElement.style);

  const projects = signal<Project[]>([]),
    selectedProjectId = signal(''),
    tickets = signal<WireTicketRow[]>([]),
    ticketRowsByProject = signal<Record<string, WireTicketRow[]>>({}),
    corruptTickets = signal<CorruptTicket[]>([]),
    selectedTicket = signal<FullTicket | null>(null);
  const ticketCountsByProject = signal<Record<string, CheckoutTicketCounts>>({});
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
  interface SidebarSearchCounts {
    projectId: string;
    signature: string;
    generation: number;
    values: Record<string, number>;
    pending: string[];
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
  interface CachedProjectProjection {
    corruptTickets: CorruptTicket[];
    repository: RepositoryStatus | null;
    repositoryError: string;
    commandDefinitions: CommandDefinition[];
    commandRuns: CommandRun[];
    searchMatchKeys?: Set<string>;
  }
  const projectProjectionById = signal<Record<string, CachedProjectProjection>>({});
  // Bounded LRU of projects whose loaded projection stays resident so tab switches paint instantly and
  // revalidate silently; evicted projects fall back to a cold (loading) activation (HS2-AZZ9TF).
  const warmProjects = createProjectWarmCache();
  const projectCloseDialog = signal<ProjectCloseDialogState | undefined>(undefined);
  const ticketLinkChoice = signal<TicketLinkChoice | undefined>(undefined);
  let pendingProjectCloseIds: string[] = [];
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
    statsProjectId = signal<string | undefined>(undefined);
  const terminalRailScreen = signal<'root' | 'ticket'>('root'),
    terminalRailDirection = signal<'forward' | 'backward'>('forward');
  const terminalGroups = signal<TerminalDashboardGroup[]>([]),
    terminalDashboardLoading = signal(false),
    terminalDashboardMessage = signal('');
  const showLoadingActivity = signal(localStorage.getItem('hotsheet.show-loading-activity') === 'true');
  const inheritGlobalShellHistory = signal(false),
    terminalSettingsMessage = signal(''),
    trashCleanupDaysByProject = signal<Record<string, number>>({}),
    trashSettingsMessagesByProject = signal<Record<string, string>>({});
  const terminalDashboardSize = signal({ width: 1200, height: 601 }),
    terminalFitAcross = signal(
      Number(localStorage.getItem('hotsheet.terminals.fit-across')) || TERMINAL_GRID_DEFAULT_ACROSS,
    ),
    terminalFitHigh = signal(Number(localStorage.getItem('hotsheet.terminals.fit-high')) || TERMINAL_GRID_DEFAULT_HIGH);
  const rememberedRoots = [...new Set(JSON.parse(localStorage.getItem('hotsheet.open-projects') || '[]') as string[])],
    initialProjectRestorePending = signal(rememberedRoots.length > 0),
    // Remembered roots still opening in the background after the active project was revealed (HS2-2BEJXD).
    projectRestorePendingRoots = signal<readonly string[]>(rememberedRoots);
  const pendingRestoreEntries = <T,>(item: (root: string) => T) =>
    projectRestorePendingRoots.value.map((root) => ({ rank: rememberedRoots.indexOf(root), item: item(root) }));
  const initialTerminalDrawerVisible = localStorage.getItem('hotsheet.terminals.drawer-open') === 'true';
  const terminalDrawerVisible = signal(initialTerminalDrawerVisible),
    terminalDrawerMounted = signal(initialTerminalDrawerVisible),
    terminalDrawerTransitioning = signal(false),
    terminalDrawerSize = signal(loadAppRegionSize(localStorage, 'app-terminal-drawer')),
    terminalDrawerMax = signal(520),
    terminalDrawerMaximized = signal(false),
    terminalDrawerBounds = signal({ width: 900, height: 320 }),
    terminalDrawerFitAcross = signal(Number(localStorage.getItem('hotsheet.terminals.drawer-fit-across')) || 2),
    terminalDrawerFitHigh = signal(Number(localStorage.getItem('hotsheet.terminals.drawer-fit-high')) || 2),
    terminalDrawerSelected = signal('grid'),
    mobileTerminalFocus = signal(INACTIVE_MOBILE_TERMINAL_FOCUS),
    mobileTerminalColumns = signal(loadMobileTerminalColumns(localStorage)),
    mobileViewportGeometry = signal(currentMobileViewportGeometry());
  const terminalDrawerChatsByProject = signal<Record<string, DrawerAIChat[]>>({}),
    terminalDrawerOrderByProject = signal<Record<string, string[]>>({}),
    terminalDrawerCreateMenuOpen = signal(false);
  const magnifiedTerminalKey = signal<string | undefined>(undefined),
    terminalVisibility = signal(parseTerminalVisibilityState(localStorage.getItem(TERMINAL_VISIBILITY_STORAGE_KEY))),
    terminalVisibilityDialogScope = signal<string | undefined>(undefined),
    terminalVisibilityFilter = signal<readonly TerminalVisibilityType[]>(TERMINAL_VISIBILITY_TYPES),
    terminalVisibilityContextMenu = signal<{ id: string; x: number; y: number } | undefined>(undefined),
    terminalVisibilityNamePrompt = signal<TerminalVisibilityNamePrompt | undefined>(undefined);
  const terminalNames = signal(parseTerminalNames(localStorage.getItem('hotsheet.terminals.names'))),
    terminalContextMenu = signal<{ key: string; x: number; y: number } | undefined>(undefined),
    terminalRename = signal<{ projectId: string; terminalId: string; value: string } | undefined>(undefined);
  let terminalDashboardGeneration = 0,
    terminalDashboardObserver: ResizeObserver | undefined,
    terminalDrawerObserver: ResizeObserver | undefined,
    terminalCreateChain: Promise<unknown> = Promise.resolve();
  let terminalDrawerTransitionTimer: number | undefined, terminalPreviewClickTimer: number | undefined;
  let pendingTerminalFocus: TerminalFocusRequest | undefined;
  const { syncTerminalViewportMounts } = createTerminalViewportsController({
    projects,
    mobileTerminalColumns: () => mobileTerminalColumns.peek(),
    get pendingTerminalFocus() {
      return pendingTerminalFocus;
    },
    set pendingTerminalFocus(value) {
      pendingTerminalFocus = value;
    },
    openTicketReference(element, slug, projectId, preferredProject) {
      ticketLinkReturnFocus = element;
      void selectLinkedTicket(slug, projectId, preferredProject);
    },
  });
  const corruptRecovery = signal<Record<string, CorruptTicketRecoveryState>>({});
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
  const projectChangeStreams = new Map<string, () => void>();
  const repositoryRefreshTimers = new Map<string, number>();
  let claimLeaseExpiryTimer: number | undefined;
  const storedWorkspacePreferences = loadWorkspacePreferences(localStorage);
  const loading = signal(false),
    error = signal(''),
    toastMessage = signal(''),
    viewMode = signal<WorkspaceViewMode>(storedWorkspacePreferences.viewMode),
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
  const ticketNextCursor = signal<string | undefined>(undefined),
    ticketCursorsByProject = signal<Record<string, string | undefined>>({}),
    ticketPageLoading = signal(false);
  // Per-column board pagination (HS2-8NBGBX): each status column paginates independently. `boardColumnPages`
  // holds the next-page cursor + loaded count per column id; `boardColumnLoading` is the per-column
  // spinner. Both are scoped to the active project+view and cleared when that changes.
  const boardColumnPages = signal<Record<string, BoardColumnPage>>({}),
    boardColumnLoading = signal<Record<string, boolean>>({});
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
  let toastTimer: number | undefined;
  function showToast(message: string) {
    toastMessage.value = message;
    if (toastTimer !== undefined) window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => {
      toastMessage.value = '';
      toastTimer = undefined;
    }, 2_500);
  }
  const activeTicketCount = signal(0),
    projectTabClaimClock = signal(Date.now());
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
    restoringProjectSession = false;
  const sidebarSize = signal(loadAppRegionSize(localStorage, 'app-sidebar')),
    inspectorSize = signal(loadAppRegionSize(localStorage, 'app-inspector'));
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
  });
  const {
    attachmentGalleryUrl,
    attachmentGalleryGeometry,
    attachmentGalleryScale,
    attachmentGalleryMarkup,
    attachmentGalleryDrawMode,
    attachmentGalleryAnnotations,
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
    shiftGallery,
  } = galleryController;
  const readerDetailsMode = signal<MarkdownEditorMode>('preview'),
    readerDetailsDraft = signal('');
  let readerDetailsEditGeneration = 0;
  const attachmentMessage = signal('');

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
  const titleEditing = signal(false),
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
  const defaultProviders = signal<Record<string, { name: string; capabilities: Capabilities } | undefined>>({});
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
    reorderCommandSettings,
    addCommandGroup,
    deleteCommandGroup,
    commandRunFor,
    showCommandDialog,
    commandIcon,
    commandDialogSurface,
  } = createCommandsController({ projects, selectedProjectId, storedWorkspacePreferences });
  const driveConnectionsByProject = signal<Record<string, ToolConnection[]>>({}),
    drivePendingByProject = signal<Record<string, boolean>>({});

  const conversationStates = signal<Record<string, ConversationState>>(loadConversationStates(localStorage)),
    conversationDrafts = signal<Record<string, string>>({}),
    conversationSelections = signal<Record<string, { model?: string; effort?: string }>>({}),
    conversationConnectionId = signal<string | undefined>(undefined),
    conversationOpen = signal(false);

  const aiConfigurationController = createAiConfigurationController({
    selectedProjectId,
    project: () => project(),
    conversationConnectionId,
    conversationSelections,
    terminalDrawerChatsByProject,
    driveConnectionsByProject,
    conversationStates,
    conversationOpen,
    createDrawerAIChat,
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
    driveOverridesByProject,
    manualModelDialog,
    aiToolLabel,
    normalizedAiSelection,
    effectiveCommandAiSelection,
    effectiveDriveSelection,
    selectDriveModel,
    selectDefaultModel,
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
    terminalDrawerCreateMenuOpen,
    terminalVisibility,
    persistTerminalVisibility,
    replaceConversationStates,
    selectDrawerItem,
    setTerminalDrawerVisible,
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
    persistPermissionHistory,
    refreshPermissions,
    startPermissionUpdates,
    resolvePermission,
    updatePermissionTimer,
    permissionPopupSurface,
  } = permissionsController;
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
    loadPermissionAutomation,
    setPermissionAutomation: (projectId, automation) => {
      permissionAutomationByProject.value = { ...permissionAutomationByProject.value, [projectId]: automation };
    },
    startPermissionUpdates,
    syncProjectChangeStreams,
    refreshProject,
    refreshCommands,
    refreshCustomViews,
    refreshDriveConnections,
    refreshTerminalDashboard,
    restoreProjectSession,
    customViewFor,
    applyCustomViewQuery,
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
    providerSettingsBusy,
    providerSettingsError,
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
  let projectActivationGeneration = 0,
    projectRefreshGeneration = 0,
    ticketCollectionGeneration = 0,
    commandRefreshGeneration = 0,
    projectSessionRestoreRun = 0;
  const ticketCollectionRefreshTask = createTrailingTask<TicketView>(250, (view) => {
    const query = ticketViewQuery(view);
    if (JSON.stringify(query) === JSON.stringify(ticketPageQuery.value)) {
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  const canUpdateSelected = () => selectedTicket.value ? (providerCapabilities.value[selectedTicket.value.connection_id]?.update ?? true) : false;
  const noteCapabilities = () =>
    selectedTicket.value ? providerCapabilities.value[selectedTicket.value.connection_id] : undefined;
  const canAddNotes = () => noteCapabilities()?.notes ?? true;
  const canEditNotes = () => noteCapabilities()?.note_edit ?? canUpdateSelected();
  const canDeleteNotes = () => noteCapabilities()?.note_delete ?? canUpdateSelected();
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  const canUseAttachments = () => selectedTicket.value ? (providerCapabilities.value[selectedTicket.value.connection_id]?.attachments ?? true) : false;
  const capabilitiesFor = (connectionId: string) => providerCapabilities.value[connectionId];
  const defaultProvider = () => defaultProviders.value[selectedProjectId.value];
  const canStageNewTicketAttachments = () => {
    const capabilities = defaultProvider()?.capabilities;
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
    savedViewDialogMode,
    savedViewName,
    savedViewQuery,
    savedViewQueryTokens,
    savedViewBusy,
    savedViewError,
    savedViewMenu,
    savedViewDeleteTargetId,
    savedViewDeleteBusy,
    savedViewDeleteError,
    openSavedViewDialog,
    openSavedViewRename,
    closeSavedViewDialog,
    updateSavedViewQuery,
    focusSavedViewQuery,
    removeSavedViewQueryToken,
    editSavedViewQueryToken,
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
    selectTicketView,
    showToast,
  });
  function applyCustomViewQuery(view: CustomView) {
    const parsed = consumeSearchTokens(view.query, true);
    batch(() => {
      searchOpen.value = true;
      searchQuery.value = parsed.text;
      searchTokens.value = parsed.tokens;
      searchHelpOpen.value = false;
      searchMatchKeys.value = undefined;
    });
    resetProgressiveTicketRendering();
    scheduleTicketSearch();
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
      if (definition) applyCustomViewQuery(definition);
      else if (customTicketViewKey(previous)) {
        searchOpen.value = false;
        searchQuery.value = '';
        searchTokens.value = [];
        searchHelpOpen.value = false;
        searchMatchKeys.value = undefined;
      }
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
      query = ticketViewQuery(view),
      generation = ++ticketCollectionGeneration;
    if (!current) return;
    const active = () =>
      generation === ticketCollectionGeneration && project()?.id === current.id && selectedView.value === view;
    try {
      // Board columns page in the active sort so each column's cursor continues its own order; the list keeps
      // its view query, which its continuation cursor is bound to.
      const board = boardRefreshSpec(view, current.id),
        index = await loadProjectTicketRefresh(
          new Api(current.apiPath),
          current.id,
          board ? sortedTicketQuery(query) : query,
          board,
        );
      if (!active()) return;
      if (index.tickets) {
        const mergedTickets = mergeRetainedCreatedRows(
          index.tickets,
          pendingCreatedTickets.retain(current.id, index.tickets),
        );
        boardColumnPages.value = index.boardPages ?? {};
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
  function terminalSession(key?: string) {
    return terminalGroups.value
      .flatMap((group) => group.sessions)
      .find((session) => `${session.projectId}:${session.id}` === key);
  }
  function terminalHiddenKeys(scope: string) {
    return activeTerminalVisibilityGroup(terminalVisibility.value, scope).hiddenKeys;
  }
  function terminalHiddenCount(scope: string, projectId?: string) {
    const live = new Set(
      terminalVisibilityItems(
        workspaceTerminalGroups(),
        projectId ? terminalProjectVisibilityScope(projectId) : scope,
      ).flatMap((group) => group.items.map((item) => item.key)),
    );
    return terminalHiddenKeys(scope).filter((key) => live.has(key)).length;
  }
  function persistTerminalVisibility(next: typeof terminalVisibility.value) {
    terminalVisibility.value = next;
    localStorage.setItem(TERMINAL_VISIBILITY_STORAGE_KEY, JSON.stringify(next));
  }
  function terminalVisibilityScopeFor(target: Element) {
    return (
      target.closest<HTMLElement>('[data-visibility-scope]')?.dataset.visibilityScope ??
      TERMINAL_DASHBOARD_VISIBILITY_SCOPE
    );
  }
  function terminalKeysForVisibilityDialog() {
    return terminalVisibilityItems(
      workspaceTerminalGroups(),
      terminalVisibilityDialogScope.value ?? TERMINAL_DASHBOARD_VISIBILITY_SCOPE,
      terminalVisibilityFilter.value,
    ).flatMap((group) => group.items.map((item) => item.key));
  }
  function drawerTabOrder(projectId: string) {
    return terminalDrawerOrderByProject.value[projectId] ?? loadDrawerTabOrder(localStorage, projectId);
  }
  function currentDrawerTabIds(projectId: string) {
    const terminalIds =
        terminalGroups.value.find((group) => group.projectId === projectId)?.sessions.map((session) => session.id) ??
        [],
      chatIds = (terminalDrawerChatsByProject.value[projectId] ?? []).map((chat) => chat.id);
    return orderedDrawerTabIds(terminalIds, chatIds, drawerTabOrder(projectId));
  }
  function persistDrawerTabOrder(projectId: string, ids: readonly string[]) {
    const order = saveDrawerTabOrder(localStorage, projectId, ids);
    terminalDrawerOrderByProject.value = { ...terminalDrawerOrderByProject.value, [projectId]: order };
    terminalGroups.value = terminalGroups.value.map((group) =>
      group.projectId === projectId
        ? { ...group, sessions: applyRememberedTabOrder(group.sessions, (item) => item.id, order) }
        : group,
    );
  }
  function focusDrawerTab(projectId: string, id: string) {
    const scheduled = document.activeElement;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const drawer = [...document.querySelectorAll<HTMLElement>('[data-component="terminal-drawer"]')].find(
            (item) => item.dataset.projectId === projectId,
          ),
          tab =
            id === 'grid'
              ? drawer?.querySelector<HTMLElement>('[data-item-id="grid"]')
              : [...(drawer?.querySelectorAll<HTMLElement>('[data-tab-id]') ?? [])]
                  .find((item) => item.dataset.tabId === id)
                  ?.querySelector<HTMLElement>('.kui-app-tab__select');
        tab?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
        if (drawerTabFocusRequestStillOwned(scheduled, document.activeElement, document.body)) tab?.focus();
      }),
    );
  }
  function focusDrawerInput(projectId: string) {
    const scheduled = document.activeElement;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const drawer = [...document.querySelectorAll<HTMLElement>('[data-component="terminal-drawer"]')].find(
            (item) => item.dataset.projectId === projectId,
          ),
          input = drawer ? selectedDrawerInput(drawer) : undefined;
        if (input && drawerTabFocusRequestStillOwned(scheduled, document.activeElement, document.body)) input.focus();
      }),
    );
  }
  async function refreshTerminalDashboard() {
    const generation = ++terminalDashboardGeneration,
      openProjects = [...projects.value];
    terminalDashboardLoading.value = true;
    terminalDashboardMessage.value = '';
    const results: Array<TerminalDashboardGroup | undefined> = await Promise.all(
      openProjects.map(async (current) => {
        try {
          const [infos] = await Promise.all([
              new Api(current.apiPath).terminals(),
              ...(current.id !== selectedProjectId.value && !Object.hasOwn(driveConnectionsByProject.value, current.id)
                ? [refreshDriveConnections(current, true)]
                : []),
            ]),
            owned = infos.filter((session) => terminalProjectOwner(openProjects, session.cwd) === current.id),
            sessions = owned.map((session, index) => ({
              ...session,
              scrollback: '',
              projectId: current.id,
              projectName: current.name,
              title:
                terminalNames.value[terminalNameKey(current.id, session.id)] ?? defaultTerminalName(session.id, index),
            }));
          return {
            projectId: current.id,
            projectName: current.name,
            sessions: applyRememberedTabOrder(sessions, (item) => item.id, drawerTabOrder(current.id)),
          } satisfies TerminalDashboardGroup;
        } catch {
          return undefined;
        }
      }),
    );
    if (generation !== terminalDashboardGeneration) return;
    terminalGroups.value = results.flatMap((group) => (group ? [group] : []));
    terminalDashboardMessage.value =
      openProjects.length > 0 && terminalGroups.value.length === 0 ? 'Terminal snapshots could not be loaded.' : '';
    terminalDashboardLoading.value = false;
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function observeTerminalDashboard(){queueMicrotask(()=>{terminalDashboardObserver?.disconnect();const target=document.querySelector<HTMLElement>('[data-terminal-grid-measure="true"]');if(!target)return;terminalDashboardObserver=new ResizeObserver(entries=>{const rect=entries[0]?.contentRect;if(!rect)return;const next={width:Math.max(1,Math.floor(rect.width)),height:Math.max(1,Math.floor(rect.height))},previous=terminalDashboardSize.value;if(next.width!==previous.width||next.height!==previous.height)terminalDashboardSize.value=next});terminalDashboardObserver.observe(target)})}
  function updateTerminalDrawerBounds(
    target: HTMLElement,
    rect: Pick<DOMRectReadOnly, 'width' | 'height'> = target.getBoundingClientRect(),
  ) {
    const next = { width: Math.max(1, Math.floor(rect.width)), height: Math.max(1, Math.floor(rect.height)) },
      previous = terminalDrawerBounds.value;
    if (next.width !== previous.width || next.height !== previous.height) terminalDrawerBounds.value = next;
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function observeTerminalDrawer(){queueMicrotask(()=>{terminalDrawerObserver?.disconnect();if(!terminalDrawerVisible.value)return;const target=document.querySelector<HTMLElement>('[data-terminal-drawer-measure="true"] .terminal-drawer__content');if(!target)return;terminalDrawerObserver=new ResizeObserver(entries=>{if(appRegionResizeDrag?.id==='app-terminal-drawer')return;const rect=entries[0]?.contentRect;if(rect)updateTerminalDrawerBounds(target,rect)});terminalDrawerObserver.observe(target)})}
  function requestDrawerInputFocus(projectId: string, id: string, chat: DrawerAIChat | undefined) {
    if (id === 'grid') return;
    if (!chat) pendingTerminalFocus = { projectId, terminalId: id };
    focusDrawerInput(projectId);
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function setTerminalDrawerVisible(visible:boolean,refresh=true){if(!visible)exitMobileTerminalFocus();if(visible===terminalDrawerVisible.value){if(visible){terminalDrawerMounted.value=true;if(refresh)void refreshTerminalDashboard();observeTerminalDrawer();settleTerminalDrawerGeometry()}return}if(terminalDrawerTransitionTimer!==undefined)window.clearTimeout(terminalDrawerTransitionTimer);if(visible){const current=project(),chat=current?terminalDrawerChatsByProject.value[current.id]?.find(item=>item.id===terminalDrawerSelected.value):undefined;if(current)requestDrawerInputFocus(current.id,terminalDrawerSelected.value,chat);terminalDrawerMounted.value=true}terminalDrawerTransitioning.value=true;terminalDrawerVisible.value=visible;localStorage.setItem('hotsheet.terminals.drawer-open',String(visible));terminalDrawerTransitionTimer=window.setTimeout(()=>{terminalDrawerTransitionTimer=undefined;terminalDrawerTransitioning.value=false;if(!terminalDrawerVisible.value)terminalDrawerMounted.value=false},220);if(visible){if(refresh)void refreshTerminalDashboard();observeTerminalDrawer()}else terminalDrawerObserver?.disconnect()}
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function selectDrawerItem(id:string){if(id!==terminalDrawerSelected.value)exitMobileTerminalFocus();const current=project(),chat=current?terminalDrawerChatsByProject.value[current.id]?.find(item=>item.id===id):undefined;if(chat)conversationConnectionId.value=chat.connectionId;terminalDrawerSelected.value=id;if(current){localStorage.setItem(`hotsheet.project.${current.id}.terminal-drawer-selection`,id);requestDrawerInputFocus(current.id,id,chat)}}
  function openTerminalInProject(key: string) {
    const session = terminalSession(key);
    if (!session) return;
    const activated =
      session.projectId === selectedProjectId.value ? undefined : activateOpenProject(session.projectId);
    pendingTerminalFocus = { projectId: session.projectId, terminalId: session.id };
    selectDrawerItem(session.id);
    terminalContextMenu.value = undefined;
    setShellMode('project');
    setTerminalDrawerVisible(true);
    if (activated) void refreshActivatedProject(activated, false);
    else void Promise.all([refreshProject({ showLoading: false }), refreshCommands()]);
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function openGridAIChat(projectId:string,chatId:string){const target=projects.value.find(item=>item.id===projectId),chat=terminalDrawerChatsByProject.value[projectId]?.find(item=>item.id===chatId);if(!target||!chat)return;const activated=projectId===selectedProjectId.value?undefined:activateOpenProject(projectId);setShellMode('project');selectDrawerItem(chat.id);setTerminalDrawerVisible(true);if(activated)void refreshActivatedProject(activated,false);else void Promise.all([refreshProject({showLoading:false}),refreshCommands()])}
  function saveTerminalName(projectId: string, terminalId: string, name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    terminalNames.value = { ...terminalNames.value, [terminalNameKey(projectId, terminalId)]: trimmed };
    localStorage.setItem('hotsheet.terminals.names', JSON.stringify(terminalNames.value));
    terminalGroups.value = terminalGroups.value.map((group) =>
      group.projectId === projectId
        ? {
            ...group,
            sessions: group.sessions.map((session) =>
              session.id === terminalId ? { ...session, title: trimmed } : session,
            ),
          }
        : group,
    );
  }
  // Serialize terminal creation so a create in flight (including its dashboard refresh) never *drops* a
  // later request — each click still opens its own terminal instead of being silently swallowed, which
  // looked like the same terminal being reused and shell commands not starting (HS2-2BKPGK).
  function queueTerminalCreate(run: () => Promise<void>): Promise<void> {
    const next = terminalCreateChain.then(run, run);
    terminalCreateChain = next.catch(() => undefined);
    return next;
  }
  async function createProjectTerminal(selection?: AiToolDefaults) {
    const current = project();
    if (!current) return;
    await queueTerminalCreate(async () => {
      if (project()?.id !== current.id) return;
      terminalDashboardLoading.value = true;
      terminalDashboardMessage.value = '';
      try {
        const created = await new Api(current.apiPath).createTerminal({
          cwd: current.root,
          ...(selection ? { connect: selection.tool, model: selection.model, effort: selection.effort } : {}),
        });
        persistTerminalVisibility(
          hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${created.id}`),
        );
        pendingTerminalFocus = { projectId: current.id, terminalId: created.id };
        selectDrawerItem(created.id);
        await refreshTerminalDashboard();
        focusDrawerTab(current.id, created.id);
      } catch (reason) {
        pendingTerminalFocus = undefined;
        terminalDashboardMessage.value = reason instanceof Error ? reason.message : String(reason);
        terminalDashboardLoading.value = false;
      }
    });
  }
  async function createShellCommandTerminal(command: CommandDefinition, current: Project) {
    if (!command.command) return;
    await queueTerminalCreate(async () => {
      if (project()?.id !== current.id) return;
      terminalDashboardLoading.value = true;
      terminalDashboardMessage.value = '';
      try {
        const created = await new Api(current.apiPath).createTerminal({
          shell_command: command.command,
          cwd: command.cwd || current.root,
        });
        saveTerminalName(current.id, created.id, command.title);
        persistTerminalVisibility(
          hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${created.id}`),
        );
        pendingTerminalFocus = { projectId: current.id, terminalId: created.id };
        selectDrawerItem(created.id);
        setTerminalDrawerVisible(true, false);
        await refreshTerminalDashboard();
      } catch (reason) {
        pendingTerminalFocus = undefined;
        terminalDashboardMessage.value = reason instanceof Error ? reason.message : String(reason);
        terminalDashboardLoading.value = false;
      }
    });
  }
  async function createDrawerAIChat(
    selection: AiToolDefaults,
    options: { connectionId?: string; drive?: boolean } = {},
  ) {
    const current = project();
    if (!current) return;
    const connectionId = options.connectionId ?? `hotsheet-drawer-chat-${browserRandomId()}`,
      existing = (terminalDrawerChatsByProject.value[current.id] ?? []).find(
        (item) => item.connectionId === connectionId,
      );
    if (existing) {
      selectDrawerItem(existing.id);
      setTerminalDrawerVisible(true);
      return existing;
    }
    drivePendingByProject.value = { ...drivePendingByProject.value, [current.id]: true };
    try {
      const created = await new Api(current.apiPath).createToolConnection({
        tool: selection.tool,
        checkout: current.id,
        connection_id: connectionId,
        model: selection.model,
        effort: selection.effort,
      });
      if (project()?.id !== current.id) return;
      driveConnectionsByProject.value = {
        ...driveConnectionsByProject.value,
        [current.id]: (driveConnectionsByProject.value[current.id] ?? [])
          .filter((item) => item.id !== created.id)
          .concat(created),
      };
      const tab = {
        id: `ai-chat:${created.id}`,
        connectionId: created.id,
        tool: selection.tool,
        name: options.drive ? `${aiToolLabel(selection.tool)} Drive` : `${aiToolLabel(selection.tool)} chat`,
        model: selection.model,
        effort: selection.effort,
        drive: options.drive,
      };
      terminalDrawerChatsByProject.value = {
        ...terminalDrawerChatsByProject.value,
        [current.id]: [...(terminalDrawerChatsByProject.value[current.id] ?? []), tab],
      };
      persistTerminalVisibility(hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${tab.id}`));
      selectDrawerItem(tab.id);
      setTerminalDrawerVisible(true);
      return tab;
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      drivePendingByProject.value = { ...drivePendingByProject.value, [current.id]: false };
    }
  }
  function disposeProjectTicketReaders(ids: readonly string[]) {
    const closing = new Set(ids),
      result = disposeTicketReaderFrames(linkedReaderStack.value, closing);
    for (const frame of result.disposed) {
      const saves = linkedReaderAutosaves.get(frame.id);
      saves?.details.cancel();
      saves?.note.cancel();
      saves?.blocked.cancel();
      linkedReaderAutosaves.delete(frame.id);
      readerReturnFocus.delete(frame.id);
      readerClosing.delete(frame.id);
      readerApprovedClose.delete(frame.id);
    }
    linkedReaderStack.value = result.retained;
    if (closing.has(selectedProjectId.value)) {
      readerDetailsAutosave.cancel();
      readerNoteAutosave.cancel();
      readerBlockedReasonAutosave.cancel();
      readerOpen.value = false;
      readerReturnFocus.delete('workspace-reader');
      readerClosing.delete('workspace-reader');
      readerApprovedClose.delete('workspace-reader');
    }
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function closeProjectIds(ids:readonly string[]){disposeProjectTicketReaders(ids);const closing=new Set(ids),before=projects.value,selectedIndex=before.findIndex(item=>item.id===selectedProjectId.value);let activation:ReturnType<typeof activateOpenProject>;for(const id of ids){projectTabRefresh.cancel(id);pendingCreatedTickets.forgetProject(id);projectsPendingActivation.delete(id);warmProjects.forget(id);aiConfigurationController.forgetAiConfiguration(id)}projects.value=before.filter(item=>!closing.has(item.id));ticketRowsByProject.value=Object.fromEntries(Object.entries(ticketRowsByProject.value).filter(([id])=>!closing.has(id)));ticketCursorsByProject.value=Object.fromEntries(Object.entries(ticketCursorsByProject.value).filter(([id])=>!closing.has(id)));boardPagesByProject.value=Object.fromEntries(Object.entries(boardPagesByProject.value).filter(([id])=>!closing.has(id)));ticketCountsByProject.value=Object.fromEntries(Object.entries(ticketCountsByProject.value).filter(([id])=>!closing.has(id)));ticketTrendByProject.value=Object.fromEntries(Object.entries(ticketTrendByProject.value).filter(([id])=>!closing.has(id)));projectProjectionById.value=Object.fromEntries(Object.entries(projectProjectionById.value).filter(([id])=>!closing.has(id)));customViewsByProject.value=Object.fromEntries(Object.entries(customViewsByProject.value).filter(([id])=>!closing.has(id)));terminalDrawerChatsByProject.value=Object.fromEntries(Object.entries(terminalDrawerChatsByProject.value).filter(([id])=>!closing.has(id)));commandSettingsDraftsByProject.value=Object.fromEntries(Object.entries(commandSettingsDraftsByProject.value).filter(([id])=>!closing.has(id)));commandSettingsMessagesByProject.value=Object.fromEntries(Object.entries(commandSettingsMessagesByProject.value).filter(([id])=>!closing.has(id)));commandSettingsSelectedByProject.value=Object.fromEntries(Object.entries(commandSettingsSelectedByProject.value).filter(([id])=>!closing.has(id)));commandSettingsExtraGroupsByProject.value=Object.fromEntries(Object.entries(commandSettingsExtraGroupsByProject.value).filter(([id])=>!closing.has(id)));if(statsProjectId.value&&closing.has(statsProjectId.value))statsProjectId.value=undefined;if(closing.has(selectedProjectId.value)){resetTicketComposer();const next=projects.value.find(item=>before.indexOf(item)>selectedIndex)?.id??[...projects.value].reverse().find(item=>before.indexOf(item)<selectedIndex)?.id??projects.value[0]?.id??'';if(next)activation=activateOpenProject(next);else selectedProjectId.value='';if(!selectedProjectId.value&&projectRestoreFailures.value.length)selectedProjectRestoreRoot.value=projectRestoreFailures.value[0].root}defaultProviders.value=Object.fromEntries(Object.entries(defaultProviders.value).filter(([id])=>!closing.has(id)));driveConnectionsByProject.value=Object.fromEntries(Object.entries(driveConnectionsByProject.value).filter(([id])=>!closing.has(id)));drivePendingByProject.value=Object.fromEntries(Object.entries(drivePendingByProject.value).filter(([id])=>!closing.has(id)));localStorage.setItem('hotsheet.open-projects',JSON.stringify(currentRememberedProjectRoots()));syncProjectChangeStreams();commandDialogId.value=undefined;commandSettingsEditingId.value=undefined;if(activation)void refreshActivatedProject(activation,terminalDrawerVisible.value);else if(project())void Promise.all([refreshProject(),refreshCommands(),refreshCustomViews(),refreshDriveConnections()]);else{commandDefinitions.value=[];commandRuns.value=[]}}
  function projectCloseResources(projectId: string): ProjectCloseResource[] {
    const terminals = (terminalGroups.value.find((group) => group.projectId === projectId)?.sessions ?? [])
      .filter((session) => session.alive)
      .map((session) => ({
        kind: 'terminal' as const,
        id: session.id,
        name: session.title ?? session.id,
        busy: session.busy,
        cwd: session.cwd,
        progress: session.progress,
        preview: session.scrollback,
      }));
    const connections = driveConnectionsByProject.value[projectId] ?? [],
      chats = (terminalDrawerChatsByProject.value[projectId] ?? [])
        .filter((chat) => !chat.localOnly)
        .map((chat) => {
          const connection = connections.find((item) => item.id === chat.connectionId),
            state = conversationStates.peek()[chat.connectionId] ?? EMPTY_CONVERSATION;
          return {
            kind: 'ai-chat' as const,
            id: chat.connectionId,
            name: chat.name,
            busy: connection?.busy,
            tool: aiToolLabel(chat.tool),
            model: chat.model ?? connection?.model,
            effort: chat.effort ?? connection?.effort,
            sessionId: connection?.session_id,
            messages: state.messages,
            activity: state.activity,
            progress: state.progress,
            totalUsage: conversationUsage(state),
            error: state.error ?? connection?.last_error,
          };
        });
    return [...terminals, ...chats];
  }
  function presentNextProjectClose() {
    while (pendingProjectCloseIds.length) {
      const projectId = pendingProjectCloseIds[0],
        target = projects.value.find((item) => item.id === projectId);
      if (!target) {
        pendingProjectCloseIds.shift();
        continue;
      }
      const resources = projectCloseResources(projectId);
      projectCloseDialog.value = {
        projectId,
        projectName: target.name,
        resources,
        selectedKey: resources[0] ? projectCloseResourceKey(resources[0]) : undefined,
      };
      return;
    }
    projectCloseDialog.value = undefined;
  }
  function requestProjectClose(ids: readonly string[]) {
    pendingProjectCloseIds = [...new Set(ids)].filter((id) => projects.value.some((item) => item.id === id));
    projectCloseDialog.value = undefined;
    presentNextProjectClose();
  }
  function confirmProjectClose() {
    const state = projectCloseDialog.value;
    if (!state || state.operation) return;
    projectCloseDialog.value = { ...state, operation: 'closing-project', error: '' };
    closeProjectIds([state.projectId]);
    pendingProjectCloseIds = pendingProjectCloseIds.filter((id) => id !== state.projectId);
    projectCloseDialog.value = undefined;
    queueMicrotask(presentNextProjectClose);
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function closeAllProjectResources(){const state=projectCloseDialog.value,target=state&&projects.value.find(item=>item.id===state.projectId);if(!state||!target||state.operation)return;projectCloseDialog.value={...state,operation:'closing-all',error:''};try{await Promise.all(state.resources.map(resource=>resource.kind==='terminal'?new Api(target.apiPath).deleteTerminal(resource.id):new Api(target.apiPath).deleteToolConnection(target.id,resource.id)));if(projectCloseDialog.value?.projectId!==state.projectId)return;closeProjectIds([state.projectId]);pendingProjectCloseIds=pendingProjectCloseIds.filter(id=>id!==state.projectId);projectCloseDialog.value=undefined;queueMicrotask(presentNextProjectClose)}catch(reason){if(projectCloseDialog.value?.projectId===state.projectId)projectCloseDialog.value={...state,error:reason instanceof Error?reason.message:String(reason)}}}
  function restoreBorrowedProjectCloseTerminal(state: ProjectCloseDialogState | undefined) {
    if (selectedProjectCloseResource(state?.resources ?? [], state?.selectedKey)?.kind !== 'terminal') return;
    requestAnimationFrame(() => window.dispatchEvent(new CustomEvent(TERMINAL_DRAWER_RESIZE_END_EVENT)));
  }
  function cancelProjectClose() {
    const state = projectCloseDialog.value;
    pendingProjectCloseIds = [];
    projectCloseDialog.value = undefined;
    restoreBorrowedProjectCloseTerminal(state);
  }
  async function closeDrawerTabIds(ids: readonly string[]) {
    const current = project(),
      group = current && terminalGroups.value.find((item) => item.projectId === current.id);
    if (!current || !group || ids.length === 0) return;
    const order = currentDrawerTabIds(current.id),
      requested = new Set(ids),
      terminalIds = new Set(group.sessions.map((item) => item.id)),
      chatIds = new Set((terminalDrawerChatsByProject.value[current.id] ?? []).map((item) => item.id)),
      closing = order.filter((id) => requested.has(id) && (terminalIds.has(id) || chatIds.has(id))),
      closingTerminals = closing.filter((id) => terminalIds.has(id));
    if (closing.length === 0) return;
    appTabContextMenu.value = undefined;
    if (closingTerminals.length) {
      terminalDashboardLoading.value = true;
      try {
        await Promise.all(closingTerminals.map((id) => new Api(current.apiPath).deleteTerminal(id)));
        await refreshTerminalDashboard();
      } catch (reason) {
        terminalDashboardMessage.value = reason instanceof Error ? reason.message : String(reason);
        terminalDashboardLoading.value = false;
        return;
      }
    }
    const nextSelected = drawerTabSelectionAfterClose(order, terminalDrawerSelected.value, closing),
      selectionChanges = nextSelected !== terminalDrawerSelected.value,
      before = terminalDrawerChatsByProject.value[current.id] ?? [];
    terminalDrawerChatsByProject.value = {
      ...terminalDrawerChatsByProject.value,
      [current.id]: before.filter((item) => !requested.has(item.id)),
    };
    persistDrawerTabOrder(
      current.id,
      order.filter((id) => !requested.has(id)),
    );
    if (selectionChanges) {
      selectDrawerItem(nextSelected);
      pendingTerminalFocus = undefined;
      focusDrawerTab(current.id, nextSelected);
    }
  }
  async function closeTerminalIds(ids: readonly string[]) {
    await closeDrawerTabIds(ids);
  }
  function closeDrawerAIChat(id: string) {
    void closeDrawerTabIds([id]);
  }
  function setShellMode(mode: ProjectTabBarMode) {
    if (terminalPreviewClickTimer !== undefined) {
      window.clearTimeout(terminalPreviewClickTimer);
      terminalPreviewClickTimer = undefined;
    }
    if (mode !== 'stats') statsProjectId.value = undefined;
    if (mode === 'terminals' && shellMode.value !== 'terminals') {
      terminalRailScreen.value = 'root';
      terminalRailDirection.value = 'forward';
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
    } else terminalDashboardObserver?.disconnect();
  }
  // Switch the workspace to a list/board/notifications/settings view from anywhere (a keyboard shortcut
  // may fire while a terminal grid or stats overlay is active), so return to the project shell first and
  // mirror the set-view-mode click handler's progressive-render reset + preference persistence (HS2-9SHYWD).
  function switchWorkspaceView(mode: WorkspaceViewMode) {
    setShellMode('project');
    resetProgressiveTicketRendering();
    viewMode.value = mode;
    persistWorkspacePreferences();
    if (mode === 'list' || mode === 'board') void refreshProject({ showLoading: false });
  }
  function selectTerminalRailProject(next: string) {
    if (next === selectedProjectId.value || !projects.value.some((item) => item.id === next)) return;
    const activated = activateOpenProject(next);
    if (!activated) return;
    terminalRailScreen.value = 'root';
    terminalRailDirection.value = 'backward';
    void refreshActivatedProject(activated, false);
  }
  // Switch the active project tab (shared by the desktop project tab click and the mobile project Select —
  // HS2-4C5RM7). `next` is a tab id: a project id, or a restore-failure tab id.
  function selectProjectTab(next: string) {
    exitMobileTerminalFocus();
    setShellMode('project');
    const failure = projectRestoreFailures.value.find((item) => projectRestoreTabId(item.root) === next);
    if (failure) {
      selectedProjectRestoreRoot.value = failure.root;
      saveActiveProjectRoot(localStorage, failure.root);
      return;
    }
    selectedProjectRestoreRoot.value = '';
    if (next === selectedProjectId.value) return;
    const activated = activateOpenProject(next);
    if (activated) void refreshActivatedProject(activated, terminalDrawerVisible.value);
  }
  const status = (value?: string): TicketStatus =>
    ['not_started', 'started', 'completed', 'verified', 'backlog', 'archive', 'deleted'].includes(value ?? '')
      ? (value as TicketStatus)
      : 'not_started';
  const priority = (value?: string): TicketPriority => priorityFromWire(value);
  const appRegionSize = (id: AppRegionId) =>
    id === 'app-sidebar'
      ? sidebarSize.value
      : id === 'app-inspector'
        ? inspectorSize.value
        : terminalDrawerMaximized.value
          ? terminalDrawerMax.value
          : Math.min(terminalDrawerSize.value, terminalDrawerMax.value);
  function setAppRegionSize(id: AppRegionId, size: number) {
    if (id === 'app-terminal-drawer') {
      const next = Math.min(terminalDrawerMax.value, normalizeAppRegionSize(id, size));
      if (terminalDrawerMaximized.value && next === terminalDrawerMax.value) return;
      terminalDrawerMaximized.value = false;
      terminalDrawerSize.value = saveAppRegionSize(localStorage, id, next);
      return;
    }
    const next = saveAppRegionSize(localStorage, id, size);
    if (id === 'app-sidebar') sidebarSize.value = next;
    else inspectorSize.value = next;
  }
  function syncTerminalDrawerMaximum() {
    const main = document.querySelector<HTMLElement>('.app-shell__main'),
      workArea = main?.querySelector<HTMLElement>('.app-shell__work-area');
    if (!main || !workArea) return;
    const next = terminalDrawerMaximum(main.getBoundingClientRect().bottom, workArea.getBoundingClientRect().top);
    if (next !== terminalDrawerMax.value) terminalDrawerMax.value = next;
  }
  function settleTerminalDrawerGeometry() {
    requestAnimationFrame(() => {
      const target = document.querySelector<HTMLElement>(
        '[data-terminal-drawer-measure="true"] .terminal-drawer__content',
      );
      if (target) updateTerminalDrawerBounds(target);
      window.dispatchEvent(new CustomEvent(TERMINAL_DRAWER_RESIZE_END_EVENT));
    });
  }
  function toggleTerminalDrawerMaximized() {
    terminalDrawerMaximized.value = !terminalDrawerMaximized.value;
    settleTerminalDrawerGeometry();
  }
  function currentMobileTerminalViewport() {
    return mobileTerminalViewport(window.visualViewport, window);
  }
  function enterMobileTerminalFocus(terminalId: string) {
    mobileTerminalFocus.value = transitionMobileTerminalFocus(mobileTerminalFocus.value, {
      type: 'terminal-focus',
      mobile: viewportMobile.value,
      terminalId,
      viewport: currentMobileTerminalViewport(),
    });
  }
  function syncMobileTerminalViewport() {
    mobileTerminalFocus.value = transitionMobileTerminalFocus(mobileTerminalFocus.value, {
      type: 'viewport-change',
      viewport: currentMobileTerminalViewport(),
    });
  }
  function exitMobileTerminalFocus() {
    mobileTerminalFocus.value = transitionMobileTerminalFocus(mobileTerminalFocus.value, { type: 'exit' });
  }
  function currentMobileViewportGeometry() {
    return {
      viewport: mobileTerminalViewport(window.visualViewport, window),
      keyboardVisible: mobileVirtualKeyboardVisible(window.visualViewport, window.innerHeight),
    };
  }
  function syncMobileViewportGeometry() {
    const next = currentMobileViewportGeometry(),
      current = mobileViewportGeometry.peek();
    if (
      next.keyboardVisible !== current.keyboardVisible ||
      (Object.keys(next.viewport) as (keyof typeof next.viewport)[]).some(
        (key) => next.viewport[key] !== current.viewport[key],
      )
    )
      mobileViewportGeometry.value = next;
  }
  function cycleMobileTerminalColumns() {
    mobileTerminalColumns.value = saveMobileTerminalColumns(
      localStorage,
      nextMobileTerminalColumns(mobileTerminalColumns.peek()),
    );
    window.dispatchEvent(new CustomEvent(MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT));
    // The size button no longer shows the column count inline (HS2-89JZSN); a toast reports it.
    showToast(`Terminal text size: ${mobileTerminalColumns.value} columns`);
  }
  function mobileMagnifiedTerminal() {
    if (!viewportMobile.value) return undefined;
    const geometry = mobileViewportGeometry.value;
    return { ...geometry, columns: mobileTerminalColumns.value };
  }
  function activeWorkspaceSort() {
    return workspaceSorts.value[sortableWorkspaceView(viewMode.value)];
  }
  function persistWorkspacePreferences() {
    saveWorkspacePreferences(localStorage, {
      viewMode: viewMode.value,
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
    ticketCursorsByProject.value = { ...ticketCursorsByProject.value, [id]: ticketNextCursor.value };
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function activateOpenProject(next:string):{project:Project;generation:number;cached:boolean}|undefined{const nextProject=projects.value.find(item=>item.id===next);if(!nextProject)return;projectTabRefresh.activate(next);const finishTiming=beginInteractionTiming('project-change',{project:next});persistProjectSessionNow();cacheActiveProjectProjection();resetTicketComposer(false);resetProgressiveTicketRendering();resetBoardColumnPages();const rows=ticketRowsByProject.value[next],cached=rows!==undefined,projection=projectProjectionById.value[next],stored=loadProjectWorkspaceSession(localStorage,next),generation=++projectActivationGeneration,live=new Set(rows?.map(ticket=>ticket.slug)??[]),selection=stored?.selectedTicketSlugs.filter(slug=>live.has(slug))??[],drawerActivation=terminalDrawerActivation(localStorage,next);batch(()=>{selectedProjectRestoreRoot.value='';selectedProjectId.value=drawerActivation.projectId;terminalDrawerSelected.value=drawerActivation.selectedId;tickets.value=rows??[];ticketNextCursor.value=ticketCursorsByProject.value[next];boardColumnPages.value=boardPagesByProject.value[next]??{};ticketCollectionState.value=undefined;corruptTickets.value=projection?.corruptTickets??[];repository.value=projection?.repository??null;repositoryError.value=projection?.repositoryError??'';commandDefinitions.value=projection?.commandDefinitions??[];commandRuns.value=projection?.commandRuns??[];commandSettingsEditingId.value=undefined;selectedView.value=stored?.selectedView==='errors'&&!projection?.corruptTickets.length?'all':stored?.selectedView??'all';searchOpen.value=stored?.searchOpen??false;searchQuery.value=stored?.searchQuery??'';searchMatchKeys.value=projection?.searchMatchKeys;selectedCorruptKey.value=undefined;selectedTicket.value=null;selectedTicketSlugs.value=selection;ticketSelectionAnchor=selection[0];error.value='';loading.value=!cached});markProjectWarm(next);aiConfigurationController.restoreAiConfiguration(nextProject);scheduleClaimLeaseExpiry();saveActiveProjectRoot(localStorage,nextProject.root);finishTiming();return{project:nextProject,generation,cached}}
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
      refreshProject({ showLoading: !cached, quiet }),
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
    if (restored) applyCustomViewQuery(restored);
    else if (customTicketViewKey(selectedView.value)) selectedView.value = 'all';
    if (includeTerminals) observeTerminalDrawer();
  }
  function scheduleProjectSessionPersistence() {
    if (restoringProjectSession) return;
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
      searchQuery.value = stored.searchQuery;
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
          const full = (await new Api(current.apiPath).checkoutTicket(current.id, row.id)).ticket;
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
  function setInspectorVisible(visible: boolean) {
    inspectorVisible.value = visible;
    persistWorkspacePreferences();
  }
  function setSidebarVisible(visible: boolean) {
    sidebarVisible.value = visible;
    persistWorkspacePreferences();
  }
  // Mobile single-column layout (HS2-ZK51WP). Below the desktop size floor the sidebar and inspector
  // overlay the single main column instead of taking horizontal space; only one can be open at a time
  // and both start closed. This ephemeral overlay state is tracked separately from the persisted
  // desktop sidebar/inspector preferences; the transitions live in ./mobile-layout for unit coverage.
  const viewportMobile = signal(isMobileViewport(window.innerWidth)),
    mobileOverlay = signal<MobileOverlayState>(MOBILE_OVERLAYS_CLOSED);
  window.addEventListener('resize', () => {
    const mobile = isMobileViewport(window.innerWidth);
    if (mobile !== viewportMobile.value) {
      viewportMobile.value = mobile;
      // Board columns load per status on desktop but the mobile layout lists one global page, so crossing
      // the breakpoint on the board reloads the matching row shape (HS2-HNZZHC).
      if (viewMode.value === 'board') void refreshProject({ showLoading: false, quiet: true });
      if (!mobile) {
        mobileOverlay.value = MOBILE_OVERLAYS_CLOSED;
        exitMobileTerminalFocus();
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
  const row = (ticket: WireTicketRow): TicketRowProps => ({
    slug: ticket.slug,
    title: ticket.title,
    status: status(ticket.status),
    priority: priority(ticket.priority),
    category: ticket.category ?? 'issue',
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
    agentName: ticket.worker_label || ticket.claimed_by || 'AI',
    updatedLabel: ago(ticket.updated_at),
  });

  function projectTabTicketRows(projectId: string) {
    return projectId === selectedProjectId.value ? tickets.value : (ticketRowsByProject.value[projectId] ?? []);
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function projectTicketCounts(projectId:string){const exact=ticketCountsByProject.value[projectId];if(exact)return exact;const rows=projectTabTicketRows(projectId),work=projectTabTicketState(rows),carry=ticketTrendByProject.value[projectId],completionTrend=carry?[...carry.trend]:ticketCompletionTrend(rows);return{total:rows.length,queued:rows.filter(isQueuedTicket).length,backlog:rows.filter(ticket=>ticket.status==='backlog').length,archive:rows.filter(isArchivedTicket).length,trash:rows.filter(isTrashedTicket).length,open:rows.filter(isOpenTicket).length,up_next:work.upNextCount,active:work.activeTicketCount,started:rows.filter(ticket=>ticket.status==='started').length,completed_today:carry?carry.completedToday:(completionTrend.at(-1)??0),completion_trend:completionTrend}}
  function publishOptimisticTicketRows(projectId: string) {
    ticketRowsByProject.value = { ...ticketRowsByProject.value, [projectId]: tickets.value };
    ticketCountsByProject.value = Object.fromEntries(
      Object.entries(ticketCountsByProject.value).filter(([id]) => id !== projectId),
    );
  }
  function scheduleClaimLeaseExpiry() {
    if (claimLeaseExpiryTimer !== undefined) window.clearTimeout(claimLeaseExpiryTimer);
    claimLeaseExpiryTimer = undefined;
    const now = Date.now(),
      openProjectRows = projects.value.flatMap((item) => projectTabTicketRows(item.id));
    projectTabClaimClock.value = now;
    activeTicketCount.value = projectTicketCounts(selectedProjectId.value).active;
    const next = nextActiveTicketExpiry(openProjectRows, now);
    if (next !== undefined) {
      const expiringProjects = projects.value.filter(
        (item) => nextActiveTicketExpiry(projectTabTicketRows(item.id), now) === next,
      );
      claimLeaseExpiryTimer = window.setTimeout(
        () => {
          claimLeaseExpiryTimer = undefined;
          const expiredAt = Date.now(),
            adjusted = { ...ticketCountsByProject.value };
          for (const current of expiringProjects) {
            if (!Object.hasOwn(adjusted, current.id)) continue;
            adjusted[current.id] = applyKnownActiveTicketExpiries(
              adjusted[current.id],
              projectTabTicketRows(current.id),
              now,
              expiredAt,
            );
          }
          ticketCountsByProject.value = adjusted;
          projectTabClaimClock.value = expiredAt;
          void Promise.allSettled(expiringProjects.map((current) => projectTabRefresh.request(current))).finally(
            scheduleClaimLeaseExpiry,
          );
        },
        Math.max(1, next - now + 25),
      );
    }
  }

  function visibleTickets() {
    let result: WireTicketRow[];
    if (searchQuery.value.trim() || searchTokens.value.length) {
      const matches = searchMatchKeys.value,
        matched = matches ? tickets.value.filter((ticket) => matches.has(ticketSearchKey(ticket))) : [],
        effective = effectiveSearch(searchQuery.value, searchTokens.value),
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
  function workspaceSearchActive() {
    return Boolean(searchQuery.value.trim() || searchTokens.value.length);
  }

  let searchTimer: number | undefined,
    searchGeneration = 0,
    workspaceSearchEditingToken = false;
  type EffectiveTicketSearch = ReturnType<typeof effectiveSearch>;
  const searchSignature = () => JSON.stringify([searchQuery.value.trim(), searchTokens.value]);
  const searchScopeQuery = (view: TicketView) => (customTicketViewKey(view) ? {} : ticketViewQuery(view));
  function sortedTicketQuery(query: CheckoutTicketQuery): CheckoutTicketQuery {
    const active = activeWorkspaceSort();
    return { ...query, sort: active.sort, direction: active.sortDirection };
  }
  function matchedSearchRows(rows: WireTicketRow[], effective: EffectiveTicketSearch) {
    const advanced = usesAdvancedSearchExpression(effective.text),
      matched = advanced ? filterAdvancedSearchResults(rows, effective.text, 'all', []) : rows,
      tags = effective.tokens
        .filter((token): token is Extract<InlineSearchToken, { kind: 'tag' }> => token.kind === 'tag')
        .map((token) => token.value.toLowerCase());
    return matched.filter((ticket) => tags.every((tag) => ticket.tags.some((value) => value.toLowerCase() === tag)));
  }
  function searchRequest(effective: EffectiveTicketSearch, view: TicketView): CheckoutTicketQuery {
    const advanced = usesAdvancedSearchExpression(effective.text),
      serverTokens = usesBooleanSearchExpression(effective.text) ? [] : effective.tokens;
    return sortedTicketQuery({
      ...searchScopeQuery(view),
      text: advanced ? '' : effective.text,
      ...tokenQuery(serverTokens),
    });
  }
  function activeSidebarSearchCount(state: SidebarSearchCounts) {
    const active = sidebarSearchCounts.value;
    return (
      active?.projectId === state.projectId &&
      active.signature === state.signature &&
      active.generation === state.generation
    );
  }
  function updateSidebarSearchCount(state: SidebarSearchCounts, view: TicketView, count: number) {
    const active = sidebarSearchCounts.value;
    if (!activeSidebarSearchCount(state) || !active) return;
    sidebarSearchCounts.value = {
      ...active,
      values: { ...active.values, [view]: count },
      pending: active.pending.filter((id) => id !== view),
    };
  }
  async function countSearchView(
    client: Api,
    current: Project,
    view: TicketView,
    effective: EffectiveTicketSearch,
    state: SidebarSearchCounts,
    first?: { rows: WireTicketRow[]; cursor?: string; query: CheckoutTicketQuery },
  ) {
    let count = first ? matchedSearchRows(first.rows, effective).length : 0,
      cursor = first?.cursor;
    const query = first?.query ?? searchRequest(effective, view);
    do {
      if (!first || cursor) {
        const page = await client.checkoutTicketPage(current.id, 500, cursor, query);
        count += matchedSearchRows(page.items, effective).length;
        cursor = page.next_cursor;
      } else cursor = undefined;
      first = undefined;
    } while (cursor && activeSidebarSearchCount(state));
    updateSidebarSearchCount(state, view, count);
  }
  function combinedCustomViewSearch(view: CustomView, effective: EffectiveTicketSearch) {
    const active = orderedSearchText(effective.text, effective.tokens, () => true).trim(),
      combined = active ? `(${view.query}) AND (${active})` : view.query,
      parsed = consumeSearchTokens(combined, true);
    return effectiveSearch(parsed.text, parsed.tokens);
  }
  async function refreshSidebarSearchCounts(
    current: Project,
    selected: TicketView,
    effective: EffectiveTicketSearch,
    state: SidebarSearchCounts,
    first: { rows: WireTicketRow[]; cursor?: string; query: CheckoutTicketQuery },
    refreshEveryView: boolean,
  ) {
    const client = new Api(current.apiPath);
    try {
      await countSearchView(client, current, selected, effective, state, first);
    } catch {
      updateSidebarSearchCount(state, selected, 0);
    }
    if (!refreshEveryView || !activeSidebarSearchCount(state)) return;
    const views = ticketSearchCountViews(customViewsFor(current.id).map((view) => view.id));
    await Promise.all(
      views
        .filter((view) => view !== selected)
        .map(async (view) => {
          const definition = customViewFor(view, current.id),
            viewSearch = definition ? combinedCustomViewSearch(definition, effective) : effective;
          try {
            await countSearchView(client, current, view, viewSearch, state);
          } catch {
            updateSidebarSearchCount(state, view, 0);
          }
        }),
    );
  }
  async function refreshTicketSearch() {
    resetBoardColumnPages();
    const current = project(),
      view = selectedView.value,
      effective = effectiveSearch(searchQuery.value, searchTokens.value),
      signature = searchSignature(),
      generation = ++searchGeneration;
    if (searchTimer !== undefined) {
      window.clearTimeout(searchTimer);
      searchTimer = undefined;
    }
    if (!current || (!effective.text && !effective.tokens.length)) {
      ticketPageQuery.value = {};
      searchMatchKeys.value = undefined;
      sidebarSearchCounts.value = undefined;
      if (current) void refreshProject({ showLoading: false });
      return;
    }
    const countViews = ticketSearchCountViews(customViewsFor(current.id).map((item) => item.id)),
      previous = sidebarSearchCounts.value,
      countsComplete =
        previous?.projectId === current.id &&
        previous.signature === signature &&
        previous.pending.length === 0 &&
        countViews.every((id) => Object.prototype.hasOwnProperty.call(previous.values, id)),
      countState: SidebarSearchCounts = {
        projectId: current.id,
        signature,
        generation,
        values: countsComplete ? { ...previous.values } : {},
        pending: countsComplete ? [view] : countViews,
      };
    sidebarSearchCounts.value = countState;
    try {
      const query = searchRequest(effective, view),
        client = new Api(current.apiPath),
        page = await client.checkoutTicketPage(current.id, 200, undefined, query),
        boolean = usesBooleanSearchExpression(effective.text),
        active = () =>
          generation === searchGeneration &&
          project()?.id === current.id &&
          selectedView.value === view &&
          searchSignature() === signature,
        allMatches = boolean
          ? await collectMatchingSearchPages(
              page,
              (cursor) => client.checkoutTicketPage(current.id, 500, cursor, query),
              (row) => matchedSearchRows([row], effective).length === 1,
              active,
            )
          : undefined;
      if (boolean && !allMatches) return;
      const rows = boolean ? allMatches! : page.items,
        matched = boolean ? rows : matchedSearchRows(rows, effective);
      if (!active()) return;
      ticketPageQuery.value = query;
      ticketNextCursor.value = boolean ? undefined : page.next_cursor;
      tickets.value = mergeTicketLinkRows(tickets.value, rows);
      ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: tickets.value };
      searchMatchKeys.value = new Set(matched.map(ticketSearchKey));
      void refreshSidebarSearchCounts(
        current,
        view,
        effective,
        countState,
        { rows, cursor: boolean ? undefined : page.next_cursor, query },
        !countsComplete,
      );
    } catch (reason) {
      if (generation === searchGeneration) {
        searchMatchKeys.value = new Set();
        sidebarSearchCounts.value = undefined;
        error.value = reason instanceof Error ? reason.message : String(reason);
      }
    }
  }
  function availableSearchTags() {
    return [...new Set(tickets.value.flatMap((ticket) => ticket.tags))].sort((a, b) => a.localeCompare(b));
  }
  function searchTagSuggestions() {
    const active = activeTagPrefix(searchQuery.value);
    if (active === undefined) return [];
    const prefix = active.toLowerCase();
    return availableSearchTags()
      .filter(
        (tag) =>
          !searchTokens.value.some(
            (token) => token.kind === 'tag' && token.value.toLowerCase() === tag.toLowerCase(),
          ) && tag.toLowerCase().startsWith(prefix),
      )
      .slice(0, 8);
  }
  function addWorkspaceSearchToken(token: InlineSearchToken, offset = searchQuery.value.length) {
    workspaceSearchEditingToken = false;
    if (!searchTokens.value.some((value) => value.kind === token.kind && value.value === token.value))
      searchTokens.value = [...searchTokens.value, { ...token, offset }];
    searchHelpOpen.value = false;
    scheduleTicketSearch();
  }
  function replaceActiveWorkspaceSearchToken(pattern: RegExp, token: InlineSearchToken) {
    const match = searchQuery.value.match(pattern);
    if (!match) {
      addWorkspaceSearchToken(token);
      return;
    }
    const raw = match[1],
      start = match.index! + match[0].lastIndexOf(raw);
    batch(() => {
      searchQuery.value = searchQuery.value.slice(0, start) + searchQuery.value.slice(start + raw.length);
      addWorkspaceSearchToken(token, start);
    });
  }
  function addWorkspaceSearchTag(tag: string) {
    const canonical = availableSearchTags().find((value) => value.toLowerCase() === tag.toLowerCase()) ?? tag,
      token = tokenFromRaw(`tag:${/\s/.test(canonical) ? `"${canonical}"` : canonical}`);
    if (token) replaceActiveWorkspaceSearchToken(/(?:^|\s)(tag:(?:"[^"]*|[^\s]*))$/i, token);
    focusWorkspaceSearch();
  }
  function readInlineSearchField(editor: HTMLElement, current: readonly InlineSearchToken[]) {
    const value = readTokenSearchField(editor, current.map(toTokenSearchToken));
    return { text: value.query, tokens: fromTokenSearchTokens(value.tokens, current) };
  }
  function readWorkspaceSearchEditor(editor: HTMLElement) {
    return readInlineSearchField(editor, searchTokens.value);
  }
  function removeWorkspaceSearchToken(raw: string) {
    const token = searchTokens.value.find((value) => value.raw === raw);
    if (!token) return false;
    const offset = token.offset ?? searchQuery.value.length;
    workspaceSearchEditingToken = false;
    searchTokens.value = searchTokens.value.filter((value) => value !== token);
    scheduleTicketSearch();
    focusWorkspaceSearch(offset);
    return true;
  }
  function focusWorkspaceSearch(offset?: number) {
    restoreInlineSearchCaret(document, '[data-token-search-editor="workspace-search"]', offset);
  }
  function restoreWorkspaceSearchEnd() {
    focusWorkspaceSearch();
  }
  function scheduleTicketSearch() {
    resetProgressiveTicketRendering();
    scheduleProjectSessionPersistence();
    searchGeneration += 1;
    sidebarSearchCounts.value = undefined;
    if (searchTimer !== undefined) window.clearTimeout(searchTimer);
    if (!searchQuery.value.trim() && !searchTokens.value.length) {
      searchTimer = undefined;
      searchMatchKeys.value = undefined;
      return;
    }
    searchMatchKeys.value = undefined;
    searchTimer = window.setTimeout(() => {
      searchTimer = undefined;
      void refreshTicketSearch();
    }, 150);
  }
  function updateTicketSearch(
    value: string,
    forceToken = false,
    currentTokens = searchTokens.value,
    parseTokens = true,
  ) {
    const parsed =
      !parseTokens || (workspaceSearchEditingToken && !forceToken)
        ? ({ text: value, tokens: [], removed: [] } as ReturnType<typeof consumeSearchTokens>)
        : consumeSearchTokens(value, forceToken);
    if (forceToken) workspaceSearchEditingToken = false;
    const shifted = currentTokens.map((token) => {
        const offset = token.offset ?? value.length,
          shift = parsed.removed.reduce(
            (total, range) => total + (range.end <= offset ? range.end - range.start : 0),
            0,
          );
        return { ...token, offset: Math.max(0, offset - shift) };
      }),
      next: InlineSearchToken[] = [...shifted];
    for (const token of parsed.tokens)
      if (!next.some((value) => value.kind === token.kind && value.value === token.value)) next.push(token);
    if (sameInlineSearchState(searchQuery.value, searchTokens.value, parsed.text, next))
      return parsed.tokens.length > 0;
    batch(() => {
      searchQuery.value = parsed.text;
      searchTokens.value = next;
      if (parsed.tokens.length) searchHelpOpen.value = false;
    });
    scheduleTicketSearch();
    return parsed.tokens.length > 0;
  }
  function editWorkspaceSearchToken(event: Event, target: Element) {
    event.preventDefault();
    const raw = data(target).tokenValue,
      token = searchTokens.value.find((value) => value.raw === raw);
    if (!raw || !token) return;
    workspaceSearchEditingToken = true;
    const offset = Math.max(0, Math.min(searchQuery.value.length, token.offset ?? searchQuery.value.length)),
      before = searchQuery.value.slice(0, offset),
      after = searchQuery.value.slice(offset),
      leading = before && !/[\s(]$/.test(before) ? ' ' : '',
      trailing = after && !/^[\s)]/.test(after) ? ' ' : '',
      insert = `${leading}${raw}${trailing}`;
    batch(() => {
      searchTokens.value = searchTokens.value
        .filter((value) => value.raw !== raw)
        .map((value) => ({
          ...value,
          offset:
            (value.offset ?? searchQuery.value.length) >= offset
              ? (value.offset ?? searchQuery.value.length) + insert.length
              : value.offset,
        }));
      searchQuery.value = before + insert + after;
    });
    scheduleTicketSearch();
    focusWorkspaceSearch(offset + leading.length + raw.length);
  }

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
      const inspector = document.querySelector<HTMLElement>(
        '[data-component="ticket-inspector"][data-presentation="sidebar"] [data-action="open-ticket-reader"]',
      );
      if (inspector) {
        inspector.focus({ preventScroll: true });
        return;
      }
      document.querySelector<HTMLElement>('[data-work-area-focus-owner]')?.focus({ preventScroll: true });
    }, 0);
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function openTicketLinkMatch(match:TicketLinkMatch){const target=projects.value.find(item=>item.id===match.projectId);if(!target){showToast(`No open project matches ${match.projectId}.`);return}ticketLinkChoice.value=undefined;const cached=ticketRowsByProject.value[target.id]??[],row=cached.find(item=>item.qualified_id===match.qualifiedId);if(row)ticketRowsByProject.value={...ticketRowsByProject.value,[target.id]:mergeTicketLinkRows(cached,[row])};try{const ticket=(await new Api(target.apiPath).checkoutTicket(target.id,match.ticketId)).ticket,capabilities=capabilitiesFor(ticket.connection_id);if(!capabilities)throw new Error(`Capabilities unavailable for ${ticket.connection_id}.`);const id=browserRandomId(),trigger=ticketLinkReturnFocus;ticketLinkReturnFocus=undefined;linkedReaderStack.value=pushTicketReaderFrame(linkedReaderStack.value,{id,open:false,projectId:target.id,projectName:target.name,apiPath:target.apiPath,ticket,activeTab:'info',capabilities,edit:ticketReaderEditState(ticket)});presentTicketReaderDialog(id,trigger,()=>{replaceLinkedReaderFrame(id,frame=>({...frame,open:true}))})}catch(reason){error.value=reason instanceof Error?reason.message:String(reason)}}
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
          tickets: await new Api(item.apiPath).checkoutTickets(item.id, {
            text: reference.slug,
            compact: true,
            limit: 500,
          }),
        })),
      ),
      linkProjects = results.flatMap((result) =>
        result.status === 'fulfilled'
          ? [{ id: result.value.project.id, name: result.value.project.name, tickets: result.value.tickets }]
          : [],
      );
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
      if (next.kind === 'adopt-remote' || next.kind === 'converged') {
        detailsDraft.value = next.draft;
        settledConflictKey = 'details';
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
      if (next.kind === 'adopt-remote' || next.kind === 'converged') {
        readerDetailsDraft.value = next.draft;
        settledConflictKey = 'details';
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
    if (titleEditing.value && !titleAutosave.pending()) {
      const next = reconcileActiveDraft(titleDraftBase, titleDraft.value, refreshed.title);
      titleDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged') {
        titleDraft.value = next.draft;
        settledConflictKey = 'title';
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
      if (next.kind === 'adopt-remote' || next.kind === 'converged') {
        blockedReasonDraft.value = next.draft;
        settledConflictKey = 'blocked_reason';
      }
      if (next.kind === 'conflict' && !conflict) {
        blockedReasonAutosave.cancel();
        conflict = {
          key: 'blocked_reason',
          field: 'blocked_reason',
          label: 'Blocked reason',
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
      if (next.kind === 'adopt-remote' || next.kind === 'converged') {
        readerBlockedReasonDraft.value = next.draft;
        settledConflictKey = 'blocked_reason';
      }
      if (next.kind === 'conflict' && !conflict) {
        readerBlockedReasonAutosave.cancel();
        conflict = {
          key: 'blocked_reason',
          field: 'blocked_reason',
          label: 'Blocked reason',
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
      if (next.kind === 'adopt-remote' || next.kind === 'converged') {
        noteDraft.value = next.draft;
        settledConflictKey = `note:${noteId}`;
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
        next = reconcileActiveDraft(readerNoteDraftBase, readerNoteDraft.value, remote);
      readerNoteDraftBase = next.base;
      if (next.kind === 'adopt-remote' || next.kind === 'converged') {
        readerNoteDraft.value = next.draft;
        settledConflictKey = `note:${readerNoteId}`;
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
    selectedTicket.value = refreshed;
    committedTickets.set(refreshed.slug, refreshed);
    if (conflict) showFieldConflict(conflict);
    else if (fieldConflict.value?.key === settledConflictKey) {
      fieldConflict.value = undefined;
      fieldConflictResolution.value = '';
    }
  }

  async function refreshProject({
    showLoading = true,
    quiet = false,
  }: { showLoading?: boolean; quiet?: boolean } = {}) {
    const current = project(),
      generation = ++projectRefreshGeneration;
    if (!current) return;
    if (showLoading) loading.value = true;
    const active = () => generation === projectRefreshGeneration && project()?.id === current.id;
    try {
      const client = new Api(current.apiPath, '', { trackBusy: !quiet }),
        view = selectedView.value,
        query = sortedTicketQuery(ticketViewQuery(view)),
        board = boardRefreshSpec(view, current.id, { rows: tickets.value, pages: boardColumnPages.value });
      const [index, repositoryResult] = await Promise.all([
        loadProjectTicketRefresh(client, current.id, query, board),
        client
          .repositoryStatus(current.id)
          .then((status) => ({ status, error: '' }))
          .catch((reason: unknown) => ({
            status: null,
            error: reason instanceof Error ? reason.message : String(reason),
          })),
      ]);
      if (!active()) return;
      markProjectWarm(current.id);
      const mergedTickets = index.tickets
          ? mergeRetainedCreatedRows(index.tickets, pendingCreatedTickets.retain(current.id, index.tickets))
          : undefined,
        searchActive = workspaceSearchActive();
      if (index.tickets && mergedTickets) {
        if (index.ticketCounts) {
          ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: index.ticketCounts };
          recordAuthoritativeTicketTrend(current.id, index.ticketCounts);
        }
        // When a workspace search is active the visible rows are the search results, not this base-view page,
        // so replacing tickets/selection with the base page here would deselect a matching ticket that is not
        // on the first base page — and an autosave edit's own change event would keep re-triggering that,
        // yanking focus out of the field being edited (HS2-6AXG6Z). Leave the rows and selection to the
        // trailing refreshTicketSearch, which merges results and preserves selection.
        if (!searchActive && selectedView.value === view) {
          const finalTickets: WireTicketRow[] = mergedTickets;
          // Board columns load independently and restore any column the user had paged to its loaded length
          // in the same refresh, so the commit below is one assignment with no collapse-then-expand flash
          // (HS2-8NBGBX, HS2-HNZZHC). A list refresh leaves no per-column pages behind.
          boardColumnPages.value = index.boardPages ?? {};
          tickets.value = finalTickets;
          ticketNextCursor.value = index.nextCursor;
          ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: finalTickets };
          const live = new Set(finalTickets.map((ticket) => ticket.slug)),
            selected = selectedTicketSlugs.value.filter((slug) => live.has(slug));
          if (selected.length !== selectedTicketSlugs.value.length) {
            selectedTicketSlugs.value = selected;
            if (ticketSelectionAnchor && !live.has(ticketSelectionAnchor)) ticketSelectionAnchor = undefined;
            if (selected.length !== 1) selectedTicket.value = null;
          }
        }
        scheduleClaimLeaseExpiry();
      }
      corruptTickets.value = index.corruptTickets ?? [];
      if (corruptTickets.value.length === 0 && selectedView.value === 'errors') selectedView.value = 'all';
      if (
        selectedCorruptKey.value &&
        !corruptTickets.value.some((ticket) => corruptTicketKey(ticket) === selectedCorruptKey.value)
      )
        selectedCorruptKey.value = undefined;
      repository.value = repositoryResult.status;
      repositoryError.value = repositoryResult.error;
      projectProjectionById.value = {
        ...projectProjectionById.value,
        [current.id]: {
          corruptTickets: corruptTickets.value,
          repository: repository.value,
          repositoryError: repositoryError.value,
          commandDefinitions: commandDefinitions.value,
          commandRuns: commandRuns.value,
        },
      };
      error.value = [index.ticketsError, index.corruptTicketsError].filter(Boolean).join(' · ');
      if (selectedTicket.value && mergedTickets) {
        const previous = selectedTicket.value,
          matching = mergedTickets.find((item) => item.id === previous.id);
        if (!matching && !searchActive) {
          selectedTicket.value = null;
          return;
        }
        // During an active search the selected ticket may legitimately be absent from the base-view page; keep
        // it selected and still reconcile its latest fields into the open inspector (HS2-6AXG6Z).
        if (matching || searchActive) {
          const refreshed = (await client.checkoutTicket(current.id, previous.id)).ticket;
          if (active() && selectedTicketSlugs.value.includes(refreshed.slug))
            reconcileRefreshedSelected(previous, refreshed);
        }
      }
      if (searchActive) void refreshTicketSearch();
    } catch (reason) {
      if (active()) error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (showLoading && active()) loading.value = false;
    }
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
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (project()?.id === current.id) ticketPageLoading.value = false;
    }
  }
  /**
   * The per-column board refresh for `view` in `projectId`, or undefined when that workspace is not a
   * per-column board (list mode, mobile, search, or a single-collection view). `loaded` is the board's
   * current rows and column pages: every column the board already paged asks for its loaded row count
   * back, so a refresh keeps an expanded column instead of collapsing it (HS2-HNZZHC).
   */
  function boardRefreshSpec(
    view: TicketView,
    projectId = selectedProjectId.value,
    loaded?: { rows: readonly WireTicketRow[]; pages: Record<string, BoardColumnPage> },
  ): BoardRefreshSpec | undefined {
    if (viewMode.value !== 'board' || viewportMobile.value) return undefined;
    if (!isPerColumnBoardView(view, projectId === selectedProjectId.value && workspaceSearchActive())) return undefined;
    const hideVerified = hideVerifiedByProject.value[projectId] ?? false,
      columns = ['not-started', 'started', 'completed', ...(hideVerified ? [] : ['verified'])].map((id) => ({
        id,
        statuses: boardColumnStatuses(id, hideVerified),
      })),
      wants: Record<string, number> = {};
    if (loaded)
      for (const column of columns)
        if (Object.hasOwn(loaded.pages, column.id))
          for (const status of column.statuses) wants[status] = countTicketsForStatus(loaded.rows, status);
    return { columns, wants };
  }
  function countTicketsForStatus(rows: readonly WireTicketRow[], status: string) {
    let total = 0;
    for (const ticket of rows) if ((ticket.status ?? 'not_started') === status) total += 1;
    return total;
  }
  // Load the next status-filtered page for one board column (HS2-8NBGBX). Rows are appended to the flat
  // `tickets.value` union (deduped) so selection/inspector/mutations are unaffected; only this column's
  // cursor + loaded count advance, leaving the other columns' pagination untouched.
  async function loadBoardColumnMore(columnId: string) {
    if (!boardColumnStatus(columnId)) {
      void loadNextTicketPage();
      return;
    }
    const current = project(),
      view = selectedView.value;
    if (!current || boardColumnLoading.value[columnId]) return;
    // A column pages its ordered statuses in turn (HS2-F2N4ZN): the merged Completed column exhausts
    // `completed`, then continues into `verified`, so verified rows beyond the initial global page stay
    // reachable through its own Load more. Single-status columns keep a one-entry status list.
    const statuses = boardColumnStatuses(columnId, hideVerifiedColumn());
    const target = nextBoardColumnFetch(statuses, boardColumnPages.value[columnId]);
    if (!target) return;
    boardColumnLoading.value = { ...boardColumnLoading.value, [columnId]: true };
    try {
      const query = sortedTicketQuery({ ...ticketViewQuery(view), status: target.status });
      const page = await new Api(current.apiPath).checkoutTicketPage(
        current.id,
        BOARD_COLUMN_PAGE_SIZE,
        target.cursor,
        query,
      );
      if (project()?.id !== current.id || selectedView.value !== view) return;
      const next = appendUniqueTicketRows(tickets.value, page.items);
      tickets.value = next;
      ticketRowsByProject.value = { ...ticketRowsByProject.value, [current.id]: next };
      ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: page.counts };
      recordAuthoritativeTicketTrend(current.id, page.counts);
      const loaded = statuses.reduce((sum, status) => sum + countTicketsForStatus(next, status), 0);
      boardColumnPages.value = {
        ...boardColumnPages.value,
        [columnId]: applyBoardColumnFetch(
          statuses,
          boardColumnPages.value[columnId],
          target.status,
          page.next_cursor,
          loaded,
        ),
      };
      // Do not reset the global progressive-render cap here — that would collapse the already-rendered rows
      // in the other columns. The appended rows render within the current cap and grow via continueProgressiveTicketRendering.
      scheduleClaimLeaseExpiry();
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (project()?.id === current.id) boardColumnLoading.value = { ...boardColumnLoading.value, [columnId]: false };
    }
  }
  const projectTabRefresh = createProjectTabRefreshCoordinator<Project, ProjectTicketRefresh>({
    waitUntilSafe: async () => {
      for (let open = openSelect(); open; open = openSelect())
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
      return loadProjectTicketRefresh(
        new Api(target.apiPath, '', { trackBusy: false }),
        target.id,
        sortedTicketQuery(ticketViewQuery(view)),
        boardRefreshSpec(view, target.id, rows && pages ? { rows, pages } : undefined),
      );
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
        ticketCursorsByProject.value = { ...ticketCursorsByProject.value, [target.id]: snapshot.nextCursor };
        boardPagesByProject.value = { ...boardPagesByProject.value, [target.id]: snapshot.boardPages ?? {} };
      }
      if (snapshot.ticketCounts) {
        ticketCountsByProject.value = { ...ticketCountsByProject.value, [target.id]: snapshot.ticketCounts };
        recordAuthoritativeTicketTrend(target.id, snapshot.ticketCounts);
      }
      scheduleClaimLeaseExpiry();
    },
  });
  function setCorruptRecovery(key: string, value: CorruptTicketRecoveryState) {
    corruptRecovery.value = { ...corruptRecovery.value, [key]: value };
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function revealCorruptTicket(key:string){const current=project(),ticket=corruptTickets.value.find(item=>corruptTicketKey(item)===key);if(!current||!ticket||corruptRecovery.value[key]?.pending)return;setCorruptRecovery(key,{pending:'reveal'});try{await revealCorruptTicketFile(current.id,ticket.path);setCorruptRecovery(key,{});showToast('Opened the file location.')}catch(reason){setCorruptRecovery(key,{message:reason instanceof Error?reason.message:String(reason),failed:true})}}
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function queueCorruptTicketRepair(key:string){const current=project(),ticket=corruptTickets.value.find(item=>corruptTicketKey(item)===key);if(!current||!ticket||ticket.error_code==='upgrade_required'||corruptRecovery.value[key]?.pending)return;setCorruptRecovery(key,{pending:'repair'});try{const created=await new Api(current.apiPath).createCorruptTicketRepair(current.id,ticket.path);setCorruptRecovery(key,{});showToast(`Queued ${created.slug} for AI repair.`);if(project()?.id===current.id)await refreshProject({showLoading:false})}catch(reason){setCorruptRecovery(key,{message:reason instanceof Error?reason.message:String(reason),failed:true})}}
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function refreshCommands(current=project(),quiet=false){const generation=++commandRefreshGeneration;if(!current)return;const active=()=>generation===commandRefreshGeneration&&project()?.id===current.id;try{const client=new Api(current.apiPath,'',{trackBusy:!quiet}),previous=JSON.stringify(commandDefinitions.value,null,2),[definitions,runs]=await Promise.all([client.commands(),client.commandRuns()]);if(!active())return;commandDefinitions.value=definitions;commandRuns.value=runs;const projection=projectProjectionById.value[current.id];projectProjectionById.value={...projectProjectionById.value,[current.id]:{corruptTickets:projection?.corruptTickets??corruptTickets.value,repository:projection?.repository??repository.value,repositoryError:projection?.repositoryError??repositoryError.value,commandDefinitions:definitions,commandRuns:runs}};const draft=commandSettingsDraftsByProject.value[current.id];if(viewMode.value!=='settings'||draft===undefined||draft===previous)setCommandSettingsDraft(current.id,JSON.stringify(definitions,null,2));setCommandSettingsMessage(current.id,'')}catch(reason){if(active())setCommandSettingsMessage(current.id,reason instanceof Error?reason.message:String(reason))}}
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
      if (selected) applyCustomViewQuery(selected);
      else selectTicketView('all');
    } catch {
      /* older or temporarily unavailable servers simply keep their last known shared view list */
    }
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function refreshDriveConnections(current=project(),restoreDrawerTabs=false,quiet=false){if(!current)return;if(project()?.id===current.id&&!aiConfigurationController.restoreAiConfiguration(current))void refreshAiConfiguration(current);try{const client=new Api(current.apiPath,'',{trackBusy:!quiet}),[active,sessions]=await Promise.all([client.activeToolConnections(),client.toolSessions().catch(()=>[])]),activeIds=new Set(active.map(connection=>connection.id)),connections=await recoverProjectConnections(client,active,sessions,current.id,current.root);if(projects.value.some(item=>item.id===current.id)){for(const connection of connections)if(!activeIds.has(connection.id)&&conversationStates.peek()[connection.id]?.activeAssistantId)updateConversation(connection.id,state=>applyConversationEvent(state,{type:'done',reason:'interrupted'}));driveConnectionsByProject.value={...driveConnectionsByProject.value,[current.id]:connections};if(restoreDrawerTabs)terminalDrawerChatsByProject.value={...terminalDrawerChatsByProject.value,[current.id]:restoreDrawerAIChats(connections,current.id,terminalDrawerChatsByProject.value[current.id],aiToolLabel)}}}catch{/* retain the last event-projected state while a project server reconnects */}}
  function replaceConversationStates(states: Record<string, ConversationState>) {
    conversationStates.value = states;
    try {
      saveConversationStates(localStorage, states);
    } catch {
      /* storage quota/privacy mode must not interrupt a live turn */
    }
  }
  function updateConversation(connectionId: string, update: (state: ConversationState) => ConversationState) {
    const conversations = conversationStates.peek();
    replaceConversationStates({
      ...conversations,
      [connectionId]: update(conversations[connectionId] ?? EMPTY_CONVERSATION),
    });
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function conversationForActivity(current:Project,tool:string,session?:string){const conversations=conversationStates.peek(),connections=(driveConnectionsByProject.value[current.id]??[]).filter(item=>item.tool.toLowerCase()===tool.toLowerCase()&&conversations[item.id]);return connections.find(item=>session&&(item.session_id===session||item.id===session))??(connections.length===1?connections[0]:undefined)}
  function beginConversation(connectionId: string, content: string) {
    updateConversation(connectionId, (state) => beginConversationTurn(state, browserRandomId(), content));
  }
  async function toggleSidebarDrive() {
    const current = project();
    if (!current || drivePendingByProject.value[current.id]) return;
    const selection = effectiveDriveSelection(current.id),
      tool = selection.tool,
      connectionId = sidebarDriveConnectionId(current.id, tool),
      existing = (driveConnectionsByProject.value[current.id] ?? []).find((item) => item.id === connectionId);
    if (existing?.busy) return;
    let tab = (terminalDrawerChatsByProject.value[current.id] ?? []).find((item) => item.connectionId === connectionId);
    if (!tab) tab = await createDrawerAIChat(selection, { connectionId, drive: true });
    if (!tab || project()?.id !== current.id) return;
    selectDrawerItem(tab.id);
    setTerminalDrawerVisible(true);
    const connections = driveConnectionsByProject.value[current.id] ?? [];
    drivePendingByProject.value = { ...drivePendingByProject.value, [current.id]: true };
    conversationConnectionId.value = connectionId;
    beginConversation(connectionId, SIDEBAR_DRIVE_PROMPT);
    try {
      const updated = await runProjectDrive(new Api(current.apiPath), connections, current.id, tool, {
        model: selection.model,
        effort: selection.effort,
      });
      if (project()?.id === current.id)
        driveConnectionsByProject.value = {
          ...driveConnectionsByProject.value,
          [current.id]: connections.filter((item) => item.id !== updated.id).concat(updated),
        };
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      updateConversation(connectionId, (value) => ({
        ...value,
        activeAssistantId: undefined,
        progress: undefined,
        error: message,
        messages: value.messages.map((item) =>
          item.id === value.activeAssistantId
            ? { ...item, status: 'failed', content: item.content || 'The workflow turn could not be started.' }
            : item,
        ),
      }));
      if (project()?.id === current.id) error.value = message;
    } finally {
      drivePendingByProject.value = { ...drivePendingByProject.value, [current.id]: false };
    }
  }
  async function openSidebarConversation() {
    const current = project();
    if (!current || drivePendingByProject.value[current.id]) return;
    const selection = normalizedAiSelection(),
      tool = selection.tool,
      connections = driveConnectionsByProject.value[current.id] ?? [],
      connectionId = projectChatConnectionId(current.id, tool);
    drivePendingByProject.value = { ...drivePendingByProject.value, [current.id]: true };
    try {
      const prepared = await prepareProjectConversation(new Api(current.apiPath), connections, current.id, tool, {
        connectionId,
        model: selection.model,
        effort: selection.effort,
      });
      if (project()?.id !== current.id) return;
      driveConnectionsByProject.value = {
        ...driveConnectionsByProject.value,
        [current.id]: connections.filter((item) => item.id !== prepared.id).concat(prepared),
      };
      conversationConnectionId.value = prepared.id;
      conversationOpen.value = true;
      queueMicrotask(() => {
        document.querySelector<Control>('[data-component="ai-conversation"]')?.show?.();
        syncConversationScroll(document, true);
        document.querySelector<HTMLTextAreaElement>('[name="conversation-draft"]')?.focus();
      });
    } catch (reason) {
      if (project()?.id === current.id) error.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      drivePendingByProject.value = { ...drivePendingByProject.value, [current.id]: false };
    }
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  async function sendConversationTurn(){const current=project(),connectionId=conversationConnectionId.value,draft=connectionId?conversationDrafts.value[connectionId]?.trim():'';if(!current||!connectionId||!draft)return;const connection=(driveConnectionsByProject.value[current.id]??[]).find(item=>item.id===connectionId),selection=conversationAiSelection(connectionId),turnSelection={...(selection.descriptor?.actions?.includes('change_model')&&selection.model?{model:selection.model}:{}),...(selection.descriptor?.actions?.includes('change_effort')&&selection.effort?{effort:selection.effort}:{})};if(!connection?.actions?.includes('send_turn')||connection.busy)return;beginConversation(connectionId,draft);conversationDrafts.value={...conversationDrafts.value,[connectionId]:''};const composer=document.querySelector<HTMLTextAreaElement>('[name="conversation-draft"]');if(composer)composer.value='';requestAnimationFrame(()=>{syncConversationScroll(document,true)});try{const updated=await new Api(current.apiPath).sendToolTurn(connectionId,draft,connection.session_id,turnSelection);if(project()?.id===current.id)driveConnectionsByProject.value={...driveConnectionsByProject.value,[current.id]:(driveConnectionsByProject.value[current.id]??[]).filter(item=>item.id!==updated.id).concat(updated)}}catch(reason){const message=reason instanceof Error?reason.message:String(reason);updateConversation(connectionId,state=>({...state,activeAssistantId:undefined,progress:undefined,error:message,messages:state.messages.map(item=>item.id===state.activeAssistantId?{...item,status:'failed',content:item.content||'The message could not be sent.'}:item)}))}}
  async function stopConversation() {
    const current = project(),
      connectionId = conversationConnectionId.value;
    if (!current || !connectionId) return;
    const connection = (driveConnectionsByProject.value[current.id] ?? []).find((item) => item.id === connectionId),
      tool = connection?.tool === 'claude' ? 'Claude' : 'Codex';
    if (
      !connection?.busy ||
      !connection.actions?.includes('interrupt') ||
      !window.confirm(`Stop the active ${tool} turn?`)
    )
      return;
    try {
      const updated = await new Api(current.apiPath).interruptToolTurn(connectionId);
      if (project()?.id === current.id)
        driveConnectionsByProject.value = {
          ...driveConnectionsByProject.value,
          [current.id]: (driveConnectionsByProject.value[current.id] ?? [])
            .filter((item) => item.id !== updated.id)
            .concat(updated),
        };
    } catch (reason) {
      updateConversation(connectionId, (state) => ({
        ...state,
        error: reason instanceof Error ? reason.message : String(reason),
      }));
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
  const openSelect = () => [...document.querySelectorAll<Control>('wa-select')].find((node) => node.open);
  const localTicketMutationBarrier = createRefreshBarrier();
  function beginLocalTicketMutation() {
    // A refresh may already have passed the barrier and be waiting on an older
    // ticket snapshot. Invalidate that active-project response before applying
    // optimistic rows; later refreshes still wait for the mutation to settle.
    projectRefreshGeneration += 1;
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
  function scheduleRepositoryRefresh(current: Project) {
    const existing = repositoryRefreshTimers.get(current.id);
    if (existing !== undefined) window.clearTimeout(existing);
    repositoryRefreshTimers.set(
      current.id,
      window.setTimeout(() => {
        repositoryRefreshTimers.delete(current.id);
        if (project()?.id === current.id) void refreshRepositoryStatus();
      }, 180),
    );
  }
  function syncProjectChangeStreams() {
    const live = new Set(projects.value.map((item) => item.id));
    for (const [id, stop] of projectChangeStreams)
      if (!live.has(id)) {
        stop();
        projectChangeStreams.delete(id);
      }
    for (const [id, timer] of repositoryRefreshTimers)
      if (!live.has(id)) {
        window.clearTimeout(timer);
        repositoryRefreshTimers.delete(id);
      }
    for (const current of projects.value)
      if (!projectChangeStreams.has(current.id)) {
        const stop = startProjectChangeStream({
          client: new Api(current.apiPath),
          beforeRefresh: () => localTicketMutationBarrier.wait(),
          shouldRefresh: (response) =>
            containsTicketChange({
              ...response,
              events: localTicketChangeAcknowledgements.unacknowledged(current.id, response.events),
            }),
          refresh: async () => {
            backgroundProjectRefresh = true;
            try {
              await projectTabRefresh.request(current);
            } finally {
              backgroundProjectRefresh = false;
            }
          },
          onEvents: async (response) => {
            let resolved = false;
            const acceptedTurns = new Set(turnStreamEvents(response));
            for (const event of response.events) {
              if (event.kind === 'permission_resolved') {
                const resolution = parsePermissionResolution(event.message),
                  key = `${current.id}:${event.id}`;
                if (resolution && permissionInbox.resolve(key, resolution.decision, resolution.scope)) {
                  permissionTimer.remove(key);
                  resolved = true;
                }
              }
              if (event.kind === 'turn_event' && event.turn && acceptedTurns.has(event.turn))
                updateConversation(event.turn.connection_id, (state) =>
                  applyConversationEvent(state, event.turn!.event),
                );
              if (event.kind === 'activity' && event.activity) {
                const activity = event.activity,
                  connection = conversationForActivity(current, activity.tool, activity.session);
                if (connection)
                  updateConversation(connection.id, (state) => applyConversationActivity(state, activity));
              }
            }
            if (resolved) {
              updatePermissionTimer();
              permissionRevision.value += 1;
              persistPermissionHistory();
            }
            if (
              response.events.some((event) => event.kind === 'permission_asked' || event.kind === 'permission_resolved')
            )
              await refreshPermissions();
            if (response.events.some((event) => event.kind === 'drive_updated')) await refreshDriveConnections(current);
            if (response.events.some((event) => event.kind === 'command_updated') && project()?.id === current.id)
              await refreshCommands(current);
            if (response.events.some((event) => event.kind === 'views_updated')) await refreshCustomViews(current);
            if (containsRepositoryChange(response, current.id)) scheduleRepositoryRefresh(current);
          },
        });
        projectChangeStreams.set(current.id, stop);
      }
  }
  // prettier-ignore
  const { updateSelected, history, updateSelectedTracked, detailsAutosave, readerDetailsAutosave, noteAutosave, readerNoteAutosave, blockedReasonAutosave, readerBlockedReasonAutosave, titleAutosave, tagsAutosave, linkedReaderFrame, linkedReaderAutosaves, replaceLinkedReaderFrame, linkedReaderSaves, flushLinkedReader, selectedRows, restoreTrashedTickets, executeBulkTicketAction, openEmptyTrash, emptyTrash, openBulkTicketDialog, copySelection, pasteSelection, copyDraggedTickets, isEditableEvent, ticketWorkAreaFocused, ordinaryTextSelected, timeline, notes, attachmentContext: ticketAttachmentContext, duplicateTargetFor, addAttachments, selectionOrder, presentTicket, cancelTicketDrafts, selectTickets, openTicketReader, closeNotWorking, presentNotWorkingDialog, openNotWorking, closeTicketCloseDialog, openTicketClose, setTicketCloseReason, searchTicketCloseTargets, submitTicketClose, openDuplicateTarget, addNotWorkingFiles, openTicketComposer, resetTicketComposer, addNewTicketFiles, submitNewTicket, submitNotWorking, } = createTicketWorkflows({ state: { get blockedReasonDraftBase() { return blockedReasonDraftBase; }, set blockedReasonDraftBase(value) { blockedReasonDraftBase = value; }, get bulkTicketSlugs() { return bulkTicketSlugs; }, set bulkTicketSlugs(value) { bulkTicketSlugs = value; }, get clipboard() { return clipboard; }, set clipboard(value) { clipboard = value; }, get detailsDraftBase() { return detailsDraftBase; }, set detailsDraftBase(value) { detailsDraftBase = value; }, get detailsEditGeneration() { return detailsEditGeneration; }, set detailsEditGeneration(value) { detailsEditGeneration = value; }, get noteDraftBase() { return noteDraftBase; }, set noteDraftBase(value) { noteDraftBase = value; }, get readerBlockedReasonDraftBase() { return readerBlockedReasonDraftBase; }, set readerBlockedReasonDraftBase(value) { readerBlockedReasonDraftBase = value; }, get readerDetailsDraftBase() { return readerDetailsDraftBase; }, set readerDetailsDraftBase(value) { readerDetailsDraftBase = value; }, get readerDetailsEditGeneration() { return readerDetailsEditGeneration; }, set readerDetailsEditGeneration(value) { readerDetailsEditGeneration = value; }, get readerNoteDraftBase() { return readerNoteDraftBase; }, set readerNoteDraftBase(value) { readerNoteDraftBase = value; }, get ticketSelectionAnchor() { return ticketSelectionAnchor; }, set ticketSelectionAnchor(value) { ticketSelectionAnchor = value; }, get titleDraftBase() { return titleDraftBase; }, set titleDraftBase(value) { titleDraftBase = value; }, }, CLOSED_NOT_WORKING_TARGET, projects, selectedProjectId, tickets, ticketRowsByProject, ticketCountsByProject, selectedTicket, selectedTicketSlugs, selectedCorruptKey, selectedView, ticketCollectionState, loading, error, attachmentMessage, inspectorTab, inspectorVisible, readerTab, readerOpen, linkedReaderStack, detailsMode, detailsDraft, readerDetailsMode, readerDetailsDraft, titleEditing, titleDraft, blockedReasonEditing, blockedReasonDraft, readerBlockedReasonEditing, readerBlockedReasonDraft, editingNoteId, readerEditingNoteId, fieldConflict, fieldConflictResolution, readerInlineFeedbackReplies, readerFeedbackChoiceSelections, readerFeedbackChoiceAnchors, codeReview, codeReviewLoading, codeReviewMessage, expandedCodeReviewCommits, duplicateBacklinkState, resolvedDuplicateTargets, ticketCloseDialog, ticketLinkChoice, bulkTicketDialog, notWorkingTarget, notWorkingNote, notWorkingFiles, notWorkingSubmitting, notWorkingError, composerExpanded, composerTitle, composerDetails, composerCategory, composerUpNext, composerAttachments, composerAttachmentMessage, composerAttachmentError, composerScreening, composerSubmitting, histories, mutationGenerations, committedTickets, singleTicketMutationSequencer, bulkTicketMutationSequencer, localTicketChangeAcknowledgements, pendingCreatedTickets, project, api, defaultProvider, capabilitiesFor, canUseAttachments, canStageNewTicketAttachments, ticketSnapshot, visibleTickets, projectTabTicketRows, projectTicketCounts, publishOptimisticTicketRows, beginLocalTicketMutation, beginLocalTicketCreation, refreshProject, refreshTicketCollection, refreshCodeReview, selectTicketView, scheduleClaimLeaseExpiry, scheduleProjectSessionPersistence, persistWorkspacePreferences, showToast, showFieldConflict, reconcileRefreshedSelected, openTicketLinkMatch, presentTicketReaderDialog, beginDetailsEdit, activeTicketSurface, draftScope, ago, });

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
      beginConversation(connection.id, HOTSHEET_SKILL_SIGNAL);
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
      showCommandDialog();
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
    showCommandDialog();
    if (!current || !run) return;
    try {
      const full = await new Api(current.apiPath).commandRun(run.id);
      if (project()?.id === current.id)
        commandRuns.value = commandRuns.value.map((item) => (item.id === full.id ? full : item));
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  function sidebarSurfaceProps(): SidebarSurfaceProps {
    const current = project()!;
    if (viewMode.value === 'settings') return { kind: 'settings', selected: settingsCategory() };
    if (viewMode.value === 'notifications') {
      const history = projectPermissionHistory(current.id),
        now = Date.now();
      return {
        kind: 'notifications',
        selected: notificationView.value,
        counts: {
          pending: projectPendingPermissions(current.id).length,
          day: history.filter((item) => item.resolvedAt >= now - 24 * 60 * 60 * 1000).length,
          week: history.length,
        },
      };
    }
    const counts = projectTicketCounts(current.id),
      searchCounts = sidebarSearchCounts.value,
      searchActive =
        workspaceSearchActive() &&
        searchCounts?.projectId === current.id &&
        searchCounts.signature === searchSignature(),
      searchView = (id: TicketView, fallback?: number) => ({
        count: searchActive ? searchCounts.values[id] : fallback,
        countLoading: searchActive && searchCounts.pending.includes(id),
        searchCount: searchActive && !searchCounts.pending.includes(id),
      }),
      repo = repository.value,
      completionTrend = counts.completion_trend ?? ticketCompletionTrend(tickets.value),
      views = [
        { id: 'all', label: 'Queue', ...searchView('all', counts.queued), icon: 'all' as const },
        { id: 'backlog', label: 'Backlog', ...searchView('backlog', counts.backlog), icon: 'backlog' as const },
        { id: 'archive', label: 'Archive', ...searchView('archive', counts.archive), icon: 'archive' as const },
        ...((counts.trash ?? 0) > 0 || selectedView.value === 'trash'
          ? [{ id: 'trash' as const, label: 'Trash', count: counts.trash ?? 0, icon: 'trash' as const }]
          : []),
        ...customViewsFor(current.id).map((view) => {
          const id = customTicketViewId(view.id);
          return { id, label: view.name, ...searchView(id), icon: 'custom' as const, manageable: true };
        }),
        ...(corruptTickets.value.length
          ? [
              {
                id: 'errors',
                label: 'Ticket errors',
                count: corruptTickets.value.length,
                attention: true,
                icon: 'errors' as const,
              },
            ]
          : []),
      ],
      selection = effectiveDriveSelection(current.id),
      tool = selection.tool,
      drive = projectDriveControlState(
        driveConnectionsByProject.value[current.id] ?? [],
        current.id,
        drivePendingByProject.value[current.id] ?? false,
        tool,
      );
    return {
      kind: 'project',
      sidebar: {
        completedToday: counts.completed_today,
        inProgress: counts.started,
        completionTrend,
        branch: repo?.branch ?? 'No git branch',
        unpushed: repo?.ahead ?? 0,
        behind: repo?.behind ?? 0,
        uncommitted: (repo?.staged ?? 0) + (repo?.unstaged ?? 0) + (repo?.untracked ?? 0),
        conflicted: repo?.conflicted ?? 0,
        repositoryError: Boolean(repositoryError.value),
        views,
        selectedViewId: selectedView.value,
        commandGroupLabel: 'Project commands',
        commands: commandDefinitions.value.map((command) => {
          const latest = commandRunFor(command.id);
          return {
            id: command.id,
            label: command.title,
            color: command.color ?? 'transparent',
            icon: commandIcon(command),
            kind: command.kind,
            group: command.group,
            running: latest?.state === 'running',
            lastRun: latest
              ? `${latest.state}${latest.exit_code === undefined ? '' : ` (exit ${latest.exit_code})`}`
              : undefined,
          };
        }),
        commandGroupExpanded: commandGroupExpanded.value,
        collapsedCommandGroups: commandGroupsCollapsed.value[current.id],
        driveRunning: drive.running,
        driveTool: tool,
        driveToolLabel: aiToolLabel(tool),
        driveDisabled: drive.disabled,
        driveDisabledReason: drive.disabledReason,
        driveOptionsOpen: driveOptionsOpen.value,
        driveTools: aiTools.value,
        driveToolsLoading: aiSettingsLoading.value,
        driveToolsError: aiSettingsMessage.value,
        driveSelection: driveOverridesByProject.value[current.id] ?? {},
        driveDefaultSelection: aiDefaults.value,
        conversationOpen:
          conversationOpen.value &&
          conversationConnectionId.value === projectChatConnectionId(current.id, aiDefaults.value.tool),
        conversationDisabled: drivePendingByProject.value[current.id],
        openCount: counts.open,
        upNextCount: counts.up_next,
        activeCount: counts.active,
        collapseControl: true,
      },
    };
  }
  function notWorkingSurfaceProps(
    target: NotWorkingTarget,
    note: string,
    files: PendingEvidence[],
    submitting: boolean,
    submissionError: string,
  ): Parameters<typeof NotWorkingSurface>[0] {
    const capabilities = target.slug ? capabilitiesFor(target.connectionId) : undefined;
    return {
      slug: target.slug || 'ticket',
      mode: target.mode,
      open: Boolean(target.slug),
      note,
      attachments: files,
      notesEnabled: capabilities?.notes ?? true,
      attachmentsEnabled: capabilities?.attachments ?? true,
      submitting,
      error: submissionError,
    };
  }
  function attachmentItems(ticket: FullTicket, current: Project) {
    const rounds = attachmentRoundNumbers(ticket.attachments, ticket.notes);
    return ticket.attachments.map((item) => ({
      id: item.id,
      name: item.filename,
      url: api().checkoutAttachmentUrl(current.id, ticket.id, item.id),
      thumbnailUrl: api().checkoutAttachmentThumbnailUrl(current.id, ticket.id, item.id),
      manageVideoPoster: true,
      annotationCount: item.annotations?.length ?? 0,
      round: rounds.get(item.id),
      batch_id: item.batch_id,
      batch_label: item.batch_label,
      actor: item.actor,
      purpose: item.purpose,
    }));
  }
  function selectedInspectorProps(slugPlacement?: 'leading' | 'center'): TicketInspectorProps | undefined {
    const ticket = selectedTicket.value,
      current = project();
    if (!ticket || !current) return;
    const backlinks =
      duplicateBacklinkState.value.key === `${current.id}:${ticket.qualified_id}`
        ? duplicateBacklinkState.value
        : undefined;
    return {
      slugPlacement,
      slug: ticket.slug,
      title: ticket.title,
      titleEditing: titleEditing.value,
      titleDraft: titleDraft.value,
      canUpdate: canUpdateSelected(),
      canAddNotes: canAddNotes(),
      canEditNotes: canEditNotes(),
      canDeleteNotes: canDeleteNotes(),
      composingNote: composingNote.value,
      composerDraft: newNoteDraft.value,
      status: status(ticket.status),
      priority: priority(ticket.priority),
      category: ticket.category ?? 'issue',
      tags: ticket.tags,
      tagSuggestions: tagSuggestions(),
      details: detailsMode.value === 'write' ? detailsDraft.value : ticket.details,
      detailsMode: detailsMode.value,
      detailsDirty: detailsDraft.value !== ticket.details,
      notes: notes(ticket),
      editingNoteId: editingNoteId.value,
      noteDraft: noteDraft.value,
      blockedReason: ticket.blocked_reason,
      blockedReasonEditing: blockedReasonEditing.value,
      blockedReasonDraft: blockedReasonDraft.value,
      providerName: 'Hot Sheet git',
      updatedLabel: `Updated ${ago(ticket.updated_at)}`,
      activeTab: inspectorTab.value,
      upNext: ticket.up_next,
      upNextEligible: ticket.status === 'not_started' || ticket.status === 'started',
      feedbackNeeded: fullTicketFeedbackNeeded(ticket),
      closeReason: ticket.close_reason,
      duplicateTarget: duplicateTargetFor(ticket),
      duplicateBacklinks: backlinks?.backlinks,
      duplicateBacklinkInaccessibleProjects: backlinks?.inaccessibleProjects,
      timelineEntries: timeline(ticket),
      attachments: attachmentItems(ticket, current),
      attachmentContext: attachmentContext(ticket),
      codeReview: codeReview.value,
      codeReviewLoading: codeReviewLoading.value,
      codeReviewMessage: codeReviewMessage.value,
      expandedCodeReviewCommits: expandedCodeReviewCommits.value,
      attachmentsEnabled: canUseAttachments(),
      attachmentMessage: attachmentMessage.value,
      fieldConflict: readerOpen.value ? undefined : fieldConflict.value,
      fieldConflictResolution: fieldConflictResolution.value,
    };
  }
  function readerLayerSurface({
    ticket,
    current,
    active,
    open,
    editable,
    frame,
    activeTab,
    stackPosition,
    stackSize,
  }: {
    ticket: FullTicket;
    current: Project;
    active: boolean;
    open: boolean;
    editable: boolean;
    frame?: TicketReaderFrame;
    activeTab: InspectorTab;
    stackPosition: number;
    stackSize: number;
  }) {
    const linked = frame?.edit,
      capabilities = frame ? { ...frame.capabilities, notes: false, note_delete: false } : undefined,
      canUpdate = editable && canUpdateSelected();
    const readerScope = frame?.id ?? 'workspace-reader';
    return (
      <ReaderLayerSurface
        slug={ticket.slug}
        title={ticket.title}
        titleEditing={editable && titleEditing.value}
        titleDraft={editable ? titleDraft.value : ticket.title}
        canUpdate={canUpdate}
        canEditText={frame ? capabilities!.update : canUpdate}
        canAddNotes={frame ? capabilities!.notes : editable && canAddNotes()}
        canEditNotes={frame ? capabilities!.note_edit : editable && canEditNotes()}
        canDeleteNotes={frame ? capabilities!.note_delete : editable && canDeleteNotes()}
        composingNote={editable && composingNote.value}
        composerDraft={editable ? newNoteDraft.value : ''}
        status={status(ticket.status)}
        priority={priority(ticket.priority)}
        category={ticket.category ?? 'issue'}
        tags={ticket.tags}
        tagSuggestions={editable ? tagSuggestions() : undefined}
        details={
          linked?.detailsMode === 'write'
            ? linked.detailsDraft
            : editable && readerDetailsMode.value === 'write'
              ? readerDetailsDraft.value
              : ticket.details
        }
        detailsMode={linked?.detailsMode ?? (editable ? readerDetailsMode.value : 'preview')}
        detailsDirty={
          Boolean(linked && linked.detailsDraft !== ticket.details) ||
          (editable && readerDetailsDraft.value !== ticket.details)
        }
        notes={notes(ticket)}
        editingNoteId={linked?.editingNoteId ?? (editable ? readerEditingNoteId.value : undefined)}
        noteDraft={linked?.noteDraft ?? (editable ? readerNoteDraft.value : '')}
        inlineFeedbackReplies={editable ? readerInlineFeedbackReplies.value : {}}
        feedbackChoiceSelections={editable ? readerFeedbackChoiceSelections.value : {}}
        blockedReason={ticket.blocked_reason}
        blockedReasonEditing={linked?.blockedReasonEditing ?? (editable && readerBlockedReasonEditing.value)}
        blockedReasonDraft={
          linked?.blockedReasonDraft ?? (editable ? readerBlockedReasonDraft.value : (ticket.blocked_reason ?? ''))
        }
        providerName="Hot Sheet git"
        updatedLabel={`Updated ${ago(ticket.updated_at)}`}
        activeTab={activeTab}
        upNext={ticket.up_next}
        upNextEligible={editable && (ticket.status === 'not_started' || ticket.status === 'started')}
        feedbackNeeded={fullTicketFeedbackNeeded(ticket)}
        closeReason={ticket.close_reason}
        duplicateTarget={duplicateTargetFor(ticket)}
        timelineEntries={timeline(ticket)}
        attachments={attachmentItems(ticket, current)}
        attachmentContext={attachmentContext(ticket, current)}
        codeReview={editable ? codeReview.value : undefined}
        codeReviewLoading={editable && codeReviewLoading.value}
        codeReviewMessage={editable ? codeReviewMessage.value : ''}
        expandedCodeReviewCommits={editable ? expandedCodeReviewCommits.value : []}
        attachmentsEnabled={editable && canUseAttachments()}
        attachmentMessage={editable ? attachmentMessage.value : ''}
        largeText={readerLargeText.value}
        fieldConflict={editable ? fieldConflict.value : undefined}
        fieldConflictResolution={editable ? fieldConflictResolution.value : ''}
        active={active}
        frameId={readerScope}
        open={open}
        projectName={current.name}
        stackPosition={stackPosition}
        stackSize={stackSize}
        style={stackPosition < stackSize ? `--ticket-reader-offset:${(stackSize - stackPosition) * -10}px` : undefined}
        attachmentMenu={attachmentMenuSurfaceProps({ readerScope })}
        evidence={changeEvidenceSurfaceProps(readerScope)}
        readOnly={frame ? !capabilities!.update && !capabilities!.note_edit : !editable}
      />
    );
  }
  function readerLayersSurfaceProps(): Parameters<typeof ReaderLayersSurface>[0] {
    const current = project(),
      base =
        selectedTicket.value && current
          ? {
              id: 'workspace-reader',
              open: readerOpen.value,
              project: current,
              ticket: selectedTicket.value,
              activeTab: readerTab.value,
              frame: undefined,
            }
          : undefined,
      linked = linkedReaderStack.value.flatMap((frame) => {
        const project = projects.value.find((item) => item.id === frame.projectId);
        return project
          ? [{ id: frame.id, open: frame.open, project, ticket: frame.ticket, activeTab: frame.activeTab, frame }]
          : [];
      }),
      openLayers = [...(base?.open ? [base] : []), ...linked.filter((layer) => layer.open)],
      top = openLayers.at(-1)?.id,
      position = new Map(openLayers.map((layer, index) => [layer.id, index + 1]));
    return {
      layers: [...(base ? [base] : []), ...linked].map((layer) => {
        const stackPosition = position.get(layer.id) ?? 1,
          active = layer.id === top;
        return readerLayerSurface({
          ticket: layer.ticket,
          current: layer.project,
          active,
          open: layer.open,
          editable: active && layer.id === 'workspace-reader',
          frame: layer.frame,
          activeTab: layer.activeTab,
          stackPosition,
          stackSize: Math.max(1, openLayers.length),
        });
      }),
    };
  }
  function settingsWorkspace(current: Project) {
    return (
      <SettingsWorkspace
        category={settingsCategory()}
        sources={{
          stores: current.stores,
          providerConnections: providerConnections.value,
          error: providerSettingsError.value,
          setupOpen: Boolean(ticketSourceSetupProject.value),
        }}
        ai={{
          tools: aiTools.value,
          selection: aiDefaults.value,
          loading: aiSettingsLoading.value,
          message: aiSettingsMessage.value,
        }}
        commands={{
          commands: commandSettingsDefinitions(current.id),
          extraGroups: commandSettingsExtraGroups(current.id),
          editingId: commandSettingsEditingId.value,
          selectedIds: commandSelection(current.id),
          iconSearch: commandIconSearch.value,
          message: commandSettingsMessage(current.id),
          aiTools: aiTools.value,
          aiDefaults: aiDefaults.value,
        }}
        lifecycle={{
          days: trashCleanupDaysByProject.value[current.id] ?? 30,
          message: trashSettingsMessagesByProject.value[current.id] ?? '',
        }}
        terminals={{
          inheritGlobalShellHistory: inheritGlobalShellHistory.value,
          message: terminalSettingsMessage.value,
        }}
        permissions={{ automation: permissionAutomation(current.id), delays: PERMISSION_DELAYS }}
        columns={{ hideVerified: hideVerifiedColumn() }}
        general={{ showLoadingActivity: showLoadingActivity.value }}
        keyboard={{
          overrides: keyboardShortcutOverrides.value,
          capturingId: capturingShortcutId.value,
          apple: appleShortcutPlatform,
        }}
      />
    );
  }
  function workspaceEmptyState(): TicketEmptyStateProps | undefined {
    if (loading.value) return { kind: 'loading' };
    const query = orderedSearchText(searchQuery.value, searchTokens.value, () => true);
    if (query) return { kind: searchMatchKeys.value === undefined ? 'searching' : 'search', query };
    const collection = ticketCollectionState.value;
    if (collection?.projectId === selectedProjectId.value && collection.view === selectedView.value)
      return {
        kind: collection.status === 'loading' ? 'view-loading' : 'view-error',
        viewLabel: ticketViewTitle(selectedView.value),
      };
    if (tickets.value.length === 0 && corruptTickets.value.length === 0) return { kind: 'project' };
    return { kind: 'view', viewLabel: ticketViewTitle(selectedView.value) };
  }
  function workspaceSurfaceProps(): WorkspaceSurfaceProps {
    const shown = visibleTickets(),
      current = project(),
      emptyState = workspaceEmptyState(),
      collectionLoading = emptyState?.kind === 'view-loading',
      hasMore = Boolean(ticketNextCursor.value) && !collectionLoading,
      more = hasMore ? (
        <button
          type="button"
          class="ticket-page-more"
          data-action="load-next-ticket-page"
          disabled={ticketPageLoading.value}
        >
          {ticketPageLoading.value ? 'Loading…' : 'Load more tickets'}
        </button>
      ) : undefined;
    if (viewMode.value === 'notifications') {
      const history = projectPermissionHistory(),
        cutoff = Date.now() - 24 * 60 * 60 * 1000,
        view = notificationView.value;
      return {
        kind: 'notifications',
        notifications: {
          title: notificationViewTitle(view),
          pending: view === 'pending' ? projectPendingPermissions() : [],
          history:
            view === 'day' ? history.filter((item) => item.resolvedAt >= cutoff) : view === 'week' ? history : [],
        },
      };
    }
    if (viewMode.value === 'settings' && current) return { kind: 'settings', content: settingsWorkspace(current) };
    if (selectedView.value === 'errors')
      return {
        kind: 'errors',
        list: {
          tickets: [],
          corruptTickets: corruptTickets.value,
          corruptRecovery: corruptRecovery.value,
          selectedCorruptKey: selectedCorruptKey.value,
          label: 'Tickets with parsing errors',
          emptyState,
        },
      };
    continueProgressiveTicketRendering(shown.length);
    if (viewMode.value === 'board' && !viewportMobile.value) {
      const counts =
          workspaceSearchActive() || collectionLoading
            ? undefined
            : ticketCountsByProject.value[selectedProjectId.value],
        hideVerified = hideVerifiedColumn(),
        groups = ticketBoardGroups(shown, selectedView.value, hideVerified);
      // Per-column pagination (HS2-8NBGBX): each status column gets its own continuation from its own
      // cursor/loaded count, so a short column can load more independently of a long one. Single-column
      // views (backlog/archive/trash) and search keep the global cursor pinned to their one column.
      const perColumn = isPerColumnBoardView(selectedView.value, workspaceSearchActive()),
        lastShown = shown.at(-1),
        globalColumnId = perColumn
          ? undefined
          : (groups.find((group) => group.tickets.some((ticket) => ticket.id === lastShown?.id))?.id ??
            [...groups].reverse().find((group) => group.tickets.length)?.id ??
            groups[0]?.id);
      const columns = groups.map((group) => {
        const total = ticketBoardGroupTotal(group.id, group.tickets.length, selectedView.value, counts, hideVerified),
          status = boardColumnStatus(group.id);
        const continuation =
          perColumn && status
            ? counts && boardColumnHasMore(total, group.tickets.length, boardColumnPages.value[group.id])
              ? { loading: boardColumnLoading.value[group.id] }
              : undefined
            : !collectionLoading && hasMore && globalColumnId === group.id
              ? { loading: ticketPageLoading.value }
              : undefined;
        return {
          ...group,
          totalCount: total,
          tickets: group.tickets.slice(0, renderedTicketLimit.value).map(row),
          continuation,
        };
      });
      return { kind: 'board', board: { columns, label: 'Project board', emptyState } };
    }
    return {
      kind: 'list',
      list: {
        tickets: shown.slice(0, renderedTicketLimit.value).map(row),
        totalCount: shown.length,
        label: 'Project tickets',
        emptyState,
      },
      more,
    };
  }
  function terminalRailSurfaceProps(): TerminalRailSurfaceProps {
    const current = project(),
      mode = viewMode.value === 'notifications' ? 'notifications' : 'list',
      railView = selectedView.value === 'errors' ? 'all' : selectedView.value,
      selection = selectedRows(),
      canCreate = canCreateTicketInView(railView),
      effective = effectiveSearch(searchQuery.value, searchTokens.value),
      matches = searchMatchKeys.value;
    let shown = ticketsForView(tickets.value, railView);
    if (effective.text || effective.tokens.length)
      shown = matches ? shown.filter((ticket) => matches.has(ticketSearchKey(ticket))) : [];
    const activeSort = activeWorkspaceSort();
    shown = shown.slice().sort((a, b) => compareWorkspaceTickets(a, b, activeSort.sort, activeSort.sortDirection));
    const pending = current ? pendingPermissions().filter((item) => item.projectId === current.id) : [],
      history = current ? permissionHistory().filter((item) => item.projectId === current.id) : [];
    const content =
      mode === 'notifications' ? (
        <NotificationCenter title="Notifications" pending={pending} history={history} />
      ) : (
        <TicketList tickets={shown.map(row)} label="Project tickets" />
      );
    const ready =
        terminalRailScreen.value === 'ticket' &&
        selectedTicketSlugs.value.length === 1 &&
        selectedTicket.value?.slug === selectedTicketSlugs.value[0],
      railInspectorProps = ready ? selectedInspectorProps('center') : undefined;
    const railTransitioning =
      terminalRailScreen.value === 'ticket' &&
      selectedTicketSlugs.value.length === 1 &&
      Boolean(selectedTicket.value) &&
      !ready;
    const inspector = (
      <div class="terminal-ticket-rail__inspector">
        <button
          type="button"
          class="terminal-ticket-rail__back"
          data-action="back-terminal-ticket-rail"
          aria-label="Back to ticket list"
          title="Back to ticket list"
        >
          <LucideIcon icon={ChevronLeft} name="chevron-left" />
        </button>
        {railInspectorProps ? (
          <Inspector {...railInspectorProps} />
        ) : railTransitioning ? (
          <TicketInspectorSkeleton slug={selectedTicketSlugs.value[0]} />
        ) : (
          <InspectorPlaceholder selectionCount={selectedTicketSlugs.value.length} />
        )}
      </div>
    );
    return {
      rail: {
        projects: projects.value.map((item) => ({ id: item.id, name: item.name })),
        selectedProjectId: current?.id ?? '',
        views: [
          { id: 'all', label: 'Queue' },
          { id: 'backlog', label: 'Backlog' },
          { id: 'archive', label: 'Archive' },
          ...((current && (projectTicketCounts(current.id).trash ?? 0) > 0) || railView === 'trash'
            ? [{ id: 'trash', label: 'Trash' }]
            : []),
          ...customViewsFor(current?.id).map((view) => ({ id: customTicketViewId(view.id), label: view.name })),
        ],
        selectedViewId: railView,
        controls: (
          <WorkspaceControls
            mode={mode}
            presentation="rail"
            searchOpen={searchOpen.value}
            searchQuery={searchQuery.value}
            searchTokens={searchTokens.value}
            searchTagSuggestions={searchTagSuggestions()}
            searchDatePrefix={activeDatePrefix(searchQuery.value)}
            searchHelpOpen={searchHelpOpen.value}
            sort={sort.value}
            sortDirection={sortDirection.value}
            notificationCount={pending.length}
            selectedTicketCount={selection.length}
            selectedTicketsUpNext={workspaceUpNextState(selection.map((ticket) => ticket.up_next))}
            selectedTicketsUpNextEligible={
              selection.length > 0 &&
              selection.every((ticket) => ticket.status === 'not_started' || ticket.status === 'started')
            }
            selectedTicketsMutable={canBulkUpdate(selection, capabilitiesFor)}
          />
        ),
        content,
        inspector,
        active: ready ? 'ticket' : 'root',
        direction: terminalRailDirection.value,
        title: mode === 'notifications' ? 'Notifications' : ticketViewTitle(railView),
        action: mode === 'list' ? ticketViewAction(railView, canCreate, 'Ticket…') : undefined,
      },
    };
  }
  const { workspaceTerminalGroups, globalWorkspaceSurfaceProps, projectTerminalDrawerProps, aiConversationSurface } =
    createTerminalPresentation({
      projects,
      project,
      shellMode,
      statsProjectId,
      canGiveFeedback: () => Boolean(selectedTicket.value && canAddNotes()),
      terminals: {
        terminalGroups,
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
        terminalDrawerCreateMenuOpen,
        mobileTerminalFocus,
        mobileMagnifiedTerminal,
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
  function ticketViewTitle(view: TicketView) {
    return view === 'archive'
      ? 'Archive'
      : view === 'trash'
        ? 'Trash'
        : view === 'backlog'
          ? 'Backlog'
          : view === 'errors'
            ? 'Ticket errors'
            : (customViewFor(view)?.name ?? 'Queue');
  }
  function ticketViewAction(view: TicketView, canCreate: boolean, label?: string) {
    return view === 'trash' ? (
      <wa-button
        class="workspace-header__text-action"
        appearance="outlined"
        variant="danger"
        data-action="open-empty-trash"
      >
        <span class="workspace-header__text-action-label">
          <LucideIcon icon={Trash2} name="trash-2" />
          <span>Empty Trash</span>
        </span>
      </wa-button>
    ) : canCreate ? (
      <QuickTicketLauncher attachmentsEnabled={canStageNewTicketAttachments()} label={label} />
    ) : undefined;
  }
  function ticketContextMenuSurface() {
    const menu = ticketContextMenu.value;
    let props: TicketContextMenuSurfaceProps['menu'];
    if (menu) {
      const ticket = tickets.value.find((item) => item.slug === menu.ticketSlug),
        slugs = selectedTicketSlugs.value.length ? selectedTicketSlugs.value : [menu.ticketSlug],
        selected = tickets.value.filter((item) => slugs.includes(item.slug)),
        onlyCompleted = slugs.length === 1 && ticket?.status === 'completed',
        reopenable = slugs.length === 1 && (ticket?.status === 'verified' || ticket?.status === 'archive'),
        allCompleted =
          slugs.length > 0 &&
          slugs.every((slug) => tickets.value.find((item) => item.slug === slug)?.status === 'completed'),
        capabilities = ticket ? capabilitiesFor(ticket.connection_id) : undefined;
      props = {
        x: menu.x,
        y: menu.y,
        category: ticket?.category,
        priority: priority(ticket?.priority),
        status: status(ticket?.status),
        upNextEligible: slugs.every((slug) => {
          const value = tickets.value.find((item) => item.slug === slug)?.status;
          return value === 'not_started' || value === 'started';
        }),
        hideUpNext: menu.hideUpNext,
        verifyAction: allCompleted && (capabilities?.update ?? true),
        notWorkingAction: onlyCompleted && (capabilities?.not_working_report ?? false),
        reopenAction: reopenable && (capabilities?.not_working_report ?? false),
        closeAction: slugs.length === 1 && Boolean(capabilities?.close && capabilities.close_reasons),
        selectionCount: slugs.length,
        canBulkUpdate: canBulkUpdate(selected, capabilitiesFor),
        allInBacklog: selected.length > 0 && selected.every((item) => item.status === 'backlog'),
        allInArchive: selected.length > 0 && selected.every(isArchivedTicket),
        allInTrash: selected.length > 0 && selected.every(isTrashedTicket),
      };
    }
    return <TicketContextMenuSurface menu={props} closeDialog={ticketCloseDialog.value} />;
  }

  function terminalOperationsSurfaceProps(): TerminalOperationsSurfaceProps {
    return {
      projects: projects.value.map((item) => {
        const rows = item.id === selectedProjectId.value ? tickets.value : (ticketRowsByProject.value[item.id] ?? []),
          counts = projectTicketCounts(item.id),
          trend = counts.completion_trend ?? ticketCompletionTrend(rows);
        return {
          id: item.id,
          name: item.name,
          completedToday: counts.completed_today,
          inProgress: counts.started,
          trend,
        };
      }),
    };
  }

  function HotSheetShell() {
    if (initialProjectRestorePending.value) return <ProjectRestoreState />;
    const current = project(),
      restoreFailure = projectRestoreFailures.value.find((item) => item.root === selectedProjectRestoreRoot.value);
    if (!current && !restoreFailure) return <AppEmptyState />;
    const popup = conversationOpen.value ? undefined : permissionPopupSurface(),
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
          upNextCount: counts.up_next,
          activeTicketCount: counts.active,
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
          mobile={viewportMobile.value}
          sidebar={
            shellMode.value === 'terminals' ? (
              <TerminalOperationsSurface {...terminalOperationsSurfaceProps()} />
            ) : (
              <></>
            )
          }
          sidebarVisible={viewportMobile.value ? mobileOverlay.value.sidebar : sidebarVisible.value}
          sidebarSize={sidebarSize.value}
          header={<WorkspaceIdentity projectName={shellMode.value === 'terminals' ? 'Workspace grid' : 'Stats'} />}
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
            shellMode.value === 'terminals' ? <TerminalRailSurface {...terminalRailSurfaceProps()} /> : undefined
          }
          inspectorVisible={viewportMobile.value ? mobileOverlay.value.inspector : inspectorVisible.value}
          inspectorSize={inspectorSize.value}
          sidePanelSeparator={magnifiedTerminalKey.value ? 'hidden' : 'auto'}
          overlay={popup}
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
          overlay={popup}
        />
      );
    if (!current) return <AppEmptyState />;
    const selection = selectedRows(),
      selectedSlug = selectedTicketSlugs.value.length === 1 ? selectedTicketSlugs.value[0] : undefined,
      selectedReady = Boolean(selectedSlug && selectedTicket.value?.slug === selectedSlug),
      selectedTransitioning = Boolean(selectedSlug && selectedTicket.value && !selectedReady),
      inspectorProps = selectedReady ? selectedInspectorProps() : undefined,
      corruptKey = selectedCorruptKey.value,
      corruptTicket = corruptKey
        ? corruptTickets.value.find((item) => corruptTicketKey(item) === corruptKey)
        : undefined,
      canCreate = !['settings', 'notifications'].includes(viewMode.value) && canCreateTicketInView(selectedView.value),
      railView = selectedView.value === 'errors' ? 'all' : selectedView.value,
      mobileViewChoices = [
        { value: 'all', label: 'Queue' },
        { value: 'backlog', label: 'Backlog' },
        { value: 'archive', label: 'Archive' },
        ...((projectTicketCounts(current.id).trash ?? 0) > 0 || railView === 'trash'
          ? [{ value: 'trash', label: 'Trash' }]
          : []),
        ...customViewsFor(current.id).map((view) => ({ value: customTicketViewId(view.id), label: view.name })),
      ];
    const workspaceTitle =
      viewMode.value === 'notifications'
        ? notificationViewTitle(notificationView.value)
        : viewMode.value === 'settings'
          ? settingsCategoryTitle(settingsCategory())
          : customTicketViewKey(selectedView.value)
            ? ticketViewTitle(selectedView.value)
            : searchQuery.value.trim() || searchTokens.value.length
              ? 'Search results'
              : ticketViewTitle(selectedView.value);
    const secondaryPageHeader = (
      <div class="app-heading" data-component="heading" data-has-icon="false">
        <Toolbar
          dividerSides=""
          leading={<ToolbarText text={workspaceTitle} id="workspace-page-title" size="xlarge" headingLevel={1} />}
          trailing={
            !['settings', 'notifications'].includes(viewMode.value)
              ? ticketViewAction(selectedView.value, canCreate)
              : undefined
          }
        />
      </div>
    );
    const pageHeader = viewportMobile.value ? (
      !['settings', 'notifications'].includes(viewMode.value) ? (
        <Toolbar
          className="app-shell__mobile-view-header"
          dividerSides=""
          leading={
            <Select
              className="app-shell__mobile-view"
              name="mobile-view"
              value={railView}
              ariaLabel="Ticket view"
              choices={mobileViewChoices}
              renderSelected={(choice) => <span>{choice.label}</span>}
            />
          }
          trailing={ticketViewAction(selectedView.value, canCreate)}
        />
      ) : (
        secondaryPageHeader
      )
    ) : undefined;
    const drawerViewAllowed = !['settings', 'notifications'].includes(viewMode.value);
    return (
      <MainShell
        tabs={tabs}
        mode="project"
        mobile={viewportMobile.value}
        sidebar={<SidebarSurface {...sidebarSurfaceProps()} />}
        sidebarVisible={viewportMobile.value ? mobileOverlay.value.sidebar : sidebarVisible.value}
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
            mode={viewportMobile.value && viewMode.value === 'board' ? 'list' : viewMode.value}
            listOnly={viewportMobile.value}
            searchOpen={searchOpen.value}
            searchQuery={searchQuery.value}
            searchTokens={searchTokens.value}
            searchTagSuggestions={searchTagSuggestions()}
            searchDatePrefix={activeDatePrefix(searchQuery.value)}
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
        projectTabAction={
          !viewportMobile.value && !['settings', 'notifications'].includes(viewMode.value)
            ? ticketViewAction(selectedView.value, canCreate)
            : undefined
        }
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
          </>
        }
        pageHeader={pageHeader}
        workspace={<WorkspaceSurface {...workspaceSurfaceProps()} />}
        workspacePresentation={
          viewMode.value === 'board' && !viewportMobile.value && selectedView.value !== 'errors'
            ? 'edge-to-edge'
            : 'inset'
        }
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
        terminalDrawerSize={appRegionSize('app-terminal-drawer')}
        terminalDrawerMax={terminalDrawerMax.value}
        terminalDrawerTransitioning={terminalDrawerTransitioning.value}
        terminalFocusMode={
          mobileTerminalFocus.value.active &&
          mobileTerminalFocus.value.terminalId === terminalDrawerSelected.value &&
          viewportMobile.value &&
          terminalDrawerVisible.value &&
          drawerViewAllowed
        }
        sidePanelSeparator={magnifiedTerminalKey.value ? 'hidden' : 'auto'}
        terminalDrawerContentOverflow={terminalDrawerCreateMenuOpen.value ? 'visible' : 'clip'}
        inspector={
          viewMode.value === 'notifications' ? (
            <NotificationInspector />
          ) : viewMode.value === 'settings' ? (
            <InspectorPlaceholder selectionCount={0} />
          ) : corruptKey ? (
            <CorruptInspector
              ticket={corruptTicket}
              recovery={corruptKey ? corruptRecovery.value[corruptKey] : undefined}
              selectionCount={selectedTicketSlugs.value.length}
            />
          ) : inspectorProps ? (
            <Inspector {...inspectorProps} />
          ) : selectedTransitioning ? (
            <TicketInspectorSkeleton slug={selectedSlug} />
          ) : (
            <InspectorPlaceholder selectionCount={selectedTicketSlugs.value.length} />
          )
        }
        inspectorVisible={viewportMobile.value ? mobileOverlay.value.inspector : inspectorVisible.value}
        inspectorSize={inspectorSize.value}
        overlay={
          <>
            {popup}
            {ticketContextMenuSurface()}
          </>
        }
      />
    );
  }

  const appRoot = document.querySelector<HTMLElement>('#app')!;
  const renderMetrics = import.meta.env.DEV ? createRenderMetrics(appRoot) : undefined;
  const activeTicketCollectionKey = () => `${selectedProjectId.value}:${selectedView.value}`;
  const ticketScrollRoot = () => appRoot.querySelector<HTMLElement>('.app-shell__workspace') ?? appRoot;
  const ticketScrollScope = () => ({
    project: selectedProjectId.value,
    mode: `${shellMode.value}:${viewMode.value === 'board' && viewportMobile.value ? 'list' : viewMode.value}`,
    view:
      viewMode.value === 'notifications'
        ? notificationView.value
        : viewMode.value === 'settings'
          ? settingsCategory()
          : selectedView.value,
  });
  let renderedTicketCollectionKey = activeTicketCollectionKey();
  let initialProjectRestoreComplete = false;
  if (renderMetrics)
    (window as typeof window & { __hotsheetRenderMetrics?: typeof renderMetrics }).__hotsheetRenderMetrics =
      renderMetrics;
  function HotSheetApp() {
    void permissionRevision.value;
    if (conversationOpen.value || terminalDrawerVisible.value || shellMode.value === 'terminals')
      void conversationStates.value;
    const ticketScrollGeneration = ticketScrollMemory.beforeRender(ticketScrollScope(), ticketScrollRoot()),
      nextTicketCollectionKey = activeTicketCollectionKey(),
      progressiveRenderPass = skipNextTicketMotion,
      canAnimateTickets = !progressiveRenderPass && nextTicketCollectionKey === renderedTicketCollectionKey,
      ticketMotion = canAnimateTickets
        ? captureTicketMotion(appRoot, renderedTicketCollectionKey)
        : { scope: '', rows: new Map() };
    skipNextTicketMotion = false;
    renderedTicketCollectionKey = nextTicketCollectionKey;
    renderMetrics?.recordPass();
    if (renderMetrics && uiStabilityDiagnostics) {
      const suppression = renderStormSuppressionReason({
        initialProjectRestoreComplete,
        foregroundLoading: loading.value,
        progressiveTicketRendering: progressiveRenderPass,
        backgroundProjectRefresh,
        activeToolTurn: Object.values(driveConnectionsByProject.value).some((connections) =>
          connections.some((connection) => connection.busy),
        ),
      });
      uiStabilityDiagnostics.recordRender(renderMetrics.snapshot(), suppression);
    }
    queueMicrotask(() => {
      const ticketCollectionPending =
        shellMode.value === 'project' &&
        (viewMode.value === 'list' || viewMode.value === 'board') &&
        (ticketCollectionState.value?.status === 'loading' ||
          (workspaceSearchActive() && searchMatchKeys.value === undefined));
      ticketScrollMemory.afterRender(
        ticketScrollGeneration,
        ticketScrollRoot(),
        !loading.value &&
          !ticketCollectionPending &&
          !ticketRenderScheduled &&
          (shellMode.value !== 'terminals' || !terminalDashboardLoading.value),
      );
      animateTicketMotion(ticketMotion, appRoot, undefined, activeTicketCollectionKey());
      syncTerminalViewportMounts();
      syncRepositoryPaginationObserver();
      syncTerminalDrawerMaximum();
      syncAttachmentGalleryMeasurement();
      syncVideoPosters(appRoot);
      syncConversationScroll(appRoot);
    });
    const target = notWorkingTarget.value,
      provider = defaultProvider(),
      visibilityScope = terminalVisibilityDialogScope.value,
      migration = hs1MigrationProject.value,
      deleteTargetId = savedViewDeleteTargetId.value,
      deleteView = deleteTargetId ? customViewsFor().find((item) => item.id === deleteTargetId) : undefined,
      showCornerLoading = loading.value && (!project() || tickets.value.length > 0 || corruptTickets.value.length > 0);
    return (
      <>
        <HotSheetShell />
        {aiConversationSurface()}
        <ProjectDialog
          open={projectDialogOpen.value}
          root={projectDialogRoot(project())}
          error={projectDialogError.value}
          recovery={unhealthyServerRecovery.value}
          recoveryBusy={unhealthyServerRecoveryBusy.value}
        />
        <RemoteProjectDialog
          open={remoteProjectDialogOpen.value}
          checkouts={remoteProjectCheckouts.value}
          loading={remoteProjectLoading.value}
          error={remoteProjectError.value}
        />
        <ProjectCloseDialog state={projectCloseDialog.value} />
        <ConversationExportDialog state={conversationExportDialog.value} />
        <TicketLinkChoiceDialog choice={ticketLinkChoice.value} />
        <SavedViewDialog
          open={savedViewDialogOpen.value}
          mode={savedViewDialogMode.value}
          name={savedViewName.value}
          query={savedViewQuery.value}
          queryTokens={savedViewQueryTokens.value}
          busy={savedViewBusy.value}
          error={savedViewError.value}
        />
        <SavedViewDeleteDialog
          open={Boolean(deleteView)}
          name={deleteView?.name ?? 'this view'}
          busy={savedViewDeleteBusy.value}
          error={savedViewDeleteError.value}
        />
        <ManualModelDialog state={manualModelDialog.value} />
        {migration && (
          <Hs1MigrationDialog
            projectName={migration.name}
            projectRoot={migration.root}
            sourcePath={migration.hs1SourcePath ?? `${migration.root}/.hotsheet`}
            databasePath={migration.hs1DatabasePath ?? `${migration.root}/.hotsheet/db`}
            postgresVersion={migration.hs1PostgresVersion}
            defaultStore={`${migration.root}.hs2`}
            open
            busy={hs1MigrationBusy.value}
            error={hs1MigrationError.value}
          />
        )}
        <TicketSourceSetupDialog
          project={ticketSourceSetupProject.value}
          providerKind={providerSetupKind.value}
          providerConnections={providerConnections.value}
          editingProviderId={providerEditingId.value}
          githubAuth={githubAuth.value}
          navigation={ticketSourceSetupNavigation.value}
          createdGitTicketStore={createdGitTicketStore.value}
          setupError={ticketSourceSetupError.value}
          remoteError={ticketSourceRemoteError.value}
          remoteBusy={ticketSourceRemoteBusy.value}
          providerBusy={providerSettingsBusy.value}
          providerError={providerSettingsError.value}
        />
        <TerminalRenameDialog target={terminalRename.value} />
        <TerminalVisibilityDialog
          open={Boolean(visibilityScope)}
          state={terminalVisibility.value}
          scope={visibilityScope ?? TERMINAL_DASHBOARD_VISIBILITY_SCOPE}
          groups={workspaceTerminalGroups()}
          types={terminalVisibilityFilter.value}
          contextMenu={terminalVisibilityContextMenu.value}
        />
        <TerminalVisibilityNameDialog prompt={terminalVisibilityNamePrompt.value} />
        {commandDialogSurface()}
        <ConnectionDetailsSurface assessment={project()?.compatibility} />
        {repositoryStatusSurface()}
        {changeEvidenceSurface()}
        <BulkTicketDialog state={bulkTicketDialog.value} />
        <TicketCloseDialog state={ticketCloseDialog.value} />
        <QuickTicketComposer
          expanded={composerExpanded.value}
          title={composerTitle.value}
          details={composerDetails.value}
          category={composerCategory.value}
          upNext={composerUpNext.value}
          providerName={provider?.name ?? 'Hot Sheet git'}
          canCreate={provider?.capabilities.create ?? true}
          attachments={composerAttachments.value}
          attachmentsEnabled={canStageNewTicketAttachments()}
          attachmentMessage={composerAttachmentMessage.value}
          attachmentError={composerAttachmentError.value}
          busy={composerScreening.value}
          submitting={composerSubmitting.value}
        />
        {target.slug && (
          <NotWorkingSurface
            {...notWorkingSurfaceProps(
              target,
              notWorkingNote.value,
              notWorkingFiles.value,
              notWorkingSubmitting.value,
              notWorkingError.value,
            )}
          />
        )}
        <ReaderLayersSurface {...readerLayersSurfaceProps()} />
        {gallerySurface()}
        <AttachmentContextMenuSurface menu={attachmentMenuSurfaceProps()} />
        <AppTabMenuSurface menu={appTabContextMenu.value} />
        {showCornerLoading && (
          <div class="app-loading" role="status">
            Loading…
          </div>
        )}
        {toastMessage.value && (
          <div class="app-toast" role="status">
            {toastMessage.value}
          </div>
        )}
        {error.value && project() && <AppError message={error.value} />}
      </>
    );
  }
  mount(appRoot, withControlledOpen(appRoot, HotSheetApp));

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
        !(event.target as Element).closest(
          '[data-component="saved-view-context-menu"], [data-action="open-saved-view-menu"]',
        )
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
    if (reader) {
      readerDetailsEditGeneration += 1;
      readerDetailsDraftBase = selectedTicket.value.details;
    } else {
      detailsEditGeneration += 1;
      detailsDraftBase = selectedTicket.value.details;
    }
    draft.value = selectedTicket.value.details;
    mode.value = 'write';
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

  // prettier-ignore
  const interactionBindingsPort: InteractionBindingsPort = {
    openProjectPicker, openRemoteProjectDialog, chooseAndOpenProject, unhealthyServerRecovery, projectDialogOpen, openRemoteCheckout, remoteProjectDialogOpen, importHs1Project,
    chooseHs1TicketStore, hs1MigrationProject, hs1MigrationBusy, hs1SourceIdentity, project, migrationJobDetails, migrationJobs, migrationConnectionErrors,
    migrationJobsByRoot, ticketSourceSetupProject, createdGitTicketStore, ticketSourceSetupNavigation, removeOldHs1Data, projects, providerSetupKind, providerEditingId,
    providerSettingsError, ticketSourceRemoteError, connectCreatedGitRemote, createProjectGitSource, chooseProjectPath, recoverUnhealthyProjectServer, repository, repositoryView,
    repositorySetupStep, repositorySetupError, repositoryFileMenu, repositorySelectedFiles, repositoryComparison, expandedCodeReviewCommits, loadRepositoryDetail, refreshRepositoryStatus,
    initializeRepository, connectRepositoryRemote, skipRepositoryRemote, repositoryDetail, showToast, error, codeReview, changeEvidenceView,
    changeEvidenceReader, selectedTicket, codeReviewMessage, openProject, currentRememberedProjectRoots, persistDrawerTabOrder, currentDrawerTabIds, focusDrawerTab,
    revealCorruptTicket, queueCorruptTicketRepair, corruptTickets, selectedCorruptKey, selectedTicketSlugs, setInspectorVisible, statsProjectId, setShellMode,
    selectTerminalRailProject, selectTicketView, terminalRailDirection, terminalRailScreen, selectProjectTab, retryProjectRestore, terminalDrawerBounds, terminalDashboardSize,
    terminalDrawerFitHigh, terminalFitAcross, terminalFitHigh, terminalSession, magnifiedTerminalKey, openTerminalInProject, terminalContextMenu, terminalVisibilityScopeFor,
    terminalVisibility, persistTerminalVisibility, terminalVisibilityFilter, terminalVisibilityContextMenu, terminalVisibilityDialogScope, terminalVisibilityNamePrompt, terminalKeysForVisibilityDialog, openGridAIChat,
    setTerminalDrawerVisible, terminalDrawerVisible, toggleTerminalDrawerMaximized, selectDrawerItem, terminalDrawerCreateMenuOpen, enterMobileTerminalFocus, exitMobileTerminalFocus, cycleMobileTerminalColumns, createProjectTerminal, aiLaunchConfiguration, createDrawerAIChat,
    openSavedConversation, requestProjectClose, projectCloseDialog, restoreBorrowedProjectCloseTerminal, cancelProjectClose, confirmProjectClose, closeAllProjectResources, closeTerminalIds,
    closeDrawerAIChat, appTabContextMenu, terminalGroups, terminalRename, closeDrawerTabIds, saveTerminalName, viewportMobile, mobileOverlay,
    selectTickets, selectionOrder, visibleTickets, selectedView, hideVerifiedColumn, cancelTicketDrafts, openTicketReader, ticketContextMenu,
    selectedRows, executeBulkTicketAction, tickets, openNotWorking, openTicketClose, copySelection, pasteSelection, openBulkTicketDialog,
    restoreTrashedTickets, bulkTicketDialog, openEmptyTrash, emptyTrash, setTicketCloseReason, searchTicketCloseTargets, ticketCloseDialog, submitTicketClose,
    closeTicketCloseDialog, openDuplicateTarget, notWorkingNote, scheduleProjectSessionPersistence, presentNotWorkingDialog, addNotWorkingFiles, draftScope, notWorkingTarget,
    notWorkingFiles, submitNotWorking, closeNotWorking, notWorkingSubmitting, keyboardShortcutOverrides, appleShortcutPlatform, isEditableEvent, openSavedViewDialog,
    savedViewMenu, openSavedViewRename, openSavedViewDelete, savedViewName, savedViewError, readInlineSearchField, savedViewQueryTokens, updateSavedViewQuery,
    focusSavedViewQuery, removeSavedViewQueryToken, editSavedViewQueryToken, savedViewQuery, saveSavedView, closeSavedViewDialog, savedViewBusy, deleteSavedView,
    closeSavedViewDelete, savedViewDeleteBusy, commandGroupExpanded, persistWorkspacePreferences, commandGroupsCollapsed, toggleSidebarDrive, driveOptionsOpen, aiTools,
    aiSettingsLoading, refreshAiConfiguration, driveOverridesByProject, normalizedAiSelection, selectDriveModel, openManualModel, effectiveDriveSelection, openSidebarConversation,
    conversationOpen, openConversationExport, pickConversationMessage, copyConversationSelection, clearConversationSelection, conversationExportDialog, finishConversationExport, updateConversationExportDraft,
    conversationConnectionId, conversationDrafts, sendConversationTurn, stopConversation, selectConversationProvider, selectConversationModel, selectConversationEffort, canAddNotes,
    updateSelected, runCommand, commandDialogId, commandStopConfirmation, commandRuns, commandSettingsEditingId, commandIconSearch, addCommandSetting,
    manualModelDialog, deleteCommandSetting, addCommandGroup, deleteCommandGroup, selectCommandRow, commandSelection, commandSettingsDefinitions, selectCommandSetting,
    reorderCommandSettings, updateCommandSetting, updateCommandAiSelection, effectiveCommandAiSelection, showLoadingActivity, inheritGlobalShellHistory, terminalSettingsMessage, trashSettingsMessagesByProject,
    trashCleanupDaysByProject, resetProgressiveTicketRendering, viewMode, setSettingsCategory, refreshProviderConnections, refreshTerminalSettings, refreshTrashSettings, capturingShortcutId,
    saveAiDefaults, selectDefaultModel, restoreCommandEditorAfterManualModel, aiDefaults, providerConnections, githubAuth, cancelGitHubSignIn, startGitHubSignIn,
    saveExternalProvider, notificationView, permissionTimer, permissionAutomationByProject, updatePermissionTimer, permissionRevision, permissionInbox, pendingPermissions,
    resolvePermission, selectedProjectId, hideVerifiedByProject, selectLinkedTicket, ticketLinkChoice, openTicketLinkMatch, cancelTicketLinkChoice, searchOpen,
    readWorkspaceSearchEditor, updateTicketSearch, restoreWorkspaceSearchEnd, removeWorkspaceSearchToken, addWorkspaceSearchTag, editWorkspaceSearchToken, searchHelpOpen, replaceActiveWorkspaceSearchToken,
    focusWorkspaceSearch, searchQuery, searchTokens, scheduleTicketSearch, sort, sortDirection, openTicketComposer, composerSubmitting,
    composerExpanded, resetTicketComposer, composerTitle, composerDetails, composerCategory, composerUpNext, addNewTicketFiles, composerAttachments,
    composerAttachmentMessage, composerAttachmentError, submitNewTicket, history, addAttachments, api, attachmentMessage, refreshProject,
    galleryImages, resetAttachmentGallery, shiftGallery, attachmentGalleryGeometry, attachmentGalleryScale, attachmentGalleryUrl, attachmentMenu, syncAttachmentGalleryMeasurement,
    activeAttachmentGalleryVideo, attachmentGalleryDuration, attachmentGalleryMarkup, finishGalleryAnnotationSession, beginGalleryAnnotationSession, attachmentGalleryDrawMode, attachmentGallerySelectedAnnotation, attachmentGalleryAnnotations,
    updateGalleryPlaybackPresentation, attachmentGalleryPlayhead, attachmentGalleryPlaying, gallerySvgClock, stopGallerySvgClock, attachmentGalleryVolumeOpen, attachmentGalleryMuted, attachmentGalleryVolume,
    canUseAttachments, updateSelectedTracked, readerOpen, readerDetailsDraft, detailsDraft, titleDraft, readerBlockedReasonDraft, blockedReasonDraft,
    readerNoteDraft, noteDraft, fieldConflictResolution, fieldConflict, canUpdateSelected, titleEditing, activeTicketSurface, titleAutosave,
    tagsAutosave, beginDetailsEdit, linkedReaderFrame, replaceLinkedReaderFrame, linkedReaderSaves, readerDetailsAutosave, detailsAutosave, beginDetailsFinish,
    finishDetailsEdit, readerEditingNoteId, editingNoteId, composingNote, newNoteDraft, readerNoteAutosave, noteAutosave, readerInlineFeedbackReplies,
    readerFeedbackChoiceSelections, readerFeedbackChoiceAnchors, canDeleteNotes, workspaceSearchActive, loadBoardColumnMore, loadNextTicketPage, readerBlockedReasonEditing, blockedReasonEditing,
    readerBlockedReasonAutosave, blockedReasonAutosave, presentTicketReaderDialog, readerTab, codeReviewLoading, refreshCodeReview, readerDialog, readerApprovedClose,
    approveTicketReaderClose, finishTicketReaderClose, readerLargeText, linkedReaderStack, inspectorTab, setSidebarVisible, sidebarVisible, appRegionSize,
    setAppRegionSize, terminalDrawerMax, syncTerminalDrawerMaximum, updateTerminalDrawerBounds, copyDraggedTickets, inspectorVisible, switchWorkspaceView, shellMode,
    terminalDrawerSelected, ticketWorkAreaFocused, ordinaryTextSelected, openCommandHistory, completePointerDetailsFinish, schedulePointerDetailsFinish,
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
            return pendingTerminalFocus;
          },
    set pendingTerminalFocus(value) {
            pendingTerminalFocus = value;
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
    get workspaceSearchEditingToken() {
            return workspaceSearchEditingToken;
          },
    set workspaceSearchEditingToken(value) {
            workspaceSearchEditingToken = value;
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
  wireHotSheetInteractions(createHotSheetInteractionBindings(interactionBindingsPort));
  // Flush the debounced session (including the in-progress new-ticket composer draft) before the page
  // is hidden, reloaded, or restarted, so a background refresh/restart never loses typed text (HS2-D4PB9Y).
  const flushProjectSessionPersistence = () => {
    if (projectSessionTimer !== undefined) {
      window.clearTimeout(projectSessionTimer);
      projectSessionTimer = undefined;
    }
    persistProjectSessionNow();
  };
  window.addEventListener('pagehide', flushProjectSessionPersistence);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') flushProjectSessionPersistence();
  });
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
      startPermissionUpdates();
      syncProjectChangeStreams();
    } finally {
      initialProjectRestoreComplete = true;
      initialProjectRestorePending.value = false;
      projectRestorePendingRoots.value = [];
    }
  })();
}
