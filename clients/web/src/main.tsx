import './components/heading.css';
import '@kerfjs/ui/webawesome.css';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './hot-sheet-tokens.css';
import './style.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { type ResizableRegionAxis, type ResizableRegionEdge } from '@kerfjs/ui/resizable-region';
import { Select } from '@kerfjs/ui/select';
import { readTokenSearchField } from '@kerfjs/ui/token-search-field';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { batch, effect, mount, signal } from 'kerfjs';
import { ChevronLeft, Trash2 } from 'lucide';

import { isTicketActivelyWorkedOn, nextActiveTicketExpiry, projectTabTicketState } from './active-ticket-work';
import {
  collectMatchingSearchPages,
  filterAdvancedSearchResults,
  usesAdvancedSearchExpression,
  usesBooleanSearchExpression,
} from './advanced-search';
import {
  applyConversationActivity,
  applyConversationEvent,
  beginConversationTurn,
  type ConversationMessage,
  type ConversationState,
  conversationUsage,
  EMPTY_CONVERSATION,
} from './ai-conversation';
import {
  type AiToolDefaults,
  type AiToolDescriptor,
  Api,
  type AttachmentMetadata,
  type Capabilities,
  type Checkout,
  type CheckoutTicketCounts,
  type CheckoutTicketQuery,
  type CodeReview,
  type CommandDefinition,
  type CommandRun,
  type CorruptTicket,
  type CustomView,
  type DuplicateBacklink,
  type FullTicket,
  type MediaAnnotation,
  type PollResponse,
  type ProviderConnection,
  type RepositoryFile,
  type RepositoryStatus,
  revealCorruptTicketFile,
  type TicketCloseReason,
  type TicketRow as WireTicketRow,
  type ToolConnection,
  TurnStreamReplayGuard,
} from './api';
import {
  type AppRegionId,
  loadAppRegionSize,
  normalizeAppRegionSize,
  saveAppRegionSize,
  terminalDrawerMaximum,
} from './app-region-resize';
import { describeUnreadableAttachments, screenAttachmentFiles } from './attachment-files';
import { attachmentRoundNumbers, attachmentUploadBatchId } from './attachment-grouping';
import {
  type AttachmentReferenceContext,
  attachmentReferences,
  attachmentReferenceUrl,
  isGalleryMediaAttachment,
  isVideoAttachment,
} from './attachment-references';
import {
  applyBoardColumnFetch,
  boardColumnHasMore,
  type BoardColumnPage,
  boardColumnStatus,
  boardColumnStatuses,
  isPerColumnBoardView,
  nextBoardColumnFetch,
} from './board-pagination';
import { isRemoteClient } from './client-origin';
import { type CommandDropTarget, emptyExtraGroups, reorderCommandsMultiple } from './command-order';
import { AIConversation } from './components/ai-conversation';
import { AppEmptyState, ProjectRestoreState } from './components/app-empty-state';
import { AppError } from './components/app-error';
import {
  attachmentGalleryAnnotationVisible,
  type AttachmentGalleryGeometry,
  type AttachmentGalleryImage,
  attachmentGalleryShiftUrl,
  type AttachmentGallerySwipeGesture,
  releaseAttachmentGalleryVideo,
} from './components/attachment-gallery';
import { BulkTicketDialog, type BulkTicketDialogState } from './components/bulk-ticket-dialog';
import { commandIconNeedsCatalog } from './components/command-icon';
import { COMMAND_EDITOR_DIALOG_ID } from './components/command-settings-editor';
import { ConversationExportDialog, type ConversationExportDialogState } from './components/conversation-export-dialog';
import { corruptTicketKey, type CorruptTicketRecoveryState } from './components/corrupt-ticket-row';
import { Hs1CleanupBanner, Hs1JobBanner, Hs1MigrationBanner, Hs1MigrationDialog } from './components/hs1-migration';
import { MainShell } from './components/main-shell';
import { ManualModelDialog, type ManualModelDialogState } from './components/manual-model-dialog';
import type { MarkdownEditorMode } from './components/markdown-editor';
import { NotificationCenter } from './components/notification-center';
import { NotificationInspector } from './components/notification-inspector';
import { type NotificationView, notificationViewTitle } from './components/notification-navigation';
import { updatePermissionCountdownText } from './components/permission-request-card';
import {
  ProjectCloseDialog,
  type ProjectCloseDialogState,
  type ProjectCloseResource,
  projectCloseResourceKey,
  selectedProjectCloseResource,
} from './components/project-close-dialog';
import { ProjectDialog, projectDialogRoot, RemoteProjectDialog } from './components/project-dialog';
import {
  ProjectRestoreError,
  type ProjectRestoreFailure,
  projectRestoreTabId,
  rememberedProjectName,
} from './components/project-restore-error';
import { type ProjectTabBarMode } from './components/project-tab-bar';
import { type AppTabKind } from './components/project-tab-context-menu';
import { type ExternalProviderKind, type GithubAuthState } from './components/provider-setup-form';
import {
  focusQuickTicketComposerTitle,
  QuickTicketComposer,
  QuickTicketLauncher,
  showQuickTicketComposer,
} from './components/quick-ticket-composer';
import {
  AIConversationSurface,
  AppTabMenuSurface,
  AttachmentContextMenuSurface,
  ChangeEvidenceSurface,
  CommandDialogSurface,
  CompatibilityBannerSurface,
  ConnectionDetailsSurface,
  GallerySurface,
  NotWorkingSurface,
  PermissionPopupSurface,
  ReaderLayersSurface,
  ReaderLayerSurface,
  RepositoryStatusSurface,
  TicketContextMenuSurface,
  type TicketContextMenuSurfaceProps,
} from './components/reader-overlay-surfaces';
import type { RepositorySetupStep } from './components/repository-setup';
import {
  type ChangeEvidenceView,
  type RepositoryFileMenu,
  type RepositoryStatusView,
} from './components/repository-status-popover';
import { SavedViewDeleteDialog, SavedViewDialog } from './components/saved-view-dialog';
import { ServerBusyBars, ServerBusyMessage } from './components/server-busy-bars';
import { type SettingsCategory, settingsCategoryTitle } from './components/settings-navigation';
import { SettingsWorkspace } from './components/settings-workspace';
import type { TicketStatus } from './components/status-badge';
import { TerminalDashboardControls, type TerminalDashboardGroup } from './components/terminal-dashboard';
import { type TerminalDrawerChatTab, type TerminalDrawerProps } from './components/terminal-drawer';
import { TerminalRenameDialog } from './components/terminal-rename-dialog';
import {
  TerminalVisibilityDialog,
  TerminalVisibilityNameDialog,
  type TerminalVisibilityNamePrompt,
} from './components/terminal-visibility-dialog';
import { TicketCloseDialog, type TicketCloseDialogState } from './components/ticket-close-dialog';
import { type CodeReviewComparison } from './components/ticket-code-review';
import type { TicketEmptyStateProps } from './components/ticket-empty-state';
import { type InspectorTab, type TicketInspectorProps } from './components/ticket-inspector';
import { TicketInspectorSkeleton } from './components/ticket-inspector-skeleton';
import { CorruptInspector, Inspector, InspectorPlaceholder } from './components/ticket-inspector-surface';
import { type TicketLinkChoice, TicketLinkChoiceDialog } from './components/ticket-link-choice-dialog';
import { TicketList } from './components/ticket-list';
import { showTicketReaderDialog, type TicketReaderDialogElement } from './components/ticket-reader';
import type { TicketPriority, TicketRowProps } from './components/ticket-row';
import { isPlainTicketReselection, updateTicketSelection } from './components/ticket-selection';
import { TicketSourceSetupDialog } from './components/ticket-source-setup-dialog';
import { SavedViewContextMenu, type SavedViewContextMenuState } from './components/view-navigation';
import {
  GlobalWorkspaceSurface,
  type GlobalWorkspaceSurfaceProps,
  ProjectTerminalDrawerSurface,
  SidebarSurface,
  type SidebarSurfaceProps,
  TerminalOperationsSurface,
  type TerminalOperationsSurfaceProps,
  TerminalRailSurface,
  type TerminalRailSurfaceProps,
  WorkspaceSurface,
  type WorkspaceSurfaceProps,
} from './components/workspace-composition-surfaces';
import {
  WorkspaceControls,
  WorkspaceIdentity,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  workspaceUpNextState,
  type WorkspaceViewMode,
} from './components/workspace-header';
import {
  buildConversationExportRequest,
  conversationExportAssets,
  type ConversationExportDestination,
  type ConversationExportDraft,
  type ConversationExportOpenResult,
  type ConversationExportScope,
  conversationExportScopeAfterMessagePick,
  type ConversationExportWriteResult,
  conversationTranscriptMarkdown,
  defaultConversationExportDraft,
  selectedConversationMessages,
  suggestedConversationExportName,
} from './conversation-export';
import { loadConversationStates, saveConversationStates } from './conversation-persistence';
import { syncConversationScroll } from './conversation-scroll';
import { customAiCommandSignalConnection, customAiCommandTicket, HOTSHEET_SKILL_SIGNAL } from './custom-ai-command';
import { createDebouncedAutosave, type DebouncedAutosave } from './debounced-autosave';
import type { DevReviewSubmission } from './dev-review';
import { devReviewRequested } from './dev-review/request';
import {
  drawerTabFocusRequestStillOwned,
  drawerTabSelectionAfterClose,
  loadDrawerTabOrder,
  orderedDrawerTabIds,
  saveDrawerTabOrder,
} from './drawer-tab-order';
import { fullTicketFeedbackNeeded, presentedNoteKind } from './feedback-needed';
import { type InlineFeedbackReply } from './feedback-replies';
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
} from './inline-search';
import { restoreInlineSearchCaret } from './inline-search-caret';
import { beginInteractionTiming } from './interaction-performance';
import { wireAttachmentAndGalleryInteractions } from './interactions/attachments-and-gallery';
import { wireCommandAndAiInteractions } from './interactions/commands-and-ai';
import { data } from './interactions/dom';
import { wireInspectorAndEditorInteractions } from './interactions/inspector-and-editor';
import { wireNavigationAndTabInteractions } from './interactions/navigation-and-tabs';
import { wireNotificationAndLinkInteractions } from './interactions/notifications-and-links';
import { wireProjectLifecycleInteractions } from './interactions/project-lifecycle';
import { wireRepositoryInteractions } from './interactions/repository';
import { wireSearchAndComposerInteractions } from './interactions/search-and-composer';
import { wireShellAndGlobalInteractions } from './interactions/shell-and-global';
import { wireTerminalInteractions } from './interactions/terminals';
import { wireTicketSelectionInteractions } from './interactions/ticket-selection';
import type {
  AttachmentMenu,
  Control,
  DetailsFinishTask,
  NotWorkingTarget,
  PendingEvidence,
  Project,
  RepositoryDetailState,
  UnhealthyServerRecovery,
} from './interactions/types';
import { wireViewAndSavedViewInteractions } from './interactions/views-and-saved-views';
import { isAppleShortcutPlatform, loadShortcutOverrides, type ShortcutChord } from './keyboard-shortcuts';
import { LocalTicketChangeAcknowledgements } from './local-ticket-changes';
import { loadLucideCatalog } from './lucide-catalog';
import { MigrationJobClient } from './migration-job-client';
import { type MigrationJob, migrationPercent, migrationPhaseLabel } from './migration-progress';
import { isMobileViewport, MOBILE_OVERLAYS_CLOSED, type MobileOverlayState } from './mobile-layout';
import { createTicketWithAttachments, describeNewTicketAttachmentFailures } from './new-ticket-attachments';
import { submitNotWorkingReport } from './not-working-workflow';
import { mergeRetainedCreatedRows, PendingCreatedTickets } from './pending-created-tickets';
import {
  DEFAULT_PERMISSION_AUTOMATION,
  formatPermissionCountdown,
  parsePermissionAutomation,
  parsePermissionHistory,
  parsePermissionResolution,
  PERMISSION_DELAYS,
  type PermissionAutomation,
  permissionBelongsToProject,
  type PermissionDecision,
  PermissionInbox,
  type PermissionItem,
  type PermissionScope,
  VisiblePermissionTimer,
} from './permission-notifications';
import { priorityFromWire } from './priority-wire';
import { afterBrowserPaint } from './project-activation';
import { containsRepositoryChange, containsTicketChange, startProjectChangeStream } from './project-change-poll';
import {
  compatibleAiEffort,
  type DrawerAIChat,
  prepareProjectConversation,
  projectChatConnectionId,
  projectDriveControlState,
  recoverProjectConnections,
  restoreDrawerAIChats,
  runProjectDrive,
  SIDEBAR_DRIVE_PROMPT,
  sidebarDriveConnectionId,
} from './project-drive';
import { projectSettingsValue, updateProjectSettingsValue } from './project-settings-state';
import { openProjectFetch, type ProjectOpenResult, restoreRememberedProjects } from './project-startup';
import { createProjectTabRefreshCoordinator } from './project-tab-refresh';
import { appendUniqueTicketRows } from './project-ticket-refresh';
import { loadProjectTicketRefresh, type ProjectTicketRefresh } from './project-ticket-refresh';
import { createRefreshBarrier } from './refresh-barrier';
import { createRenderMetrics } from './render-metrics';
import { customViewNameAvailable, uniqueCustomViewId } from './saved-views';
import { computeServerBusyBarCount, serverBusy, serverBusyMessage } from './server-busy';
import { applyRememberedTabOrder, replaceTabInPlace } from './tab-order';
import { TERMINAL_GRID_DEFAULT_ACROSS, TERMINAL_GRID_DEFAULT_HIGH } from './terminal-grid-layout';
import { defaultTerminalName, parseTerminalNames, terminalNameKey } from './terminal-names';
import { ProgressiveTerminalWorkQueue } from './terminal-progressive-work';
import { terminalDrawerActivation, terminalProjectOwner } from './terminal-project-scope';
import { TERMINAL_DRAWER_RESIZE_END_EVENT } from './terminal-viewport';
import {
  mountTerminalViewport,
  terminalBrowserWebSocketUrl,
  type TerminalFocusRequest,
  terminalViewportShouldAutoFocus,
} from './terminal-viewport';
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
} from './terminal-visibility';
import { hasUnresolvedBlocker } from './ticket-blocking';
import { ticketBoardGroups, ticketBoardGroupTotal } from './ticket-board-layout';
import {
  bulkTagChoices,
  type BulkTicketAction,
  BulkTicketMutationSequencer,
  bulkTicketPatch,
  canAtomicallyBulkUpdate,
  canBulkUpdate,
} from './ticket-bulk-operations';
import { loadLastTicketCategory } from './ticket-category-preference';
import {
  duplicateReference,
  type DuplicateTarget,
  duplicateTargetKey,
  parseDuplicateReference,
  resolveDuplicateReferenceTarget,
  validateTicketClose,
} from './ticket-close';
import { ticketCompletionTrend } from './ticket-completion-trend';
import { loadTicketEditorSizes } from './ticket-editor-size';
import {
  isTicketConcurrencyConflict,
  reconcileActiveDraft,
  reconcileTicketPatch,
  type TicketFieldConflict,
} from './ticket-field-reconciliation';
import {
  parseTicketLinkReference,
  resolveTicketLink,
  type TicketLinkMatch,
  ticketLinkMatchKey,
} from './ticket-link-resolution';
import { animateTicketMotion, captureTicketMotion, waitForTicketMotionSettled } from './ticket-motion';
import { projectTicketPatch, reportMutationTiming, ticketRowFromFull } from './ticket-mutation';
import {
  type ClipboardTicket,
  deduplicateTitle,
  TicketHistory,
  type TicketPatch,
  type TicketSnapshot,
} from './ticket-operations';
import {
  activeTicketReaderProject,
  disposeTicketReaderFrames,
  popTicketReaderFrame,
  pushTicketReaderFrame,
  reconcileTicketReaderFrame,
  ticketReaderEditState,
  type TicketReaderFrame,
  updateTicketReaderFrame,
} from './ticket-reader-stack';
import { TicketScrollMemory } from './ticket-scroll-state';
import { ticketTimelineEntries } from './ticket-timeline-data';
import { copiedTicketPlacement } from './ticket-transfer';
import {
  canCreateTicketInView,
  createdTicketVisibleInView,
  customTicketViewId,
  customTicketViewKey,
  isArchivedTicket,
  isOpenTicket,
  isQueuedTicket,
  isTrashedTicket,
  newTicketCreationPlacement,
  selectionAfterTicketViewChange,
  selectionVisibleInView,
  ticketSearchCountViews,
  ticketsForView,
  type TicketView,
  ticketViewQuery,
} from './ticket-views';
import { createTrailingTask } from './trailing-task';
import { renderStormSuppressionReason, type UiStabilityDiagnostics } from './ui-stability-diagnostics';
import { ensureVideoPoster, syncVideoPosters } from './video-posters';
import { loadWorkspacePreferences, saveWorkspacePreferences, sortableWorkspaceView } from './workspace-preferences';
import {
  activeProjectRoot,
  deleteDraftFiles,
  dismissHs1CleanupPrompt,
  hs1CleanupPromptDismissed,
  hs1MigrationPromptDismissed,
  loadDraftFiles,
  loadProjectWorkspaceSession,
  saveActiveProjectRoot,
  saveDraftFile,
  saveProjectWorkspaceSession,
} from './workspace-session';
import { compareWorkspaceTickets } from './workspace-ticket-sort';

const submitDevReview = async (submission: DevReviewSubmission) => {
  const response = await fetch('/__hotsheet/dev-review/tickets', {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-hotsheet-dev-review': '1' },
      body: JSON.stringify(submission),
    }),
    result = (await response.json()) as { slug?: string; error?: string };
  if (!response.ok || !result.slug) throw new Error(result.error ?? 'Ticket creation failed.');
  return { slug: result.slug };
};
let uiStabilityDiagnostics: UiStabilityDiagnostics | undefined;
const turnStreamReplayGuard = new TurnStreamReplayGuard();
const localTicketChangeAcknowledgements = new LocalTicketChangeAcknowledgements();
// Retains just-created rows so an eventually-consistent index that hasn't indexed them yet
// cannot drop them from a background refresh (HS2-Y5PDHW).
const pendingCreatedTickets = new PendingCreatedTickets();
const turnStreamEvents = (response: PollResponse) => turnStreamReplayGuard.events(response);
let backgroundProjectRefresh = false;
if (import.meta.env.DEV) {
  const dev = await import('kerfjs/dev');
  dev.enableWarnings({ valueOnlyRerender: true, listRebind: true, invariants: 'throw' });
  if (!import.meta.env.HOTSHEET_WEB_STABLE_DEV) await import('./dev-reload-diagnostics-entry');
  if (devReviewRequested(location.href, true)) {
    const [{ installUiStabilityDiagnostics }, devReview] = await Promise.all([
      import('./ui-stability-diagnostics'),
      import('./dev-review'),
    ]);
    uiStabilityDiagnostics = installUiStabilityDiagnostics({
      onThrash: async (diagnostic) => {
        await submitDevReview({
          notes: 'UI stability diagnostics detected repeated unexpected control dismissal or render thrashing.',
          captures: [],
          attachments: [diagnostic],
          actorRole: 'system',
          pageUrl: location.href,
          viewport: { width: innerWidth, height: innerHeight },
        });
      },
    });
    // The visible Dev Review overlay is desktop-only: it clutters mobile-width viewports and its
    // modifier-gated review interactions do not apply there. Install it only above the mobile
    // breakpoint and re-sync on resize (HS2-9KT6RQ). The headless UI-stability diagnostics stay on.
    let devReviewOverlay: { destroy(): void } | undefined;
    const syncDevReviewOverlay = () => {
      if (isMobileViewport(window.innerWidth)) {
        devReviewOverlay?.destroy();
        devReviewOverlay = undefined;
        return;
      }
      devReviewOverlay ??= devReview.installDevReview({
        submit: submitDevReview,
        diagnostics: () => uiStabilityDiagnostics!.attachment(),
      });
    };
    syncDevReviewOverlay();
    window.addEventListener('resize', syncDevReviewOverlay);
  }
  (
    window as typeof window & { __hotsheetUiStabilityDiagnostics?: UiStabilityDiagnostics }
  ).__hotsheetUiStabilityDiagnostics = uiStabilityDiagnostics;
}

loadTicketEditorSizes(localStorage, document.documentElement.style);

const projects = signal<Project[]>([]),
  selectedProjectId = signal(''),
  projectRestoreFailures = signal<ProjectRestoreFailure[]>([]),
  selectedProjectRestoreRoot = signal(''),
  tickets = signal<WireTicketRow[]>([]),
  ticketRowsByProject = signal<Record<string, WireTicketRow[]>>({}),
  corruptTickets = signal<CorruptTicket[]>([]),
  selectedTicket = signal<FullTicket | null>(null),
  repository = signal<RepositoryStatus | null>(null),
  repositoryError = signal(''),
  repositoryRefreshing = signal(false);
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
const duplicateBacklinkState = signal<{ key: string; backlinks: DuplicateBacklink[]; inaccessibleProjects: string[] }>({
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
const projectDialogOpen = signal(false),
  projectDialogError = signal('');
// Remote clients (loaded from a non-loopback origin) cannot browse the server filesystem, so they pick
// from the projects the server already knows about instead of the folder picker (HS2-VFNCXG).
const remoteProjectDialogOpen = signal(false),
  remoteProjectCheckouts = signal<Checkout[]>([]),
  remoteProjectLoading = signal(false),
  remoteProjectError = signal('');

const unhealthyServerRecovery = signal<UnhealthyServerRecovery | undefined>(undefined),
  unhealthyServerRecoveryBusy = signal(false);
const projectCloseDialog = signal<ProjectCloseDialogState | undefined>(undefined);
const ticketLinkChoice = signal<TicketLinkChoice | undefined>(undefined);
let pendingProjectCloseIds: string[] = [];
const customViewsByProject = signal<Record<string, CustomView[]>>({}),
  savedViewDialogOpen = signal(false),
  savedViewDialogMode = signal<'create' | 'rename'>('create'),
  savedViewTargetId = signal<string | undefined>(undefined),
  savedViewName = signal(''),
  savedViewQuery = signal(''),
  savedViewQueryTokens = signal<InlineSearchToken[]>([]),
  savedViewBusy = signal(false),
  savedViewError = signal(''),
  savedViewMenu = signal<SavedViewContextMenuState | undefined>(undefined),
  savedViewDeleteTargetId = signal<string | undefined>(undefined),
  savedViewDeleteBusy = signal(false),
  savedViewDeleteError = signal('');
const hs1MigrationProject = signal<Project | undefined>(undefined),
  hs1MigrationBusy = signal(false),
  hs1MigrationError = signal('');
const migrationJobsByRoot = signal<Partial<Record<string, MigrationJob>>>({}),
  migrationConnectionErrors = signal<Record<string, string>>({}),
  migrationJobDetails = signal<Record<string, boolean>>({});
const migrationJobs = new MigrationJobClient(
  (job) => {
    migrationJobsByRoot.value = { ...migrationJobsByRoot.value, [job.root]: job };
    migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [job.root]: '' };
    if (job.status === 'running')
      projects.value = projects.value.map((item) =>
        item.root === job.root ? { ...item, hs1CleanupEligible: false } : item,
      );
  },
  async (job) => {
    const target = projects.value.find((item) => item.root === job.root);
    if (!target) return;
    // Replayed terminal jobs are history; current receipts/origin determine eligibility.
    const response = await fetch('/__hotsheet/projects/open', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ root: target.root }),
    });
    const current = (await response.json()) as Project & { error?: string };
    if (!response.ok) throw new Error(current.error ?? 'Could not refresh the completed migration.');
    if (migrationJobsByRoot.value[job.root]?.attempt !== job.attempt) return;
    migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [job.root]: '' };
    projects.value = projects.value.map((item) => (item.root === target.root ? { ...item, ...current } : item));
    if (job.kind === 'import') {
      const descriptors = await new Api(target.apiPath).providers(),
        selected = descriptors.find((item) => item.default) ?? descriptors.at(0);
      if (migrationJobsByRoot.value[job.root]?.attempt !== job.attempt) return;
      defaultProviders.value = {
        ...defaultProviders.value,
        [target.id]: selected ? { name: selected.display_name, capabilities: selected.capabilities } : undefined,
      };
      providerCapabilities.value = {
        ...providerCapabilities.value,
        ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
      };
      await projectTabRefresh.request(target);
    }
  },
  (root, message) => {
    migrationConnectionErrors.value = { ...migrationConnectionErrors.value, [root]: message };
  },
);
const ticketSourceSetupProject = signal<Project | undefined>(undefined),
  ticketSourceSetupError = signal('');
const providerConnections = signal<ProviderConnection[]>([]),
  providerSetupKind = signal<ExternalProviderKind | undefined>(undefined),
  providerEditingId = signal<string | undefined>(undefined),
  providerSettingsBusy = signal(false),
  providerSettingsError = signal('');
const githubAuth = signal<GithubAuthState | undefined>(undefined);
const ticketSourceSetupNavigation = signal<'none' | 'push' | 'pop'>('none'),
  createdGitTicketStore = signal(''),
  ticketSourceRemoteBusy = signal(false),
  ticketSourceRemoteError = signal('');
const repositoryView = signal<RepositoryStatusView>('unstaged'),
  repositoryFileMenu = signal<RepositoryFileMenu | undefined>(undefined),
  repositorySelectedFiles = signal<string[]>([]);
const repositorySetupStep = signal<RepositorySetupStep | undefined>(undefined),
  repositorySetupBusy = signal(false),
  repositorySetupError = signal('');
let repositoryFileSelectionAnchor: string | undefined;
const changeEvidenceView = signal<ChangeEvidenceView>('docs');
// Which surface owns the change-evidence popover: a ticket-reader frame id when launched from inside
// the modal reader (so it renders as a reader descendant and stays interactive, not inert beneath the
// modal top layer — HS2-6EV2ES / HS2-EZ10RS), or undefined for the non-modal inspector (app root).
const changeEvidenceReader = signal<string | undefined>(undefined);
const repositoryComparison = signal<CodeReviewComparison>({ active: false, side: 'a' }),
  expandedCodeReviewCommits = signal<string[]>([]);

const repositoryDetail = signal<RepositoryDetailState>({
  view: 'unstaged',
  files: [],
  commits: [],
  loading: false,
  loaded: false,
  error: '',
});
let repositoryDetailGeneration = 0,
  repositoryPaginationObserver: IntersectionObserver | undefined;
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
  initialProjectRestorePending = signal(rememberedRoots.length > 0);
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
  terminalDrawerSelected = signal('grid');
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
const terminalViewportMounts = new Map<HTMLElement, () => void>();
const terminalViewportCandidates = new Set<HTMLElement>();
const terminalViewportObservationTargets = new Map<HTMLElement, HTMLElement>();
const terminalViewportCandidatesByTarget = new Map<HTMLElement, HTMLElement>();
let terminalViewportObserver: IntersectionObserver | undefined;
const terminalViewportWork = new ProgressiveTerminalWorkQueue<HTMLElement>({
  mountsPerTurn: 2,
  disposalsPerTurn: 2,
  schedule: (work) => requestAnimationFrame(() => window.setTimeout(work, 0)),
  mount: (element) => {
    mountTerminalViewportElement(element);
  },
  dispose: (work) => {
    work();
  },
});
const corruptRecovery = signal<Record<string, CorruptTicketRecoveryState>>({});
const selectedCorruptKey = signal<string | undefined>(undefined);
const selectedTicketSlugs = signal<string[]>([]);
const ticketContextMenu = signal<{ x: number; y: number; ticketSlug: string; hideUpNext?: boolean } | undefined>(
  undefined,
);
const ticketCloseDialog = signal<TicketCloseDialogState | undefined>(undefined);
let ticketCloseSearchGeneration = 0,
  ticketCloseSearchTimer: number | undefined;
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
let draggedTickets: { slugs: string[]; source: Project } | undefined;
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
    workspaceSorts.value = { ...workspaceSorts.value, [mode]: { ...workspaceSorts.value[mode], sortDirection: value } };
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
let composerAttachmentEpoch = 0;
const activeComposerScreenings = new Set<symbol>();
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
const attachmentGalleryUrl = signal<string | undefined>(undefined);
const attachmentGalleryGeometry = signal<AttachmentGalleryGeometry>({
    naturalWidth: 0,
    naturalHeight: 0,
    availableWidth: 0,
    availableHeight: 0,
  }),
  attachmentGalleryScale = signal<number | undefined>(undefined);
