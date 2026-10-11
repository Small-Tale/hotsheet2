import type { TokenSearchModel } from '@kerfjs/ui/token-search-model';
import type { Signal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

import type {
  Checkout,
  CorruptTicket,
  CustomView,
  ProviderAccount,
  ProviderConnection,
  TicketRow as WireTicketRow,
  ToolConnection,
} from '../api';
import { type BulkUpdateProgress } from '../bulk-update-progress';
import { AiFeedbackDialog, type AiFeedbackDialogState } from '../components/ai-feedback-dialog';
import { AppError } from '../components/app-error';
import { AppLoadingIndicator } from '../components/app-loading-indicator';
import { BulkTicketDialog, type BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import type { ConversationExportDialogState } from '../components/conversation-export-dialog';
import { ConversationExportDialog } from '../components/conversation-export-dialog';
import { Hs1MigrationDialog } from '../components/hs1-migration';
import type { ManualModelDialogState } from '../components/manual-model-dialog';
import { ManualModelDialog } from '../components/manual-model-dialog';
import { type NotificationView } from '../components/notification-navigation';
import { ProjectCloseDialog, type ProjectCloseDialogState } from '../components/project-close-dialog';
import { ProjectDialog, projectDialogRoot, RemoteProjectDialog } from '../components/project-dialog';
import type { ProjectStripMode } from '../components/project-strip-actions';
import { type AppTabKind } from '../components/project-tab-context-menu';
import type { ExternalProviderKind, GithubAuthState } from '../components/provider-setup-form';
import { QuickTicketComposer } from '../components/quick-ticket-composer';
import {
  AppTabMenuSurface,
  AttachmentContextMenuSurface,
  ConnectionDetailsSurface,
  NotWorkingSurface,
  ReaderLayersSurface,
} from '../components/reader-overlay-surfaces';
import { SavedViewDeleteDialog, SavedViewDialog } from '../components/saved-view-dialog';
import { type SettingsCategory } from '../components/settings-navigation';
import {
  TerminalCopyDialog,
  type TerminalCopyState,
  TerminalEditMenu,
  type TerminalEditMenuState,
  TerminalPasteDialog,
  type TerminalPasteState,
} from '../components/terminal-clipboard-dialogs';
import { type TerminalDashboardGroup } from '../components/terminal-dashboard';
import { TerminalRenameDialog, type TerminalRenameTarget } from '../components/terminal-rename-dialog';
import {
  TerminalVisibilityDialog,
  TerminalVisibilityNameDialog,
  type TerminalVisibilityNamePrompt,
} from '../components/terminal-visibility-dialog';
import { TicketCloseDialog, type TicketCloseDialogState } from '../components/ticket-close-dialog';
import { TicketLinkChoiceDialog } from '../components/ticket-link-choice-dialog';
import { TicketSourceSetupDialog } from '../components/ticket-source-setup-dialog';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { syncConversationScroll } from '../conversation-scroll';
import type { NotWorkingTarget, PendingEvidence, Project, UnhealthyServerRecovery } from '../interactions/types';
import { type ProjectTicketSources, type TicketSourceChoice, writableTicketSources } from '../new-ticket-source';
import type { RenderMetrics } from '../render-metrics';
import {
  TERMINAL_DASHBOARD_VISIBILITY_SCOPE,
  type TerminalVisibilityState,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import type { TicketLinkMatch, TicketLinkReference } from '../ticket-link-resolution';
import { animateTicketMotion, captureTicketMotion } from '../ticket-motion';
import type { TicketScrollMemory } from '../ticket-scroll-state';
import { type TicketView } from '../ticket-views';
import type { TrackedSizeObserver } from '../tracked-size-observer';
import type { UiStabilityDiagnostics } from '../ui-stability-diagnostics';
import { renderStormSuppressionReason } from '../ui-stability-diagnostics';
import { syncVideoPosters } from '../video-posters';

/** Live application bindings createHotSheetApp reads (HS2-K0VC2C). */
export interface HotSheetAppDependencies {
  activeTicketCollectionKey: () => string;
  aiConversationSurface: () => SafeHtml | null;
  aiFeedbackDialog: Signal<AiFeedbackDialogState | undefined>;
  appRoot: HTMLElement;
  appTabContextMenu: Signal<
    { x: number; y: number; kind: AppTabKind; id: string; direction: 'left' | 'right' } | undefined
  >;
  attachmentMenuSurfaceProps: ({
    insideGallery,
    readerScope,
  }?: {
    insideGallery?: boolean;
    readerScope?: string;
  }) => Parameters<typeof AttachmentContextMenuSurface>[0]['menu'];
  backgroundProjectRefresh: () => boolean;
  bulkTicketDialog: Signal<BulkTicketDialogState | undefined>;
  bulkUpdateProgress: Signal<BulkUpdateProgress | undefined>;
  canStageNewTicketAttachments: () => boolean;
  changeEvidenceSurface: (readerScope?: string) => SafeHtml;
  commandDialogSurface: () => SafeHtml;
  composerAttachmentError: Signal<boolean>;
  composerAttachmentMessage: Signal<string>;
  composerAttachments: Signal<PendingEvidence[]>;
  composerCategory: Signal<string>;
  composerDetails: Signal<string>;
  composerExpanded: Signal<boolean>;
  composerScreening: Signal<boolean>;
  composerSubmitting: Signal<boolean>;
  composerTitle: Signal<string>;
  composerUpNext: Signal<boolean>;
  conversationExportDialog: Signal<ConversationExportDialogState | undefined>;
  conversationOpen: Signal<boolean>;
  conversationRenderRevision: Signal<number>;
  corruptTickets: Signal<CorruptTicket[]>;
  createdGitTicketStore: Signal<string>;
  customViewsFor: (projectId?: string) => CustomView[];
  defaultProvider: () => ProjectTicketSources | undefined;
  defaultProviders: Signal<Record<string, ProjectTicketSources | undefined>>;
  deferContainedListScrollRestore: boolean;
  driveConnectionsByProject: Signal<Record<string, ToolConnection[]>>;
  error: Signal<string>;
  gallerySurface: () => SafeHtml;
  githubAuth: Signal<GithubAuthState | undefined>;
  HotSheetShell: () => SafeHtml;
  hs1MigrationBusy: Signal<boolean>;
  hs1MigrationError: Signal<string>;
  hs1MigrationProject: Signal<Project | undefined>;
  initialProjectRestoreComplete: () => boolean;
  loading: Signal<boolean>;
  manualModelDialog: Signal<ManualModelDialogState | undefined>;
  newTicketSource: () => TicketSourceChoice | undefined;
  notWorkingError: Signal<string>;
  notWorkingFiles: Signal<PendingEvidence[]>;
  notWorkingNote: Signal<string>;
  notWorkingSubmitting: Signal<boolean>;
  notWorkingSurfaceProps: (
    target: NotWorkingTarget,
    note: string,
    files: PendingEvidence[],
    submitting: boolean,
    submissionError: string,
  ) => Parameters<typeof NotWorkingSurface>[0];
  notWorkingTarget: Signal<NotWorkingTarget>;
  permissionRevision: Signal<number>;
  project: () => Project | undefined;
  projectCloseDialog: Signal<ProjectCloseDialogState | undefined>;
  projectDialogError: Signal<string>;
  projectDialogOpen: Signal<boolean>;
  providerAccountChoice: Signal<string | undefined>;
  providerAccounts: Signal<ProviderAccount[]>;
  providerConnections: Signal<ProviderConnection[]>;
  providerEditingId: Signal<string | undefined>;
  providerRemovingId: Signal<string | undefined>;
  providerSettingsBusy: Signal<boolean>;
  providerSettingsError: Signal<string>;
  providerSetupKind: Signal<ExternalProviderKind | undefined>;
  readerLayersSurfaceProps: () => Parameters<typeof ReaderLayersSurface>[0];
  remoteProjectCheckouts: Signal<Checkout[]>;
  remoteProjectDialogOpen: Signal<boolean>;
  remoteProjectError: Signal<string>;
  remoteProjectLoading: Signal<boolean>;
  initialRenderedTicketCollectionKey: string;
  renderMetrics: RenderMetrics | undefined;
  repositoryStatusSurface: () => SafeHtml;
  savedViewBusy: Signal<boolean>;
  savedViewDeleteBusy: Signal<boolean>;
  savedViewDeleteError: Signal<string>;
  savedViewDeleteTargetId: Signal<string | undefined>;
  savedViewDialogMode: Signal<'create' | 'rename'>;
  savedViewDialogOpen: Signal<boolean>;
  savedViewDialogSession: Signal<number>;
  savedViewError: Signal<string>;
  savedViewHelpOpen: Signal<boolean>;
  savedViewName: Signal<string>;
  savedViewSearchModel: TokenSearchModel;
  searchMatchKeys: Signal<Set<string> | undefined>;
  shellMode: Signal<ProjectStripMode>;
  consumeSkipNextTicketMotion: () => boolean;
  syncAttachmentGalleryMeasurement: () => void;
  syncRepositoryNavStack: () => void;
  syncRepositoryPaginationObserver: () => void;
  syncTerminalDrawerMaximum: () => void;
  syncTerminalRailNavStack: () => void;
  syncTerminalViewportMounts: () => void;
  terminalCopy: Signal<TerminalCopyState | undefined>;
  terminalDashboardLoading: Signal<boolean>;
  terminalDashboardSizeObserver: TrackedSizeObserver;
  terminalDrawerSizeObserver: TrackedSizeObserver;
  terminalDrawerVisible: Signal<boolean>;
  terminalEditMenu: Signal<TerminalEditMenuState | undefined>;
  terminalPaste: Signal<TerminalPasteState | undefined>;
  terminalRename: Signal<TerminalRenameTarget | undefined>;
  terminalVisibility: Signal<TerminalVisibilityState>;
  terminalVisibilityContextMenu: Signal<{ id: string; x: number; y: number } | undefined>;
  terminalVisibilityDialogScope: Signal<string | undefined>;
  terminalVisibilityFilter: Signal<readonly TerminalVisibilityType[]>;
  terminalVisibilityNamePrompt: Signal<TerminalVisibilityNamePrompt | undefined>;
  ticketCloseDialog: Signal<TicketCloseDialogState | undefined>;
  ticketCollectionState: Signal<{ projectId: string; view: TicketView; status: 'loading' | 'error' } | undefined>;
  ticketLinkChoice: Signal<{ kind: 'choose'; reference: TicketLinkReference; matches: TicketLinkMatch[] } | undefined>;
  ticketRenderScheduled: () => boolean;
  tickets: Signal<WireTicketRow[]>;
  ticketScrollMemory: TicketScrollMemory;
  ticketScrollRoot: () => HTMLElement;
  ticketScrollScope: () => { project: string; mode: string; view: NotificationView | SettingsCategory | TicketView };
  ticketSourceRemoteBusy: Signal<boolean>;
  ticketSourceRemoteError: Signal<string>;
  ticketSourceSetupError: Signal<string>;
  ticketSourceSetupNavigation: Signal<'pop' | 'push' | 'none'>;
  ticketSourceSetupProject: Signal<Project | undefined>;
  uiStabilityDiagnostics: UiStabilityDiagnostics | undefined;
  unhealthyServerRecovery: Signal<UnhealthyServerRecovery | undefined>;
  unhealthyServerRecoveryBusy: Signal<boolean>;
  viewMode: Signal<WorkspaceViewMode>;
  workspaceSearchActive: () => boolean;
  workspaceTerminalGroups: () => TerminalDashboardGroup[];
}

/**
 * The root application component: global effects, the shell, dialogs, toasts, and overlays.
 * Extracted from the application runtime without behavior changes (HS2-K0VC2C).
 */
export function createHotSheetApp(dependencies: HotSheetAppDependencies) {
  const {
    activeTicketCollectionKey,
    aiConversationSurface,
    aiFeedbackDialog,
    appRoot,
    appTabContextMenu,
    attachmentMenuSurfaceProps,
    backgroundProjectRefresh,
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
    driveConnectionsByProject,
    error,
    gallerySurface,
    githubAuth,
    HotSheetShell,
    hs1MigrationBusy,
    hs1MigrationError,
    hs1MigrationProject,
    initialProjectRestoreComplete,
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
    consumeSkipNextTicketMotion,
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
    ticketRenderScheduled,
    tickets,
    ticketScrollMemory,
    ticketScrollRoot,
    ticketScrollScope,
    ticketSourceRemoteBusy,
    ticketSourceRemoteError,
    ticketSourceSetupError,
    ticketSourceSetupNavigation,
    ticketSourceSetupProject,
    uiStabilityDiagnostics,
    unhealthyServerRecovery,
    unhealthyServerRecoveryBusy,
    viewMode,
    workspaceSearchActive,
    workspaceTerminalGroups,
  } = dependencies;
  let renderedTicketCollectionKey = initialRenderedTicketCollectionKey;
  function HotSheetApp() {
    void permissionRevision.value;
    if (conversationOpen.value || terminalDrawerVisible.value || shellMode.value === 'terminals')
      void conversationRenderRevision.value;
    const ticketScrollGeneration = ticketScrollMemory.beforeRender(ticketScrollScope(), ticketScrollRoot()),
      nextTicketCollectionKey = activeTicketCollectionKey(),
      progressiveRenderPass = consumeSkipNextTicketMotion(),
      canAnimateTickets = !progressiveRenderPass && nextTicketCollectionKey === renderedTicketCollectionKey,
      ticketMotion = canAnimateTickets
        ? captureTicketMotion(appRoot, renderedTicketCollectionKey)
        : { scope: '', rows: new Map() };
    renderedTicketCollectionKey = nextTicketCollectionKey;
    renderMetrics?.recordPass();
    if (renderMetrics && uiStabilityDiagnostics) {
      const suppression = renderStormSuppressionReason({
        initialProjectRestoreComplete: initialProjectRestoreComplete(),
        foregroundLoading: loading.value,
        progressiveTicketRendering: progressiveRenderPass,
        backgroundProjectRefresh: backgroundProjectRefresh(),
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
          !ticketRenderScheduled() &&
          (shellMode.value !== 'terminals' || !terminalDashboardLoading.value),
        deferContainedListScrollRestore && shellMode.value === 'project' && viewMode.value === 'list',
      );
      animateTicketMotion(ticketMotion, appRoot, undefined, activeTicketCollectionKey());
      syncTerminalViewportMounts();
      syncTerminalRailNavStack();
      syncRepositoryNavStack();
      terminalDrawerSizeObserver.sync();
      terminalDashboardSizeObserver.sync();
      syncRepositoryPaginationObserver();
      syncTerminalDrawerMaximum();
      syncAttachmentGalleryMeasurement();
      syncVideoPosters(appRoot);
      syncConversationScroll(appRoot);
    });
    const target = notWorkingTarget.value,
      provider = defaultProvider(),
      composerTarget = newTicketSource(),
      visibilityScope = terminalVisibilityDialogScope.value,
      migration = hs1MigrationProject.value,
      deleteTargetId = savedViewDeleteTargetId.value,
      deleteView = deleteTargetId ? customViewsFor().find((item) => item.id === deleteTargetId) : undefined,
      showCornerLoading =
        bulkUpdateProgress.value !== undefined ||
        (loading.value && (!project() || tickets.value.length > 0 || corruptTickets.value.length > 0));
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
          searchModel={savedViewSearchModel}
          helpOpen={savedViewHelpOpen.value}
          busy={savedViewBusy.value}
          error={savedViewError.value}
          session={savedViewDialogSession.value}
        />
        <SavedViewDeleteDialog
          open={Boolean(deleteView)}
          name={deleteView?.name ?? 'this view'}
          busy={savedViewDeleteBusy.value}
          error={savedViewDeleteError.value}
        />
        <ManualModelDialog state={manualModelDialog.value} />
        <AiFeedbackDialog state={aiFeedbackDialog.value} />
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
          editingGitSource={defaultProviders.value[ticketSourceSetupProject.value?.id ?? '']?.sources.find(
            (source) => source.connectionId === providerEditingId.value && source.provider === 'git',
          )}
          accounts={providerAccounts.value}
          chosenAccount={providerAccountChoice.value}
          projectDefault={Boolean(
            defaultProviders.value[ticketSourceSetupProject.value?.id ?? '']?.sources.some(
              (source) => source.connectionId === providerEditingId.value && source.default,
            ),
          )}
          sourceColor={
            defaultProviders.value[ticketSourceSetupProject.value?.id ?? '']?.sources.find(
              (source) => source.connectionId === providerEditingId.value,
            )?.color
          }
          hasProjectDefault={Boolean(
            defaultProviders.value[ticketSourceSetupProject.value?.id ?? '']?.sources.some((source) => source.default),
          )}
          githubAuth={githubAuth.value}
          navigation={ticketSourceSetupNavigation.value}
          createdGitTicketStore={createdGitTicketStore.value}
          setupError={ticketSourceSetupError.value}
          remoteError={ticketSourceRemoteError.value}
          remoteBusy={ticketSourceRemoteBusy.value}
          providerBusy={providerSettingsBusy.value}
          providerError={providerSettingsError.value}
          removingProviderId={providerRemovingId.value}
        />
        <TerminalRenameDialog target={terminalRename.value} />
        <TerminalCopyDialog state={terminalCopy.value} />
        <TerminalPasteDialog state={terminalPaste.value} />
        <TerminalEditMenu state={terminalEditMenu.value} />
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
          providerName={composerTarget?.name ?? provider?.name ?? 'Hot Sheet git'}
          defaultSourceName={provider?.name ?? 'Hot Sheet git'}
          sourceIsDefault={composerTarget?.default ?? true}
          sources={writableTicketSources(provider).map((source) => ({
            value: source.connectionId,
            label: source.name,
          }))}
          source={composerTarget?.connectionId}
          canCreate={composerTarget?.capabilities.create ?? provider?.capabilities.create ?? true}
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
        {showCornerLoading && <AppLoadingIndicator progress={bulkUpdateProgress.value} />}
        {error.value && project() && <AppError message={error.value} />}
      </>
    );
  }
  return { HotSheetApp };
}