const attachmentGalleryMarkup = signal(false),
  attachmentGalleryDrawMode = signal(false),
  attachmentGalleryAnnotations = signal<MediaAnnotation[]>([]),
  attachmentGallerySelectedAnnotation = signal<string | undefined>(undefined),
  attachmentGalleryPlayhead = signal(0),
  attachmentGalleryDuration = signal(0),
  attachmentGalleryPlaying = signal(false),
  attachmentGalleryVolume = signal(1),
  attachmentGalleryMuted = signal(false),
  attachmentGalleryVolumeOpen = signal(false);
let attachmentAnnotationGesture:
  | {
      kind: 'draw' | 'move' | 'resize';
      pointerId: number;
      startX: number;
      startY: number;
      surface: DOMRect;
      annotation: MediaAnnotation;
      handle?: string;
    }
  | undefined;
let attachmentRangeGesture:
  { pointerId: number; annotationId: string; endpoint: 'start' | 'end'; track: DOMRect } | undefined;
let attachmentSwipeGesture: AttachmentGallerySwipeGesture | undefined;
let attachmentAnnotationSession:
  { projectId: string; ticketId: string; attachmentId: string; before: MediaAnnotation[] } | undefined;
let attachmentAnnotationSave = Promise.resolve();
let draggedGroupedAttachmentId: string | undefined;
let attachmentGallerySvgFrame: number | undefined, attachmentGallerySvgPreviousFrame: number | undefined;
let attachmentGalleryLivePlayhead = 0,
  attachmentGalleryLiveVolume = 1;
let attachmentGalleryObserver: ResizeObserver | undefined;

const attachmentMenu = signal<AttachmentMenu | undefined>(undefined);
const readerDetailsMode = signal<MarkdownEditorMode>('preview'),
  readerDetailsDraft = signal('');
let readerDetailsEditGeneration = 0;
const attachmentMessage = signal('');
const codeReview = signal<CodeReview | undefined>(undefined),
  codeReviewLoading = signal(false),
  codeReviewMessage = signal('');
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
const commandDefinitions = signal<CommandDefinition[]>([]),
  commandRuns = signal<CommandRun[]>([]),
  commandGroupExpanded = signal(storedWorkspacePreferences.commandGroupExpanded),
  commandGroupsCollapsed = signal(storedWorkspacePreferences.commandGroupsCollapsed),
  commandDialogId = signal<string | undefined>(undefined),
  commandStopConfirmation = signal(false),
  commandSettingsEditingId = signal<string | undefined>(undefined),
  commandSettingsDraftsByProject = signal<Record<string, string>>({}),
  commandSettingsMessagesByProject = signal<Record<string, string>>({}),
  commandSettingsSelectedByProject = signal<Record<string, string[]>>({}),
  commandSettingsExtraGroupsByProject = signal<Record<string, string[]>>({}),
  commandIconSearch = signal('');
let commandSelectionAnchor: string | undefined;
// Lazily load the full Lucide catalog once a project's commands reference an icon outside the
// bundled popular set, so the sidebar can render those custom icons without a picker being opened.
effect(() => {
  if (commandDefinitions.value.some((command) => commandIconNeedsCatalog(command.icon))) void loadLucideCatalog();
});
const driveConnectionsByProject = signal<Record<string, ToolConnection[]>>({}),
  drivePendingByProject = signal<Record<string, boolean>>({});
const aiTools = signal<AiToolDescriptor[]>([]),
  aiDefaults = signal<AiToolDefaults>({ tool: 'codex' }),
  aiSettingsLoading = signal(false),
  aiSettingsMessage = signal(''),
  driveOptionsOpen = signal(false),
  driveOverridesByProject = signal<Record<string, Partial<AiToolDefaults>>>({}),
  manualModelDialog = signal<ManualModelDialogState | undefined>(undefined);
let manualModelDialogShown = false;
let aiConfigurationProjectId = '';
const conversationStates = signal<Record<string, ConversationState>>(loadConversationStates(localStorage)),
  conversationDrafts = signal<Record<string, string>>({}),
  conversationSelections = signal<Record<string, { model?: string; effort?: string }>>({}),
  conversationConnectionId = signal<string | undefined>(undefined),
  conversationOpen = signal(false);
const conversationSelectionScopes = signal<Record<string, ConversationExportScope | undefined>>({});
const conversationExportDialog = signal<ConversationExportDialogState | undefined>(undefined);
let commandLongPressTimer: number | undefined,
  commandLongPressFired = false;
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
function loadPermissionHistory() {
  try {
    return parsePermissionHistory(JSON.parse(localStorage.getItem('hotsheet.permission-history') || '[]'));
  } catch {
    return [];
  }
}
function loadPermissionAutomation(projectId: string) {
  try {
    return parsePermissionAutomation(
      JSON.parse(localStorage.getItem(`hotsheet.project.${projectId}.permission-automation`) || 'null'),
    );
  } catch {
    return DEFAULT_PERMISSION_AUTOMATION;
  }
}
const storedPermissionHistory = loadPermissionHistory();
const permissionRevision = signal(0),
  permissionInbox = new PermissionInbox(storedPermissionHistory),
  permissionTimer = new VisiblePermissionTimer();
const permissionAutomationByProject = signal<Record<string, PermissionAutomation>>({});
const permissionResolutionErrors = signal<Record<string, string>>({});
let permissionPolling = false,
  permissionTimerInterval: number | undefined,
  permissionCountdown: { key: string; remainingMs: number } | undefined;
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
const currentRememberedProjectRoots = () => [
  ...new Set([...projects.value.map((item) => item.root), ...projectRestoreFailures.value.map((item) => item.root)]),
];
const settingsCategory = () => selectedSettingsCategory.value;
const commandSettingsDraft = (projectId = selectedProjectId.value) =>
  projectSettingsValue(
    commandSettingsDraftsByProject.value,
    projectId,
    JSON.stringify(commandDefinitions.value, null, 2),
  );
const commandSettingsMessage = (projectId = selectedProjectId.value) =>
  projectSettingsValue(commandSettingsMessagesByProject.value, projectId, '');
function setSettingsCategory(_projectId: string, category: SettingsCategory) {
  selectedSettingsCategory.value = category;
}
function setCommandSettingsDraft(projectId: string, draft: string) {
  commandSettingsDraftsByProject.value = updateProjectSettingsValue(
    commandSettingsDraftsByProject.value,
    projectId,
    draft,
  );
}
function setCommandSettingsMessage(projectId: string, message: string) {
  commandSettingsMessagesByProject.value = updateProjectSettingsValue(
    commandSettingsMessagesByProject.value,
    projectId,
    message,
  );
}
function isCommandSettingsDefinition(value: unknown): value is CommandDefinition {
  return (
    typeof value === 'object' &&
    value !== null &&
    'id' in value &&
    typeof value.id === 'string' &&
    'title' in value &&
    typeof value.title === 'string'
  );
}
function commandSettingsDefinitions(projectId = selectedProjectId.value): CommandDefinition[] {
  try {
    const parsed: unknown = JSON.parse(commandSettingsDraft(projectId));
    if (!Array.isArray(parsed)) return commandDefinitions.value;
    const entries: unknown[] = parsed;
    return entries.filter(isCommandSettingsDefinition);
  } catch {
    return commandDefinitions.value;
  }
}
function setCommandSettingsDefinitions(projectId: string, definitions: CommandDefinition[]) {
  setCommandSettingsDraft(projectId, JSON.stringify(definitions, null, 2));
  setCommandSettingsMessage(projectId, '');
}
function commandSelection(projectId = selectedProjectId.value): string[] {
  return commandSettingsSelectedByProject.value[projectId] ?? [];
}
function setCommandSelection(projectId: string, ids: readonly string[]) {
  commandSettingsSelectedByProject.value = { ...commandSettingsSelectedByProject.value, [projectId]: [...ids] };
}
function selectCommandSetting(projectId: string, id: string | undefined) {
  setCommandSelection(projectId, id ? [id] : []);
  commandSelectionAnchor = id;
}
/** Apply a click on a command row to the multi-selection, honoring toggle (Cmd/Ctrl) and range (Shift) intent. */
function selectCommandRow(projectId: string, id: string, intent: { toggle?: boolean; range?: boolean }) {
  const ordered = commandSettingsDefinitions(projectId).map((command) => command.id),
    current = commandSelection(projectId);
  if (intent.range && commandSelectionAnchor) {
    const from = ordered.indexOf(commandSelectionAnchor),
      to = ordered.indexOf(id);
    if (from >= 0 && to >= 0) {
      const [lo, hi] = from < to ? [from, to] : [to, from];
      setCommandSelection(projectId, ordered.slice(lo, hi + 1));
      return;
    }
  }
  if (intent.toggle) {
    const next = current.includes(id) ? current.filter((item) => item !== id) : [...current, id];
    setCommandSelection(projectId, next);
    commandSelectionAnchor = id;
    return;
  }
  selectCommandSetting(projectId, id);
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
const pendingPermissions = () => permissionInbox.pending();
const permissionHistory = () => permissionInbox.history();
const projectPendingPermissions = (projectId = selectedProjectId.value) =>
  pendingPermissions().filter((item) => item.projectId === projectId);
const projectPermissionHistory = (projectId = selectedProjectId.value) =>
  permissionHistory().filter((item) => item.projectId === projectId);
const visiblePermission = () => permissionInbox.visible();
const permissionCount = (projectId?: string) =>
  pendingPermissions().filter((item) => !projectId || item.projectId === projectId).length;
const permissionAutomation = (projectId: string) =>
  permissionAutomationByProject.value[projectId] ?? DEFAULT_PERMISSION_AUTOMATION;
const persistPermissionHistory = () => {
  localStorage.setItem('hotsheet.permission-history', JSON.stringify(permissionInbox.history()));
};
const api = () => new Api(project()?.apiPath ?? '');
const customViewsFor = (projectId = selectedProjectId.value) => customViewsByProject.value[projectId] ?? [];
function customViewFor(view: TicketView, projectId = selectedProjectId.value) {
  const id = customTicketViewKey(view);
  return id ? customViewsFor(projectId).find((item) => item.id === id) : undefined;
}
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
    const index = await loadProjectTicketRefresh(new Api(current.apiPath), current.id, query);
    if (!active()) return;
    if (index.tickets) {
      const mergedTickets = mergeRetainedCreatedRows(
        index.tickets,
        pendingCreatedTickets.retain(current.id, index.tickets),
      );
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
      terminalGroups.value.find((group) => group.projectId === projectId)?.sessions.map((session) => session.id) ?? [],
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
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function setTerminalDrawerVisible(visible:boolean,refresh=true){if(visible===terminalDrawerVisible.value){if(visible){terminalDrawerMounted.value=true;if(refresh)void refreshTerminalDashboard();observeTerminalDrawer();settleTerminalDrawerGeometry()}return}if(terminalDrawerTransitionTimer!==undefined)window.clearTimeout(terminalDrawerTransitionTimer);if(visible){const current=project(),chat=current&&terminalDrawerChatsByProject.value[current.id]?.some(item=>item.id===terminalDrawerSelected.value);if(current&&terminalDrawerSelected.value!=='grid'&&!chat)pendingTerminalFocus={projectId:current.id,terminalId:terminalDrawerSelected.value};terminalDrawerMounted.value=true}terminalDrawerTransitioning.value=true;terminalDrawerVisible.value=visible;localStorage.setItem('hotsheet.terminals.drawer-open',String(visible));terminalDrawerTransitionTimer=window.setTimeout(()=>{terminalDrawerTransitionTimer=undefined;terminalDrawerTransitioning.value=false;if(!terminalDrawerVisible.value)terminalDrawerMounted.value=false},220);if(visible){if(refresh)void refreshTerminalDashboard();observeTerminalDrawer()}else terminalDrawerObserver?.disconnect()}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function selectDrawerItem(id:string){const current=project(),chat=current?terminalDrawerChatsByProject.value[current.id]?.find(item=>item.id===id):undefined;if(current&&id!=='grid'&&!chat)pendingTerminalFocus={projectId:current.id,terminalId:id};if(chat)conversationConnectionId.value=chat.connectionId;terminalDrawerSelected.value=id;if(current)localStorage.setItem(`hotsheet.project.${current.id}.terminal-drawer-selection`,id)}
function openTerminalInProject(key: string) {
  const session = terminalSession(key);
  if (!session) return;
  const activated = session.projectId === selectedProjectId.value ? undefined : activateOpenProject(session.projectId);
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
      persistTerminalVisibility(hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${created.id}`));
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
      persistTerminalVisibility(hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${created.id}`));
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
function aiToolLabel(tool: string) {
  return (
    aiTools.value.find((item) => item.id === tool)?.display_name ?? `${tool.slice(0, 1).toUpperCase()}${tool.slice(1)}`
  );
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function normalizedAiSelection(value:Partial<AiToolDefaults>={}):AiToolDefaults{const tool=value.tool??aiDefaults.value.tool??aiTools.value[0]?.id??'codex',descriptor=aiTools.value.find(item=>item.id===tool),model=value.model??(tool===aiDefaults.value.tool?aiDefaults.value.model:undefined)??descriptor?.default_model??descriptor?.models[0]?.id,modelDescriptor=descriptor?.models.find(item=>item.id===model),efforts=modelDescriptor?.effort_levels??[],requestedEffort=value.effort??(tool===aiDefaults.value.tool&&model===aiDefaults.value.model?aiDefaults.value.effort:undefined)??descriptor?.default_effort,effort=requestedEffort&&efforts.includes(requestedEffort)?requestedEffort:efforts.at(0);return{tool,...(model?{model}:{}),...(effort?{effort}:{})}}
function effectiveCommandAiSelection(command: CommandDefinition): AiToolDefaults {
  return normalizedAiSelection({
    tool: command.tool ?? aiDefaults.value.tool,
    model: command.model ?? (!command.tool ? aiDefaults.value.model : undefined),
    effort: command.effort ?? (!command.tool && !command.model ? aiDefaults.value.effort : undefined),
  });
}
function effectiveDriveSelection(projectId = selectedProjectId.value) {
  return normalizedAiSelection(driveOverridesByProject.value[projectId]);
}
function selectDriveModel(model: string) {
  const current = project();
  if (!current || !model) return;
  const active = effectiveDriveSelection(current.id),
    descriptor = aiTools.value.find((item) => item.id === active.tool),
    effort = descriptor?.models.find((item) => item.id === model)?.effort_levels?.[0];
  driveOverridesByProject.value = {
    ...driveOverridesByProject.value,
    [current.id]: { tool: active.tool, model, ...(effort ? { effort } : {}) },
  };
}
function selectDefaultModel(model: string) {
  if (!model) return;
  const descriptor = aiTools.value.find((item) => item.id === aiDefaults.value.tool),
    effort = descriptor?.models.find((item) => item.id === model)?.effort_levels?.[0];
  void saveAiDefaults({ tool: aiDefaults.value.tool, model, ...(effort ? { effort } : {}) });
}
function selectConversationModel(model: string) {
  const current = project(),
    connectionId = conversationConnectionId.value;
  if (!current || !connectionId || !model) return;
  const selection = conversationAiSelection(connectionId),
    effort = selection.descriptor?.models.find((item) => item.id === model)?.effort_levels?.[0];
  conversationSelections.value = {
    ...conversationSelections.value,
    [connectionId]: { model, ...(effort ? { effort } : {}) },
  };
  terminalDrawerChatsByProject.value = {
    ...terminalDrawerChatsByProject.value,
    [current.id]: (terminalDrawerChatsByProject.value[current.id] ?? []).map((item) =>
      item.connectionId === connectionId ? { ...item, model, effort } : item,
    ),
  };
  driveConnectionsByProject.value = {
    ...driveConnectionsByProject.value,
    [current.id]: (driveConnectionsByProject.value[current.id] ?? []).map((item) =>
      item.id === connectionId ? { ...item, model, effort } : item,
    ),
  };
}
function selectConversationEffort(effort: string) {
  const current = project(),
    connectionId = conversationConnectionId.value;
  if (!current || !connectionId || !effort) return;
  conversationSelections.value = {
    ...conversationSelections.value,
    [connectionId]: {
      ...conversationSelections.value[connectionId],
      model: conversationAiSelection(connectionId).model,
      effort,
    },
  };
  terminalDrawerChatsByProject.value = {
    ...terminalDrawerChatsByProject.value,
    [current.id]: (terminalDrawerChatsByProject.value[current.id] ?? []).map((item) =>
      item.connectionId === connectionId ? { ...item, effort } : item,
    ),
  };
  driveConnectionsByProject.value = {
    ...driveConnectionsByProject.value,
    [current.id]: (driveConnectionsByProject.value[current.id] ?? []).map((item) =>
      item.id === connectionId ? { ...item, effort } : item,
    ),
  };
}
function aiToolOptions() {
  return aiTools.value.map((descriptor) => ({ id: descriptor.id, label: descriptor.display_name }));
}
/**
 * Change the provider (tool) of the active chat and re-seed the new provider's session with the prior
 * transcript as one read-only context turn, then continue live (HS2-PRBGRB). No earlier turn is
 * re-executed: the transcript is handed over as context text framed so the new provider does not act
 * on it. Providers keep separate sessions, so this opens a fresh chat for the target provider.
 */
async function selectConversationProvider(providerId: string) {
  const current = project(),
    connectionId = conversationConnectionId.value;
  if (!current || !connectionId || !providerId) return;
  const from = conversationAiSelection(connectionId);
  if (providerId === from.tool) return;
  const descriptor = aiTools.value.find((item) => item.id === providerId);
  if (!descriptor) return;
  const state = conversationStates.peek()[connectionId] ?? EMPTY_CONVERSATION,
    hasHistory = state.messages.length > 0;
  const transcript = hasHistory
    ? conversationTranscriptMarkdown(
        { conversationId: connectionId, tool: aiToolLabel(from.tool) },
        state.messages,
        state.activity,
      )
    : '';
  const selection = normalizedAiSelection({ tool: providerId });
  conversationOpen.value = false;
  const tab = await createDrawerAIChat(selection);
  if (!tab || project()?.id !== current.id) return;
  const newConnectionId = tab.connectionId;
  if (!hasHistory) {
    showToast(`Switched this chat to ${aiToolLabel(providerId)}.`);
    return;
  }
  const connection = (driveConnectionsByProject.value[current.id] ?? []).find((item) => item.id === newConnectionId);
  if (!connection?.actions?.includes('send_turn')) {
    showToast(`Switched to ${aiToolLabel(providerId)}; it could not accept the transcript.`);
    return;
  }
  const seed = `Here is a transcript of my earlier conversation with ${aiToolLabel(from.tool)}, provided for context only. Please read it so you have the full history, but do not take any actions based on it yet — wait for my next message.\n\n${transcript}`;
  const turnSelection = {
    ...(descriptor.actions?.includes('change_model') && selection.model ? { model: selection.model } : {}),
    ...(descriptor.actions?.includes('change_effort') && selection.effort ? { effort: selection.effort } : {}),
  };
  beginConversation(newConnectionId, seed);
  requestAnimationFrame(() => {
    syncConversationScroll(document, true);
  });
  try {
    const updated = await new Api(current.apiPath).sendToolTurn(
      newConnectionId,
      seed,
      connection.session_id,
      turnSelection,
    );
    if (project()?.id === current.id)
      driveConnectionsByProject.value = {
        ...driveConnectionsByProject.value,
        [current.id]: (driveConnectionsByProject.value[current.id] ?? [])
          .filter((item) => item.id !== updated.id)
          .concat(updated),
      };
    showToast(`Re-seeded ${aiToolLabel(providerId)} with the ${aiToolLabel(from.tool)} transcript.`);
  } catch (reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    updateConversation(newConnectionId, (value) => ({
      ...value,
      activeAssistantId: undefined,
      progress: undefined,
      error: message,
      messages: value.messages.map((item) =>
        item.id === value.activeAssistantId
          ? {
              ...item,
              status: 'failed',
              content: item.content || 'The transcript could not be sent to the new provider.',
            }
          : item,
      ),
    }));
  }
}
function openManualModel(target: 'settings' | 'drive' | 'conversation' | 'command', commandId?: string) {
  const connectionId = conversationConnectionId.value;
  const command = commandId ? commandSettingsDefinitions().find((item) => item.id === commandId) : undefined,
    selection =
      target === 'drive'
        ? effectiveDriveSelection()
        : target === 'conversation' && connectionId
          ? conversationAiSelection(connectionId)
          : target === 'command' && command
            ? effectiveCommandAiSelection(command)
            : aiDefaults.value,
    descriptor = aiTools.value.find((item) => item.id === selection.tool),
    custom =
      selection.model && !descriptor?.models.some((model) => model.id === selection.model) ? selection.model : '';
  // The conversation model popup is a native wa-dropdown (no signal); close the open one so it
  // does not stay open behind — and after — the modal manual-model dialog (HS2-0W8QD9).
  if (target === 'conversation')
    document.querySelectorAll<Control>('.ai-conversation__model-menu[open]').forEach((menu) => menu.hide?.());
  else if (target === 'command')
    document
      .querySelectorAll<Control>('.command-settings-editor__ai-selection wa-dropdown[open]')
      .forEach((menu) => menu.hide?.());
  manualModelDialogShown = false;
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  manualModelDialog.value={target,providerName:descriptor?.display_name??selection.tool??'this provider',value:custom??'',...(commandId?{commandId}:{})};
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      const dialog = document.querySelector<Control>('[data-component="manual-model-dialog"]');
      dialog?.show?.();
      dialog?.querySelector<Control>('[name="manual-model"]')?.focus();
    }),
  );
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function restoreCommandEditorAfterManualModel(state:ManualModelDialogState|undefined){if(state?.target!=='command'||!state.commandId)return;commandSettingsEditingId.value=state.commandId;requestAnimationFrame(()=>requestAnimationFrame(()=>{(document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control)?.showPopover?.()}))}
function aiLaunchConfiguration(kind: 'ai-shell' | 'ai-chat', customize: boolean) {
  const current = project(),
    base = effectiveDriveSelection(current?.id);
  if (!current || !customize) return base;
  const provider = window.prompt('AI provider', base.tool);
  if (provider === null) return;
  const descriptor = aiTools.value.find(
    (item) =>
      item.id.toLowerCase() === provider.trim().toLowerCase() ||
      item.display_name.toLowerCase() === provider.trim().toLowerCase(),
  );
  if (!descriptor) {
    error.value = `Unknown AI provider: ${provider}`;
    return;
  }
  const initial = normalizedAiSelection({ tool: descriptor.id }),
    model = window.prompt('Model (leave blank for provider default)', initial.model ?? ''),
    selectedModel = descriptor.models.find((item) => item.id === model?.trim()),
    effort = selectedModel?.effort_levels?.length
      ? window.prompt('Effort level (leave blank for provider default)', initial.effort ?? '')
      : '';
  const selection = normalizedAiSelection({
      tool: descriptor.id,
      model: model?.trim() || undefined,
      effort: effort?.trim() || undefined,
    }),
    detail = {
      projectId: current.id,
      kind,
      provider: selection.tool,
      model: selection.model,
      effort: selection.effort,
    };
  document.dispatchEvent(new CustomEvent('hotsheet-ai-launch-configuration', { detail }));
  return selection;
}
async function createDrawerAIChat(selection: AiToolDefaults, options: { connectionId?: string; drive?: boolean } = {}) {
  const current = project();
  if (!current) return;
  const connectionId = options.connectionId ?? `hotsheet-drawer-chat-${crypto.randomUUID()}`,
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
function closeProjectIds(ids:readonly string[]){disposeProjectTicketReaders(ids);const closing=new Set(ids),before=projects.value,selectedIndex=before.findIndex(item=>item.id===selectedProjectId.value);let activation:ReturnType<typeof activateOpenProject>;for(const id of ids){projectTabRefresh.cancel(id);pendingCreatedTickets.forgetProject(id);projectsPendingActivation.delete(id)}projects.value=before.filter(item=>!closing.has(item.id));ticketRowsByProject.value=Object.fromEntries(Object.entries(ticketRowsByProject.value).filter(([id])=>!closing.has(id)));ticketCursorsByProject.value=Object.fromEntries(Object.entries(ticketCursorsByProject.value).filter(([id])=>!closing.has(id)));ticketCountsByProject.value=Object.fromEntries(Object.entries(ticketCountsByProject.value).filter(([id])=>!closing.has(id)));ticketTrendByProject.value=Object.fromEntries(Object.entries(ticketTrendByProject.value).filter(([id])=>!closing.has(id)));projectProjectionById.value=Object.fromEntries(Object.entries(projectProjectionById.value).filter(([id])=>!closing.has(id)));customViewsByProject.value=Object.fromEntries(Object.entries(customViewsByProject.value).filter(([id])=>!closing.has(id)));terminalDrawerChatsByProject.value=Object.fromEntries(Object.entries(terminalDrawerChatsByProject.value).filter(([id])=>!closing.has(id)));commandSettingsDraftsByProject.value=Object.fromEntries(Object.entries(commandSettingsDraftsByProject.value).filter(([id])=>!closing.has(id)));commandSettingsMessagesByProject.value=Object.fromEntries(Object.entries(commandSettingsMessagesByProject.value).filter(([id])=>!closing.has(id)));commandSettingsSelectedByProject.value=Object.fromEntries(Object.entries(commandSettingsSelectedByProject.value).filter(([id])=>!closing.has(id)));commandSettingsExtraGroupsByProject.value=Object.fromEntries(Object.entries(commandSettingsExtraGroupsByProject.value).filter(([id])=>!closing.has(id)));if(statsProjectId.value&&closing.has(statsProjectId.value))statsProjectId.value=undefined;if(closing.has(selectedProjectId.value)){resetTicketComposer();const next=projects.value.find(item=>before.indexOf(item)>selectedIndex)?.id??[...projects.value].reverse().find(item=>before.indexOf(item)<selectedIndex)?.id??projects.value[0]?.id??'';if(next)activation=activateOpenProject(next);else selectedProjectId.value='';if(!selectedProjectId.value&&projectRestoreFailures.value.length)selectedProjectRestoreRoot.value=projectRestoreFailures.value[0].root}defaultProviders.value=Object.fromEntries(Object.entries(defaultProviders.value).filter(([id])=>!closing.has(id)));driveConnectionsByProject.value=Object.fromEntries(Object.entries(driveConnectionsByProject.value).filter(([id])=>!closing.has(id)));drivePendingByProject.value=Object.fromEntries(Object.entries(drivePendingByProject.value).filter(([id])=>!closing.has(id)));localStorage.setItem('hotsheet.open-projects',JSON.stringify(currentRememberedProjectRoots()));syncProjectChangeStreams();commandDialogId.value=undefined;commandSettingsEditingId.value=undefined;if(activation)void refreshActivatedProject(activation,terminalDrawerVisible.value);else if(project())void Promise.all([refreshProject(),refreshCommands(),refreshCustomViews(),refreshDriveConnections()]);else{commandDefinitions.value=[];commandRuns.value=[]}}
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
function mountTerminalViewportElement(element: HTMLElement) {
  if (!element.isConnected || terminalViewportMounts.has(element)) return;
  stopObservingTerminalViewport(element);
  const projectId = element.dataset.projectId,
    terminalId = element.dataset.terminalId,
    current = projects.value.find((item) => item.id === projectId);
  if (!current || !terminalId) return;
  const interactive = element.dataset.displayMode === 'interactive',
    autoFocus =
      interactive &&
      (element.closest('.terminal-dashboard__magnified') !== null ||
        terminalViewportShouldAutoFocus(pendingTerminalFocus, current.id, terminalId));
  terminalViewportMounts.set(
    element,
    mountTerminalViewport(element, {
      url: terminalBrowserWebSocketUrl(current.apiPath, terminalId),
      autoFocus,
      onTicketReference: interactive
        ? (reference) => {
            const parsed = parseTicketLinkReference(reference);
            if (!parsed) return;
            ticketLinkReturnFocus = element;
            void selectLinkedTicket(parsed.slug, parsed.projectId, current.id);
          }
        : undefined,
    }),
  );
  if (autoFocus) pendingTerminalFocus = undefined;
}
function stopObservingTerminalViewport(element: HTMLElement) {
  const target = terminalViewportObservationTargets.get(element);
  if (target) {
    terminalViewportObserver?.unobserve(target);
    terminalViewportCandidatesByTarget.delete(target);
  }
  terminalViewportObservationTargets.delete(element);
  terminalViewportCandidates.delete(element);
}
function ensureTerminalViewportObserver() {
  if (terminalViewportObserver || typeof IntersectionObserver === 'undefined') return terminalViewportObserver;
  terminalViewportObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        const element = terminalViewportCandidatesByTarget.get(entry.target as HTMLElement);
        if (!element) continue;
        if (entry.isIntersecting) terminalViewportWork.enqueueMount(element);
        else terminalViewportWork.cancelMount(element);
      }
    },
    { rootMargin: '160px' },
  );
  return terminalViewportObserver;
}
function syncTerminalViewportMounts() {
  const elements = new Set(document.querySelectorAll<HTMLElement>('[data-component="terminal-viewport"]'));
  for (const [element, dispose] of terminalViewportMounts)
    if (!elements.has(element)) {
      terminalViewportMounts.delete(element);
      terminalViewportWork.enqueueDisposal(dispose);
    }
  for (const element of terminalViewportCandidates)
    if (!elements.has(element)) {
      terminalViewportWork.cancelMount(element);
      stopObservingTerminalViewport(element);
    }
  for (const element of elements) {
    if (terminalViewportMounts.has(element) || terminalViewportCandidates.has(element)) continue;
    if (element.dataset.mountPolicy !== 'visible-progressive') {
      mountTerminalViewportElement(element);
      continue;
    }
    terminalViewportCandidates.add(element);
    const observer = ensureTerminalViewportObserver(),
      target = element.closest<HTMLElement>('[data-component="terminal-tile"]') ?? element;
    if (observer) {
      terminalViewportObservationTargets.set(element, target);
      terminalViewportCandidatesByTarget.set(target, element);
      observer.observe(target);
    } else terminalViewportWork.enqueueMount(element);
  }
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
  ticketRowsByProject.value = { ...ticketRowsByProject.value, [id]: tickets.value };
  ticketCursorsByProject.value = { ...ticketCursorsByProject.value, [id]: ticketNextCursor.value };
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
function activateOpenProject(next:string):{project:Project;generation:number;cached:boolean}|undefined{const nextProject=projects.value.find(item=>item.id===next);if(!nextProject)return;projectTabRefresh.activate(next);const finishTiming=beginInteractionTiming('project-change',{project:next});persistProjectSessionNow();cacheActiveProjectProjection();resetTicketComposer(false);resetProgressiveTicketRendering();resetBoardColumnPages();const rows=ticketRowsByProject.value[next],cached=rows!==undefined,projection=projectProjectionById.value[next],stored=loadProjectWorkspaceSession(localStorage,next),generation=++projectActivationGeneration,live=new Set(rows?.map(ticket=>ticket.slug)??[]),selection=stored?.selectedTicketSlugs.filter(slug=>live.has(slug))??[],drawerActivation=terminalDrawerActivation(localStorage,next);batch(()=>{selectedProjectRestoreRoot.value='';selectedProjectId.value=drawerActivation.projectId;terminalDrawerSelected.value=drawerActivation.selectedId;tickets.value=rows??[];ticketNextCursor.value=ticketCursorsByProject.value[next];ticketCollectionState.value=undefined;corruptTickets.value=projection?.corruptTickets??[];repository.value=projection?.repository??null;repositoryError.value=projection?.repositoryError??'';commandDefinitions.value=projection?.commandDefinitions??[];commandRuns.value=projection?.commandRuns??[];commandSettingsEditingId.value=undefined;selectedView.value=stored?.selectedView==='errors'&&!projection?.corruptTickets.length?'all':stored?.selectedView??'all';searchOpen.value=stored?.searchOpen??false;searchQuery.value=stored?.searchQuery??'';searchMatchKeys.value=projection?.searchMatchKeys;selectedCorruptKey.value=undefined;selectedTicket.value=null;selectedTicketSlugs.value=selection;ticketSelectionAnchor=selection[0];error.value='';loading.value=!cached});scheduleClaimLeaseExpiry();saveActiveProjectRoot(localStorage,nextProject.root);finishTiming();return{project:nextProject,generation,cached}}
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
  const category = settingsCategory(),
    settingsRefresh =
      category === 'sources'
        ? refreshProviderConnections(current)
        : category === 'terminals'
          ? refreshTerminalSettings(current)
          : category === 'lifecycle'
            ? refreshTrashSettings(current)
            : Promise.resolve(),
    aiRefresh =
      category === 'ai' || aiConfigurationProjectId !== current.id
        ? refreshAiConfiguration(current)
        : Promise.resolve();
  await Promise.all([
    refreshProject({ showLoading: !cached }),
    refreshCommands(current),
    refreshCustomViews(current),
    settingsRefresh,
    aiRefresh,
    ...(firstActivation ? [refreshDriveConnections(current, true)] : []),
    ...(includeTerminals ? [refreshTerminalDashboard()] : []),
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
    selectedView.value = stored.selectedView === 'errors' && !corruptTickets.value.length ? 'all' : stored.selectedView;
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
        const files = await loadDraftFiles(draftScope('not-working', current.id), stored.notWorking.attachments).catch(
          () => [],
        );
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
  if (mobile === viewportMobile.value) return;
  viewportMobile.value = mobile;
  if (!mobile) mobileOverlay.value = MOBILE_OVERLAYS_CLOSED;
});
const ticketSnapshot = (slug: string) => tickets.value.find((item) => item.slug === slug) as TicketSnapshot | undefined;
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
  if (next !== undefined)
    claimLeaseExpiryTimer = window.setTimeout(
      () => {
        claimLeaseExpiryTimer = undefined;
        scheduleClaimLeaseExpiry();
      },
      Math.max(1, next - now + 25),
    );
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
  return { ...searchScopeQuery(view), text: advanced ? '' : effective.text, ...tokenQuery(serverTokens) };
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
        !searchTokens.value.some((token) => token.kind === 'tag' && token.value.toLowerCase() === tag.toLowerCase()) &&
        tag.toLowerCase().startsWith(prefix),
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
function updateTicketSearch(value: string, forceToken = false, currentTokens = searchTokens.value, parseTokens = true) {
  const parsed =
    !parseTokens || (workspaceSearchEditingToken && !forceToken)
      ? ({ text: value, tokens: [], removed: [] } as ReturnType<typeof consumeSearchTokens>)
      : consumeSearchTokens(value, forceToken);
  if (forceToken) workspaceSearchEditingToken = false;
  const shifted = currentTokens.map((token) => {
      const offset = token.offset ?? value.length,
        shift = parsed.removed.reduce((total, range) => total + (range.end <= offset ? range.end - range.start : 0), 0);
      return { ...token, offset: Math.max(0, offset - shift) };
    }),
    next: InlineSearchToken[] = [...shifted];
  for (const token of parsed.tokens)
    if (!next.some((value) => value.kind === token.kind && value.value === token.value)) next.push(token);
  if (sameInlineSearchState(searchQuery.value, searchTokens.value, parsed.text, next)) return parsed.tokens.length > 0;
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
async function openTicketLinkMatch(match:TicketLinkMatch){const target=projects.value.find(item=>item.id===match.projectId);if(!target){showToast(`No open project matches ${match.projectId}.`);return}ticketLinkChoice.value=undefined;const cached=ticketRowsByProject.value[target.id]??[],row=cached.find(item=>item.qualified_id===match.qualifiedId);if(row)ticketRowsByProject.value={...ticketRowsByProject.value,[target.id]:mergeTicketLinkRows(cached,[row])};try{const ticket=(await new Api(target.apiPath).checkoutTicket(target.id,match.ticketId)).ticket,capabilities=capabilitiesFor(ticket.connection_id);if(!capabilities)throw new Error(`Capabilities unavailable for ${ticket.connection_id}.`);const id=crypto.randomUUID(),trigger=ticketLinkReturnFocus;ticketLinkReturnFocus=undefined;linkedReaderStack.value=pushTicketReaderFrame(linkedReaderStack.value,{id,open:false,projectId:target.id,projectName:target.name,apiPath:target.apiPath,ticket,activeTab:'info',capabilities,edit:ticketReaderEditState(ticket)});presentTicketReaderDialog(id,trigger,()=>{replaceLinkedReaderFrame(id,frame=>({...frame,open:true}))})}catch(reason){error.value=reason instanceof Error?reason.message:String(reason)}}
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
        tickets: await new Api(item.apiPath).checkoutTickets(item.id, { text: reference.slug, compact: true }),
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

async function refreshProject({ showLoading = true }: { showLoading?: boolean } = {}) {
  const current = project(),
    generation = ++projectRefreshGeneration;
  if (!current) return;
  if (showLoading) loading.value = true;
  const active = () => generation === projectRefreshGeneration && project()?.id === current.id;
  try {
    const client = new Api(current.apiPath),
      query = ticketViewQuery(selectedView.value);
    const [index, repositoryResult] = await Promise.all([
      loadProjectTicketRefresh(client, current.id, query),
      client
        .repositoryStatus(current.id)
        .then((status) => ({ status, error: '' }))
        .catch((reason: unknown) => ({
          status: null,
          error: reason instanceof Error ? reason.message : String(reason),
        })),
    ]);
    if (!active()) return;
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
      if (!searchActive) {
        let finalTickets: WireTicketRow[] = mergedTickets;
        // Restore user-expanded board columns to their loaded length before committing, so a background refresh
        // doesn't reset a column's pagination to the baseline (HS2-8NBGBX). Computed then set once — no flash.
        if (viewMode.value === 'board' && !viewportMobile.value && isPerColumnBoardView(selectedView.value, false)) {
          const restored = await fetchExpandedBoardColumnRows(current, selectedView.value, mergedTickets);
          if (!active()) return;
          if (restored) {
            finalTickets = restored.rows;
            boardColumnPages.value = restored.pages;
          }
        }
        ticketPageQuery.value = query;
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
const BOARD_COLUMN_PAGE_SIZE = 100;
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
    const query = { ...ticketViewQuery(view), status: target.status };
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
// A background refresh replaces the flat baseline page, which would collapse any board column the user
// had paged past the baseline (the reported symptom). This fetches each expanded column back up to the
// length it had and returns the merged rows + updated per-column pages, so the caller can set the final
// state in one assignment (no collapse-then-expand flash). Only columns the user explicitly paged are
// restored (HS2-8NBGBX). Returns undefined when there is nothing to restore.
async function fetchExpandedBoardColumnRows(
  current: Project,
  view: TicketView,
  baseRows: readonly WireTicketRow[],
): Promise<{ rows: WireTicketRow[]; pages: Record<string, BoardColumnPage> } | undefined> {
  const hideVerified = hideVerifiedColumn(),
    previousRows = tickets.value,
    client = new Api(current.apiPath);
  const columnStatuses = (columnId: string) => boardColumnStatuses(columnId, hideVerified);
  const loadedAcross = (rows: readonly WireTicketRow[], statuses: readonly string[]) =>
    statuses.reduce((sum, status) => sum + countTicketsForStatus(rows, status), 0);
  // A column is expanded (needs restoring) when the user had paged it past what the fresh baseline
  // reloads. Each of its status streams is refetched only when its own rows shrank versus baseline,
  // so the merged Completed column restores `completed` and `verified` independently (HS2-F2N4ZN).
  const expanded = Object.entries(boardColumnPages.value).filter(([columnId]) => {
    const statuses = columnStatuses(columnId);
    return statuses.length && loadedAcross(previousRows, statuses) > loadedAcross(baseRows, statuses);
  });
  if (!expanded.length) return undefined;
  let rows = [...baseRows];
  const pages = { ...boardColumnPages.value };
  for (const [columnId, page] of expanded) {
    const statuses = columnStatuses(columnId),
      streams: Record<string, { cursor?: string; exhausted?: boolean }> = {};
    for (const status of statuses) {
      const want = countTicketsForStatus(previousRows, status);
      if (want <= countTicketsForStatus(baseRows, status)) {
        streams[status] = page.streams?.[status] ?? {};
        continue;
      }
      const result = await client
        .checkoutTicketPage(current.id, Math.max(BOARD_COLUMN_PAGE_SIZE, want), undefined, {
          ...ticketViewQuery(view),
          status,
        })
        .catch(() => undefined);
      if (result) {
        rows = appendUniqueTicketRows(rows, result.items);
        streams[status] = { cursor: result.next_cursor, exhausted: !result.next_cursor };
      } else streams[status] = page.streams?.[status] ?? {};
    }
    // prettier-ignore
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    pages[columnId]={loaded:loadedAcross(rows,statuses),exhausted:statuses.every(status=>streams[status]?.exhausted===true),streams};
  }
  return { rows, pages };
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
  loadBackground: async (target) => loadProjectTicketRefresh(new Api(target.apiPath), target.id),
  publishBackground: (target, snapshot) => {
    if (snapshot.tickets)
      ticketRowsByProject.value = {
        ...ticketRowsByProject.value,
        [target.id]: mergeRetainedCreatedRows(
          snapshot.tickets,
          pendingCreatedTickets.retain(target.id, snapshot.tickets),
        ),
      };
    if (snapshot.ticketCounts) {
      ticketCountsByProject.value = { ...ticketCountsByProject.value, [target.id]: snapshot.ticketCounts };
      recordAuthoritativeTicketTrend(target.id, snapshot.ticketCounts);
    }
    scheduleClaimLeaseExpiry();
  },
});
async function refreshRepositoryStatus() {
  const current = project();
  if (!current || repositoryRefreshing.value) return;
  repositoryRefreshing.value = true;
  try {
    const status = await new Api(current.apiPath).repositoryStatus(current.id);
    if (project()?.id === current.id) {
      repository.value = status;
      repositoryError.value = '';
      repositorySetupError.value = '';
      if (status.initialized === false) repositorySetupStep.value = 'initialize';
      else if (repositorySetupStep.value !== 'remote') repositorySetupStep.value = undefined;
      if (status.initialized !== false && document.querySelector('#repository-status-popover:popover-open'))
        void loadRepositoryDetail(repositoryView.value, true);
    }
  } catch (reason) {
    if (project()?.id === current.id) {
      repository.value = null;
      repositoryError.value = reason instanceof Error ? reason.message : String(reason);
    }
  } finally {
    if (project()?.id === current.id) repositoryRefreshing.value = false;
  }
}
async function initializeRepository() {
  const current = project();
  if (!current || repositorySetupBusy.value) return;
  repositorySetupBusy.value = true;
  repositorySetupError.value = '';
  try {
    const status = await new Api(current.apiPath).initializeRepository(current.id);
    if (project()?.id !== current.id) return;
    repository.value = status;
    repositoryError.value = '';
    repositorySetupStep.value = 'remote';
  } catch (reason) {
    if (project()?.id === current.id)
      repositorySetupError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    if (project()?.id === current.id) repositorySetupBusy.value = false;
  }
}
async function connectRepositoryRemote(form: HTMLFormElement) {
  const current = project();
  if (!current || repositorySetupBusy.value) return;
  const remote = (form.elements.namedItem('repository-remote') as HTMLInputElement | null)?.value.trim() ?? '';
  if (!remote) {
    repositorySetupError.value = 'Enter a remote URL.';
    return;
  }
  repositorySetupBusy.value = true;
  repositorySetupError.value = '';
  try {
    const status = await new Api(current.apiPath).configureRepositoryRemote(current.id, remote);
    if (project()?.id !== current.id) return;
    repository.value = status;
    repositoryError.value = '';
    repositorySetupStep.value = undefined;
    showToast('Origin remote added.');
    void loadRepositoryDetail(repositoryView.value, true);
  } catch (reason) {
    if (project()?.id === current.id)
      repositorySetupError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    if (project()?.id === current.id) repositorySetupBusy.value = false;
  }
}
function skipRepositoryRemote() {
  repositorySetupStep.value = undefined;
  repositorySetupError.value = '';
  showToast('Git initialized without a remote.');
  void loadRepositoryDetail(repositoryView.value, true);
}
async function loadRepositoryDetail(view: RepositoryStatusView, reset = false) {
  const current = project(),
    previous = repositoryDetail.value;
  if (!current) return;
  if (!reset && previous.view === view && (previous.loading || (previous.loaded && previous.nextCursor === undefined)))
    return;
  const generation = ++repositoryDetailGeneration,
    cursor = !reset && previous.view === view ? (previous.nextCursor ?? 0) : 0,
    base =
      !reset && previous.view === view
        ? previous
        : { view, files: [], commits: [], loading: false, loaded: false, error: '' };
  repositoryDetail.value = { ...base, view, loading: true, error: '' };
  try {
    const page =
      view === 'commits'
        ? await new Api(current.apiPath).repositoryCommits(current.id, cursor)
        : await new Api(current.apiPath).repositoryFiles(current.id, view, cursor);
    if (generation !== repositoryDetailGeneration || project()?.id !== current.id) return;
    repositoryDetail.value = {
      view,
      files: view === 'commits' ? base.files : [...base.files, ...(page.items as RepositoryFile[])],
      commits: view === 'commits' ? [...base.commits, ...(page.items as CodeReview['commits'])] : base.commits,
      nextCursor: page.next_cursor ?? undefined,
      loading: false,
      loaded: true,
      error: '',
    };
  } catch (reason) {
    if (generation !== repositoryDetailGeneration || project()?.id !== current.id) return;
    repositoryDetail.value = {
      ...base,
      view,
      loading: false,
      loaded: true,
      error: reason instanceof Error ? reason.message : String(reason),
    };
  }
}
function syncRepositoryPaginationObserver() {
  repositoryPaginationObserver?.disconnect();
  const root = document.querySelector('.repository-status-popover__detail'),
    sentinel = document.querySelector('[data-repository-pagination-sentinel="true"]');
  repositoryPaginationObserver = undefined;
  if (!root || !sentinel || repositoryDetail.value.loading) return;
  repositoryPaginationObserver = new IntersectionObserver(
    (entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void loadRepositoryDetail(repositoryDetail.value.view);
    },
    { root, rootMargin: '0px 0px 120px' },
  );
  repositoryPaginationObserver.observe(sentinel);
}
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
async function refreshCommands(current=project()){const generation=++commandRefreshGeneration;if(!current)return;const active=()=>generation===commandRefreshGeneration&&project()?.id===current.id;try{const client=new Api(current.apiPath),previous=JSON.stringify(commandDefinitions.value,null,2),[definitions,runs]=await Promise.all([client.commands(),client.commandRuns()]);if(!active())return;commandDefinitions.value=definitions;commandRuns.value=runs;const projection=projectProjectionById.value[current.id];projectProjectionById.value={...projectProjectionById.value,[current.id]:{corruptTickets:projection?.corruptTickets??corruptTickets.value,repository:projection?.repository??repository.value,repositoryError:projection?.repositoryError??repositoryError.value,commandDefinitions:definitions,commandRuns:runs}};const draft=commandSettingsDraftsByProject.value[current.id];if(viewMode.value!=='settings'||draft===undefined||draft===previous)setCommandSettingsDraft(current.id,JSON.stringify(definitions,null,2));setCommandSettingsMessage(current.id,'')}catch(reason){if(active())setCommandSettingsMessage(current.id,reason instanceof Error?reason.message:String(reason))}}
function updateCommandSetting(projectId: string, id: string, field: string, value: string) {
  const definitions = commandSettingsDefinitions(projectId),
    next = definitions.map((command) => {
      if (command.id !== id) return command;
      const updated: CommandDefinition = { ...command };
      if (field === 'args') updated.args = value.split('\n').filter((argument) => argument.length > 0);
      else if (field === 'kind') {
        updated.kind = value as NonNullable<CommandDefinition['kind']>;
        if (value === 'program') {
          delete updated.command;
          delete updated.prompt;
          delete updated.tool;
          delete updated.model;
          delete updated.effort;
        } else if (value === 'shell') {
          delete updated.program;
          delete updated.args;
          delete updated.prompt;
          delete updated.tool;
          delete updated.model;
          delete updated.effort;
        } else {
          delete updated.program;
          delete updated.args;
          delete updated.command;
        }
      } else if (field === 'id' || field === 'title') updated[field] = value;
      else if (
        [
          'program',
          'group',
          'cwd',
          'confirmation',
          'command',
          'prompt',
          'tool',
          'model',
          'effort',
          'color',
          'icon',
        ].includes(field)
      )
        (updated as unknown as Record<string, unknown>)[field] = value || undefined;
      return updated;
    });
  setCommandSettingsDefinitions(projectId, next);
  if (field === 'id') {
    selectCommandSetting(projectId, value);
    if (commandSettingsEditingId.value === id) commandSettingsEditingId.value = value;
  }
  scheduleCommandAutosave(projectId);
}
function updateCommandAiSelection(
  projectId: string,
  id: string,
  selection: Partial<Pick<CommandDefinition, 'tool' | 'model' | 'effort'>>,
) {
  const next = commandSettingsDefinitions(projectId).map((command) => {
    if (command.id !== id) return command;
    const updated = { ...command };
    if (selection.tool) updated.tool = selection.tool;
    else delete updated.tool;
    if (selection.model) updated.model = selection.model;
    else delete updated.model;
    if (selection.effort) updated.effort = selection.effort;
    else delete updated.effort;
    return updated;
  });
  setCommandSettingsDefinitions(projectId, next);
  scheduleCommandAutosave(projectId);
}
function addCommandSetting(projectId: string) {
  const existing = new Set(commandSettingsDefinitions(projectId).map((command) => command.id));
  let index = 1,
    id = 'command-1';
  while (existing.has(id)) id = `command-${++index}`;
  setCommandSettingsDefinitions(projectId, [
    ...commandSettingsDefinitions(projectId),
    { id, title: 'New command', kind: 'shell', command: '' },
  ]);
  selectCommandSetting(projectId, id);
  scheduleCommandAutosave(projectId);
  return id;
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function deleteCommandSetting(projectId:string,id:string){const definitions=commandSettingsDefinitions(projectId),index=definitions.findIndex(command=>command.id===id),next=definitions.filter(command=>command.id!==id);setCommandSettingsDefinitions(projectId,next);selectCommandSetting(projectId,next[Math.min(Math.max(index,0),next.length-1)]?.id);scheduleCommandAutosave(projectId);if(commandSettingsEditingId.value===id){commandSettingsEditingId.value=undefined;(document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control).hidePopover?.()}}
function commandSettingsExtraGroups(projectId = selectedProjectId.value) {
  return commandSettingsExtraGroupsByProject.value[projectId] ?? [];
}
function setCommandSettingsExtraGroups(projectId: string, groups: string[]) {
  commandSettingsExtraGroupsByProject.value = { ...commandSettingsExtraGroupsByProject.value, [projectId]: groups };
}
function reorderCommandSettings(projectId: string, sourceIds: readonly string[], target: CommandDropTarget) {
  const next = reorderCommandsMultiple(commandSettingsDefinitions(projectId), sourceIds, target);
  setCommandSettingsDefinitions(projectId, next);
  setCommandSettingsExtraGroups(projectId, emptyExtraGroups(next, commandSettingsExtraGroups(projectId)));
  scheduleCommandAutosave(projectId);
}
function addCommandGroup(projectId: string) {
  const name = window.prompt('New group name')?.trim();
  if (!name) return;
  const existing = new Set([
    ...commandSettingsDefinitions(projectId)
      .map((command) => command.group?.trim())
      .filter(Boolean),
    ...commandSettingsExtraGroups(projectId),
  ]);
  if (existing.has(name)) {
    setCommandSettingsMessage(projectId, `A group named "${name}" already exists.`);
    return;
  }
  setCommandSettingsExtraGroups(projectId, [...commandSettingsExtraGroups(projectId), name]);
}
function deleteCommandGroup(projectId: string, group: string) {
  setCommandSettingsExtraGroups(
    projectId,
    commandSettingsExtraGroups(projectId).filter((item) => item !== group),
  );
}
let commandAutosaveTimer: ReturnType<typeof setTimeout> | undefined;
function scheduleCommandAutosave(projectId: string) {
  if (commandAutosaveTimer) clearTimeout(commandAutosaveTimer);
  commandAutosaveTimer = setTimeout(() => {
    commandAutosaveTimer = undefined;
    void saveCommandSettings(projectId);
  }, 600);
}
async function saveCommandSettings(projectId: string) {
  const current = projects.value.find((item) => item.id === projectId);
  if (!current) return;
  const definitions = commandSettingsDefinitions(projectId),
    validation = commandSettingsValidation(definitions);
  if (validation) {
    setCommandSettingsMessage(projectId, validation);
    return;
  }
  setCommandSettingsMessage(projectId, 'Saving…');
  try {
    const saved = await new Api(current.apiPath).saveCommands(definitions);
    if (!projects.value.some((item) => item.id === projectId)) return;
    if (selectedProjectId.value === projectId) commandDefinitions.value = saved;
    setCommandSettingsMessage(projectId, 'Saved.');
  } catch (reason) {
    setCommandSettingsMessage(projectId, reason instanceof Error ? reason.message : String(reason));
  }
}
function commandSettingsValidation(definitions: CommandDefinition[]): string | undefined {
  const ids = new Set<string>();
  for (const [index, command] of definitions.entries()) {
    const label = command.title.trim() || `Command ${index + 1}`;
    if (!command.id.trim()) return `${label} needs an identifier.`;
    if (ids.has(command.id)) return `Command identifiers must be unique: ${command.id}.`;
    ids.add(command.id);
    if (!command.title.trim()) return `${command.id} needs a button label.`;
    const type = command.kind ?? 'program';
    if (type === 'program' && !command.program?.trim()) return `${label} needs a program.`;
    if (type === 'shell' && !command.command?.trim()) return `${label} needs a shell command.`;
    if (type === 'ai' && !command.prompt?.trim()) return `${label} needs an AI prompt.`;
  }
  return undefined;
}
async function refreshCustomViews(current = project()) {
  if (!current) return;
  try {
    const views = await new Api(current.apiPath).customViews();
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
async function refreshDriveConnections(current=project(),restoreDrawerTabs=false){if(!current)return;if(project()?.id===current.id&&aiConfigurationProjectId!==current.id)void refreshAiConfiguration(current);try{const client=new Api(current.apiPath),[active,sessions]=await Promise.all([client.activeToolConnections(),client.toolSessions().catch(()=>[])]),activeIds=new Set(active.map(connection=>connection.id)),connections=await recoverProjectConnections(client,active,sessions,current.id,current.root);if(projects.value.some(item=>item.id===current.id)){for(const connection of connections)if(!activeIds.has(connection.id)&&conversationStates.peek()[connection.id]?.activeAssistantId)updateConversation(connection.id,state=>applyConversationEvent(state,{type:'done',reason:'interrupted'}));driveConnectionsByProject.value={...driveConnectionsByProject.value,[current.id]:connections};if(restoreDrawerTabs)terminalDrawerChatsByProject.value={...terminalDrawerChatsByProject.value,[current.id]:restoreDrawerAIChats(connections,current.id,terminalDrawerChatsByProject.value[current.id],aiToolLabel)}}}catch{/* retain the last event-projected state while a project server reconnects */}}
async function refreshAiConfiguration(current = project(), refresh = false) {
  if (!current) return;
  aiSettingsLoading.value = true;
  try {
    const client = new Api(current.apiPath),
      tools = await client.aiTools(refresh),
      defaults = await client.aiSettings();
    if (project()?.id !== current.id) return;
    aiTools.value = tools;
    aiDefaults.value = defaults;
    aiConfigurationProjectId = current.id;
    aiSettingsMessage.value = '';
  } catch (reason) {
    aiConfigurationProjectId = '';
    if (project()?.id === current.id)
      aiSettingsMessage.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    if (project()?.id === current.id) aiSettingsLoading.value = false;
  }
}
async function saveAiDefaults(value: AiToolDefaults) {
  const current = project();
  if (!current) return;
  aiSettingsLoading.value = true;
  aiSettingsMessage.value = 'Saving…';
  try {
    const saved = await new Api(current.apiPath).saveAiSettings(value);
    if (project()?.id !== current.id) return;
    aiDefaults.value = saved;
    aiSettingsMessage.value = 'Saved locally.';
  } catch (reason) {
    if (project()?.id === current.id)
      aiSettingsMessage.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    if (project()?.id === current.id) aiSettingsLoading.value = false;
  }
}
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
  updateConversation(connectionId, (state) => beginConversationTurn(state, crypto.randomUUID(), content));
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function conversationAiSelection(connectionId:string){const current=project(),connection=current?(driveConnectionsByProject.value[current.id]??[]).find(item=>item.id===connectionId):undefined,chat=current?(terminalDrawerChatsByProject.value[current.id]??[]).find(item=>item.connectionId===connectionId):undefined,override=conversationSelections.value[connectionId],tool=chat?.tool??connection?.tool??aiDefaults.value.tool,descriptor=aiTools.value.find(item=>item.id===tool),model=override?.model??chat?.model??connection?.model??descriptor?.default_model,modelDescriptor=descriptor?.models.find(item=>item.id===model),efforts=modelDescriptor?.effort_levels??[],effort=compatibleAiEffort(efforts,override?.effort,chat?.effort,connection?.effort,descriptor?.default_effort);return{tool,descriptor,model,effort,efforts}}
function conversationSelectedMessages(connectionId: string, messages: readonly ConversationMessage[]) {
  const scope = conversationSelectionScopes.value[connectionId];
  return scope?.kind === 'range' ? [...selectedConversationMessages(messages, scope)] : [];
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function pickConversationMessage(connectionId:string,messageId:string){const messages=conversationStates.peek()[connectionId]?.messages??[],scope=conversationSelectionScopes.value[connectionId]??{kind:'all' as const},next=conversationExportScopeAfterMessagePick(messages,scope,messageId);conversationSelectionScopes.value={...conversationSelectionScopes.value,[connectionId]:next}}
function clearConversationSelection(connectionId: string) {
  conversationSelectionScopes.value = Object.fromEntries(
    Object.entries(conversationSelectionScopes.value).filter(([id]) => id !== connectionId),
  );
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function copyConversationSelection(connectionId:string){const messages=conversationSelectedMessages(connectionId,conversationStates.peek()[connectionId]?.messages??[]);if(!messages.length)return;const tool=aiToolLabel(conversationAiSelection(connectionId).tool);try{await navigator.clipboard.writeText(conversationTranscriptMarkdown({conversationId:connectionId,tool},messages));showToast(`${messages.length} selected message${messages.length===1?'':'s'} copied to clipboard.`)}catch(reason){error.value=`Copy failed: ${reason instanceof Error?reason.message:String(reason)}`}}
function updateConversationExportDraft(update: (draft: ConversationExportDraft) => ConversationExportDraft) {
  const state = conversationExportDialog.value;
  if (state) conversationExportDialog.value = { ...state, draft: update(state.draft), error: '' };
}
function openConversationExport() {
  const current = project(),
    connectionId = conversationConnectionId.value;
  if (!current || !connectionId) return;
  const state = conversationStates.peek()[connectionId] ?? EMPTY_CONVERSATION;
  if (!state.messages.length) return;
  const connection = (driveConnectionsByProject.value[current.id] ?? []).find((item) => item.id === connectionId),
    chat = (terminalDrawerChatsByProject.value[current.id] ?? []).find((item) => item.connectionId === connectionId),
    selection = conversationAiSelection(connectionId),
    tool = chat?.tool ?? connection?.tool ?? selection.tool,
    scope = conversationSelectionScopes.value[connectionId],
    selectedRange =
      scope?.kind === 'range' && selectedConversationMessages(state.messages, scope).length ? scope : undefined,
    draft = { ...defaultConversationExportDraft(), ...(selectedRange ? { scope: selectedRange } : {}) };
  conversationExportDialog.value = {
    source: {
      conversationId: chat?.sourceConversationId ?? connectionId,
      tool,
      projectId: current.id,
      sessionId: chat?.sourceSessionId ?? connection?.session_id,
      model: selection.model,
      effort: selection.effort,
      resumable: !chat?.readOnly,
    },
    messages: [...state.messages],
    activity: [...(state.activity ?? [])],
    draft,
    summaryAvailable: true,
    step: selectedRange ? 1 : 2,
    navigation: 'none',
    selectedRange,
  };
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function pickConversationExportDestination(){const state=conversationExportDialog.value;if(!state||state.busy)return;conversationExportDialog.value={...state,busy:true,error:''};try{const response=await fetch('/__hotsheet/conversation-exports/destination',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({suggestedName:suggestedConversationExportName(state.source.tool)})}),result=await response.json() as {destination?:ConversationExportDestination;error?:string};if(!response.ok)throw new Error(result.error??'Could not choose a conversation export destination.');if(conversationExportDialog.value){if(!result.destination){conversationExportDialog.value={...state,busy:false};return}conversationExportDialog.value={...state,busy:false,draft:{...state.draft,destination:result.destination,writeMode:'create'},error:''}}}catch(reason){if(conversationExportDialog.value)conversationExportDialog.value={...state,busy:false,error:reason instanceof Error?reason.message:String(reason)}}}
async function saveConversationExport() {
  const state = conversationExportDialog.value;
  if (!state || state.busy) return;
  try {
    const request = buildConversationExportRequest(state.source, state.messages, state.draft),
      messages = [...selectedConversationMessages(state.messages, state.draft.scope)];
    conversationExportDialog.value = { ...state, busy: true, error: '' };
    const assets = await conversationExportAssets(state.messages, state.draft),
      response = await fetch('/__hotsheet/conversation-exports/write', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ request, messages, activity: state.activity ?? [], assets }),
      }),
      result = (await response.json()) as ConversationExportWriteResult & { error?: string };
    if (!response.ok) throw new Error(result.error ?? 'Could not save the conversation.');
    conversationExportDialog.value = undefined;
    showToast(`Saved conversation revision ${result.manifest.revision}.`);
  } catch (reason) {
    if (conversationExportDialog.value)
      conversationExportDialog.value = {
        ...conversationExportDialog.value,
        busy: false,
        error: reason instanceof Error ? reason.message : String(reason),
      };
  }
}
async function finishConversationExport() {
  const state = conversationExportDialog.value;
  if (!state || state.busy) return;
  if (!state.draft.destination) {
    await pickConversationExportDestination();
    const selected = conversationExportDialog.value;
    if (!selected?.draft.destination || selected.draft.destination.existing) return;
  }
  await saveConversationExport();
}
async function openSavedConversation() {
  const current = project();
  if (!current) return;
  terminalDrawerCreateMenuOpen.value = false;
  try {
    const response = await fetch('/__hotsheet/conversation-exports/open', { method: 'POST' }),
      result = (await response.json()) as { conversation?: ConversationExportOpenResult; error?: string };
    if (!response.ok) throw new Error(result.error ?? 'Could not open the saved conversation.');
    if (!result.conversation) return;
    const saved = result.conversation,
      source = saved.manifest.source,
      reopen = saved.manifest.reopen,
      canResume = Boolean(
        reopen.resumesOriginalSession && reopen.sessionId && (!source.projectId || source.projectId === current.id),
      );
    let connectionId = `hotsheet-saved-chat-${crypto.randomUUID()}`,
      readOnly = true,
      localOnly = true,
      resumeError = '';
    if (canResume) {
      try {
        const created = await new Api(current.apiPath).createToolConnection({
          tool: source.tool,
          checkout: current.id,
          connection_id: connectionId,
          session_id: reopen.sessionId,
          model: source.model,
          effort: source.effort,
        });
        connectionId = created.id;
        localOnly = false;
        driveConnectionsByProject.value = {
          ...driveConnectionsByProject.value,
          [current.id]: (driveConnectionsByProject.value[current.id] ?? [])
            .filter((item) => item.id !== created.id)
            .concat(created),
        };
        readOnly = !created.actions?.includes('send_turn');
      } catch (reason) {
        resumeError = reason instanceof Error ? reason.message : String(reason);
      }
    }
    const tab: DrawerAIChat = {
      id: `ai-chat:${connectionId}`,
      connectionId,
      tool: source.tool,
      name: `${aiToolLabel(source.tool)} saved chat`,
      model: source.model,
      effort: source.effort,
      readOnly,
      localOnly,
      savedSource: saved.displayPath,
      sourceConversationId: source.conversationId,
      sourceSessionId: source.sessionId,
    };
    terminalDrawerChatsByProject.value = {
      ...terminalDrawerChatsByProject.value,
      [current.id]: [...(terminalDrawerChatsByProject.value[current.id] ?? []), tab],
    };
    persistTerminalVisibility(hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${tab.id}`));
    replaceConversationStates({
      ...conversationStates.peek(),
      [connectionId]: { messages: saved.messages, activity: saved.activity },
    });
    selectDrawerItem(tab.id);
    setTerminalDrawerVisible(true);
    showToast(
      readOnly
        ? resumeError
          ? `Opened read-only; resume failed: ${resumeError}`
          : 'Opened saved conversation read-only.'
        : 'Opened saved conversation; you can continue it.',
    );
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  }
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
async function refreshTerminalSettings(current = project()) {
  if (!current) return;
  try {
    const value = await new Api(current.apiPath).terminalSettings();
    if (project()?.id !== current.id) return;
    inheritGlobalShellHistory.value = value.inherit_global_shell_history;
    terminalSettingsMessage.value = '';
  } catch (reason) {
    if (project()?.id === current.id)
      terminalSettingsMessage.value = reason instanceof Error ? reason.message : String(reason);
  }
}
async function refreshTrashSettings(current = project()) {
  if (!current) return;
  trashSettingsMessagesByProject.value = { ...trashSettingsMessagesByProject.value, [current.id]: 'Loading…' };
  try {
    const value = await new Api(current.apiPath).trashSettings(current.id);
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
async function refreshCodeReview() {
  const current = project(),
    ticket = selectedTicket.value;
  if (!current || !ticket) return;
  codeReviewLoading.value = true;
  codeReviewMessage.value = '';
  try {
    const review = await new Api(current.apiPath).codeReview(current.id, ticket.id);
    if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) codeReview.value = review;
  } catch (reason) {
    if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) {
      codeReview.value = undefined;
      codeReviewMessage.value = reason instanceof Error ? reason.message : String(reason);
    }
  } finally {
    if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) codeReviewLoading.value = false;
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
              updateConversation(event.turn.connection_id, (state) => applyConversationEvent(state, event.turn!.event));
            if (event.kind === 'activity' && event.activity) {
              const activity = event.activity,
                connection = conversationForActivity(current, activity.tool, activity.session);
              if (connection) updateConversation(connection.id, (state) => applyConversationActivity(state, activity));
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
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function refreshPermissions(){if(permissionPolling)return;permissionPolling=true;try{let changed=false;await Promise.all(projects.value.map(async current=>{const client=new Api(current.apiPath);const [requests,connections]=await Promise.all([client.permissions(),client.activeToolConnections().catch(()=>[])]),owned=requests.filter(request=>permissionBelongsToProject(request,connections,projects.value,current.id));changed=permissionInbox.reconcile(current,owned,connections)||changed}));if(changed){updatePermissionTimer();permissionRevision.value+=1;persistPermissionHistory()}}catch{/* individual server disconnects remain represented by their last known requests */}finally{permissionPolling=false}}
function startPermissionUpdates() {
  if (permissionTimerInterval === undefined) permissionTimerInterval = window.setInterval(updatePermissionTimer, 1_000);
  void refreshPermissions();
}
async function resolvePermission(
  item: PermissionItem,
  decision: PermissionDecision,
  scope: PermissionScope,
  automatic = false,
) {
  const finishTiming = beginInteractionTiming('permission-decision', { project: item.projectId }),
    owning = projects.value.find((value) => value.id === item.projectId);
  if (!owning) return;
  permissionResolutionErrors.value = Object.fromEntries(
    Object.entries(permissionResolutionErrors.value).filter(([key]) => key !== item.key),
  );
  if (!permissionInbox.resolve(item.key, decision, scope, automatic)) return;
  persistPermissionHistory();
  permissionTimer.remove(item.key);
  updatePermissionTimer();
  permissionRevision.value += 1;
  finishTiming();
  try {
    await new Api(owning.apiPath).resolvePermission(item.id, decision, scope);
  } catch (reason) {
    permissionInbox.restore(item);
    persistPermissionHistory();
    permissionResolutionErrors.value = {
      ...permissionResolutionErrors.value,
      [item.key]:
        reason instanceof Error && reason.message.includes('404')
          ? 'This request changed before Hot Sheet could answer it. Review it and try again.'
          : `Could not send the permission decision. ${reason instanceof Error ? reason.message : String(reason)}`,
    };
    updatePermissionTimer();
    permissionRevision.value += 1;
    if (reason instanceof Error && reason.message.includes('404')) await refreshPermissions();
  }
}
function updatePermissionTimer() {
  const item = visiblePermission();
  if (!item) {
    permissionTimer.hide();
    permissionCountdown = undefined;
    return;
  }
  const setting = permissionAutomation(item.projectId);
  if (setting.action === 'off') {
    permissionTimer.hide();
    permissionCountdown = undefined;
    return;
  }
  const remaining = permissionTimer.tick(item.key, setting.delayMs);
  permissionCountdown = remaining === undefined ? undefined : { key: item.key, remainingMs: remaining };
  if (remaining !== undefined) updatePermissionCountdownText(document, item.key, formatPermissionCountdown(remaining));
  if (remaining === 0) void resolvePermission(item, setting.action, 'once', true);
}
async function applyTicketPatch(slug: string, patch: TicketPatch) {
  const current = project(),
    ticket = tickets.value.find((item) => item.slug === slug);
  if (!current || !ticket) return false;
  const interaction = Object.hasOwn(patch, 'up_next')
      ? 'ticket-up-next-change'
      : Object.hasOwn(patch, 'status')
        ? 'ticket-status-change'
        : 'ticket-change',
    finishTiming = beginInteractionTiming(interaction, { slug }),
    releaseRefresh = beginLocalTicketMutation();
  const selectedBefore = selectedTicket.value?.slug === slug ? selectedTicket.value : null,
    started = performance.now(),
    generation = (mutationGenerations.get(slug) ?? 0) + 1;
  let rollbackRow = ticket,
    rollbackSelected = selectedBefore;
  mutationGenerations.set(slug, generation);
  batch(() => {
    tickets.value = tickets.value.map((item) => (item.slug === slug ? projectTicketPatch(item, patch) : item));
    publishOptimisticTicketRows(current.id);
    if (selectedBefore) selectedTicket.value = projectTicketPatch(selectedBefore, patch);
  });
  const optimistic = performance.now() - started;
  finishTiming();
  // Serialize the network section per ticket so the user's own rapid sequential edits each base off the
  // previous edit's committed token (last-write-wins) instead of self-conflicting (HS2-K9SG2R).
  return singleTicketMutationSequencer.enqueue(slug, async () => {
    try {
      // Base off the last edit this client committed for the ticket (its up-to-date token), falling back to
      // the pre-edit selection or a fresh fetch. A real external write still fails the token check below.
      const base =
        committedTickets.get(slug) ?? selectedBefore ?? (await api().checkoutTicket(current.id, ticket.id)).ticket;
      let updated: FullTicket;
      try {
        updated = (
          await api().updateCheckoutTicket(
            current.id,
            ticket.id,
            base.concurrency_token ? { ...patch, expected_token: base.concurrency_token } : patch,
          )
        ).ticket;
        localTicketChangeAcknowledgements.acknowledge(current.id, {
          store: updated.connection_id,
          id: updated.id,
          kind: 'updated',
        });
      } catch (reason) {
        if (!isTicketConcurrencyConflict(reason)) throw reason;
        const remote = (await api().checkoutTicket(current.id, ticket.id)).ticket,
          reconciled = reconcileTicketPatch(base, remote, patch);
        committedTickets.set(slug, remote);
        rollbackRow = ticketRowFromFull(ticket, remote);
        rollbackSelected = remote;
        if (mutationGenerations.get(slug) !== generation) return true;
        tickets.value = tickets.value.map((item) => (item.slug === slug ? ticketRowFromFull(item, remote) : item));
        publishOptimisticTicketRows(current.id);
        if (selectedTicket.value?.slug === slug) reconcileRefreshedSelected(base, remote);
        const conflict = reconciled.conflicts[0];
        // prettier-ignore
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
        if(conflict){showFieldConflict(conflict);reportMutationTiming({slug,optimistic_ms:optimistic,request_ms:performance.now()-started,outcome:'rolled_back'});return false}
        if (Object.keys(reconciled.retry).length === 0) updated = remote;
        else {
          updated = (
            await api().updateCheckoutTicket(
              current.id,
              ticket.id,
              remote.concurrency_token
                ? { ...reconciled.retry, expected_token: remote.concurrency_token }
                : reconciled.retry,
            )
          ).ticket;
          localTicketChangeAcknowledgements.acknowledge(current.id, {
            store: updated.connection_id,
            id: updated.id,
            kind: 'updated',
          });
        }
      }
      // Record the committed token even when this edit is stale for UI purposes: its server write advanced
      // the token, so the next queued edit must base off it.
      committedTickets.set(slug, updated);
      if (mutationGenerations.get(slug) !== generation) {
        reportMutationTiming({
          slug,
          optimistic_ms: optimistic,
          request_ms: performance.now() - started,
          outcome: 'stale',
        });
        return true;
      }
      tickets.value = tickets.value.map((item) => (item.slug === slug ? ticketRowFromFull(item, updated) : item));
      publishOptimisticTicketRows(current.id);
      if (selectedTicket.value?.slug === slug) selectedTicket.value = updated;
      recordCommittedDraftBases(patch);
      error.value = updated.warnings?.join('\n') ?? '';
      reportMutationTiming({
        slug,
        optimistic_ms: optimistic,
        request_ms: performance.now() - started,
        outcome: 'committed',
      });
      return true;
    } catch (reason) {
      if (mutationGenerations.get(slug) === generation) {
        tickets.value = tickets.value.map((item) => (item.slug === slug ? rollbackRow : item));
        publishOptimisticTicketRows(current.id);
        if (rollbackSelected && selectedTicket.value?.slug === slug) selectedTicket.value = rollbackSelected;
        error.value = reason instanceof Error ? reason.message : String(reason);
        reportMutationTiming({
          slug,
          optimistic_ms: optimistic,
          request_ms: performance.now() - started,
          outcome: 'rolled_back',
        });
      }
      return false;
    } finally {
      releaseRefresh();
    }
  });
}
async function updateSelected(patch: Record<string, unknown>) {
  const linked = linkedReaderStack.value.at(-1);
  return linked
    ? updateLinkedReader(linked.id, patch)
    : selectedTicket.value
      ? applyTicketPatch(selectedTicket.value.slug, patch)
      : false;
}
function history(projectId = project()?.id ?? '') {
  let value = histories.get(projectId);
  if (!value) {
    value = new TicketHistory(ticketSnapshot, applyTicketPatch);
    histories.set(projectId, value);
  }
  return value;
}
function recordCommittedDraftBases(patch: TicketPatch) {
  if (typeof patch.details === 'string' && detailsMode.value === 'write') detailsDraftBase = patch.details;
  if (typeof patch.details === 'string' && readerDetailsMode.value === 'write') readerDetailsDraftBase = patch.details;
  if (typeof patch.title === 'string' && titleEditing.value) titleDraftBase = patch.title;
  if (Object.hasOwn(patch, 'blocked_reason') && blockedReasonEditing.value)
    blockedReasonDraftBase = typeof patch.blocked_reason === 'string' ? patch.blocked_reason : '';
  if (Object.hasOwn(patch, 'blocked_reason') && readerBlockedReasonEditing.value)
    readerBlockedReasonDraftBase = typeof patch.blocked_reason === 'string' ? patch.blocked_reason : '';
  if (typeof patch.note_id === 'string' && patch.note_id === editingNoteId.value && typeof patch.note === 'string')
    noteDraftBase = patch.note;
  if (
    typeof patch.note_id === 'string' &&
    patch.note_id === readerEditingNoteId.value &&
    typeof patch.note === 'string'
  )
    readerNoteDraftBase = patch.note;
}
async function updateSelectedTracked(patch: TicketPatch) {
  const linked = linkedReaderStack.value.at(-1);
  return linked
    ? updateLinkedReader(linked.id, patch)
    : selectedTicket.value
      ? history().execute(selectedTicket.value.slug, patch)
      : false;
}
const detailsAutosave = createDebouncedAutosave((value: string) => updateSelectedTracked({ details: value }));
const readerDetailsAutosave = createDebouncedAutosave((value: string) => updateSelectedTracked({ details: value }));
const noteAutosave = createDebouncedAutosave(({ id, value }: { id: string; value: string }) =>
  updateSelected({ note_id: id, note: value }),
);
const readerNoteAutosave = createDebouncedAutosave(({ id, value }: { id: string; value: string }) =>
  updateSelected({ note_id: id, note: value }),
);
const blockedReasonAutosave = createDebouncedAutosave((value: string) =>
  updateSelected({ blocked_reason: value.trim() || null }),
);
const readerBlockedReasonAutosave = createDebouncedAutosave((value: string) =>
  updateSelected({ blocked_reason: value.trim() || null }),
);
const titleAutosave = createDebouncedAutosave((value: string) => updateSelectedTracked({ title: value.trim() }));
const tagsAutosave = createDebouncedAutosave((value: string[]) => updateSelectedTracked({ tags: value }));
function linkedReaderFrame(target: Element) {
  const id = target.closest<HTMLElement>('[data-reader-frame-id]')?.dataset.readerFrameId;
  return id && id !== 'workspace-reader' ? linkedReaderStack.value.find((frame) => frame.id === id) : undefined;
}
function replaceLinkedReaderFrame(id: string, update: (frame: TicketReaderFrame) => TicketReaderFrame) {
  linkedReaderStack.value = updateTicketReaderFrame(linkedReaderStack.value, id, update);
}
async function updateLinkedReader(frameId: string, patch: Record<string, unknown>) {
  const frame = linkedReaderStack.value.find((item) => item.id === frameId);
  if (!frame || !frame.capabilities.update) return false;
  const client = new Api(frame.apiPath);
  try {
    const ticket = (
      await client.updateCheckoutTicket(
        frame.projectId,
        frame.ticket.id,
        frame.ticket.concurrency_token ? { ...patch, expected_token: frame.ticket.concurrency_token } : patch,
      )
    ).ticket;
    replaceLinkedReaderFrame(frameId, (current) => reconcileTicketReaderFrame(current, ticket));
    return true;
  } catch (reason) {
    if (isTicketConcurrencyConflict(reason)) {
      const ticket = (await client.checkoutTicket(frame.projectId, frame.ticket.id)).ticket;
      replaceLinkedReaderFrame(frameId, (current) => reconcileTicketReaderFrame(current, ticket));
      error.value = 'This linked ticket changed remotely. Your draft was preserved; review it and try again.';
    } else error.value = reason instanceof Error ? reason.message : String(reason);
    return false;
  }
}
const linkedReaderAutosaves = new Map<
  string,
  {
    details: DebouncedAutosave<string>;
    note: DebouncedAutosave<{ id: string; value: string }>;
    blocked: DebouncedAutosave<string>;
  }
>();
function linkedReaderSaves(id: string) {
  let saves = linkedReaderAutosaves.get(id);
  if (!saves) {
    saves = {
      details: createDebouncedAutosave((value) => updateLinkedReader(id, { details: value })),
      note: createDebouncedAutosave(({ id: noteId, value }) =>
        updateLinkedReader(id, { note_id: noteId, note: value }),
      ),
      blocked: createDebouncedAutosave((value) => updateLinkedReader(id, { blocked_reason: value.trim() || null })),
    };
    linkedReaderAutosaves.set(id, saves);
  }
  return saves;
}
async function flushLinkedReader(frameId: string) {
  const saves = linkedReaderAutosaves.get(frameId);
  if (!saves) return true;
  const results = await Promise.all([saves.details.flush(), saves.note.flush(), saves.blocked.flush()]);
  return results.every(Boolean);
}
function selectedRows() {
  const selected = new Set(selectedTicketSlugs.value);
  return tickets.value.filter((ticket) => selected.has(ticket.slug));
}
function pruneSelectionForCurrentView() {
  const next = selectionVisibleInView(tickets.value, selectedTicketSlugs.value, selectedView.value);
  if (next.length === selectedTicketSlugs.value.length) return;
  selectedTicketSlugs.value = next;
  if (ticketSelectionAnchor && !next.includes(ticketSelectionAnchor)) ticketSelectionAnchor = next[0];
  if (selectedTicket.value && !next.includes(selectedTicket.value.slug)) {
    cancelTicketDrafts();
    selectedTicket.value = null;
  }
  if (next.length !== 1) selectedTicket.value = null;
}
interface BulkOperation {
  slug: string;
  id: string;
  patch: TicketPatch;
}
interface BulkApplyResult {
  complete: boolean;
  succeeded: Set<string>;
}
function setProjectTicketRows(projectId: string, rows: WireTicketRow[]) {
  if (!projects.value.some((item) => item.id === projectId)) return;
  if (project()?.id === projectId) tickets.value = rows;
  ticketRowsByProject.value = { ...ticketRowsByProject.value, [projectId]: rows };
  ticketCountsByProject.value = Object.fromEntries(
    Object.entries(ticketCountsByProject.value).filter(([id]) => id !== projectId),
  );
}
function reportBulkFailure(current: Project, message: string) {
  if (project()?.id === current.id) error.value = message;
  else showToast(`${current.name}: ${message}`);
}
async function applyBulkOperations(current: Project, operations: BulkOperation[]): Promise<BulkApplyResult> {
  if (operations.length === 0) return { complete: false, succeeded: new Set<string>() };
  const finishTiming = beginInteractionTiming('bulk-ticket-change', { count: operations.length, project: current.id }),
    releaseRefresh = beginLocalTicketMutation(),
    slugs = new Set(operations.map((item) => item.slug)),
    before = projectTabTicketRows(current.id).filter((ticket) => slugs.has(ticket.slug)),
    selectedBefore = project()?.id === current.id ? selectedTicket.value : null;
  setProjectTicketRows(
    current.id,
    projectTabTicketRows(current.id).map((ticket) => {
      const operation = operations.find((item) => item.slug === ticket.slug);
      return operation ? projectTicketPatch(ticket, operation.patch) : ticket;
    }),
  );
  if (project()?.id === current.id && selectedTicket.value) {
    const operation = operations.find((item) => item.slug === selectedTicket.value?.slug);
    if (operation) selectedTicket.value = projectTicketPatch(selectedTicket.value, operation.patch);
  }
  finishTiming();
  try {
    const client = new Api(current.apiPath),
      requestOperations = operations.map((operation) => {
        const ticket = before.find((item) => item.slug === operation.slug),
          token = selectedBefore?.slug === operation.slug ? selectedBefore.concurrency_token : ticket?.updated_at;
        return { ...operation, patch: token ? { ...operation.patch, expected_token: token } : operation.patch };
      }),
      atomic = canAtomicallyBulkUpdate(before, capabilitiesFor),
      updated: FullTicket[] = [],
      failures: Array<{ slug: string; message: string }> = [];
    if (atomic)
      updated.push(
        ...(await client.batchUpdateCheckoutTickets(
          current.id,
          requestOperations.map(({ id, patch }) => ({ id, patch })),
        )),
      );
    else {
      showToast(`Updating tickets… 0 of ${requestOperations.length}`);
      for (const operation of requestOperations) {
        try {
          updated.push((await client.updateCheckoutTicket(current.id, operation.id, operation.patch)).ticket);
        } catch (reason) {
          failures.push({ slug: operation.slug, message: reason instanceof Error ? reason.message : String(reason) });
        }
        showToast(`Updating tickets… ${updated.length + failures.length} of ${requestOperations.length}`);
      }
    }
    for (const ticket of updated)
      localTicketChangeAcknowledgements.acknowledge(current.id, {
        store: ticket.connection_id,
        id: ticket.id,
        kind: 'updated',
      });
    const succeeded = new Set(updated.map((item) => item.slug));
    let next = projectTabTicketRows(current.id).map((ticket) => {
      const match = updated.find((item) => item.id === ticket.id);
      return match ? ticketRowFromFull(ticket, match) : ticket;
    });
    if (failures.length)
      next = next.map((ticket) =>
        failures.some((item) => item.slug === ticket.slug)
          ? (before.find((item) => item.slug === ticket.slug) ?? ticket)
          : ticket,
      );
    setProjectTicketRows(current.id, next);
    if (project()?.id === current.id) {
      const selectedUpdate = selectedTicket.value && updated.find((item) => item.id === selectedTicket.value?.id);
      if (selectedUpdate) selectedTicket.value = selectedUpdate;
      else if (selectedBefore && failures.some((item) => item.slug === selectedBefore.slug))
        selectedTicket.value = selectedBefore;
      pruneSelectionForCurrentView();
    }
    const failure = failures.length
      ? `Updated ${updated.length} of ${operations.length} tickets. ${failures.length} failed; ${failures[0].slug}: ${failures[0].message}`
      : '';
    if (failure) reportBulkFailure(current, failure);
    else if (project()?.id === current.id) error.value = '';
    return { complete: failures.length === 0, succeeded };
  } catch (reason) {
    setProjectTicketRows(
      current.id,
      projectTabTicketRows(current.id).map((ticket) => before.find((item) => item.slug === ticket.slug) ?? ticket),
    );
    if (project()?.id === current.id && selectedBefore && slugs.has(selectedBefore.slug))
      selectedTicket.value = selectedBefore;
    reportBulkFailure(current, reason instanceof Error ? reason.message : String(reason));
    return { complete: false, succeeded: new Set<string>() };
  } finally {
    releaseRefresh();
  }
}
/** Restore Trash tickets through the server so each returns to its pre-deletion status. */
async function restoreTrashedTickets(targetSlugs: readonly string[]) {
  const current = project();
  if (!current) return;
  const api = new Api(current.apiPath),
    rows = projectTabTicketRows(current.id).filter(
      (ticket) => targetSlugs.includes(ticket.slug) && isTrashedTicket(ticket),
    );
  try {
    for (const row of rows) await api.restoreCheckoutTicket(current.id, row.id);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  }
  await refreshProject({ showLoading: false });
}
function executeBulkTicketAction(action: BulkTicketAction, targetSlugs = selectedTicketSlugs.value) {
  const requestedProject = project(),
    requestedSlugs = [...targetSlugs];
  bulkTicketDialog.value = undefined;
  bulkTicketSlugs = [];
  if (!requestedProject) return Promise.resolve(false);
  return bulkTicketMutationSequencer.enqueue(requestedProject.id, async () => {
    const selected = projectTabTicketRows(requestedProject.id).filter((ticket) => requestedSlugs.includes(ticket.slug));
    if (!canBulkUpdate(selected, capabilitiesFor)) {
      reportBulkFailure(requestedProject, 'One or more selected ticket providers do not support ticket updates.');
      return false;
    }
    const operations: BulkOperation[] = selected.flatMap((ticket) => {
      const patch = bulkTicketPatch(ticket, action);
      return patch ? [{ slug: ticket.slug, id: ticket.id, patch }] : [];
    });
    if (operations.length === 0) return true;
    const inverse = operations.map((operation) => {
        const ticket = selected.find((item) => item.slug === operation.slug)!;
        const keys = new Set(Object.keys(operation.patch));
        if ('status' in operation.patch) keys.add('up_next');
        return {
          slug: operation.slug,
          id: operation.id,
          patch: Object.fromEntries([...keys].map((key) => [key, ticket[key as keyof typeof ticket]])),
        };
      }),
      result = await applyBulkOperations(requestedProject, operations),
      succeededOperations = operations.filter((operation) => result.succeeded.has(operation.slug)),
      succeededInverse = inverse.filter((operation) => result.succeeded.has(operation.slug));
    if (succeededOperations.length)
      history(requestedProject.id).recordExternal(
        async () => (await applyBulkOperations(requestedProject, succeededInverse)).complete,
        async () => (await applyBulkOperations(requestedProject, succeededOperations)).complete,
      );
    return result.complete;
  });
}
function openEmptyTrash() {
  const current = project();
  if (!current) return;
  const count = projectTicketCounts(current.id).trash ?? 0;
  if (count > 0) bulkTicketDialog.value = { kind: 'empty-trash', count };
}
async function emptyTrash() {
  const current = project(),
    dialog = bulkTicketDialog.value;
  if (!current || dialog?.kind !== 'empty-trash' || dialog.busy) return;
  const beforeRows = projectTabTicketRows(current.id),
    beforeCounts = projectTicketCounts(current.id),
    remaining = beforeRows.filter((ticket) => !isTrashedTicket(ticket));
  bulkTicketDialog.value = undefined;
  setProjectTicketRows(current.id, remaining);
  ticketCountsByProject.value = {
    ...ticketCountsByProject.value,
    [current.id]: { ...beforeCounts, total: Math.max(0, beforeCounts.total - dialog.count), trash: 0 },
  };
  selectedTicketSlugs.value = [];
  selectedTicket.value = null;
  ticketSelectionAnchor = undefined;
  selectTicketView('all', { refresh: false });
  ticketCollectionState.value = { projectId: current.id, view: 'all', status: 'loading' };
  scheduleClaimLeaseExpiry();
  try {
    const result = await new Api(current.apiPath).emptyCheckoutTrash(current.id);
    if (project()?.id === current.id) await refreshTicketCollection('all');
    showToast(`Emptied Trash — ${result.purged} ticket${result.purged === 1 ? '' : 's'} permanently removed.`);
  } catch (reason) {
    if (!projects.value.some((item) => item.id === current.id)) return;
    setProjectTicketRows(current.id, beforeRows);
    ticketCountsByProject.value = { ...ticketCountsByProject.value, [current.id]: beforeCounts };
    const message = reason instanceof Error ? reason.message : String(reason);
    if (project()?.id === current.id) {
      selectTicketView('trash', { refresh: false });
      await refreshTicketCollection('trash');
      if (project()?.id === current.id) error.value = `Empty Trash failed: ${message}`;
    } else showToast(`${current.name}: Empty Trash failed — ${message}`);
  }
}
function openBulkTicketDialog(kind: 'add-tag' | 'remove-tag' | 'delete', targetSlugs = selectedTicketSlugs.value) {
  const selected = tickets.value.filter((ticket) => targetSlugs.includes(ticket.slug));
  if (!canBulkUpdate(selected, capabilitiesFor)) return;
  bulkTicketSlugs = selected.map((ticket) => ticket.slug);
  bulkTicketDialog.value =
    kind === 'delete'
      ? { kind: 'delete', count: selected.length }
      : {
          kind: 'tag',
          mode: kind === 'add-tag' ? 'add' : 'remove',
          count: selected.length,
          choices: bulkTagChoices(selected),
        };
  if (kind !== 'delete') queueMicrotask(() => document.querySelector<HTMLElement>('[name="bulk-ticket-tag"]')?.focus());
}
function clipboardTicket(ticket: FullTicket | WireTicketRow, loaded?: FullTicket): ClipboardTicket {
  const full = 'attachments' in ticket ? ticket : loaded;
  return {
    id: ticket.id,
    slug: ticket.slug,
    connection_id: ticket.connection_id,
    native_id: ticket.native_id,
    title: ticket.title,
    details: full?.details ?? '',
    category: ticket.category ?? 'issue',
    priority: ticket.priority,
    status: ticket.status,
    up_next: ticket.up_next,
    tags: ticket.tags,
    notes: full?.notes.map((note) => ({ kind: note.kind, text: note.text, summary: note.summary })) ?? [],
    attachments: full?.attachments.map((item) => ({ id: item.id, filename: item.filename })) ?? [],
  };
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function copySelection(cut:boolean){const current=project(),rows=selectedRows();if(!current||!rows.length)return;const snapshot={tickets:rows.map(ticket=>clipboardTicket(ticket,selectedTicket.value?.id===ticket.id?selectedTicket.value:undefined)),cut,source:current};clipboard=snapshot;selectedTicketSlugs.value=[...selectedTicketSlugs.value];void Promise.all(rows.map(ticket=>api().checkoutTicket(current.id,ticket.id).then(value=>value.ticket))).then(full=>{if(clipboard!==snapshot)return;snapshot.tickets=full.map(ticket=>clipboardTicket(ticket));const text=full.map(ticket=>[`${ticket.slug}: ${ticket.title}`,ticket.details,...ticket.notes.map(note=>`- ${note.text}`)].filter(Boolean).join('\n\n')).join('\n\n');void navigator.clipboard?.writeText(text).catch(()=>undefined)}).catch((reason:unknown)=>{error.value=reason instanceof Error?reason.message:String(reason)})}
async function patchTransferTickets(client: Api, checkout: string, changes: Array<{ id: string; status: string }>) {
  for (const change of changes) await client.updateCheckoutTicket(checkout, change.id, { status: change.status });
  return true;
}
async function pasteSelection() {
  const current = project(),
    snapshot = clipboard;
  if (!current || !snapshot?.tickets.length) return;
  const destinationApi = api(),
    sourceApi = new Api(snapshot.source.apiPath),
    titles = tickets.value.map((ticket) => ticket.title),
    created: Array<{ id: string; slug: string; status: string }> = [];
  try {
    for (const source of snapshot.tickets) {
      const title = deduplicateTitle(source.title, titles);
      titles.push(title);
      let ticket = await destinationApi.createCheckoutTicket(current.id, {
        title,
        details: source.details,
        category: source.category,
        priority: source.priority,
        ...copiedTicketPlacement(source),
        tags: source.tags,
      });
      created.push({ id: ticket.id, slug: ticket.slug, status: ticket.status ?? 'not_started' });
      for (const note of source.notes)
        ticket = (
          await destinationApi.updateCheckoutTicket(current.id, ticket.id, {
            note: note.text,
            note_kind: note.kind,
            note_summary: note.summary,
          })
        ).ticket;
      for (const attachment of source.attachments)
        ticket = await destinationApi.copyAttachment(
          { connection_id: source.connection_id, native_id: source.native_id, attachment_id: attachment.id },
          { connection_id: ticket.connection_id, native_id: ticket.native_id },
        );
    }
    if (snapshot.cut) {
      for (const source of snapshot.tickets)
        await sourceApi.updateCheckoutTicket(snapshot.source.id, source.id, { status: 'deleted' });
      clipboard = undefined;
    }
    const originals = snapshot.tickets.map((ticket) => ({ id: ticket.id, status: ticket.status ?? 'not_started' }));
    history().recordExternal(
      async () => {
        await patchTransferTickets(
          destinationApi,
          current.id,
          created.map((ticket) => ({ id: ticket.id, status: 'deleted' })),
        );
        if (snapshot.cut) await patchTransferTickets(sourceApi, snapshot.source.id, originals);
        await refreshProject();
        return true;
      },
      async () => {
        await patchTransferTickets(
          destinationApi,
          current.id,
          created.map((ticket) => ({ id: ticket.id, status: ticket.status })),
        );
        if (snapshot.cut)
          await patchTransferTickets(
            sourceApi,
            snapshot.source.id,
            originals.map((ticket) => ({ id: ticket.id, status: 'deleted' })),
          );
        await refreshProject();
        return true;
      },
    );
    await refreshProject();
    selectedTicketSlugs.value = created.map((ticket) => ticket.slug);
    ticketSelectionAnchor = created[0]?.slug;
  } catch (reason) {
    for (const ticket of created)
      await destinationApi.updateCheckoutTicket(current.id, ticket.id, { status: 'deleted' }).catch(() => undefined);
    error.value = reason instanceof Error ? reason.message : String(reason);
    await refreshProject();
  }
}
async function copyDraggedTickets(destination: Project, drag: { slugs: string[]; source: Project }) {
  const sourceRows = tickets.value.filter((ticket) => drag.slugs.includes(ticket.slug));
  if (project()?.id !== drag.source.id || sourceRows.length === 0) return;
  const sourceApi = new Api(drag.source.apiPath),
    destinationApi = new Api(destination.apiPath),
    created: Array<{ id: string; slug: string; status: string }> = [];
  try {
    const [sources, destinationRows] = await Promise.all([
        Promise.all(
          sourceRows.map((ticket) =>
            sourceApi.checkoutTicket(drag.source.id, ticket.id).then((result) => result.ticket),
          ),
        ),
        destination.id === drag.source.id
          ? Promise.resolve(tickets.value)
          : destinationApi.checkoutTickets(destination.id),
      ]),
      titles = destinationRows.map((ticket) => ticket.title);
    for (const source of sources) {
      const title = deduplicateTitle(source.title, titles);
      titles.push(title);
      let ticket = await destinationApi.createCheckoutTicket(destination.id, {
        title,
        details: source.details,
        category: source.category ?? 'issue',
        priority: source.priority,
        ...copiedTicketPlacement(source),
        tags: source.tags,
      });
      created.push({ id: ticket.id, slug: ticket.slug, status: ticket.status ?? 'not_started' });
      for (const note of source.notes.filter((note) => note.kind !== 'activity'))
        ticket = (
          await destinationApi.updateCheckoutTicket(destination.id, ticket.id, {
            note: note.text,
            note_kind: note.kind,
          })
        ).ticket;
      for (const attachment of source.attachments)
        ticket = await destinationApi.copyAttachment(
          { connection_id: source.connection_id, native_id: source.native_id, attachment_id: attachment.id },
          { connection_id: ticket.connection_id, native_id: ticket.native_id },
        );
    }
    const refreshDestination = async () => {
      if (project()?.id === destination.id) await refreshProject();
      return true;
    };
    history().recordExternal(
      async () => {
        await patchTransferTickets(
          destinationApi,
          destination.id,
          created.map((ticket) => ({ id: ticket.id, status: 'deleted' })),
        );
        return refreshDestination();
      },
      async () => {
        await patchTransferTickets(
          destinationApi,
          destination.id,
          created.map((ticket) => ({ id: ticket.id, status: ticket.status })),
        );
        return refreshDestination();
      },
    );
    if (project()?.id === destination.id) {
      await refreshProject();
      selectedTicketSlugs.value = created.map((ticket) => ticket.slug);
      ticketSelectionAnchor = created[0]?.slug;
    }
    showToast(`${created.length} ticket${created.length === 1 ? '' : 's'} copied to ${destination.name}.`);
  } catch (reason) {
    for (const ticket of created)
      await destinationApi
        .updateCheckoutTicket(destination.id, ticket.id, { status: 'deleted' })
        .catch(() => undefined);
    error.value = reason instanceof Error ? reason.message : String(reason);
    if (project()?.id === destination.id) await refreshProject();
  }
}
function isEditableEvent(event: Event) {
  return event
    .composedPath()
    .some(
      (target) =>
        target instanceof HTMLElement &&
        (target.matches(
          'input, textarea, select, wa-input, wa-textarea, wa-select, [role="textbox"], [contenteditable]:not([contenteditable="false"])',
        ) ||
          Boolean(target.closest('wa-dialog[open]'))),
    );
}
function ticketWorkAreaFocused() {
  const area = document.querySelector<HTMLElement>('.app-shell__work-area'),
    active = document.activeElement;
  return Boolean(
    area &&
    active &&
    (active === area || area.contains(active)) &&
    area.querySelector('[data-component="ticket-list"], [data-component="ticket-board"]'),
  );
}
function ordinaryTextSelected() {
  const selection = document.getSelection();
  return Boolean(selection && !selection.isCollapsed && selection.toString().length);
}
function hs1SourceIdentity(value: Project) {
  return [value.hs1DatabasePath, value.hs1PostgresVersion].filter(Boolean).join('\u0000');
}
function retainProjectRestoreFailure(root: string, message: string, recoveryPid?: number, busy = false) {
  const failure: ProjectRestoreFailure = { root, name: rememberedProjectName(root), error: message, recoveryPid, busy };
  projectRestoreFailures.value = [...projectRestoreFailures.value.filter((item) => item.root !== root), failure];
}
const projectsPendingActivation = new Set<string>();
async function wireOpenedProject(root: string, opened: Extract<ProjectOpenResult, { ok: true }>) {
  const { project: value, providers: descriptors } = opened;
  projectRestoreFailures.value = projectRestoreFailures.value.filter((item) => item.root !== root);
  if (selectedProjectRestoreRoot.value === root) selectedProjectRestoreRoot.value = '';
  projects.value = replaceTabInPlace(projects.value, (item) => item.id, value);
  hideVerifiedByProject.value = {
    ...hideVerifiedByProject.value,
    [value.id]: localStorage.getItem(`hotsheet.project.${value.id}.hide-verified-column`) === 'true',
  };
  permissionAutomationByProject.value = {
    ...permissionAutomationByProject.value,
    [value.id]: loadPermissionAutomation(value.id),
  };
  const selected = descriptors.find((item) => item.default) ?? descriptors.at(0);
  defaultProviders.value = {
    ...defaultProviders.value,
    [value.id]: selected ? { name: selected.display_name, capabilities: selected.capabilities } : undefined,
  };
  providerCapabilities.value = {
    ...providerCapabilities.value,
    ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
  };
  projectsPendingActivation.add(value.id);
  // Join the durable migration snapshot before deciding whether activation should present onboarding.
  await migrationJobs
    .join(value.root)
    .then((job) => {
      if (!job)
        migrationJobsByRoot.value = Object.fromEntries(
          Object.entries(migrationJobsByRoot.value).filter(([root]) => root !== value.root),
        );
    })
    .catch(() => undefined);
}
function presentOpenedProjectSetup(value: Project) {
  projectsPendingActivation.delete(value.id);
  const migrationTarget =
      value.needsHs1Migration &&
      !migrationJobsByRoot.value[value.root] &&
      !hs1MigrationPromptDismissed(localStorage, value.id, hs1SourceIdentity(value))
        ? value
        : undefined,
    setupTarget = value.needsTicketSetup && !value.needsHs1Migration ? value : undefined,
    openingDialog = document.querySelector<Control>('[data-project-dialog]'),
    waitForProjectDialog = Boolean((migrationTarget || setupTarget) && openingDialog?.open);
  ticketSourceSetupNavigation.value = 'none';
  createdGitTicketStore.value = '';
  ticketSourceRemoteError.value = '';
  hs1MigrationError.value = '';
  const presentSetup = () => {
    if (project()?.id !== value.id) return;
    hs1MigrationProject.value = migrationTarget;
    ticketSourceSetupProject.value = setupTarget;
  };
  if (waitForProjectDialog) openingDialog!.addEventListener('wa-after-hide', presentSetup, { once: true });
  projectDialogOpen.value = false;
  if (!waitForProjectDialog) presentSetup();
  else {
    hs1MigrationProject.value = undefined;
    ticketSourceSetupProject.value = undefined;
  }
  ticketSourceSetupError.value = '';
}
async function activateOpenedProject(value: Project) {
  if (selectedProjectId.value !== value.id) {
    if (!activateOpenProject(value.id)) throw new Error('Could not activate the opened project.');
  } else {
    saveActiveProjectRoot(localStorage, value.root);
    terminalDrawerSelected.value =
      localStorage.getItem(`hotsheet.project.${value.id}.terminal-drawer-selection`) || 'grid';
  }
  presentOpenedProjectSetup(value);
  await Promise.all([
    refreshProject(),
    refreshCommands(value),
    refreshCustomViews(value),
    refreshDriveConnections(value, true),
    ...(terminalDrawerVisible.value ? [refreshTerminalDashboard()] : []),
  ]);
  await restoreProjectSession(value);
  const restored = customViewFor(selectedView.value, value.id);
  if (restored) applyCustomViewQuery(restored);
  else if (customTicketViewKey(selectedView.value)) selectedView.value = 'all';
  if (terminalDrawerVisible.value) observeTerminalDrawer();
}
async function openProject(
  root: string,
  ticketStore?: string,
  remember = true,
  reportError = true,
  retainFailure = false,
) {
  loading.value = true;
  unhealthyServerRecovery.value = undefined;
  if (reportError) error.value = '';
  try {
    const opened = await openProjectFetch(root, ticketStore);
    if (!opened.ok) {
      unhealthyServerRecovery.value = opened.recovery;
      throw new Error(opened.error);
    }
    await wireOpenedProject(root, opened);
    if (remember) localStorage.setItem('hotsheet.open-projects', JSON.stringify(currentRememberedProjectRoots()));
    startPermissionUpdates();
    syncProjectChangeStreams();
    await activateOpenedProject(opened.project);
    return true;
  } catch (reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    if (retainFailure) retainProjectRestoreFailure(root, message, unhealthyServerRecovery.value?.expected.pid);
    if (reportError) error.value = message;
    return false;
  } finally {
    loading.value = false;
  }
}
async function retryProjectRestore(root: string) {
  const failure = projectRestoreFailures.value.find((item) => item.root === root);
  if (!failure || failure.busy) return;
  retainProjectRestoreFailure(root, failure.error, failure.recoveryPid, true);
  await openProject(root, undefined, false, false, true);
}
async function recoverUnhealthyProjectServer() {
  const recovery = unhealthyServerRecovery.value,
    form = document.querySelector<HTMLFormElement>('[data-action="open-project-form"]');
  if (!recovery || !form || unhealthyServerRecoveryBusy.value) return;
  unhealthyServerRecoveryBusy.value = true;
  projectDialogError.value = '';
  try {
    const response = await fetch('/__hotsheet/server/recover-unhealthy', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(recovery),
      }),
      result = (await response.json()) as { recovered?: boolean; error?: string };
    if (!response.ok || !result.recovered)
      throw new Error(result.error ?? 'Could not recover the unresponsive server.');
    const root = (form.querySelector('[name="project-root"]') as Control).value,
      store = (form.querySelector('[name="ticket-store"]') as Control).value;
    await openProject(root, store || undefined);
  } catch (reason) {
    projectDialogError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    unhealthyServerRecoveryBusy.value = false;
  }
}
async function chooseHs1TicketStore() {
  const choice = await fetch('/__hotsheet/folders/choose', { method: 'POST' }),
    chosen = (await choice.json()) as { path?: string; error?: string };
  if (!choice.ok) {
    hs1MigrationError.value = chosen.error ?? 'Could not choose a ticket repository folder.';
    return;
  }
  const input = document.querySelector<Control>('[name="hs1-ticket-store"]');
  if (input && chosen.path) input.value = chosen.path;
}
async function importHs1Project(form: HTMLFormElement) {
  const target = hs1MigrationProject.value,
    location = form.querySelector<Control>('[name="hs1-ticket-store"]')?.value.trim();
  if (!target || !location || hs1MigrationBusy.value) return;
  hs1MigrationBusy.value = true;
  hs1MigrationError.value = '';
  try {
    await migrationJobs.start({ projectId: target.id, root: target.root, location, kind: 'import' });
    if (hs1MigrationProject.value?.id === target.id) hs1MigrationProject.value = undefined;
  } catch (reason) {
    hs1MigrationError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    hs1MigrationBusy.value = false;
  }
}
async function removeOldHs1Data() {
  const current = project();
  if (
    !current ||
    !current.hs1CleanupEligible ||
    !window.confirm('Delete the old Hot Sheet 1 files from this project? Backups will be kept.')
  )
    return;
  try {
    const response = await fetch(`/__hotsheet/projects/${encodeURIComponent(current.id)}/hs1-data`, {
        method: 'DELETE',
      }),
      result = (await response.json()) as { removed?: string[]; error?: string };
    if (!response.ok) throw new Error(result.error ?? 'Could not remove the old Hot Sheet 1 files.');
    dismissHs1CleanupPrompt(localStorage, current.id, hs1SourceIdentity(current));
    projects.value = projects.value.map((item) =>
      item.id === current.id
        ? {
            ...item,
            hs1CleanupEligible: false,
            needsHs1Migration: false,
            hs1DatabasePath: undefined,
            hs1SourcePath: undefined,
            hs1PostgresVersion: undefined,
          }
        : item,
    );
    showToast(
      `Removed ${result.removed?.length ?? 0} old Hot Sheet 1 item${result.removed?.length === 1 ? '' : 's'}; backups were kept.`,
    );
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  }
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function createProjectGitSource(custom=false){const target=ticketSourceSetupProject.value;if(!target)return;ticketSourceSetupError.value='';loading.value=true;try{let location:string|undefined;if(custom){const choice=await fetch('/__hotsheet/folders/choose',{method:'POST'}),chosen=await choice.json() as {path?:string;error?:string};if(!choice.ok)throw new Error(chosen.error??'Could not choose a ticket repository folder.');if(!chosen.path)return;location=chosen.path}const response=await fetch('/__hotsheet/projects/setup-git',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({root:target.root,location})}),result=await response.json() as {ticketStore?:string;connectionId?:string;error?:string};if(!response.ok||!result.ticketStore||!result.connectionId)throw new Error(result.error??'Could not create the git ticket store.');const client=new Api(target.apiPath),checkout=await client.addCheckoutSource(target.id,{id:result.connectionId,provider:'git',locator:result.ticketStore,name:'Hot Sheet git',default:Boolean(target.needsTicketSetup),settings:{}},Boolean(target.needsTicketSetup)),updated={...target,stores:checkout.stores,needsTicketSetup:false};projects.value=projects.value.map(item=>item.id===target.id?updated:item);ticketSourceSetupProject.value=updated;createdGitTicketStore.value=result.ticketStore;ticketSourceSetupNavigation.value='push';requestAnimationFrame(()=>document.querySelector<HTMLElement>('[name="ticket-store-remote"]')?.focus());const descriptors=await client.providers(),selected=descriptors.find(item=>item.default)??descriptors[0];defaultProviders.value={...defaultProviders.value,[target.id]:selected?{name:selected.display_name,capabilities:selected.capabilities}:undefined};providerCapabilities.value={...providerCapabilities.value,...Object.fromEntries(descriptors.map(item=>[item.connection_id,item.capabilities]))};await refreshProject()}catch(reason){ticketSourceSetupError.value=reason instanceof Error?reason.message:String(reason)}finally{loading.value=false}}
async function connectCreatedGitRemote(form: HTMLFormElement) {
  const target = ticketSourceSetupProject.value,
    store = createdGitTicketStore.value,
    remote = form.querySelector<Control>('[name="ticket-store-remote"]')?.value.trim();
  if (!target || !store || !remote || ticketSourceRemoteBusy.value) return;
  ticketSourceRemoteBusy.value = true;
  ticketSourceRemoteError.value = '';
  try {
    if (target.hs1ImportCompleted) {
      await migrationJobs.start({ projectId: target.id, root: target.root, location: store, kind: 'backup', remote });
      if (ticketSourceSetupProject.value?.id === target.id) {
        ticketSourceSetupProject.value = undefined;
        createdGitTicketStore.value = '';
      }
      return;
    }
    const response = await fetch('/__hotsheet/projects/setup-git-remote', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ store, remote }),
      }),
      result = (await response.json()) as { connected?: boolean; error?: string };
    if (!response.ok || !result.connected) throw new Error(result.error ?? 'Could not connect the Git remote.');
    showToast('Ticket repository connected and backed up.');
    ticketSourceSetupProject.value = undefined;
    createdGitTicketStore.value = '';
  } catch (reason) {
    ticketSourceRemoteError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    ticketSourceRemoteBusy.value = false;
  }
}
async function refreshProviderConnections(current = project()) {
  if (!current) return;
  try {
    const connections = await new Api(current.apiPath).connections();
    if (project()?.id !== current.id) return;
    providerConnections.value = connections;
    providerSettingsError.value = '';
  } catch (reason) {
    if (project()?.id === current.id)
      providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
  }
}
async function saveExternalProvider(form: HTMLFormElement) {
  const current = ticketSourceSetupProject.value ?? project(),
    kind = providerSetupKind.value,
    editingId = providerEditingId.value;
  if (!current || !kind || providerSettingsBusy.value) return;
  const values = new FormData(form),
    read = (field: string) => {
      const value = values.get(field);
      return typeof value === 'string' ? value.trim() : '';
    },
    id = editingId ?? read('connection-id'),
    name = read('connection-name'),
    locator = read('connection-locator'),
    credential =
      kind === 'github' ? (githubAuth.value?.credential ?? read('credential-reference')) : read('credential-reference'),
    makeDefault = values.get('make-default') === 'on';
  if (!/^[a-z0-9][a-z0-9-]*$/.test(id)) {
    providerSettingsError.value = 'Connection ID must use lowercase letters, numbers, and hyphens.';
    return;
  }
  if (!locator || (kind !== 'jira' && !locator.includes('/'))) {
    providerSettingsError.value =
      kind === 'jira' ? 'Enter the Jira project key.' : 'Enter a namespace/repository path.';
    return;
  }
  if (!credential) {
    providerSettingsError.value = 'Enter an existing OS-keychain credential reference.';
    return;
  }
  const settings: Record<string, unknown> = { credential: { secret: credential } },
    apiBase = read('api-base'),
    email = read('jira-email');
  if (apiBase) settings[kind === 'jira' ? 'base_url' : 'api_base'] = apiBase;
  if (kind === 'jira') {
    if (!email || !apiBase) {
      providerSettingsError.value = 'Jira requires the account email and site URL.';
      return;
    }
    settings.email = email;
  }
  const connection: ProviderConnection = {
    id,
    provider: kind,
    locator,
    name: name || null,
    default: makeDefault,
    settings,
  };
  providerSettingsBusy.value = true;
  providerSettingsError.value = '';
  try {
    const client = new Api(current.apiPath),
      saved = editingId
        ? await client.updateConnection(editingId, connection)
        : await client.createConnection(connection);
    await client.addCheckoutSource(current.id, saved, makeDefault);
    if (editingId && !makeDefault && providerConnections.value.find((item) => item.id === editingId)?.default)
      await client.setCheckoutDefaultSource(current.id, null);
    providerConnections.value = await client.connections();
    const descriptors = await client.providers(),
      selected = descriptors.find((item) => item.default) ?? descriptors[0];
    // prettier-ignore
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    defaultProviders.value={...defaultProviders.value,[current.id]:selected?{name:selected.display_name,capabilities:selected.capabilities}:undefined};
    providerCapabilities.value = {
      ...providerCapabilities.value,
      ...Object.fromEntries(descriptors.map((item) => [item.connection_id, item.capabilities])),
    };
    projects.value = projects.value.map((item) =>
      item.id === current.id ? { ...item, needsTicketSetup: false } : item,
    );
    ticketSourceSetupProject.value = undefined;
    providerSetupKind.value = undefined;
    providerEditingId.value = undefined;
    await refreshProject();
    showToast(editingId ? `${saved.name ?? saved.id} updated.` : `${saved.name ?? saved.id} connected.`);
  } catch (reason) {
    providerSettingsError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    providerSettingsBusy.value = false;
  }
}
function githubWebBase(apiBase: string) {
  const normalized = apiBase.trim().replace(/\/$/, '');
  if (!normalized || normalized === 'https://api.github.com') return 'https://github.com';
  return normalized.replace(/\/api\/v3$/, '');
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function startGitHubSignIn(form:HTMLFormElement){const target=ticketSourceSetupProject.value;if(!target||githubAuth.value?.state==='waiting')return;providerSettingsError.value='';try{const apiBase=form.querySelector<Control>('[name="api-base"]')?.value??'',client=new Api(target.apiPath),started=await client.startGitHubAuth(githubWebBase(apiBase));githubAuth.value={session:started.session_id,userCode:started.user_code,verificationUri:started.verification_uri,state:'waiting'};const result=await client.waitGitHubAuth(started.session_id);if(githubAuth.value?.session!==started.session_id||result.state==='pending')return;if(result.state==='authorized'){githubAuth.value={...githubAuth.value,state:'authorized',credential:result.credential_reference};try{const listed=await client.githubAuthRepositories(started.session_id);if(githubAuth.value?.session===started.session_id)githubAuth.value={...githubAuth.value,repositories:listed.repositories}}catch(reason){if(githubAuth.value?.session===started.session_id)githubAuth.value={...githubAuth.value,message:reason instanceof Error?reason.message:String(reason)}}}else githubAuth.value={...githubAuth.value,state:result.state,message:result.state==='error'?result.message:undefined}}catch(reason){providerSettingsError.value=reason instanceof Error?reason.message:String(reason)}}
function cancelGitHubSignIn() {
  const target = ticketSourceSetupProject.value,
    current = githubAuth.value;
  if (target && current?.state === 'waiting') void new Api(target.apiPath).cancelGitHubAuth(current.session);
  if (current) githubAuth.value = { ...current, state: 'cancelled' };
}
async function chooseProjectPath(button: Element) {
  projectDialogError.value = '';
  try {
    const input = button.closest('.project-dialog__path')?.querySelector<Control>('wa-input');
    if (!input) throw new Error('Could not find the project path field.');
    const endpoint = new URL('/__hotsheet/folders/choose', window.location.href);
    const response = await fetch(endpoint, { method: 'POST' }),
      text = await response.text();
    let result: { path?: string; error?: string } = {};
    try {
      result = text ? (JSON.parse(text) as typeof result) : {};
    } catch {
      throw new Error(`The folder chooser returned an invalid response (${response.status}).`);
    }
    if (!response.ok) throw new Error(result.error ?? 'Could not open the folder chooser.');
    if (!result.path) return;
    input.value = result.path;
    input.focus();
  } catch (reason) {
    projectDialogError.value = reason instanceof Error ? reason.message : String(reason);
  }
}
async function chooseAndOpenProject() {
  error.value = '';
  projectDialogError.value = '';
  try {
    const response = await fetch(new URL('/__hotsheet/folders/choose', window.location.href), { method: 'POST' }),
      text = await response.text();
    let result: { path?: string; error?: string } = {};
    try {
      result = text ? (JSON.parse(text) as typeof result) : {};
    } catch {
      throw new Error(`The folder chooser returned an invalid response (${response.status}).`);
    }
    if (!response.ok) throw new Error(result.error ?? 'Could not open the folder chooser.');
    if (result.path) await openProject(result.path);
  } catch (reason) {
    projectDialogError.value = reason instanceof Error ? reason.message : String(reason);
    projectDialogOpen.value = true;
  }
}
/** Open the appropriate project picker: the filesystem dialog on the same device, or the server's
 * known-projects list when the client is on a different device (HS2-VFNCXG). */
function openProjectPicker() {
  error.value = '';
  projectDialogError.value = '';
  unhealthyServerRecovery.value = undefined;
  if (isRemoteClient()) void openRemoteProjectDialog();
  else projectDialogOpen.value = true;
}
async function openRemoteProjectDialog() {
  remoteProjectError.value = '';
  remoteProjectLoading.value = true;
  remoteProjectDialogOpen.value = true;
  try {
    // Use a plain relative request (every other client fetch does) rather than `new URL(..., location.href)`,
    // which surfaced a cryptic engine SyntaxError ("The string did not match the expected pattern.") on some
    // mobile browsers, and always present a clear, actionable message instead of a raw browser exception (HS2-91PCDZ).
    const response = await fetch('/__hotsheet/checkouts');
    if (!response.ok) throw new Error(`the server responded with ${response.status}`);
    remoteProjectCheckouts.value = (await response.json()) as Checkout[];
  } catch (reason) {
    console.error('Could not load the server open-projects list', reason);
    remoteProjectError.value =
      'Could not load the projects open on the Hot Sheet server. Check the connection to the server and try again.';
  } finally {
    remoteProjectLoading.value = false;
  }
}
async function openRemoteCheckout(root: string) {
  remoteProjectDialogOpen.value = false;
  await openProject(root);
}
function timeline(ticket: FullTicket) {
  return ticketTimelineEntries(ticket).map((entry) => ({ ...entry, time: ago(entry.timestamp) }));
}
function notes(ticket: FullTicket) {
  return ticket.notes.map((note) => {
    const aiAuthored = note.text.includes('hotsheet:activity-distillation:v1:');
    return {
      id: note.id,
      kind: presentedNoteKind(note, ticket.notes),
      author: aiAuthored ? 'Hot Sheet AI' : 'Hot Sheet',
      time: ago(note.created_at),
      body: note.text,
      aiAuthored,
      aiTool: aiAuthored ? 'Hot Sheet AI' : undefined,
    } as const;
  });
}
function attachmentContext(ticket: FullTicket, current = project()): AttachmentReferenceContext | undefined {
  return current
    ? {
        baseUrl: current.apiPath,
        checkout: current.id,
        ticket: ticket.slug,
        attachments: ticket.attachments.map((item) => ({ id: item.id, filename: item.filename })),
      }
    : undefined;
}
function duplicateTarget(project: Project, ticket: WireTicketRow): DuplicateTarget {
  return {
    id: ticket.id,
    slug: ticket.slug,
    title: ticket.title,
    projectId: project.id,
    projectName: project.name,
    connectionId: ticket.connection_id,
    nativeId: ticket.native_id,
    qualifiedId: ticket.qualified_id,
  };
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
function duplicateTargetFor(ticket:FullTicket){if(!ticket.duplicate_of)return undefined;const reference=parseDuplicateReference(ticket.duplicate_of),projectRows=reference?projects.value.filter(item=>item.id===reference.project_id):projects.value,match=projectRows.flatMap(item=>(item.id===selectedProjectId.value?tickets.value:ticketRowsByProject.value[item.id]??[]).map(row=>({project:item,row}))).find(({row})=>reference?row.connection_id===reference.connection_id&&row.native_id===reference.native_id:row.id===ticket.duplicate_of||row.native_id===ticket.duplicate_of),target=match?duplicateTarget(match.project,match.row):resolvedDuplicateTargets.value[ticket.duplicate_of];return target?{id:ticket.duplicate_of,projectName:target.projectName,slug:target.slug,title:target.title}:undefined}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function resolveDuplicateTargetFor(ticket:FullTicket){const value=ticket.duplicate_of;if(!value||resolvedDuplicateTargets.value[value])return;const target=await resolveDuplicateReferenceTarget(value,projects.value,(item,id)=>new Api((item as Project).apiPath).checkoutTicket(item.id,id).then(result=>result.ticket)).catch(()=>undefined);if(target)resolvedDuplicateTargets.value={...resolvedDuplicateTargets.value,[value]:target}}
function galleryImages(ticket = selectedTicket.value): AttachmentGalleryImage[] {
  const current = project();
  if (!ticket || !current) return [];
  const context = attachmentContext(ticket)!,
    images: AttachmentGalleryImage[] = ticket.attachments
      .filter((item) => isGalleryMediaAttachment(item.filename))
      .map((item) => ({
        id: item.id,
        name: item.filename,
        url: api().checkoutAttachmentUrl(current.id, ticket.id, item.id),
        thumbnailUrl: isVideoAttachment(item.filename)
          ? api().checkoutAttachmentThumbnailUrl(current.id, ticket.id, item.id)
          : undefined,
        aliases: [attachmentReferenceUrl(context, { filename: item.filename })],
        ticket: ticket.slug,
        attachmentId: item.id,
      })),
    seen = new Set(images.map((image) => `${ticket.slug}\0${image.name}`));
  for (const note of ticket.notes)
    for (const reference of attachmentReferences(note.text, context)) {
      if (!isGalleryMediaAttachment(reference.filename)) continue;
      const referencedTicket = reference.ticket ?? ticket.slug,
        key = `${referencedTicket}\0${reference.filename}`,
        url = attachmentReferenceUrl(context, reference);
      if (!seen.has(key)) {
        seen.add(key);
        images.push({
          id: `${referencedTicket}:${reference.filename}`,
          name: reference.filename,
          url,
          ticket: referencedTicket,
        });
      }
    }
  return images;
}
async function uploadAttachmentWithPoster(
  client: Api,
  current: Project,
  ticket: FullTicket,
  file: File,
  metadata: AttachmentMetadata,
) {
  const previousIds = new Set(ticket.attachments.map((item) => item.id)),
    result = await client.addCheckoutAttachment(current.id, ticket.id, file, metadata),
    added = result.ticket.attachments.find((item) => !previousIds.has(item.id));
  if (added && isVideoAttachment(file.name)) {
    const posterUrl = client.checkoutAttachmentThumbnailUrl(current.id, ticket.id, added.id);
    void ensureVideoPoster(posterUrl, file);
  }
  return result.ticket;
}
async function addAttachments(slug: string, files: FileList | File[]) {
  const current = project(),
    ticket = selectedTicket.value?.slug === slug ? selectedTicket.value : undefined;
  if (!current || !ticket || files.length === 0 || !canUseAttachments()) return;
  loading.value = true;
  attachmentMessage.value = `Adding ${files.length} attachment${files.length === 1 ? '' : 's'}…`;
  try {
    const screened = await screenAttachmentFiles(Array.from(files)),
      batch_id = attachmentUploadBatchId(ticket.attachments, ticket.notes, () => crypto.randomUUID());
    let updated: FullTicket = ticket;
    for (const file of screened.readable)
      updated = await uploadAttachmentWithPoster(api(), current, updated, file, { batch_id, actor: { role: 'human' } });
    if (updated !== ticket && selectedTicket.value?.id === ticket.id) selectedTicket.value = updated;
    if (screened.readable.length) await refreshProject();
    const warning = describeUnreadableAttachments(screened.unreadable);
    attachmentMessage.value = warning;
    error.value = warning;
    if (screened.readable.length)
      showToast(`${screened.readable.length} attachment${screened.readable.length === 1 ? '' : 's'} added.`);
  } catch (reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    attachmentMessage.value = `Attachment failed: ${message}`;
    error.value = message;
  } finally {
    loading.value = false;
  }
}
function selectionOrder(target: Element) {
  const column = target.closest('[data-component="ticket-board-column"]');
  return column
    ? [...column.querySelectorAll<HTMLElement>('[data-action="select-ticket-row"]')].map(
        (item) => data(item).ticketSlug!,
      )
    : visibleTickets().map((ticket) => ticket.slug);
}
function presentTicket(ticket: FullTicket) {
  const changed = selectedTicket.value?.id !== ticket.id,
    current = project(),
    backlinkKey = current ? `${current.id}:${ticket.qualified_id}` : '';
  batch(() => {
    selectedCorruptKey.value = undefined;
    selectedTicket.value = ticket;
    committedTickets.clear();
    committedTickets.set(ticket.slug, ticket);
    duplicateBacklinkState.value = { key: backlinkKey, backlinks: [], inaccessibleProjects: [] };
    if (changed) {
      codeReview.value = undefined;
      codeReviewMessage.value = '';
      codeReviewLoading.value = false;
      expandedCodeReviewCommits.value = [];
      readerInlineFeedbackReplies.value = {};
      readerFeedbackChoiceSelections.value = {};
      readerFeedbackChoiceAnchors.clear();
    }
    detailsEditGeneration += 1;
    readerDetailsEditGeneration += 1;
    detailsMode.value = 'preview';
    readerDetailsMode.value = 'preview';
    detailsDraft.value = ticket.details;
    readerDetailsDraft.value = ticket.details;
    detailsDraftBase = ticket.details;
    readerDetailsDraftBase = ticket.details;
    titleEditing.value = false;
    titleDraft.value = ticket.title;
    titleDraftBase = ticket.title;
    blockedReasonEditing.value = false;
    readerBlockedReasonEditing.value = false;
    blockedReasonDraft.value = ticket.blocked_reason ?? '';
    readerBlockedReasonDraft.value = ticket.blocked_reason ?? '';
    blockedReasonDraftBase = ticket.blocked_reason ?? '';
    readerBlockedReasonDraftBase = ticket.blocked_reason ?? '';
    noteDraftBase = '';
    readerNoteDraftBase = '';
    editingNoteId.value = undefined;
    readerEditingNoteId.value = undefined;
    fieldConflict.value = undefined;
    fieldConflictResolution.value = '';
    inspectorVisible.value = true;
    error.value = '';
  });
  persistWorkspacePreferences();
  void resolveDuplicateTargetFor(ticket);
  if (current)
    void new Api(current.apiPath)
      .checkoutTicketDuplicateBacklinks(current.id, ticket.qualified_id)
      .then((result) => {
        if (project()?.id === current.id && selectedTicket.value?.qualified_id === ticket.qualified_id)
          duplicateBacklinkState.value = {
            key: backlinkKey,
            backlinks: result.backlinks,
            inaccessibleProjects: [...new Set(result.inaccessible_projects.map((item) => item.project_name))],
          };
      })
      .catch(() => undefined);
  if (inspectorTab.value === 'code-review') void refreshCodeReview();
}
function cancelTicketDrafts() {
  detailsEditGeneration += 1;
  readerDetailsEditGeneration += 1;
  detailsAutosave.cancel();
  readerDetailsAutosave.cancel();
  noteAutosave.cancel();
  readerNoteAutosave.cancel();
  blockedReasonAutosave.cancel();
  readerBlockedReasonAutosave.cancel();
  titleAutosave.cancel();
  tagsAutosave.cancel();
  fieldConflict.value = undefined;
  fieldConflictResolution.value = '';
}
function selectTickets(
  slug: string,
  intent: { range?: boolean; toggle?: boolean } = {},
  ordered = visibleTickets().map((ticket) => ticket.slug),
) {
  const loaded = selectedTicket.value;
  if (isPlainTicketReselection(selectedTicketSlugs.value, loaded?.slug, slug, intent)) return Promise.resolve(loaded);
  const finishTiming = beginInteractionTiming('ticket-selection', { slug }),
    next = updateTicketSelection(
      ordered,
      { anchor: ticketSelectionAnchor, selected: new Set(selectedTicketSlugs.value) },
      slug,
      intent,
    ),
    selected = [...next.selected];
  ticketSelectionAnchor = next.anchor;
  batch(() => {
    selectedCorruptKey.value = undefined;
    selectedTicketSlugs.value = selected;
    cancelTicketDrafts();
    if (selected.length !== 1) selectedTicket.value = null;
  });
  scheduleProjectSessionPersistence();
  finishTiming();
  if (selected.length !== 1) return Promise.resolve(undefined);
  const ticket = tickets.value.find((item) => item.slug === selected[0]),
    current = project();
  if (ticket && current)
    return api()
      .checkoutTicket(current.id, ticket.id)
      .then((result) => {
        if (
          project()?.id === current.id &&
          selectedTicketSlugs.value.length === 1 &&
          selectedTicketSlugs.value[0] === result.ticket.slug
        )
          presentTicket(result.ticket);
        return result.ticket;
      })
      .catch((reason: unknown) => {
        error.value = reason instanceof Error ? reason.message : String(reason);
        return undefined;
      });
  return Promise.resolve(undefined);
}
async function openTicketReader(slug: string, ordered?: string[], trigger?: HTMLElement) {
  const ticket = await selectTickets(slug, {}, ordered);
  if (!ticket) return;
  presentTicketReaderDialog('workspace-reader', trigger, () => {
    readerOpen.value = true;
    if (readerTab.value === 'code-review' && !codeReviewLoading.value) void refreshCodeReview();
  });
}
function closeNotWorking(clearStored = true) {
  const ids = notWorkingFiles.value.map((item) => item.id),
    scope = draftScope('not-working', notWorkingTarget.value.projectId);
  notWorkingTarget.value = CLOSED_NOT_WORKING_TARGET;
  notWorkingNote.value = '';
  notWorkingFiles.value = [];
  notWorkingSubmitting.value = false;
  notWorkingError.value = '';
  if (clearStored) void deleteDraftFiles(scope, ids);
  scheduleProjectSessionPersistence();
}
function presentNotWorkingDialog() {
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      const dialog = document.querySelector<Control>('[data-component="not-working-dialog"]'),
        native = dialog?.shadowRoot?.querySelector<HTMLDialogElement>('dialog');
      if (!native?.open) dialog?.show?.();
      dialog?.querySelector<HTMLTextAreaElement>('[name="not-working-note"]')?.focus();
    }),
  );
}
function openNotWorking(ticket: WireTicketRow, mode: NotWorkingTarget['mode'] = 'not-working') {
  const current = project();
  if (!current) return;
  notWorkingTarget.value = {
    projectId: current.id,
    apiPath: current.apiPath,
    ticketId: ticket.id,
    slug: ticket.slug,
    connectionId: ticket.connection_id,
    mode,
  };
  notWorkingNote.value = '';
  notWorkingFiles.value = [];
  notWorkingSubmitting.value = false;
  notWorkingError.value = '';
  scheduleProjectSessionPersistence();
  presentNotWorkingDialog();
}
function closeTicketCloseDialog() {
  ticketCloseSearchGeneration += 1;
  if (ticketCloseSearchTimer !== undefined) window.clearTimeout(ticketCloseSearchTimer);
  ticketCloseSearchTimer = undefined;
  ticketCloseDialog.value = undefined;
}
function openTicketClose(ticket: WireTicketRow) {
  const current = project();
  if (!current) return;
  ticketCloseDialog.value = {
    source: duplicateTarget(current, ticket),
    reason: 'completed',
    query: '',
    candidates: [],
  };
  queueMicrotask(() => document.querySelector<Control>('[name="ticket-close-reason"]')?.focus());
}
function setTicketCloseReason(reason: TicketCloseReason) {
  const current = ticketCloseDialog.value;
  if (!current) return;
  ticketCloseDialog.value = {
    ...current,
    reason,
    selected: reason === 'duplicate' ? current.selected : undefined,
    error: '',
  };
  if (reason === 'duplicate')
    queueMicrotask(() => document.querySelector<Control>('[name="ticket-close-target-search"]')?.focus());
}
function searchTicketCloseTargets(query: string) {
  const state = ticketCloseDialog.value;
  if (!state) return;
  const generation = ++ticketCloseSearchGeneration;
  if (ticketCloseSearchTimer !== undefined) window.clearTimeout(ticketCloseSearchTimer);
  ticketCloseDialog.value = {
    ...state,
    query,
    selected: undefined,
    candidates: [],
    searching: Boolean(query.trim()),
    error: '',
  };
  if (!query.trim()) return;
  ticketCloseSearchTimer = window.setTimeout(() => {
    void Promise.allSettled(
      projects.value.map(async (project) => ({
        project,
        rows: await new Api(project.apiPath).checkoutTickets(project.id, { text: query, compact: true }),
      })),
    ).then((results) => {
      const active = ticketCloseDialog.value;
      if (
        generation !== ticketCloseSearchGeneration ||
        !active ||
        duplicateTargetKey(active.source) !== duplicateTargetKey(state.source)
      )
        return;
      const candidates = new Map<string, DuplicateTarget>(),
        errors: string[] = [];
      for (const result of results) {
        if (result.status === 'rejected') {
          errors.push(result.reason instanceof Error ? result.reason.message : String(result.reason));
          continue;
        }
        for (const row of result.value.rows) {
          const candidate = duplicateTarget(result.value.project, row);
          candidates.set(duplicateTargetKey(candidate), candidate);
        }
      }
      ticketCloseDialog.value = {
        ...active,
        candidates: [...candidates.values()].slice(0, 20),
        searching: false,
        error: candidates.size || !errors.length ? '' : errors[0],
      };
    });
  }, 150);
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function submitTicketClose(){const state=ticketCloseDialog.value,current=projects.value.find(item=>item.id===state?.source.projectId);if(!state||!current||state.submitting)return;const validation=validateTicketClose(state.reason,state.source,state.selected);if(validation){ticketCloseDialog.value={...state,error:validation};return}ticketCloseDialog.value={...state,submitting:true,error:''};try{const result=await new Api(current.apiPath).closeCheckoutTicket(current.id,state.source.qualifiedId,state.reason,state.selected?duplicateReference(state.selected):undefined);if(project()?.id!==current.id)return;await refreshProject();presentTicket(result.ticket);closeTicketCloseDialog();showToast(state.reason==='duplicate'?`${state.source.slug} marked as a duplicate of ${state.selected!.slug}.`:`${state.source.slug} closed as ${state.reason.replace('_',' ')}.`)}catch(reason){const active=ticketCloseDialog.value;if(active)ticketCloseDialog.value={...active,submitting:false,error:reason instanceof Error?reason.message:String(reason)}}}
async function openDuplicateTarget(id: string) {
  const reference = parseDuplicateReference(id);
  if (reference) {
    const target = projects.value.find((item) => item.id === reference.project_id);
    if (!target) {
      showToast(`Open project ${reference.project_id} to view this duplicate target.`);
      return;
    }
    try {
      const ticket = (
        await new Api(target.apiPath).checkoutTicket(target.id, `${reference.connection_id}:${reference.native_id}`)
      ).ticket;
      await openTicketLinkMatch({
        projectId: target.id,
        projectName: target.name,
        ticketId: ticket.qualified_id,
        qualifiedId: ticket.qualified_id,
        connectionId: ticket.connection_id,
        slug: ticket.slug,
        title: ticket.title,
        status: ticket.status,
      });
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
    return;
  }
  const matches = (
    await Promise.allSettled(
      projects.value.map(async (target) => ({
        target,
        ticket: (await new Api(target.apiPath).checkoutTicket(target.id, id)).ticket,
      })),
    )
  ).flatMap((result) =>
    result.status === 'fulfilled'
      ? [
          {
            projectId: result.value.target.id,
            projectName: result.value.target.name,
            ticketId: result.value.ticket.qualified_id,
            qualifiedId: result.value.ticket.qualified_id,
            connectionId: result.value.ticket.connection_id,
            slug: result.value.ticket.slug,
            title: result.value.ticket.title,
            status: result.value.ticket.status,
          },
        ]
      : [],
  );
  const unique = new Map(matches.map((match) => [ticketLinkMatchKey(match), match]));
  if (unique.size === 0) {
    showToast(`No exact match for ${id}.`);
    return;
  }
  if (unique.size > 1) {
    ticketLinkChoice.value = { kind: 'choose', reference: { raw: id, slug: id }, matches: [...unique.values()] };
    return;
  }
  await openTicketLinkMatch([...unique.values()][0]);
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function addNotWorkingFiles(files:FileList|File[]){const target=notWorkingTarget.value;if(!target||!(capabilitiesFor(target.connectionId)?.attachments??true))return;const screened=await screenAttachmentFiles(Array.from(files)),pending=screened.readable.map(file=>({id:crypto.randomUUID(),name:file.name,file}));notWorkingFiles.value=[...notWorkingFiles.value,...pending];await Promise.all(pending.map(item=>saveDraftFile(draftScope('not-working',target.projectId),item.id,item.file))).catch(()=>undefined);notWorkingError.value=describeUnreadableAttachments(screened.unreadable);scheduleProjectSessionPersistence();presentNotWorkingDialog()}
function openTicketComposer(trigger?: HTMLElement) {
  if (composerExpanded.value) return;
  if (trigger && document.activeElement !== trigger) trigger.focus({ preventScroll: true });
  composerExpanded.value = true;
  scheduleProjectSessionPersistence();
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      showQuickTicketComposer(document);
      focusQuickTicketComposerTitle(document);
    }),
  );
}
function resetTicketComposer(clearStored = true) {
  const ids = composerAttachments.value.map((item) => item.id),
    scope = draftScope('composer');
  composerAttachmentEpoch += 1;
  activeComposerScreenings.clear();
  composerScreening.value = false;
  composerSubmitting.value = false;
  composerExpanded.value = false;
  composerTitle.value = '';
  composerDetails.value = '';
  composerUpNext.value = false;
  composerAttachments.value = [];
  composerAttachmentMessage.value = '';
  composerAttachmentError.value = false;
  if (clearStored) void deleteDraftFiles(scope, ids);
  scheduleProjectSessionPersistence();
}
async function addNewTicketFiles(files: FileList | File[]) {
  const origin = project(),
    epoch = composerAttachmentEpoch,
    screening = Symbol();
  openTicketComposer();
  if (!origin || !canStageNewTicketAttachments()) {
    composerAttachmentMessage.value = 'The default ticket provider cannot create tickets with attachments.';
    composerAttachmentError.value = true;
    return;
  }
  activeComposerScreenings.add(screening);
  composerScreening.value = true;
  composerAttachmentMessage.value = 'Checking attachment files…';
  composerAttachmentError.value = false;
  try {
    const screened = await screenAttachmentFiles(Array.from(files));
    if (composerAttachmentEpoch !== epoch || project()?.id !== origin.id) return;
    const pending = screened.readable.map((file) => ({ id: crypto.randomUUID(), name: file.name, file }));
    composerAttachments.value = [...composerAttachments.value, ...pending];
    await Promise.all(
      pending.map((item) => saveDraftFile(draftScope('composer', origin.id), item.id, item.file)),
    ).catch(() => undefined);
    const warning = describeUnreadableAttachments(screened.unreadable);
    composerAttachmentMessage.value = warning;
    composerAttachmentError.value = Boolean(warning);
    scheduleProjectSessionPersistence();
    requestAnimationFrame(() => requestAnimationFrame(() => focusQuickTicketComposerTitle(document)));
  } finally {
    activeComposerScreenings.delete(screening);
    if (composerAttachmentEpoch === epoch && project()?.id === origin.id)
      composerScreening.value = activeComposerScreenings.size > 0;
  }
}
async function submitNewTicket() {
  const origin = project(),
    provider = defaultProvider(),
    title = composerTitle.value.trim();
  if (
    !origin ||
    !title ||
    composerScreening.value ||
    composerSubmitting.value ||
    !(provider?.capabilities.create ?? true)
  )
    return;
  const client = new Api(origin.apiPath),
    files = composerAttachments.value.map((item) => item.file),
    batch_id = crypto.randomUUID(),
    placement = newTicketCreationPlacement(selectedView.value, composerUpNext.value),
    finishLocalCreation = beginLocalTicketCreation();
  composerSubmitting.value = true;
  composerAttachmentMessage.value = files.length
    ? `Creating ticket and adding ${files.length} attachment${files.length === 1 ? '' : 's'}…`
    : 'Creating ticket…';
  composerAttachmentError.value = false;
  try {
    const result = await createTicketWithAttachments(
      files,
      async () => {
        const created = await client.createCheckoutTicket(origin.id, {
          title,
          details: composerDetails.value,
          category: composerCategory.value,
          ...placement,
        });
        localTicketChangeAcknowledgements.acknowledge(origin.id, {
          store: created.connection_id,
          id: created.id,
          kind: 'created',
        });
        pendingCreatedTickets.register(origin.id, created);
        return created;
      },
      async (created, file) => {
        const updated = await uploadAttachmentWithPoster(client, origin, created, file, {
          batch_id,
          actor: { role: 'human' },
        });
        localTicketChangeAcknowledgements.acknowledge(origin.id, {
          store: updated.connection_id,
          id: updated.id,
          kind: 'attachment_added',
        });
        return updated;
      },
      (created) => {
        resetTicketComposer();
        if (project()?.id !== origin.id) return;
        cancelTicketDrafts();
        tickets.value = [created, ...tickets.value.filter((ticket) => ticket.id !== created.id)];
        if (!createdTicketVisibleInView(created, selectedView.value)) selectTicketView('all');
        selectedTicket.value = null;
        selectedTicketSlugs.value = [created.slug];
        ticketSelectionAnchor = created.slug;
        presentTicket(created);
        beginDetailsEdit();
        window.setTimeout(() => {
          const source = activeTicketSurface().querySelector<HTMLElement>('[name="markdown-source"]'),
            active = document.activeElement; // Only claim focus for the details editor if the user has not already focused another control (e.g. opened the priority select) — otherwise this delayed focus steals it and closes their popup (HS2-43F14D).
          if (
            source &&
            (!active ||
              active === document.body ||
              active === document.documentElement ||
              active.closest('[data-component="quick-ticket-composer-launcher"]'))
          )
            source.focus();
        }, 300);
      },
    );
    const failure = describeNewTicketAttachmentFailures(result.failed);
    if (project()?.id !== origin.id) return;
    if (selectedTicket.value?.id === result.ticket.id)
      selectedTicket.value = { ...selectedTicket.value, attachments: result.ticket.attachments };
    if (failure) {
      inspectorTab.value = 'attachments';
      attachmentMessage.value = failure;
      error.value = failure;
    }
  } catch (reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    composerAttachmentMessage.value = `Ticket creation failed: ${message}`;
    composerAttachmentError.value = true;
    composerSubmitting.value = false;
  } finally {
    await finishLocalCreation();
  }
}
// prettier-ignore
// eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
async function submitNotWorking(){const target=notWorkingTarget.value;if(!target.slug||notWorkingSubmitting.value)return;const owning=projects.value.find(item=>item.id===target.projectId);if(!owning){notWorkingError.value='The project is no longer open.';return}const client=new Api(target.apiPath),capabilities=capabilitiesFor(target.connectionId),note=(capabilities?.notes??true)?notWorkingNote.value:'',files=[...notWorkingFiles.value],scope=draftScope('not-working',target.projectId);notWorkingSubmitting.value=true;notWorkingError.value='';notWorkingTarget.value=CLOSED_NOT_WORKING_TARGET;try{const current=(await client.checkoutTicket(target.projectId,target.ticketId)).ticket;let full:FullTicket=current;await submitNotWorkingReport({note,files:files.map(item=>item.file)},{report:async(text,evidence)=>{full=await client.reportNotWorking(target.connectionId,target.ticketId,text,evidence,current.concurrency_token)}});if(project()?.id===owning.id){setProjectTicketRows(owning.id,projectTabTicketRows(owning.id).map(row=>row.id===full.id?ticketRowFromFull(row,full):row));selectedTicketSlugs.value=[target.slug];ticketSelectionAnchor=target.slug;presentTicket(full)}notWorkingNote.value='';notWorkingFiles.value=[];notWorkingSubmitting.value=false;void deleteDraftFiles(scope,files.map(item=>item.id));scheduleProjectSessionPersistence()}catch(reason){notWorkingTarget.value=target;notWorkingNote.value=note;notWorkingFiles.value=files;notWorkingError.value=reason instanceof Error?reason.message:String(reason);notWorkingSubmitting.value=false;scheduleProjectSessionPersistence();presentNotWorkingDialog()}}

function showSavedViewDialog() {
  savedViewDialogOpen.value = true;
  const name = document.querySelector<Control>('[name="saved-view-name"]');
  // wa-input keeps a dirty live value independently from its value/defaultValue
  // attribute. Programmatic Create/Edit transitions own both directions of this
  // controlled field, so synchronize the property before native autofocus runs.
  if (name && name.value !== savedViewName.value) name.value = savedViewName.value;
}
function setSavedViewQuery(value: string) {
  const parsed = consumeSearchTokens(value, true);
  savedViewQuery.value = parsed.text;
  savedViewQueryTokens.value = parsed.tokens;
}
function openSavedViewDialog() {
  const selected = customViewFor(selectedView.value),
    query = selected?.query ?? orderedSearchText(searchQuery.value, searchTokens.value, () => true);
  savedViewDialogMode.value = 'create';
  savedViewTargetId.value = undefined;
  savedViewName.value = '';
  setSavedViewQuery(query);
  savedViewError.value = '';
  savedViewBusy.value = false;
  showSavedViewDialog();
}
function openSavedViewRename(viewId: string) {
  const id = customTicketViewKey(viewId as TicketView),
    view = id && customViewsFor().find((item) => item.id === id);
  if (!view) return;
  savedViewDialogMode.value = 'rename';
  savedViewTargetId.value = view.id;
  savedViewName.value = view.name;
  setSavedViewQuery(view.query);
  savedViewError.value = '';
  savedViewBusy.value = false;
  savedViewMenu.value = undefined;
  showSavedViewDialog();
}
function closeSavedViewDialog() {
  if (savedViewBusy.value) return;
  savedViewDialogOpen.value = false;
  savedViewTargetId.value = undefined;
  savedViewError.value = '';
}
function updateSavedViewQuery(
  value: string,
  forceToken = false,
  currentTokens = savedViewQueryTokens.value,
  commitToken = forceToken,
) {
  const parsed = consumeSearchTokens(value, commitToken),
    shifted = currentTokens.map((token) => {
      const offset = token.offset ?? value.length,
        shift = parsed.removed.reduce((total, range) => total + (range.end <= offset ? range.end - range.start : 0), 0);
      return { ...token, offset: Math.max(0, offset - shift) };
    }),
    next: InlineSearchToken[] = [...shifted];
  for (const token of parsed.tokens)
    if (!next.some((value) => value.kind === token.kind && value.value === token.value)) next.push(token);
  savedViewQuery.value = parsed.text;
  savedViewQueryTokens.value = next;
  savedViewError.value = '';
  return parsed.tokens.length > 0;
}
function focusSavedViewQuery(offset?: number) {
  restoreInlineSearchCaret(document, '[data-token-search-editor="saved-view-query"]', offset);
}
function removeSavedViewQueryToken(raw: string) {
  const token = savedViewQueryTokens.value.find((value) => value.raw === raw);
  if (!token) return false;
  const offset = token.offset ?? savedViewQuery.value.length;
  savedViewQueryTokens.value = savedViewQueryTokens.value.filter((value) => value !== token);
  savedViewError.value = '';
  focusSavedViewQuery(offset);
  return true;
}
function editSavedViewQueryToken(event: Event, target: Element) {
  event.preventDefault();
  const raw = data(target).tokenValue,
    token = savedViewQueryTokens.value.find((value) => value.raw === raw);
  if (!raw || !token) return;
  const offset = Math.max(0, Math.min(savedViewQuery.value.length, token.offset ?? savedViewQuery.value.length)),
    before = savedViewQuery.value.slice(0, offset),
    after = savedViewQuery.value.slice(offset),
    leading = before && !/[\s(]$/.test(before) ? ' ' : '',
    trailing = after && !/^[\s)]/.test(after) ? ' ' : '',
    insert = `${leading}${raw}${trailing}`;
  batch(() => {
    savedViewQueryTokens.value = savedViewQueryTokens.value
      .filter((value) => value.raw !== raw)
      .map((value) => ({
        ...value,
        offset:
          (value.offset ?? savedViewQuery.value.length) >= offset
            ? (value.offset ?? savedViewQuery.value.length) + insert.length
            : value.offset,
      }));
    savedViewQuery.value = before + insert + after;
  });
  focusSavedViewQuery(offset + leading.length + raw.length);
}
async function saveSavedView(form: HTMLFormElement) {
  const current = project();
  if (!current || savedViewBusy.value) return;
  const existing = customViewsFor(current.id),
    targetId = savedViewDialogMode.value === 'rename' ? savedViewTargetId.value : undefined,
    target = targetId ? existing.find((item) => item.id === targetId) : undefined,
    name = (form.querySelector<Control>('[name="saved-view-name"]')?.value ?? '').trim(),
    query = orderedSearchText(savedViewQuery.value, savedViewQueryTokens.value, () => true).trim();
  if (targetId && !target) {
    savedViewError.value = 'That shared view no longer exists.';
    return;
  }
  if (!name) {
    savedViewError.value = 'Enter a view name.';
    return;
  }
  if (name.length > 80) {
    savedViewError.value = 'View names can be at most 80 characters.';
    return;
  }
  if (!customViewNameAvailable(name, existing, targetId)) {
    savedViewError.value = 'That view name is already in use.';
    return;
  }
  if (!query) {
    savedViewError.value = 'Enter a search query.';
    return;
  }
  if (query.length > 2_000) {
    savedViewError.value = 'Search queries can be at most 2,000 characters.';
    return;
  }
  const savedView: CustomView = target
      ? { ...target, name, query }
      : { id: uniqueCustomViewId(name, existing), name, query },
    next = target ? existing.map((item) => (item.id === target.id ? savedView : item)) : [...existing, savedView];
  savedViewBusy.value = true;
  savedViewError.value = '';
  try {
    const saved = await new Api(current.apiPath).saveCustomViews(next);
    if (project()?.id !== current.id) return;
    customViewsByProject.value = { ...customViewsByProject.value, [current.id]: saved };
    savedViewDialogOpen.value = false;
    if (target) {
      selectTicketView(customTicketViewId(savedView.id));
      showToast(`Updated ${savedView.name}.`);
    } else {
      selectTicketView(customTicketViewId(savedView.id));
      showToast(`Created ${savedView.name}.`);
    }
  } catch (reason) {
    if (project()?.id === current.id) savedViewError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    if (project()?.id === current.id) savedViewBusy.value = false;
  }
}
function openSavedViewDelete(viewId: string) {
  const id = customTicketViewKey(viewId as TicketView);
  if (!id || !customViewsFor().some((item) => item.id === id)) return;
  savedViewDeleteTargetId.value = id;
  savedViewDeleteBusy.value = false;
  savedViewDeleteError.value = '';
  requestAnimationFrame(() =>
    requestAnimationFrame(() =>
      document.querySelector<Control>('[data-component="saved-view-delete-dialog"]')?.show?.(),
    ),
  );
}
function closeSavedViewDelete() {
  if (savedViewDeleteBusy.value) return;
  savedViewDeleteTargetId.value = undefined;
  savedViewDeleteError.value = '';
}
async function deleteSavedView() {
  const current = project(),
    targetId = savedViewDeleteTargetId.value;
  if (!current || !targetId || savedViewDeleteBusy.value) return;
  const existing = customViewsFor(current.id),
    target = existing.find((item) => item.id === targetId);
  if (!target) {
    savedViewDeleteTargetId.value = undefined;
    return;
  }
  savedViewDeleteBusy.value = true;
  savedViewDeleteError.value = '';
  try {
    const saved = await new Api(current.apiPath).saveCustomViews(existing.filter((item) => item.id !== targetId));
    if (project()?.id !== current.id) return;
    customViewsByProject.value = { ...customViewsByProject.value, [current.id]: saved };
    savedViewDeleteTargetId.value = undefined;
    if (selectedView.value === customTicketViewId(targetId)) selectTicketView('all');
    showToast(`Deleted ${target.name}.`);
  } catch (reason) {
    if (project()?.id === current.id)
      savedViewDeleteError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    if (project()?.id === current.id) savedViewDeleteBusy.value = false;
  }
}
function commandRunFor(commandId: string) {
  return commandRuns.value.find((run) => run.command_id === commandId);
}
function showCommandDialog() {
  queueMicrotask(() => {
    const dialog = document.querySelector<HTMLDialogElement>(
      '[data-component="command-run-dialog"], [data-component="command-cancellation-dialog"]',
    );
    if (dialog && !dialog.open) dialog.showModal();
  });
}
function commandIcon(command: CommandDefinition): string {
  return (
    command.icon ??
    (command.kind === 'ai' ||
    command.program?.includes('hotsheet') ||
    command.args?.some((value) => value.includes('trigger'))
      ? 'send'
      : command.id.includes('test') || command.title.toLowerCase().includes('test')
        ? 'test'
        : 'build')
  );
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
      tickets.value = [created, ...tickets.value.filter((ticket) => ticket.id !== created.id)];
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
      workspaceSearchActive() && searchCounts?.projectId === current.id && searchCounts.signature === searchSignature(),
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
      terminals={{ inheritGlobalShellHistory: inheritGlobalShellHistory.value, message: terminalSettingsMessage.value }}
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
        history: view === 'day' ? history.filter((item) => item.resolvedAt >= cutoff) : view === 'week' ? history : [],
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
        workspaceSearchActive() || collectionLoading ? undefined : ticketCountsByProject.value[selectedProjectId.value],
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
function workspaceTerminalGroups(): TerminalDashboardGroup[] {
  const conversations = conversationStates.peek();
  return terminalGroups.value.map((group) => {
    const connections = driveConnectionsByProject.value[group.projectId] ?? [],
      chats = (terminalDrawerChatsByProject.value[group.projectId] ?? []).map((chat) => {
        const connection = connections.find((item) => item.id === chat.connectionId),
          state = conversations[chat.connectionId] ?? EMPTY_CONVERSATION;
        return {
          id: chat.id,
          projectId: group.projectId,
          projectName: group.projectName,
          name: chat.name,
          tool: aiToolLabel(chat.tool),
          busy: connection?.busy ?? false,
          summary: state.progress ?? state.messages.at(-1)?.content ?? state.activity?.at(-1)?.summary,
        };
      });
    return { ...group, chats, itemOrder: drawerTabOrder(group.projectId) };
  });
}
function globalWorkspaceSurfaceProps(): GlobalWorkspaceSurfaceProps {
  if (shellMode.value === 'terminals') {
    const groups = workspaceTerminalGroups();
    return {
      kind: 'terminals',
      dashboard: {
        groups,
        width: terminalDashboardSize.value.width,
        height: terminalDashboardSize.value.height,
        fitAcross: terminalFitAcross.value,
        fitHigh: terminalFitHigh.value,
        grouping: 'flow',
        magnifiedKey: magnifiedTerminalKey.value,
        hiddenKeys: terminalHiddenKeys(TERMINAL_DASHBOARD_VISIBILITY_SCOPE),
        loading: terminalDashboardLoading.value,
        message: terminalDashboardMessage.value,
        contextMenu: terminalContextMenu.value,
      },
    };
  }
  const statsProject = projects.value.find((item) => item.id === statsProjectId.value);
  return { kind: 'stats', projectName: statsProject?.name };
}
function projectTerminalDrawerProps(): TerminalDrawerProps | undefined {
  const current = project();
  if (!current) return;
  const conversations = conversationStates.peek(),
    group = terminalGroups.value.find((item) => item.projectId === current.id),
    chatTabs: TerminalDrawerChatTab[] = (terminalDrawerChatsByProject.value[current.id] ?? []).map((chat) => {
      const connection = (driveConnectionsByProject.value[current.id] ?? []).find(
          (item) => item.id === chat.connectionId,
        ),
        state = conversations[chat.connectionId] ?? EMPTY_CONVERSATION,
        selection = conversationAiSelection(chat.connectionId),
        descriptor = selection.descriptor,
        tool = aiToolLabel(chat.tool),
        selectedMessageIds = conversationSelectedMessages(chat.connectionId, state.messages).map(
          (message) => message.id,
        );
      return {
        id: chat.id,
        name: chat.name,
        tool,
        busy: connection?.busy ?? false,
        summary: state.progress ?? state.messages.at(-1)?.content ?? state.activity?.at(-1)?.summary,
        content: (
          <AIConversation
            open
            presentation="embedded"
            tool={tool}
            sessionId={connection?.session_id ?? chat.sourceSessionId}
            selectionId={chat.connectionId}
            selectedMessageIds={selectedMessageIds}
            messages={state.messages}
            draft={conversationDrafts.value[chat.connectionId] ?? ''}
            busy={connection?.busy ?? false}
            progress={state.progress}
            interruptible={Boolean(connection?.actions?.includes('interrupt'))}
            permissions={pendingPermissions().filter(
              (item) => item.projectId === current.id && item.connection === chat.connectionId,
            )}
            activity={state.activity}
            totalUsage={conversationUsage(state)}
            error={state.error ?? connection?.last_error}
            providerId={chat.tool}
            providers={aiToolOptions()}
            canChangeProvider={!chat.readOnly && aiTools.value.length > 1}
            model={selection.model}
            effort={selection.effort}
            models={descriptor?.models}
            efforts={selection.efforts}
            canChangeModel={descriptor?.actions?.includes('change_model')}
            canChangeEffort={descriptor?.actions?.includes('change_effort')}
            readOnly={chat.readOnly}
            savedSource={chat.savedSource}
          />
        ),
      };
    });
  return {
    projectId: current.id,
    projectName: current.name,
    sessions: group?.sessions ?? [],
    chatTabs,
    tabOrder: drawerTabOrder(current.id),
    width: terminalDrawerBounds.value.width,
    height: terminalDrawerBounds.value.height,
    fitAcross: terminalDrawerFitAcross.value,
    fitHigh: terminalDrawerFitHigh.value,
    selectedId: terminalDrawerSelected.value,
    magnifiedKey: magnifiedTerminalKey.value,
    loading: terminalDashboardLoading.value,
    message: terminalDashboardMessage.value,
    maximized: terminalDrawerMaximized.value,
    createMenuOpen: terminalDrawerCreateMenuOpen.value,
  };
}
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
function permissionPopupSurface() {
  const permission = visiblePermission();
  if (!permission) return undefined;
  const automation = permissionAutomation(permission.projectId),
    countdown =
      permissionCountdown?.key === permission.key
        ? formatPermissionCountdown(permissionCountdown.remainingMs)
        : undefined,
    error = permissionResolutionErrors.value[permission.key];
  return (
    <PermissionPopupSurface
      popup={{
        item: permission,
        state: error ? 'failed' : 'pending',
        error,
        countdown,
        countdownAction: automation.action === 'off' ? undefined : automation.action,
      }}
    />
  );
}
function aiConversationSurface() {
  if (!conversationOpen.value) return null;
  const current = project(),
    connectionId = conversationConnectionId.value;
  if (!current || !connectionId) return null;
  const connection = (driveConnectionsByProject.value[current.id] ?? []).find((item) => item.id === connectionId);
  if (!connection) return null;
  const state = conversationStates.peek()[connectionId] ?? EMPTY_CONVERSATION,
    selection = conversationAiSelection(connectionId),
    descriptor = selection.descriptor;
  return (
    <AIConversationSurface
      conversation={{
        open: true,
        tool: aiToolLabel(connection.tool),
        sessionId: connection.session_id,
        selectionId: connectionId,
        selectedMessageIds: conversationSelectedMessages(connectionId, state.messages).map((message) => message.id),
        messages: state.messages,
        draft: conversationDrafts.value[connectionId] ?? '',
        busy: connection.busy,
        progress: state.progress,
        interruptible: Boolean(connection.actions?.includes('interrupt')),
        permissions: pendingPermissions().filter(
          (item) => item.projectId === current.id && item.connection === connectionId,
        ),
        activity: state.activity,
        totalUsage: conversationUsage(state),
        error: state.error ?? connection.last_error,
        feedbackAvailable: Boolean(selectedTicket.value && canAddNotes()),
        foreground: permissionPopupSurface(),
        providerId: connection.tool,
        providers: aiToolOptions(),
        canChangeProvider: aiTools.value.length > 1,
        model: selection.model,
        effort: selection.effort,
        models: descriptor?.models,
        efforts: selection.efforts,
        canChangeModel: descriptor?.actions?.includes('change_model'),
        canChangeEffort: descriptor?.actions?.includes('change_effort'),
      }}
    />
  );
}
function repositoryStatusSurface() {
  const detail = repositoryDetail.value;
  return (
    <RepositoryStatusSurface
      repository={
        project()
          ? {
              status: repository.value,
              error: repositoryError.value,
              initialized: repository.value?.initialized !== false,
              setupStep: repositorySetupStep.value,
              setupBusy: repositorySetupBusy.value,
              setupError: repositorySetupError.value,
              refreshing: repositoryRefreshing.value,
              view: repositoryView.value,
              fileMenu: repositoryFileMenu.value,
              selectedFiles: repositorySelectedFiles.value,
              comparison: repositoryComparison.value,
              expandedCommits: expandedCodeReviewCommits.value,
              detailFiles: detail.view === repositoryView.value ? detail.files : [],
              detailCommits: detail.view === repositoryView.value ? detail.commits : [],
              detailLoading: detail.view === repositoryView.value && detail.loading,
              detailError: detail.view === repositoryView.value ? detail.error : '',
              detailHasMore: detail.view === repositoryView.value && detail.nextCursor !== undefined,
            }
          : undefined
      }
    />
  );
}
function changeEvidenceSurfaceProps(readerScope?: string): Parameters<typeof ChangeEvidenceSurface>[0]['evidence'] {
  if (changeEvidenceReader.value !== readerScope) return;
  return {
    review: codeReview.value,
    view: changeEvidenceView.value,
    fileMenu: repositoryFileMenu.value,
    selectedFiles: repositorySelectedFiles.value,
    platform: repository.value?.platform,
  };
}
function changeEvidenceSurface(readerScope?: string) {
  return <ChangeEvidenceSurface evidence={changeEvidenceSurfaceProps(readerScope)} />;
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
function commandDialogSurface() {
  const id = commandDialogId.value,
    command = commandDefinitions.value.find((item) => item.id === id),
    run = id ? commandRunFor(id) : undefined;
  return <CommandDialogSurface command={command} run={run} confirmStop={commandStopConfirmation.value} />;
}
function gallerySurface() {
  const active = attachmentGalleryUrl.value,
    images = galleryImages(),
    image = active ? images.find((item) => item.url === active || item.aliases?.includes(active)) : undefined;
  return (
    <GallerySurface
      gallery={
        active
          ? {
              images,
              activeUrl: active,
              geometry: attachmentGalleryGeometry.value,
              selectedScale: attachmentGalleryScale.value,
              annotations: attachmentGalleryAnnotations.value,
              markup: attachmentGalleryMarkup.value,
              drawMode: attachmentGalleryDrawMode.value,
              selectedAnnotation: attachmentGallerySelectedAnnotation.value,
              playheadMs: attachmentGalleryLivePlayhead,
              durationMs: attachmentGalleryDuration.value,
              playing: attachmentGalleryPlaying.value,
              volume: attachmentGalleryVolume.value,
              muted: attachmentGalleryMuted.value,
              volumeOpen: attachmentGalleryVolumeOpen.value,
              annotationEnabled: Boolean(image?.attachmentId),
            }
          : undefined
      }
      menu={attachmentMenuSurfaceProps({ insideGallery: true })}
    />
  );
}
function stopGallerySvgClock() {
  if (attachmentGallerySvgFrame !== undefined) cancelAnimationFrame(attachmentGallerySvgFrame);
  attachmentGallerySvgFrame = undefined;
  attachmentGallerySvgPreviousFrame = undefined;
}
function galleryTimeLabel(milliseconds: number) {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
function updateGalleryPlaybackPresentation(milliseconds: number) {
  const next = Math.max(0, Math.min(attachmentGalleryDuration.value, Math.round(milliseconds)));
  attachmentGalleryLivePlayhead = next;
  const gallery = document.querySelector<HTMLElement>('[data-component="attachment-gallery"]');
  if (!gallery) return;
  const slider = gallery.querySelector<HTMLInputElement>('input[name="gallery-playhead"]');
  if (slider && slider.value !== String(next)) slider.value = String(next);
  const current = gallery.querySelector<HTMLElement>('[data-gallery-current-time="true"]');
  if (current) current.textContent = galleryTimeLabel(next);
  for (const annotation of gallery.querySelectorAll<HTMLElement>(
    '.attachment-gallery__annotation[data-annotation-start]',
  )) {
    const start = Number(annotation.dataset.annotationStart),
      end = Number(annotation.dataset.annotationEnd ?? annotation.dataset.annotationStart);
    annotation.hidden = !attachmentGalleryAnnotationVisible(
      { id: 'presentation', x: 0, y: 0, width: 0, height: 0, start_ms: start, end_ms: end, text: '' },
      next,
      attachmentGalleryDuration.value,
    );
  }
}
function gallerySvgClock(timestamp: number) {
  if (!attachmentGalleryPlaying.value || !attachmentGalleryDuration.value) {
    stopGallerySvgClock();
    return;
  }
  const elapsed = attachmentGallerySvgPreviousFrame === undefined ? 0 : timestamp - attachmentGallerySvgPreviousFrame;
  attachmentGallerySvgPreviousFrame = timestamp;
  updateGalleryPlaybackPresentation((attachmentGalleryLivePlayhead + elapsed) % attachmentGalleryDuration.value);
  attachmentGallerySvgFrame = requestAnimationFrame(gallerySvgClock);
}
const svgDurationMilliseconds = (value: string) => {
  const match = value.trim().match(/^(\d+(?:\.\d+)?)(ms|s)$/);
  return match ? Number(match[1]) * (match[2] === 's' ? 1000 : 1) : 0;
};
async function detectAnimatedGallerySvg(url: string) {
  try {
    const response = await fetch(url);
    if (!response.ok || attachmentGalleryUrl.value !== url) return;
    const document = new DOMParser().parseFromString(await response.text(), 'image/svg+xml'),
      animations = [...document.querySelectorAll('animate, animateMotion, animateTransform, set')],
      duration = Math.max(0, ...animations.map((node) => svgDurationMilliseconds(node.getAttribute('dur') ?? '')));
    if (!animations.length || attachmentGalleryUrl.value !== url) return;
    attachmentGalleryDuration.value = duration || 5000;
    attachmentGalleryPlaying.value = true;
    attachmentGallerySvgFrame = requestAnimationFrame(gallerySvgClock);
  } catch {
    /* the image remains viewable even if animation metadata cannot be inspected */
  }
}
function activeAttachmentGalleryVideo(target?: EventTarget | null) {
  const video = document.querySelector<HTMLVideoElement>('.attachment-gallery video');
  return video && (!target || target === video) ? video : undefined;
}
function disposeAttachmentGalleryVideo() {
  const video = activeAttachmentGalleryVideo();
  if (video) releaseAttachmentGalleryVideo(video);
}
function resetAttachmentGallery(url?: string) {
  finishGalleryAnnotationSession();
  stopGallerySvgClock();
  disposeAttachmentGalleryVideo();
  attachmentGalleryObserver?.disconnect();
  attachmentGalleryObserver = undefined;
  const image = url ? galleryImages().find((item) => item.url === url || item.aliases?.includes(url)) : undefined,
    attachment = selectedTicket.value?.attachments.find((item) => item.id === image?.attachmentId);
  batch(() => {
    attachmentGalleryUrl.value = url;
    attachmentGalleryScale.value = undefined;
    attachmentGalleryGeometry.value = { naturalWidth: 0, naturalHeight: 0, availableWidth: 0, availableHeight: 0 };
    attachmentMenu.value = undefined;
    attachmentGalleryMarkup.value = false;
    attachmentGalleryDrawMode.value = false;
    attachmentGallerySelectedAnnotation.value = undefined;
    attachmentGalleryPlayhead.value = 0;
    attachmentGalleryDuration.value = 0;
    attachmentGalleryPlaying.value = false;
    attachmentGalleryVolume.value = 1;
    attachmentGalleryMuted.value = false;
    attachmentGalleryVolumeOpen.value = false;
    attachmentGalleryAnnotations.value = attachment?.annotations?.map((item) => ({ ...item })) ?? [];
  });
  attachmentGalleryLivePlayhead = 0;
  attachmentGalleryLiveVolume = 1;
  attachmentAnnotationGesture = undefined;
  attachmentRangeGesture = undefined;
  attachmentSwipeGesture = undefined;
  if (image?.name.toLowerCase().endsWith('.svg')) void detectAnimatedGallerySvg(url!);
}
function syncAttachmentGalleryMeasurement() {
  attachmentGalleryObserver?.disconnect();
  attachmentGalleryObserver = undefined;
  const gallery = document.querySelector<HTMLDialogElement>('dialog[data-component="attachment-gallery"]');
  if (gallery && !gallery.open) gallery.showModal();
  const stage = document.querySelector<HTMLElement>('[data-gallery-zoom-stage="true"]'),
    media = document.querySelector<HTMLImageElement | HTMLVideoElement>('[data-gallery-media="true"]');
  if (!stage || !media) return;
  const update = () => {
    const canvas = stage.firstElementChild instanceof HTMLElement ? stage.firstElementChild : undefined,
      style = canvas ? getComputedStyle(canvas) : undefined,
      horizontal =
        (Number.parseFloat(style?.paddingLeft ?? '0') || 0) + (Number.parseFloat(style?.paddingRight ?? '0') || 0),
      vertical =
        (Number.parseFloat(style?.paddingTop ?? '0') || 0) + (Number.parseFloat(style?.paddingBottom ?? '0') || 0),
      naturalWidth = media instanceof HTMLVideoElement ? media.videoWidth : media.naturalWidth,
      naturalHeight = media instanceof HTMLVideoElement ? media.videoHeight : media.naturalHeight,
      next = {
        naturalWidth,
        naturalHeight,
        availableWidth: Math.max(1, stage.clientWidth - horizontal),
        availableHeight: Math.max(1, stage.clientHeight - vertical),
      },
      previous = attachmentGalleryGeometry.value;
    if (
      Object.keys(next).some(
        (key) => next[key as keyof AttachmentGalleryGeometry] !== previous[key as keyof AttachmentGalleryGeometry],
      )
    )
      attachmentGalleryGeometry.value = next;
  };
  update();
  attachmentGalleryObserver = new ResizeObserver(update);
  attachmentGalleryObserver.observe(stage);
}
// The menu renders inside whichever surface owns the trigger. When it was opened from within the
// modal ticket reader (menu.reader = that reader's frame id) it must render as a descendant of that
// dialog, or the reader's modality leaves it inert and painted beneath (HS2-EZ10RS); otherwise it
// renders at the app root (the side inspector or the media gallery).
function attachmentMenuSurfaceProps({
  insideGallery = false,
  readerScope,
}: { insideGallery?: boolean; readerScope?: string } = {}): Parameters<typeof AttachmentContextMenuSurface>[0]['menu'] {
  const menu = attachmentMenu.value;
  if (!menu) return;
  if (readerScope !== undefined) {
    if (menu.reader !== readerScope) return;
  } else if (menu.reader || Boolean(attachmentGalleryUrl.value) !== insideGallery) return;
  const reveal = /Mac/i.test(navigator.userAgent)
    ? 'Show in Finder'
    : /Win/i.test(navigator.userAgent)
      ? 'Show in File Explorer'
      : 'Show in file manager';
  return { x: menu.x, y: menu.y, kind: menu.kind, revealLabel: reveal };
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

function renderMainShell() {
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
  const tabs = [
    ...projects.value.map((item) => {
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
          shellMode.value === 'terminals' ? <TerminalOperationsSurface {...terminalOperationsSurfaceProps()} /> : <></>
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
        overlay={popup}
      />
    );
  }
  if (restoreFailure)
    return (
      <MainShell
        tabs={tabs}
        mode="project"
        header={<WorkspaceIdentity projectName={restoreFailure.name} />}
        pageHeader={
          <div class="app-heading" data-component="heading" data-has-icon="false">
            <Toolbar
              dividerSides=""
              leading={
                <ToolbarText text="Project unavailable" id="workspace-page-title" size="xlarge" headingLevel={1} />
              }
            />
          </div>
        }
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
    corruptTicket = corruptKey ? corruptTickets.value.find((item) => corruptTicketKey(item) === corruptKey) : undefined;
  const canCreate =
    !['settings', 'notifications'].includes(viewMode.value) && canCreateTicketInView(selectedView.value);
  // Mobile replaces the page-header view title with a view Select (reusing the workspace-grid rail's
  // choices) so ticket views can be switched without the desktop title chrome (HS2-4C5RM7). Settings and
  // Notifications keep their plain titled header.
  const railView = selectedView.value === 'errors' ? 'all' : selectedView.value;
  const mobileViewChoices = [
    { value: 'all', label: 'Queue' },
    { value: 'backlog', label: 'Backlog' },
    { value: 'archive', label: 'Archive' },
    ...((projectTicketCounts(current.id).trash ?? 0) > 0 || railView === 'trash'
      ? [{ value: 'trash', label: 'Trash' }]
      : []),
    ...customViewsFor(current.id).map((view) => ({ value: customTicketViewId(view.id), label: view.name })),
  ];
  const desktopPageHeader = (
    <div class="app-heading" data-component="heading" data-has-icon="false">
      <Toolbar
        dividerSides=""
        leading={
          <ToolbarText
            text={
              viewMode.value === 'notifications'
                ? notificationViewTitle(notificationView.value)
                : viewMode.value === 'settings'
                  ? settingsCategoryTitle(settingsCategory())
                  : customTicketViewKey(selectedView.value)
                    ? ticketViewTitle(selectedView.value)
                    : searchQuery.value.trim() || searchTokens.value.length
                      ? 'Search results'
                      : ticketViewTitle(selectedView.value)
            }
            id="workspace-page-title"
            size="xlarge"
            headingLevel={1}
          />
        }
        trailing={
          !['settings', 'notifications'].includes(viewMode.value)
            ? ticketViewAction(selectedView.value, canCreate)
            : undefined
        }
      />
    </div>
  );
  const pageHeader =
    viewportMobile.value && !['settings', 'notifications'].includes(viewMode.value) ? (
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
      desktopPageHeader
    );
  // The bottom terminal drawer belongs to ticket views only; Settings and Notifications hide it
  // while preserving the user's open/closed preference so it returns on the next ticket view (HS2-EQEJC7).
  const drawerViewAllowed = !['settings', 'notifications'].includes(viewMode.value);
  return (
    <MainShell
      tabs={tabs}
      mode="project"
      mobile={viewportMobile.value}
      sidebar={<SidebarSurface {...sidebarSurfaceProps()} />}
      sidebarVisible={viewportMobile.value ? mobileOverlay.value.sidebar : sidebarVisible.value}
      sidebarSize={sidebarSize.value}
      header={viewportMobile.value ? <></> : <WorkspaceIdentity projectName={current.name} />}
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
mount(appRoot, () => {
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
      {renderMainShell()}
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
});

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
function activeGalleryAttachment() {
  const active = attachmentGalleryUrl.value,
    image = active ? galleryImages().find((item) => item.url === active || item.aliases?.includes(active)) : undefined;
  return selectedTicket.value?.attachments.find((item) => item.id === image?.attachmentId);
}
function beginGalleryAnnotationSession() {
  const current = project(),
    ticket = selectedTicket.value,
    attachment = activeGalleryAttachment();
  if (!current || !ticket || !attachment) return;
  attachmentAnnotationSession = {
    projectId: current.id,
    ticketId: ticket.id,
    attachmentId: attachment.id,
    before: attachmentGalleryAnnotations.value.map((item) => ({ ...item })),
  };
}
function finishGalleryAnnotationSession() {
  const session = attachmentAnnotationSession;
  if (!session) return;
  attachmentAnnotationSession = undefined;
  const annotations = attachmentGalleryAnnotations.value.map((item) => ({ ...item }));
  if (JSON.stringify(session.before) === JSON.stringify(annotations)) return;
  attachmentAnnotationSave = attachmentAnnotationSave.then(async () => {
    try {
      const result = await api().updateCheckoutAttachmentAnnotations(
        session.projectId,
        session.ticketId,
        session.attachmentId,
        annotations,
      );
      if (selectedTicket.value?.id === session.ticketId) selectedTicket.value = result.ticket;
      showToast('Annotations saved.');
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  });
}
function shiftGallery(delta: number) {
  const active = attachmentGalleryUrl.value;
  if (!active) return;
  const url = attachmentGalleryShiftUrl(galleryImages(), active, delta);
  if (url) resetAttachmentGallery(url);
}
function activeTicketSurface(): ParentNode {
  return (readerOpen.value ? document.querySelector('[data-component="ticket-reader"]') : null) ?? document;
}

wireProjectLifecycleInteractions({
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
  providerSettingsError,
  ticketSourceRemoteError,
  connectCreatedGitRemote,
  createProjectGitSource,
  chooseProjectPath,
  recoverUnhealthyProjectServer,
});

wireRepositoryInteractions({
  repository,
  repositoryView,
  repositorySetupStep,
  repositorySetupError,
  repositoryFileMenu,
  repositorySelectedFiles,
  get repositoryFileSelectionAnchor() {
    return repositoryFileSelectionAnchor;
  },
  set repositoryFileSelectionAnchor(value) {
    repositoryFileSelectionAnchor = value;
  },
  repositoryComparison,
  expandedCodeReviewCommits,
  loadRepositoryDetail,
  refreshRepositoryStatus,
  initializeRepository,
  connectRepositoryRemote,
  skipRepositoryRemote,
  repositoryDetail,
  project,
  showToast,
  error,
  codeReview,
  changeEvidenceView,
  changeEvidenceReader,
  selectedTicket,
  codeReviewMessage,
  openProject,
});
let draggedCommandIds: string[] = [];
function clearCommandDropIndicators() {
  document
    .querySelectorAll<HTMLElement>('[data-command-drop-position]')
    .forEach((element) => delete element.dataset.commandDropPosition);
  document
    .querySelectorAll<HTMLElement>('[data-command-drop-active]')
    .forEach((element) => delete element.dataset.commandDropActive);
}
function clearCommandDrag() {
  draggedCommandIds = [];
  document
    .querySelectorAll<HTMLElement>('[data-command-dragging]')
    .forEach((element) => delete element.dataset.commandDragging);
  clearCommandDropIndicators();
}

wireNavigationAndTabInteractions({
  projects,
  currentRememberedProjectRoots,
  project,
  persistDrawerTabOrder,
  currentDrawerTabIds,
  focusDrawerTab,
  revealCorruptTicket,
  queueCorruptTicketRepair,
  corruptTickets,
  selectedCorruptKey,
  selectedTicket,
  selectedTicketSlugs,
  get ticketSelectionAnchor() {
    return ticketSelectionAnchor;
  },
  set ticketSelectionAnchor(value) {
    ticketSelectionAnchor = value;
  },
  setInspectorVisible,
  error,
  statsProjectId,
  setShellMode,
  selectTerminalRailProject,
  selectTicketView,
  terminalRailDirection,
  terminalRailScreen,
  selectProjectTab,
  retryProjectRestore,
});

wireTerminalInteractions({
  terminalDrawerBounds,
  terminalDashboardSize,
  terminalDrawerFitHigh,
  terminalFitAcross,
  terminalFitHigh,
  get terminalPreviewClickTimer() {
    return terminalPreviewClickTimer;
  },
  set terminalPreviewClickTimer(value) {
    terminalPreviewClickTimer = value;
  },
  terminalSession,
  get pendingTerminalFocus() {
    return pendingTerminalFocus;
  },
  set pendingTerminalFocus(value) {
    pendingTerminalFocus = value;
  },
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
  terminalDrawerCreateMenuOpen,
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
  projects,
  currentDrawerTabIds,
  project,
  terminalGroups,
  terminalRename,
  closeDrawerTabIds,
  saveTerminalName,
});

wireTicketSelectionInteractions({
  viewportMobile,
  mobileOverlay,
  selectTickets,
  selectionOrder,
  selectedTicketSlugs,
  terminalRailDirection,
  terminalRailScreen,
  selectedTicket,
  visibleTickets,
  selectedView,
  hideVerifiedColumn,
  cancelTicketDrafts,
  selectedCorruptKey,
  get ticketSelectionAnchor() {
    return ticketSelectionAnchor;
  },
  set ticketSelectionAnchor(value) {
    ticketSelectionAnchor = value;
  },
  setInspectorVisible,
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
  get bulkTicketSlugs() {
    return bulkTicketSlugs;
  },
  set bulkTicketSlugs(value) {
    bulkTicketSlugs = value;
  },
  bulkTicketDialog,
  openEmptyTrash,
  emptyTrash,
  setTicketCloseReason,
  searchTicketCloseTargets,
  ticketCloseDialog,
  submitTicketClose,
  closeTicketCloseDialog,
  get ticketLinkReturnFocus() {
    return ticketLinkReturnFocus;
  },
  set ticketLinkReturnFocus(value) {
    ticketLinkReturnFocus = value;
  },
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
});

wireViewAndSavedViewInteractions({
  selectedCorruptKey,
  selectedTicketSlugs,
  get ticketSelectionAnchor() {
    return ticketSelectionAnchor;
  },
  set ticketSelectionAnchor(value) {
    ticketSelectionAnchor = value;
  },
  selectedTicket,
  selectTicketView,
  isEditableEvent,
  openSavedViewDialog,
  savedViewMenu,
  openSavedViewRename,
  openSavedViewDelete,
  savedViewName,
  savedViewError,
  readInlineSearchField,
  savedViewQueryTokens,
  updateSavedViewQuery,
  focusSavedViewQuery,
  removeSavedViewQueryToken,
  editSavedViewQueryToken,
  savedViewQuery,
  saveSavedView,
  closeSavedViewDialog,
  savedViewBusy,
  deleteSavedView,
  closeSavedViewDelete,
  savedViewDeleteBusy,
});

wireCommandAndAiInteractions({
  commandGroupExpanded,
  persistWorkspacePreferences,
  project,
  commandGroupsCollapsed,
  toggleSidebarDrive,
  driveOptionsOpen,
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
  selectedTicket,
  canAddNotes,
  updateSelected,
  showToast,
  get commandLongPressFired() {
    return commandLongPressFired;
  },
  set commandLongPressFired(value) {
    commandLongPressFired = value;
  },
  runCommand,
  commandDialogId,
  commandStopConfirmation,
  commandRuns,
  error,
  commandSettingsEditingId,
  commandIconSearch,
  addCommandSetting,
  manualModelDialog,
  deleteCommandSetting,
  addCommandGroup,
  deleteCommandGroup,
  selectCommandRow,
  commandSelection,
  get draggedCommandIds() {
    return draggedCommandIds;
  },
  set draggedCommandIds(value) {
    draggedCommandIds = value;
  },
  commandSettingsDefinitions,
  selectCommandSetting,
  clearCommandDropIndicators,
  clearCommandDrag,
  reorderCommandSettings,
  updateCommandSetting,
  updateCommandAiSelection,
  effectiveCommandAiSelection,
  showLoadingActivity,
  inheritGlobalShellHistory,
  terminalSettingsMessage,
  trashSettingsMessagesByProject,
  trashCleanupDaysByProject,
  resetProgressiveTicketRendering,
  viewMode,
  setSettingsCategory,
  refreshProviderConnections,
  refreshTerminalSettings,
  refreshTrashSettings,
  capturingShortcutId,
  keyboardShortcutOverrides,
  appleShortcutPlatform,
  saveAiDefaults,
  selectDefaultModel,
  restoreCommandEditorAfterManualModel,
  get manualModelDialogShown() {
    return manualModelDialogShown;
  },
  set manualModelDialogShown(value) {
    manualModelDialogShown = value;
  },
  aiDefaults,
  ticketSourceSetupProject,
  providerSetupKind,
  providerEditingId,
  providerSettingsError,
  createdGitTicketStore,
  ticketSourceSetupNavigation,
  providerConnections,
  githubAuth,
  cancelGitHubSignIn,
  startGitHubSignIn,
  saveExternalProvider,
});

wireNotificationAndLinkInteractions({
  notificationView,
  project,
  permissionTimer,
  get permissionCountdown() {
    return permissionCountdown;
  },
  set permissionCountdown(value) {
    permissionCountdown = value;
  },
  permissionAutomationByProject,
  updatePermissionTimer,
  permissionRevision,
  permissionInbox,
  pendingPermissions,
  resolvePermission,
  error,
  selectedProjectId,
  hideVerifiedByProject,
  get ticketLinkReturnFocus() {
    return ticketLinkReturnFocus;
  },
  set ticketLinkReturnFocus(value) {
    ticketLinkReturnFocus = value;
  },
  selectLinkedTicket,
  ticketLinkChoice,
  openTicketLinkMatch,
  cancelTicketLinkChoice,
});

wireSearchAndComposerInteractions({
  searchOpen,
  readWorkspaceSearchEditor,
  updateTicketSearch,
  restoreWorkspaceSearchEnd,
  readInlineSearchField,
  savedViewQueryTokens,
  updateSavedViewQuery,
  focusSavedViewQuery,
  removeWorkspaceSearchToken,
  removeSavedViewQueryToken,
  addWorkspaceSearchTag,
  editWorkspaceSearchToken,
  searchHelpOpen,
  replaceActiveWorkspaceSearchToken,
  focusWorkspaceSearch,
  get workspaceSearchEditingToken() {
    return workspaceSearchEditingToken;
  },
  set workspaceSearchEditingToken(value) {
    workspaceSearchEditingToken = value;
  },
  searchQuery,
  searchTokens,
  scheduleTicketSearch,
  sort,
  sortDirection,
  resetProgressiveTicketRendering,
  persistWorkspacePreferences,
  selectedRows,
  executeBulkTicketAction,
  ticketContextMenu,
  viewMode,
  openTicketComposer,
  composerSubmitting,
  composerExpanded,
  resetTicketComposer,
  composerTitle,
  scheduleProjectSessionPersistence,
  composerDetails,
  composerCategory,
  composerUpNext,
  addNewTicketFiles,
  draftScope,
  composerAttachments,
  composerAttachmentMessage,
  composerAttachmentError,
  get draggedTickets() {
    return draggedTickets;
  },
  set draggedTickets(value) {
    draggedTickets = value;
  },
  submitNewTicket,
  tickets,
  history,
});

wireAttachmentAndGalleryInteractions({
  selectedTicket,
  addAttachments,
  project,
  api,
  attachmentMessage,
  showToast,
  refreshProject,
  get draggedGroupedAttachmentId() {
    return draggedGroupedAttachmentId;
  },
  set draggedGroupedAttachmentId(value) {
    draggedGroupedAttachmentId = value;
  },
  galleryImages,
  resetAttachmentGallery,
  shiftGallery,
  attachmentGalleryGeometry,
  attachmentGalleryScale,
  attachmentGalleryUrl,
  attachmentMenu,
  syncAttachmentGalleryMeasurement,
  activeAttachmentGalleryVideo,
  attachmentGalleryDuration,
  error,
  attachmentGalleryMarkup,
  finishGalleryAnnotationSession,
  beginGalleryAnnotationSession,
  attachmentGalleryDrawMode,
  attachmentGallerySelectedAnnotation,
  attachmentGalleryAnnotations,
  updateGalleryPlaybackPresentation,
  attachmentGalleryPlayhead,
  get attachmentRangeGesture() {
    return attachmentRangeGesture;
  },
  set attachmentRangeGesture(value) {
    attachmentRangeGesture = value;
  },
  get attachmentAnnotationGesture() {
    return attachmentAnnotationGesture;
  },
  set attachmentAnnotationGesture(value) {
    attachmentAnnotationGesture = value;
  },
  get attachmentGalleryLivePlayhead() {
    return attachmentGalleryLivePlayhead;
  },
  set attachmentGalleryLivePlayhead(value) {
    attachmentGalleryLivePlayhead = value;
  },
  attachmentGalleryPlaying,
  get attachmentGallerySvgPreviousFrame() {
    return attachmentGallerySvgPreviousFrame;
  },
  set attachmentGallerySvgPreviousFrame(value) {
    attachmentGallerySvgPreviousFrame = value;
  },
  get attachmentGallerySvgFrame() {
    return attachmentGallerySvgFrame;
  },
  set attachmentGallerySvgFrame(value) {
    attachmentGallerySvgFrame = value;
  },
  gallerySvgClock,
  stopGallerySvgClock,
  attachmentGalleryVolumeOpen,
  get attachmentGalleryLiveVolume() {
    return attachmentGalleryLiveVolume;
  },
  set attachmentGalleryLiveVolume(value) {
    attachmentGalleryLiveVolume = value;
  },
  attachmentGalleryMuted,
  attachmentGalleryVolume,
  get attachmentSwipeGesture() {
    return attachmentSwipeGesture;
  },
  set attachmentSwipeGesture(value) {
    attachmentSwipeGesture = value;
  },
  canUseAttachments,
});

wireInspectorAndEditorInteractions({
  selectedTicket,
  updateSelectedTracked,
  showToast,
  error,
  readerOpen,
  readerDetailsDraft,
  get readerDetailsDraftBase() {
    return readerDetailsDraftBase;
  },
  set readerDetailsDraftBase(value) {
    readerDetailsDraftBase = value;
  },
  detailsDraft,
  get detailsDraftBase() {
    return detailsDraftBase;
  },
  set detailsDraftBase(value) {
    detailsDraftBase = value;
  },
  titleDraft,
  get titleDraftBase() {
    return titleDraftBase;
  },
  set titleDraftBase(value) {
    titleDraftBase = value;
  },
  readerBlockedReasonDraft,
  get readerBlockedReasonDraftBase() {
    return readerBlockedReasonDraftBase;
  },
  set readerBlockedReasonDraftBase(value) {
    readerBlockedReasonDraftBase = value;
  },
  blockedReasonDraft,
  get blockedReasonDraftBase() {
    return blockedReasonDraftBase;
  },
  set blockedReasonDraftBase(value) {
    blockedReasonDraftBase = value;
  },
  readerNoteDraft,
  get readerNoteDraftBase() {
    return readerNoteDraftBase;
  },
  set readerNoteDraftBase(value) {
    readerNoteDraftBase = value;
  },
  noteDraft,
  get noteDraftBase() {
    return noteDraftBase;
  },
  set noteDraftBase(value) {
    noteDraftBase = value;
  },
  fieldConflictResolution,
  fieldConflict,
  canUpdateSelected,
  titleEditing,
  activeTicketSurface,
  titleAutosave,
  tagsAutosave,
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
  canAddNotes,
  composingNote,
  newNoteDraft,
  scheduleProjectSessionPersistence,
  updateSelected,
  readerNoteAutosave,
  noteAutosave,
  readerInlineFeedbackReplies,
  readerFeedbackChoiceSelections,
  readerFeedbackChoiceAnchors,
  project,
  canDeleteNotes,
  api,
  refreshProject,
  viewMode,
  selectedView,
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
  codeReviewMessage,
  codeReview,
});

wireShellAndGlobalInteractions({
  viewportMobile,
  mobileOverlay,
  setInspectorVisible,
  setSidebarVisible,
  sidebarVisible,
  get appRegionResizeDrag() {
    return appRegionResizeDrag;
  },
  set appRegionResizeDrag(value) {
    appRegionResizeDrag = value;
  },
  appRegionSize,
  setTerminalDrawerVisible,
  setAppRegionSize,
  terminalDrawerMax,
  syncTerminalDrawerMaximum,
  updateTerminalDrawerBounds,
  get draggedTickets() {
    return draggedTickets;
  },
  set draggedTickets(value) {
    draggedTickets = value;
  },
  project,
  selectedTicketSlugs,
  tickets,
  history,
  executeBulkTicketAction,
  copyDraggedTickets,
  projects,
  capturingShortcutId,
  keyboardShortcutOverrides,
  isEditableEvent,
  appleShortcutPlatform,
  searchOpen,
  focusWorkspaceSearch,
  inspectorVisible,
  terminalDrawerVisible,
  switchWorkspaceView,
  setShellMode,
  shellMode,
  statsProjectId,
  selectedProjectId,
  selectProjectTab,
  currentDrawerTabIds,
  terminalDrawerSelected,
  selectDrawerItem,
  openTicketComposer,
  ticketWorkAreaFocused,
  ordinaryTextSelected,
  get clipboard() {
    return clipboard;
  },
  set clipboard(value) {
    clipboard = value;
  },
  copySelection,
  pasteSelection,
  ticketContextMenu,
  appTabContextMenu,
  terminalContextMenu,
  terminalVisibilityContextMenu,
  repositoryFileMenu,
  attachmentMenu,
  get commandLongPressFired() {
    return commandLongPressFired;
  },
  set commandLongPressFired(value) {
    commandLongPressFired = value;
  },
  get commandLongPressTimer() {
    return commandLongPressTimer;
  },
  set commandLongPressTimer(value) {
    commandLongPressTimer = value;
  },
  openCommandHistory,
  attachmentGalleryUrl,
  shiftGallery,
  resetAttachmentGallery,
  magnifiedTerminalKey,
  completePointerDetailsFinish,
  schedulePointerDetailsFinish,
});
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
    });
    startPermissionUpdates();
    syncProjectChangeStreams();
  } finally {
    initialProjectRestoreComplete = true;
    initialProjectRestorePending.value = false;
  }
})();
