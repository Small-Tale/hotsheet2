import { wireContentItems } from '@kerfjs/ui/wire-content-items';
import { delegate, delegateCapture, effect, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  type AiToolDefaults,
  type AiToolDescriptor,
  Api,
  type CommandDefinition,
  type CommandRun,
  type FullTicket,
  type GuidanceAction,
  type GuidanceOverview,
  type ProviderConnection,
} from '../api';
import { type CommandDropTarget } from '../command-order';
import { wireAiConversation } from '../components/ai-conversation-interactions';
import { type AiFeedbackDialogState } from '../components/ai-feedback-dialog';
import { wireAppComboboxes } from '../components/app-combobox-wiring';
import { COMMAND_EDITOR_DIALOG_ID } from '../components/command-settings-editor';
import { wireCommandSettingsEditor } from '../components/command-settings-editor-interactions';
import { type ConversationExportDialogState } from '../components/conversation-export-dialog';
import { type ManualModelDialogState } from '../components/manual-model-dialog';
import { type ExternalProviderKind, type GithubAuthState } from '../components/provider-setup-form';
import { type SettingsCategory } from '../components/settings-navigation';
import { TERMINAL_LIMIT_FIELDS } from '../components/terminal-memory-settings';
import { trashDraftIsDirty, type TrashRetentionDraft } from '../components/trash-settings';
import { wireTrashSettings } from '../components/trash-settings-interactions';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { revealContextPopupMenu } from '../context-menu-position';
import { syncControlledOpen } from '../controlled-open';
import { type ConversationExportDraft } from '../conversation-export';
import { type ConversationHistoryDirection } from '../conversation-history';
import { type ConversationSelectionModifiers } from '../conversation-selection';
import { terminalLimitChoiceValue, type TerminalLimitsSettings } from '../features/terminal-limits-settings';
import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';
import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { beginInteractionTiming } from '../interaction-performance';
import { chordFromEvent, saveShortcutOverrides, type ShortcutChord, shortcutDef } from '../keyboard-shortcuts';
import { loadLucideCatalog } from '../lucide-catalog';
import { toggleCollapsedCommandGroup } from '../workspace-preferences';
import { data } from './dom';
import { type Control, type Project } from './types';

let sessionFeedbackRaterId: string | undefined;
function feedbackRaterId(): string {
  if (sessionFeedbackRaterId) return sessionFeedbackRaterId;
  try {
    const key = 'hotsheet.aiFeedbackRaterId';
    const saved = window.localStorage.getItem(key);
    if (saved) return (sessionFeedbackRaterId = saved);
    const id = `feedback-rater:${crypto.randomUUID()}`;
    window.localStorage.setItem(key, id);
    return (sessionFeedbackRaterId = id);
  } catch {
    return (sessionFeedbackRaterId = `feedback-rater:${crypto.randomUUID()}`);
  }
}

/** Live application bindings used by this handler group. */
export interface CommandAndAiInteractionsDependencies {
  readonly commandGroupExpanded: Signal<boolean>;
  readonly persistWorkspacePreferences: () => void;
  readonly project: () => Project | undefined;
  readonly commandGroupsCollapsed: Signal<Record<string, string[]>>;
  readonly toggleSidebarDrive: () => Promise<void>;
  readonly driveOptionsOpen: Signal<boolean>;
  readonly driveOptionsAnchor: Signal<{ x: number; y: number } | undefined>;
  readonly aiTools: Signal<AiToolDescriptor[]>;
  readonly aiSettingsLoading: Signal<boolean>;
  readonly refreshAiConfiguration: (current?: Project, refresh?: boolean) => Promise<void>;
  readonly guidanceOverview: Signal<GuidanceOverview | undefined>;
  readonly guidanceLoading: Signal<boolean>;
  readonly guidanceQuery: Signal<string>;
  readonly guidanceOwnerFilter: Signal<string>;
  readonly guidanceTagFilter: Signal<string>;
  readonly guidanceRepository: Signal<string>;
  readonly guidanceSourceId: Signal<string>;
  readonly guidanceRef: Signal<string>;
  readonly updateGuidance: (action: GuidanceAction) => Promise<void>;
  readonly driveOverridesByProject: Signal<Record<string, Partial<AiToolDefaults>>>;
  readonly normalizedAiSelection: (value?: Partial<AiToolDefaults>) => AiToolDefaults;
  readonly selectDriveModel: (model: string) => void;
  readonly openManualModel: (
    target: 'settings' | 'drive' | 'conversation' | 'command',
    commandId?: string,
    providerId?: string,
  ) => void;
  readonly effectiveDriveSelection: (projectId?: string) => AiToolDefaults;
  readonly openSidebarConversation: () => Promise<void>;
  readonly conversationOpen: Signal<boolean>;
  readonly openConversationExport: (options?: { connectionId?: string; messageIds?: readonly string[] }) => void;
  readonly pickConversationMessage: (
    connectionId: string,
    messageId: string,
    modifiers?: ConversationSelectionModifiers,
  ) => void;
  readonly contextMessageIds: (connectionId: string, messageId: string) => readonly string[];
  readonly copyConversationMessages: (connectionId: string, messageIds: readonly string[]) => Promise<void>;
  readonly downloadConversationMessages: (connectionId: string, messageIds: readonly string[]) => void;
  readonly downloadConversationSelection: (connectionId: string) => void;
  readonly copyConversationSelection: (connectionId: string) => Promise<void>;
  readonly clearConversationSelection: (connectionId: string) => void;
  readonly conversationExportDialog: Signal<ConversationExportDialogState | undefined>;
  readonly finishConversationExport: () => Promise<void>;
  readonly updateConversationExportDraft: (update: (draft: ConversationExportDraft) => ConversationExportDraft) => void;
  readonly setConversationDraft: (value: string) => void;
  readonly recallConversationHistory: (
    direction: ConversationHistoryDirection,
    currentValue: string,
  ) => string | undefined;
  readonly sendConversationTurn: () => Promise<void>;
  readonly stopConversation: () => Promise<void>;
  readonly selectConversationProvider: (providerId: string) => Promise<void>;
  readonly selectConversationModel: (model: string) => void;
  readonly selectConversationEffort: (effort: string) => void;
  readonly selectConversationPermissionMode: (mode: string) => void;
  readonly selectedTicket: Signal<FullTicket | null>;
  readonly canGiveFeedback: () => boolean;
  readonly aiFeedbackDialog: Signal<AiFeedbackDialogState | undefined>;
  readonly updateSelected: (patch: Record<string, unknown>) => Promise<boolean>;
  readonly showToast: (message: string) => void;
  commandLongPressFired: boolean;
  readonly runCommand: (commandId: string) => Promise<void>;
  readonly commandDialogId: Signal<string | undefined>;
  readonly commandStopConfirmation: Signal<boolean>;
  readonly commandRuns: Signal<CommandRun[]>;
  readonly error: Signal<string>;
  readonly commandSettingsEditingId: Signal<string | undefined>;
  readonly commandIconSearch: Signal<string>;
  readonly addCommandSetting: (projectId: string) => string;
  readonly manualModelDialog: Signal<ManualModelDialogState | undefined>;
  readonly deleteCommandSetting: (projectId: string, id: string) => void;
  readonly addCommandGroup: (projectId: string) => void;
  readonly deleteCommandGroup: (projectId: string, group: string) => void;
  readonly selectCommandRow: (projectId: string, id: string, intent: { toggle?: boolean; range?: boolean }) => void;
  readonly commandSelection: (projectId?: string) => string[];
  readonly commandSettingsDefinitions: (projectId?: string) => CommandDefinition[];
  readonly selectCommandSetting: (projectId: string, id: string | undefined) => void;
  readonly reorderCommandSettings: (projectId: string, sourceIds: readonly string[], target: CommandDropTarget) => void;
  readonly updateCommandSetting: (projectId: string, id: string, field: string, value: string) => void;
  readonly updateCommandAiSelection: (
    projectId: string,
    id: string,
    selection: Partial<Pick<CommandDefinition, 'tool' | 'model' | 'effort'>>,
  ) => void;
  readonly effectiveCommandAiSelection: (command: CommandDefinition) => AiToolDefaults;
  readonly showLoadingActivity: Signal<boolean>;
  readonly inheritGlobalShellHistory: Signal<boolean>;
  readonly terminalSettingsMessage: Signal<string>;
  readonly trashSettingsMessagesByProject: Signal<Record<string, string>>;
  readonly trashCleanupDaysByProject: Signal<Record<string, number>>;
  readonly trashRetentionDraftsByProject: Signal<Record<string, TrashRetentionDraft>>;
  readonly resetProgressiveTicketRendering: () => void;
  readonly viewMode: Signal<WorkspaceViewMode>;
  readonly refreshProject: (options?: { showLoading?: boolean }) => Promise<unknown>;
  readonly setSettingsCategory: (_projectId: string, category: SettingsCategory) => void;
  readonly refreshProviderConnections: (current?: Project) => Promise<void>;
  readonly refreshTerminalSettings: (current?: Project) => Promise<void>;
  /** Machine-wide terminal limits (HS2-Y5DYC0); optional so focused harnesses can omit them. */
  readonly terminalLimitsSettings?: Pick<TerminalLimitsSettings, 'refresh' | 'save'>;
  readonly refreshTrashSettings: (current?: Project) => Promise<void>;
  readonly capturingShortcutId: Signal<string | undefined>;
  readonly keyboardShortcutOverrides: Signal<Record<string, ShortcutChord>>;
  readonly appleShortcutPlatform: boolean;
  readonly saveAiDefaults: (value: AiToolDefaults) => Promise<void>;
  readonly selectDefaultModel: (model: string, tool?: string) => void;
  readonly selectDefaultEffort: (effort: string, tool?: string) => void;
  /** Returns false when the mode was not saved (unknown, or a warned mode the user declined). */
  readonly selectDefaultPermissionMode: (mode: string, tool?: string) => boolean;
  readonly selectDefaultProvider: (tool: string) => void;
  readonly restoreCommandEditorAfterManualModel: (state: ManualModelDialogState | undefined) => void;
  readonly aiDefaults: Signal<AiToolDefaults>;
  readonly ticketSourceSetupProject: Signal<Project | undefined>;
  readonly providerSetupKind: Signal<ExternalProviderKind | undefined>;
  readonly providerEditingId: Signal<string | undefined>;
  readonly requestProjectSourceRemoval: (id: string) => void;
  readonly refreshProviderAccounts: (current?: Project) => Promise<void>;
  readonly signOutProviderAccount: (id: string) => Promise<void>;
  readonly requestUnusedAccountSourceRemoval: (account: string, source: string) => void;
  readonly cancelUnusedAccountSourceRemoval: () => void;
  readonly removeUnusedAccountSource: (account: string, source: string) => Promise<void>;
  readonly useGithubAccount: (id: string) => Promise<void>;
  /** The GitLab or Jira account prefilling a new source's form (HS2-F5HNJN). */
  readonly providerAccountChoice: Signal<string | undefined>;
  readonly useProviderAccount: (id: string) => void;
  readonly setProjectDefaultSource: (id: string) => Promise<void>;
  readonly setProjectSourceColor: (id: string, color: string) => Promise<void>;
  readonly retryProviderOutbox: (connectionId: string, operationId: string) => Promise<void>;
  readonly discardProviderOutbox: (connectionId: string, operationId: string) => Promise<void>;
  readonly providerSettingsError: Signal<string>;
  readonly createdGitTicketStore: Signal<string>;
  readonly ticketSourceSetupNavigation: Signal<'none' | 'push' | 'pop'>;
  readonly providerConnections: Signal<ProviderConnection[]>;
  readonly githubAuth: Signal<GithubAuthState | undefined>;
  readonly cancelGitHubSignIn: () => void;
  readonly startGitHubSignIn: (form: HTMLFormElement) => Promise<void>;
  readonly saveExternalProvider: (form: HTMLFormElement) => Promise<void>;
  readonly providerRemovingId: Signal<string | undefined>;
  readonly requestProviderRemoval: () => void;
  readonly cancelProviderRemoval: () => void;
  readonly removeExternalProvider: () => Promise<void>;
  readonly toggleProviderDisabled: () => Promise<void>;
  readonly refreshGitHubRepositories: () => Promise<void>;
  readonly loadEditingGitHubRepositories: (connection: ProviderConnection, current: Project) => Promise<void>;
  readonly chooseGitHubEnterprise: (enterprise: boolean) => void;
  readonly copyGitHubCode: () => Promise<void>;
  readonly reopenGitHubSignIn: () => void;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireCommandAndAiInteractions(dependencies: CommandAndAiInteractionsDependencies) {
  const lifetime = createScope();
  const {
    commandGroupExpanded,
    persistWorkspacePreferences,
    project,
    commandGroupsCollapsed,
    toggleSidebarDrive,
    driveOptionsOpen,
    driveOptionsAnchor,
    aiTools,
    aiSettingsLoading,
    refreshAiConfiguration,
    guidanceOverview,
    guidanceLoading,
    guidanceQuery,
    guidanceOwnerFilter,
    guidanceTagFilter,
    guidanceRepository,
    guidanceSourceId,
    guidanceRef,
    updateGuidance,
    driveOverridesByProject,
    normalizedAiSelection,
    selectDriveModel,
    openManualModel,
    effectiveDriveSelection,
    openSidebarConversation,
    conversationOpen,
    openConversationExport,
    pickConversationMessage,
    contextMessageIds,
    copyConversationMessages,
    downloadConversationMessages,
    downloadConversationSelection,
    copyConversationSelection,
    clearConversationSelection,
    conversationExportDialog,
    finishConversationExport,
    updateConversationExportDraft,
    setConversationDraft,
    recallConversationHistory,
    sendConversationTurn,
    stopConversation,
    selectConversationProvider,
    selectConversationModel,
    selectConversationEffort,
    selectConversationPermissionMode,
    selectedTicket,
    canGiveFeedback,
    aiFeedbackDialog,
    updateSelected,
    showToast,
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
    refreshProject,
    setSettingsCategory,
    refreshProviderConnections,
    refreshTerminalSettings,
    terminalLimitsSettings,
    refreshTrashSettings,
    capturingShortcutId,
    keyboardShortcutOverrides,
    appleShortcutPlatform,
    selectDefaultModel,
    selectDefaultEffort,
    selectDefaultPermissionMode,
    selectDefaultProvider,
    restoreCommandEditorAfterManualModel,
    ticketSourceSetupProject,
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
    retryProviderOutbox,
    discardProviderOutbox,
    providerSettingsError,
    createdGitTicketStore,
    ticketSourceSetupNavigation,
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
  } = dependencies;
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.toggleCommandGroup.selector, () => {
      commandGroupExpanded.value = !commandGroupExpanded.value;
      persistWorkspacePreferences();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.toggleCommandSection.selector, (_event, target) => {
      const current = project(),
        group = target.closest<HTMLElement>('[data-command-group]')?.dataset.commandGroup;
      if (!current || !group) return;
      commandGroupsCollapsed.value = toggleCollapsedCommandGroup(commandGroupsCollapsed.value, current.id, group);
      persistWorkspacePreferences();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.toggleDrive.selector, () => {
      void toggleSidebarDrive();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.toggleDriveOptions.selector, (event, target) => {
      event.stopPropagation();
      const opening = !driveOptionsOpen.value;
      if (opening) {
        // The menu opens above the drive row from its top-left corner (HS2-2EHD8R).
        const row = target.closest('.project-sidebar__drive-row')?.getBoundingClientRect();
        if (row) driveOptionsAnchor.value = { x: row.left, y: row.top };
      }
      driveOptionsOpen.value = opening;
      if (opening && !aiTools.value.length && !aiSettingsLoading.value) void refreshAiConfiguration(undefined, true);
    }),
  );
  effect(() => {
    // Partial dependencies in the handler-transition tests may omit this signal.
    if ((driveOptionsOpen as typeof driveOptionsOpen | undefined)?.value) revealContextPopupMenu('drive-options');
  });
  // Web Awesome closes a menu when a choice is picked; the Drive menu stays open across provider,
  // model, and effort picks, so each choice re-reveals the re-rendered menu (HS2-2EHD8R).
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectDriveDefault.selector, () => {
      const current = project();
      if (!current) return;
      driveOverridesByProject.value = { ...driveOverridesByProject.value, [current.id]: {} };
      revealContextPopupMenu('drive-options');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectDriveTool.selector, (_event, target) => {
      const current = project(),
        tool = data(target).value;
      if (!current || !tool) return;
      driveOverridesByProject.value = {
        ...driveOverridesByProject.value,
        [current.id]: normalizedAiSelection({ tool }),
      };
      revealContextPopupMenu('drive-options');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectDriveModel.selector, (_event, target) => {
      selectDriveModel(data(target).value ?? '');
      revealContextPopupMenu('drive-options');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.openDriveManualModel.selector, () => {
      openManualModel('drive');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectDriveEffort.selector, (_event, target) => {
      const current = project(),
        effort = data(target).value;
      if (!current || !effort) return;
      driveOverridesByProject.value = {
        ...driveOverridesByProject.value,
        [current.id]: { ...effectiveDriveSelection(current.id), effort },
      };
      revealContextPopupMenu('drive-options');
    }),
  );
  document.addEventListener(
    'pointerdown',
    (event) => {
      const insideDriveRow = event
        .composedPath()
        .some((item) => item instanceof Element && item.matches('.project-sidebar__drive-row'));
      if (driveOptionsOpen.value && !insideDriveRow) driveOptionsOpen.value = false;
    },
    { capture: true, signal: lifetime.signal },
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.openConversation.selector, () => {
      void openSidebarConversation();
    }),
  );
  lifetime.add(
    wireAiConversation(document.body, {
      onHidden: () => {
        conversationOpen.value = false;
      },
      save: openConversationExport,
      pickMessage: pickConversationMessage,
      contextMessageIds,
      copyMessages: (connectionId, messageIds) => {
        void copyConversationMessages(connectionId, messageIds);
      },
      downloadMessages: downloadConversationMessages,
      setDraft: setConversationDraft,
      recallHistory: recallConversationHistory,
      send: () => {
        void sendConversationTurn();
      },
      stop: () => {
        void stopConversation();
      },
      selectProvider: (id) => {
        void selectConversationProvider(id);
      },
      selectModel: selectConversationModel,
      selectEffort: selectConversationEffort,
      selectPermissionMode: selectConversationPermissionMode,
      openManualModel: () => {
        openManualModel('conversation');
      },
    }),
  );
  // Selectable transcript messages are interactive Kerf ContentItems; Kerf gives them Enter/Space
  // activation, which reaches the ordinary delegated click handler (HS2-P16J04).
  lifetime.add(wireContentItems(document.body));
  // One Kerf Combobox wiring owner handles repository and Add tag fields (HS2-C32ERK).
  lifetime.add(wireAppComboboxes(document.body));
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.copyConversationSelection.selector, (_event, target) => {
      const connectionId = target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId;
      if (connectionId) void copyConversationSelection(connectionId);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      COMMANDS_AND_AI_ACTIONS.downloadConversationSelection.selector,
      (_event, target) => {
        const connectionId = target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId;
        if (connectionId) downloadConversationSelection(connectionId);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.clearConversationSelection.selector, (_event, target) => {
      const connectionId = target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId;
      if (connectionId) clearConversationSelection(connectionId);
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'click', COMMANDS_AND_AI_TARGETS.conversationExportDialog.selector, (event) => {
      const action = event
          .composedPath()
          .find((item): item is HTMLElement => item instanceof HTMLElement && Boolean(item.dataset.action))
          ?.dataset.action,
        state = conversationExportDialog.value;
      if (!state) return;
      if (action === 'next-conversation-export-step') {
        conversationExportDialog.value = { ...state, step: 2, navigation: 'push', error: '' };
        return;
      }
      if (action === 'previous-conversation-export-step') {
        conversationExportDialog.value = { ...state, step: 1, navigation: 'pop', error: '' };
        return;
      }
      if (action === 'cancel-conversation-export') {
        conversationExportDialog.value = undefined;
        return;
      }
      if (action === 'finish-conversation-export') {
        event.preventDefault();
        void finishConversationExport();
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="conversation-export-scope"]', (_event, target) => {
      const state = conversationExportDialog.value;
      if (!state) return;
      const kind = (target as HTMLInputElement).value;
      if (kind === 'all') updateConversationExportDraft((draft) => ({ ...draft, scope: { kind: 'all' } }));
      else if (state.selectedScope)
        updateConversationExportDraft((draft) => ({ ...draft, scope: state.selectedScope! }));
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="conversation-export-write-mode"]', (_event, target) => {
      const value = (target as HTMLInputElement).value;
      if (value === 'reexport' || value === 'overwrite')
        updateConversationExportDraft((draft) => ({ ...draft, writeMode: value }));
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="conversation-export-attachments"]', (_event, target) => {
      updateConversationExportDraft((draft) => ({
        ...draft,
        bundle: { ...draft.bundle, includeAttachments: (target as HTMLInputElement).checked },
      }));
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="conversation-export-media"]', (_event, target) => {
      updateConversationExportDraft((draft) => ({
        ...draft,
        bundle: { ...draft.bundle, includeMedia: (target as HTMLInputElement).checked },
      }));
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="conversation-export-summary"]', (_event, target) => {
      updateConversationExportDraft((draft) => ({
        ...draft,
        bundle: { ...draft.bundle, includeSummary: (target as HTMLInputElement).checked },
      }));
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', COMMANDS_AND_AI_ACTIONS.submitConversationExport.selector, (event) => {
      event.preventDefault();
      void finishConversationExport();
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'kui-request-close',
      COMMANDS_AND_AI_TARGETS.conversationExportDialog.selector,
      (event, target) => {
        if (event.target !== target) return;
        if (conversationExportDialog.value?.busy) {
          event.preventDefault();
          return;
        }
        conversationExportDialog.value = undefined;
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.rateAiContent.selector, (_event, target) => {
      const ticket = selectedTicket.value,
        rating = data(target).aiFeedbackRating,
        targetId = data(target).aiFeedbackTarget;
      if (!ticket || !targetId || !['helpful', 'not-helpful'].includes(rating ?? '') || !canGiveFeedback()) return;
      const rater = feedbackRaterId();
      const wireRating = rating === 'helpful' ? 'helpful' : 'not_helpful';
      const previous = ticket.notes.find((note) => note.ai_feedback?.target === targetId && note.actor?.id === rater);
      if (previous?.ai_feedback?.rating === wireRating && window.confirm('Withdraw your previous rating?')) {
        void updateSelected({
          ai_feedback: { target: targetId, rating: null },
          actor: { role: 'human', id: rater },
        }).then((saved) => {
          if (saved) showToast('AI feedback withdrawn.');
        });
        return;
      }
      aiFeedbackDialog.value = {
        ticketId: ticket.id,
        target: targetId,
        rating: wireRating,
        rater,
        explanation: previous?.ai_feedback?.explanation ?? '',
        revising: Boolean(previous),
      };
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          // Open the rendered Kerf dialog now (not on the next render) so the field can take focus.
          syncControlledOpen(document);
          const dialog = document.querySelector<Control>(COMMANDS_AND_AI_TARGETS.aiFeedbackDialog.selector);
          dialog?.querySelector<HTMLTextAreaElement>('[name="ai-feedback-explanation"]')?.focus();
        }),
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.cancelAiFeedback.selector, () => {
      aiFeedbackDialog.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', COMMANDS_AND_AI_ACTIONS.submitAiFeedback.selector, (event, target) => {
      event.preventDefault();
      const state = aiFeedbackDialog.value;
      if (!state || selectedTicket.value?.id !== state.ticketId) return;
      const detail = target.querySelector<HTMLTextAreaElement>('[name="ai-feedback-explanation"]')?.value.trim() ?? '';
      aiFeedbackDialog.value = undefined;
      void updateSelected({
        ai_feedback: { target: state.target, rating: state.rating, explanation: detail || null },
        actor: { role: 'human', id: state.rater },
      }).then((saved) => {
        if (saved) showToast(state.revising ? 'AI feedback updated.' : 'AI feedback saved as a ticket note.');
      });
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'kui-request-close', COMMANDS_AND_AI_TARGETS.aiFeedbackDialog.selector, () => {
      aiFeedbackDialog.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.runCommand.selector, (event, target) => {
      if (dependencies.commandLongPressFired) {
        event.preventDefault();
        dependencies.commandLongPressFired = false;
        return;
      }
      void runCommand(data(target).itemId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.selector, () => {
      commandDialogId.value = undefined;
      commandStopConfirmation.value = false;
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'kui-request-close',
      '[data-dialog-component="command-run-dialog"], [data-dialog-component="command-cancellation-dialog"]',
      () => {
        commandDialogId.value = undefined;
        commandStopConfirmation.value = false;
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.confirmStopCommand.selector, (_event, target) => {
      const current = project(),
        runId = data(target).runId;
      if (!current || !runId) return;
      void new Api(current.apiPath)
        .cancelCommandRun(runId)
        .then((run) => {
          commandRuns.value = commandRuns.value.map((item) => (item.id === run.id ? run : item));
          commandDialogId.value = undefined;
          commandStopConfirmation.value = false;
        })
        .catch((reason: unknown) => {
          error.value = reason instanceof Error ? reason.message : String(reason);
        });
    }),
  );
  function openCommandEditor(id: string) {
    commandSettingsEditingId.value = id;
    commandIconSearch.value = '';
    void loadLucideCatalog();
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
    (document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control).showPopover?.();
  }
  const projectId = () => project()?.id;
  lifetime.add(
    wireCommandSettingsEditor(document.body, {
      ownsCommand: (id) => {
        const current = projectId();
        return Boolean(current && commandSettingsDefinitions(current).some((command) => command.id === id));
      },
      openEditor: openCommandEditor,
      closeEditor: () => {
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
        (document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control).hidePopover?.();
        commandSettingsEditingId.value = undefined;
        commandIconSearch.value = '';
      },
      onEditorClosed: () => {
        if (manualModelDialog.value?.target === 'command') return;
        commandSettingsEditingId.value = undefined;
        commandIconSearch.value = '';
      },
      addCommand: () => {
        const current = projectId();
        if (current) openCommandEditor(addCommandSetting(current));
      },
      deleteCommand: (id) => {
        const current = projectId();
        if (current) deleteCommandSetting(current, id);
      },
      addGroup: () => {
        const current = projectId();
        if (current) addCommandGroup(current);
      },
      deleteGroup: (group) => {
        const current = projectId();
        if (current) deleteCommandGroup(current, group);
      },
      selection: () => {
        const current = projectId();
        return current ? commandSelection(current) : [];
      },
      orderedIds: () => {
        const current = projectId();
        return current ? commandSettingsDefinitions(current).map((command) => command.id) : [];
      },
      selectRow: (id, intent) => {
        const current = projectId();
        if (current) selectCommandRow(current, id, intent);
      },
      selectOnly: (id) => {
        const current = projectId();
        if (current) selectCommandSetting(current, id);
      },
      reorder: (sources, target) => {
        const current = projectId();
        if (current) reorderCommandSettings(current, sources, target);
      },
      updateField: (id, field, value) => {
        const current = projectId();
        if (current) updateCommandSetting(current, id, field, value);
      },
      setIconSearch: (query) => {
        commandIconSearch.value = query;
      },
      chooseAi: (id, choice) => {
        const current = projectId(),
          command = commandSettingsDefinitions(current).find((item) => item.id === id);
        if (!current) return;
        if (choice.kind === 'default') {
          updateCommandAiSelection(current, id, {});
          return;
        }
        if (choice.kind === 'tool') {
          if (aiTools.value.some((item) => item.id === choice.value))
            updateCommandAiSelection(current, id, normalizedAiSelection({ tool: choice.value }));
          return;
        }
        if (!command) return;
        const active = effectiveCommandAiSelection(command);
        updateCommandAiSelection(
          current,
          id,
          choice.kind === 'model'
            ? normalizedAiSelection({ tool: active.tool, model: choice.value, effort: active.effort })
            : { ...active, effort: choice.value },
        );
      },
      openManualModel: (id) => {
        openManualModel('command', id);
      },
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', COMMANDS_AND_AI_ACTIONS.toggleLoadingActivity.selector, (_event, target) => {
      const checked = (target as HTMLInputElement).checked;
      showLoadingActivity.value = checked;
      localStorage.setItem('hotsheet.show-loading-activity', String(checked));
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', COMMANDS_AND_AI_ACTIONS.toggleGlobalShellHistory.selector, (_event, target) => {
      const current = project(),
        checked = (target as HTMLInputElement).checked;
      if (!current) return;
      inheritGlobalShellHistory.value = checked;
      // Progress is the global server-busy indicator and success is silent; only a failure shows
      // in the status line (HS2-GHA1D3).
      terminalSettingsMessage.value = '';
      void new Api(current.apiPath)
        .saveTerminalSettings({ inherit_global_shell_history: checked })
        .then((value) => {
          if (project()?.id !== current.id) return;
          inheritGlobalShellHistory.value = value.inherit_global_shell_history;
        })
        .catch((reason: unknown) => {
          if (project()?.id !== current.id) return;
          inheritGlobalShellHistory.value = !checked;
          terminalSettingsMessage.value = reason instanceof Error ? reason.message : String(reason);
        });
    }),
  );
  // App Settings → Terminal memory (HS2-Y5DYC0): each choice saves all three machine-wide limits at once;
  // "default" clears that limit back to the memory-derived default.
  lifetime.add(
    delegate(document.body, 'change', COMMANDS_AND_AI_ACTIONS.saveTerminalLimits.selector, (_event, target) => {
      if (!terminalLimitsSettings) return;
      const choice = (name: string) =>
        terminalLimitChoiceValue(target.querySelector<Control>(`[name="${name}"]`)?.value ?? undefined);
      void terminalLimitsSettings.save({
        history_lines: choice(TERMINAL_LIMIT_FIELDS.historyLines),
        replay_bytes: choice(TERMINAL_LIMIT_FIELDS.replayBytes),
        warm_terminals: choice(TERMINAL_LIMIT_FIELDS.warmTerminals),
      });
    }),
  );
  // Trash retention (HS2-KPD942, HS2-F55956): edits stay a per-project draft until "Apply now";
  // a debounced server preview says how many Trash tickets the new window would purge, and
  // leaving the page with an unapplied draft is held until the user applies or reverts it.
  let trashPreviewTimer: ReturnType<typeof setTimeout> | undefined, heldNavigation: HTMLElement | undefined;
  const trashDraft = (projectId: string): TrashRetentionDraft | undefined =>
    trashRetentionDraftsByProject.value[projectId];
  function setTrashDraft(projectId: string, draft: TrashRetentionDraft | undefined) {
    const rest = Object.fromEntries(
      Object.entries(trashRetentionDraftsByProject.value).filter(([id]) => id !== projectId),
    );
    trashRetentionDraftsByProject.value = draft ? { ...rest, [projectId]: draft } : rest;
  }
  function setTrashMessage(projectId: string, message: string) {
    trashSettingsMessagesByProject.value = { ...trashSettingsMessagesByProject.value, [projectId]: message };
  }
  const trashDays = (value: string) => {
    const days = Number(value.trim());
    return value.trim() !== '' && Number.isSafeInteger(days) && days >= 1 ? days : undefined;
  };
  function previewTrashPurge(current: Project, value: string) {
    clearTimeout(trashPreviewTimer);
    const days = trashDays(value);
    if (days === undefined || days === trashCleanupDaysByProject.value[current.id]) return;
    trashPreviewTimer = setTimeout(() => {
      void new Api(current.apiPath)
        .trashPurgePreview(current.id, days)
        .then((preview) => {
          const draft = trashDraft(current.id);
          if (draft && trashDays(draft.value) === preview.trash_cleanup_days)
            setTrashDraft(current.id, { ...draft, purgeCount: preview.purge_count });
        })
        // A missing preview only hides the hint; applying still validates on the server.
        .catch(() => undefined);
    }, 250);
  }
  /** Continue the navigation that an unapplied draft held, once it was applied or reverted. */
  function releaseHeldNavigation() {
    const target = heldNavigation;
    heldNavigation = undefined;
    if (target?.isConnected) target.click();
  }
  function applyTrashSettings(current: Project, value: string): void {
    const days = trashDays(value);
    if (days === undefined) {
      setTrashMessage(current.id, 'Enter a positive whole number of days.');
      return;
    }
    setTrashMessage(current.id, '');
    void new Api(current.apiPath)
      .saveTrashSettings(current.id, { trash_cleanup_days: days })
      .then((saved) => {
        if (project()?.id !== current.id) return;
        trashCleanupDaysByProject.value = {
          ...trashCleanupDaysByProject.value,
          [current.id]: saved.trash_cleanup_days,
        };
        setTrashDraft(current.id, undefined);
        showToast('Trash retention applied.');
        releaseHeldNavigation();
      })
      .catch((reason: unknown) => {
        if (project()?.id === current.id)
          setTrashMessage(current.id, reason instanceof Error ? reason.message : String(reason));
      });
  }
  // The Trash retention gestures share the UX demo's wire helper (HS2-WFFDAA).
  lifetime.add(
    wireTrashSettings(document.body, {
      input: (value) => {
        const current = project();
        if (!current) return;
        setTrashDraft(current.id, { value });
        setTrashMessage(current.id, '');
        previewTrashPurge(current, value);
      },
      submit: (form) => {
        const current = project();
        if (!current) return;
        applyTrashSettings(
          current,
          trashDraft(current.id)?.value ?? form.querySelector<Control>('[name="trash-cleanup-days"]')?.value ?? '',
        );
      },
      applyLeaving: () => {
        const current = project(),
          draft = current && trashDraft(current.id);
        if (current && draft) applyTrashSettings(current, draft.value);
      },
      revert: () => {
        const current = project();
        if (!current) return;
        clearTimeout(trashPreviewTimer);
        setTrashDraft(current.id, undefined);
        setTrashMessage(current.id, '');
        releaseHeldNavigation();
      },
      // Closing the warning (Escape or its close button) stays on Lifecycle with the draft intact.
      dismissLeaving: () => {
        const current = project(),
          draft = current && trashDraft(current.id);
        heldNavigation = undefined;
        if (current && draft?.leaving) setTrashDraft(current.id, { ...draft, leaving: false });
      },
    }),
  );
  // Navigation that leaves Lifecycle with an unapplied retention draft is held (HS2-F55956).
  const heldNavigationSelector = [
    COMMANDS_AND_AI_ACTIONS.setViewMode.selector,
    COMMANDS_AND_AI_ACTIONS.selectSettingsCategory.selector,
    NAVIGATION_AND_TABS_ACTIONS.selectProjectTab.selector,
  ].join(', ');
  const unappliedTrashDraft = () => {
    const current = project();
    if (!current || !document.querySelector('[data-component="trash-settings"]')) return undefined;
    const draft = trashDraft(current.id);
    return trashDraftIsDirty(trashCleanupDaysByProject.value[current.id] ?? 30, draft) ? { current, draft } : undefined;
  };
  lifetime.add(
    delegateCapture(document.body, 'click', heldNavigationSelector, (event, target) => {
      const pending = unappliedTrashDraft();
      if (!pending?.draft || heldNavigation === target) return;
      if (
        target.matches(COMMANDS_AND_AI_ACTIONS.selectSettingsCategory.selector) &&
        data(target).itemId === 'lifecycle'
      )
        return;
      event.preventDefault();
      event.stopPropagation();
      heldNavigation = target as HTMLElement;
      setTrashDraft(pending.current.id, { ...pending.draft, leaving: true });
    }),
  );
  document.defaultView?.addEventListener(
    'beforeunload',
    (event) => {
      if (unappliedTrashDraft()) event.preventDefault();
    },
    { signal: lifetime.signal },
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.setViewMode.selector, (_event, target) => {
      const mode = (data(target).segmentValue ?? data(target).viewMode) as WorkspaceViewMode,
        finishTiming = beginInteractionTiming('workspace-mode-change', { mode });
      resetProgressiveTicketRendering();
      viewMode.value = mode;
      persistWorkspacePreferences();
      if (mode === 'list' || mode === 'board') void refreshProject({ showLoading: false });
      finishTiming();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectSettingsCategory.selector, (_event, target) => {
      const current = project(),
        category = (data(target).itemId ?? 'sources') as SettingsCategory;
      if (!current) return;
      if (category !== 'keyboard') setCapturingShortcut(undefined);
      setSettingsCategory(current.id, category);
      if (category === 'sources') void refreshProviderConnections();
      if (category === 'accounts') void refreshProviderAccounts();
      if (category === 'terminals') void refreshTerminalSettings();
      if (category === 'terminal-memory') void terminalLimitsSettings?.refresh();
      if (category === 'lifecycle') void refreshTrashSettings();
      if (category === 'ai' || category === 'commands') void refreshAiConfiguration(undefined, true);
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', 'input[name="guidance-search"]', (_event, target) => {
      guidanceQuery.value = (target as HTMLInputElement).value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', 'input[name="guidance-source-id"]', (_event, target) => {
      guidanceSourceId.value = (target as HTMLInputElement).value;
      const registered = guidanceOverview.value?.sources.find((source) => source.id === guidanceSourceId.value);
      if (registered) {
        guidanceRepository.value = registered.repository;
        guidanceRef.value = registered.requested_ref;
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', 'input[name="guidance-repository"]', (_event, target) => {
      guidanceRepository.value = (target as HTMLInputElement).value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', 'input[name="guidance-ref"]', (_event, target) => {
      guidanceRef.value = (target as HTMLInputElement).value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name="guidance-owner-filter"]', (_event, target) => {
      guidanceOwnerFilter.value = (target as HTMLSelectElement).value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name="guidance-tag-filter"]', (_event, target) => {
      guidanceTagFilter.value = (target as HTMLSelectElement).value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name="guidance-owner"]', (_event, target) => {
      const owner = (target as HTMLSelectElement).value;
      if (owner === 'builtin' || owner === 'rulesync') void updateGuidance({ action: 'owner', owner });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.registerGuidanceSource.selector, () => {
      if (!guidanceLoading.value)
        void updateGuidance({
          action: 'register',
          id: guidanceSourceId.value,
          repository: guidanceRepository.value,
          ref: guidanceRef.value,
        });
    }),
  );
  const toggleGuidance = (target: Element, field: 'rules' | 'skills') => {
    const item = target.closest<HTMLElement>(
      '[data-guidance-source][data-guidance-rule], [data-guidance-source][data-guidance-skill]',
    );
    const sourceId = item?.dataset.guidanceSource,
      name = item?.dataset.guidanceRule ?? item?.dataset.guidanceSkill,
      source = guidanceOverview.value?.sources.find((item) => item.id === sourceId);
    if (!source || !name || guidanceLoading.value) return;
    const rules = source.rules.filter((rule) => rule.selected).map((rule) => rule.name),
      skills = source.skills.filter((skill) => skill.selected).map((skill) => skill.name),
      chosen = field === 'rules' ? rules : skills;
    if ((target as HTMLInputElement).checked && !chosen.includes(name)) chosen.push(name);
    if (!(target as HTMLInputElement).checked && chosen.includes(name)) chosen.splice(chosen.indexOf(name), 1);
    void updateGuidance({ action: 'select', source: source.id, rules, skills });
  };
  lifetime.add(
    delegate(document.body, 'change', COMMANDS_AND_AI_ACTIONS.toggleGuidanceRule.selector, (_event, target) => {
      toggleGuidance(target, 'rules');
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', COMMANDS_AND_AI_ACTIONS.toggleGuidanceSkill.selector, (_event, target) => {
      toggleGuidance(target, 'skills');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.applyGuidance.selector, () => {
      if (!guidanceLoading.value) void updateGuidance({ action: 'apply' });
    }),
  );
  function setCapturingShortcut(id: string | undefined) {
    capturingShortcutId.value = id;
    if (id !== undefined)
      requestAnimationFrame(() =>
        requestAnimationFrame(() =>
          document.querySelector<HTMLElement>(`[data-shortcut-capture="${CSS.escape(id)}"]`)?.focus(),
        ),
      );
  }
  function persistShortcutOverrides(next: Record<string, ShortcutChord>) {
    keyboardShortcutOverrides.value = next;
    saveShortcutOverrides(next, localStorage);
  }
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.editShortcut.selector, (_event, target) => {
      const id = data(target).shortcutId;
      if (id && shortcutDef(id)?.editable) setCapturingShortcut(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.cancelShortcutCapture.selector, () => {
      setCapturingShortcut(undefined);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.resetShortcut.selector, (_event, target) => {
      const id = data(target).shortcutId;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      if (!id || !keyboardShortcutOverrides.value[id]) return;
      persistShortcutOverrides(
        Object.fromEntries(Object.entries(keyboardShortcutOverrides.value).filter(([entryId]) => entryId !== id)),
      );
      if (capturingShortcutId.value === id) setCapturingShortcut(undefined);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.resetAllShortcuts.selector, () => {
      persistShortcutOverrides({});
      setCapturingShortcut(undefined);
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', '[data-shortcut-capture]', (event, target) => {
      const keyboard = event as KeyboardEvent,
        id = data(target).shortcutCapture;
      if (!id || capturingShortcutId.value !== id || !shortcutDef(id)?.editable) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (keyboard.key === 'Escape') {
        setCapturingShortcut(undefined);
        return;
      }
      const chord = chordFromEvent(keyboard, appleShortcutPlatform);
      if (!chord) return;
      persistShortcutOverrides({ ...keyboardShortcutOverrides.value, [id]: chord });
      setCapturingShortcut(undefined);
    }),
  );
  // Project Settings → AI tools (HS2-EK24KF): the default provider, then each provider's own model
  // and effort; the provider a field belongs to is its block's `data-ai-provider`.
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name="ai-default-tool"]', (_event, target) => {
      selectDefaultProvider((target as Control).value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name^="ai-provider-model-"]', (_event, target) => {
      const block = target.closest<HTMLElement>('[data-ai-provider]'),
        tool = block?.dataset.aiProvider,
        model = (target as Control).value;
      if (!tool) return;
      if (block.dataset.otherModelValue === model) openManualModel('settings', undefined, tool);
      else selectDefaultModel(model, tool);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name^="ai-provider-effort-"]', (_event, target) => {
      const tool = target.closest<HTMLElement>('[data-ai-provider]')?.dataset.aiProvider;
      if (tool) selectDefaultEffort((target as Control).value, tool);
    }),
  );
  // Each provider's default chat permission mode (HS2-NHRDMW). Declining a warned mode puts the
  // Select back on the saved value.
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name^="ai-provider-permission-mode-"]', (_event, target) => {
      const block = target.closest<HTMLElement>('[data-ai-provider]'),
        tool = block?.dataset.aiProvider,
        control = target as Control;
      if (!tool) return;
      if (!selectDefaultPermissionMode(control.value, tool) && block.dataset.permissionMode)
        control.value = block.dataset.permissionMode;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.cancelManualModel.selector, () => {
      const state = manualModelDialog.value;
      manualModelDialog.value = undefined;
      restoreCommandEditorAfterManualModel(state);
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', COMMANDS_AND_AI_ACTIONS.submitManualModel.selector, (event, target) => {
      event.preventDefault();
      const state = manualModelDialog.value,
        model = target.querySelector<Control>('[name="manual-model"]')?.value.trim();
      if (!state || !model) return;
      manualModelDialog.value = undefined;
      if (state.target === 'drive') selectDriveModel(model);
      else if (state.target === 'conversation') selectConversationModel(model);
      else if (state.target === 'command') {
        const current = project(),
          command = state.commandId
            ? commandSettingsDefinitions(current?.id).find((item) => item.id === state.commandId)
            : undefined;
        if (current && command && state.commandId)
          updateCommandAiSelection(current.id, state.commandId, {
            tool: effectiveCommandAiSelection(command).tool,
            model,
          });
        restoreCommandEditorAfterManualModel(state);
      } else selectDefaultModel(model, state.providerId);
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'kui-request-close', COMMANDS_AND_AI_TARGETS.manualModelDialog.selector, () => {
      const state = manualModelDialog.value;
      manualModelDialog.value = undefined;
      restoreCommandEditorAfterManualModel(state);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.openProviderDialog.selector, () => {
      const current = project();
      if (!current) return;
      ticketSourceSetupProject.value = current;
      providerSetupKind.value = undefined;
      providerEditingId.value = undefined;
      providerSettingsError.value = '';
      createdGitTicketStore.value = '';
      ticketSourceSetupNavigation.value = 'none';
      providerAccountChoice.value = undefined;
      void refreshProviderConnections(current);
      // Signed-in accounts a new source can reuse (HS2-SM9PM8).
      void refreshProviderAccounts(current);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.editProviderConnection.selector, (_event, target) => {
      const current = project(),
        id = data(target).itemId,
        connection = providerConnections.value.find((item) => item.id === id),
        gitSource = (target as HTMLElement).closest('[data-source-provider="git"]');
      if (!current || !id || (!connection && !gitSource)) return;
      ticketSourceSetupNavigation.value = 'none';
      ticketSourceSetupProject.value = current;
      providerSetupKind.value = connection?.provider as ExternalProviderKind | undefined;
      providerEditingId.value = id;
      providerRemovingId.value = undefined;
      providerSettingsError.value = '';
      if (connection) void loadEditingGitHubRepositories(connection, current);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.removeProjectSource.selector, (_event, target) => {
      const id = data(target).sourceId;
      if (id) requestProjectSourceRemoval(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.useGithubAccount.selector, (_event, target) => {
      const id = data(target).itemId;
      if (id) void useGithubAccount(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.useProviderAccount.selector, (_event, target) => {
      const id = data(target).itemId;
      if (id) useProviderAccount(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.signOutAccount.selector, (_event, target) => {
      const id = data(target).itemId;
      if (id) void signOutProviderAccount(id);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      COMMANDS_AND_AI_ACTIONS.requestUnusedAccountSourceRemoval.selector,
      (_event, target) => {
        const source = data(target).sourceId,
          account = data(target).accountId;
        if (source && account) requestUnusedAccountSourceRemoval(account, source);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.cancelUnusedAccountSourceRemoval.selector, () => {
      cancelUnusedAccountSourceRemoval();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.removeUnusedAccountSource.selector, (_event, target) => {
      const source = data(target).sourceId,
        account = data(target).accountId;
      if (source && account) void removeUnusedAccountSource(account, source);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'kui-select[name="project-default-source"]', (_event, target) => {
      const id = (target as Control).value;
      if (id) void setProjectDefaultSource(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="project-source-color"][data-source-id]', (_event, target) => {
      const option = target as HTMLInputElement;
      const id = option.dataset.sourceId;
      if (id && option.checked) void setProjectSourceColor(id, option.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.retryProviderOutbox.selector, (_event, target) => {
      const { connectionId, operationId } = (target as HTMLElement).dataset;
      if (connectionId && operationId) void retryProviderOutbox(connectionId, operationId);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', INSPECTOR_AND_EDITOR_ACTIONS.discardProviderOutbox.selector, (_event, target) => {
      const { connectionId, operationId } = (target as HTMLElement).dataset;
      if (connectionId && operationId) void discardProviderOutbox(connectionId, operationId);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.requestProviderRemoval.selector, () => {
      requestProviderRemoval();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.cancelProviderRemoval.selector, () => {
      cancelProviderRemoval();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.confirmProviderRemoval.selector, () => {
      void removeExternalProvider();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.toggleProviderDisabled.selector, () => {
      void toggleProviderDisabled();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.refreshGithubRepositories.selector, () => {
      void refreshGitHubRepositories();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.chooseGithubEnterprise.selector, () => {
      chooseGitHubEnterprise(true);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.chooseGithubDotcom.selector, () => {
      chooseGitHubEnterprise(false);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.copyGithubCode.selector, () => {
      void copyGitHubCode();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.reopenGithubSignIn.selector, () => {
      reopenGitHubSignIn();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectProviderKind.selector, (_event, target) => {
      ticketSourceSetupNavigation.value = 'push';
      providerSetupKind.value = (data(target).itemId ?? data(target).providerKind) as ExternalProviderKind;
      providerEditingId.value = undefined;
      providerSettingsError.value = '';
      githubAuth.value = undefined;
      providerAccountChoice.value = undefined;
      // Signed-in accounts a new source can reuse, also when onboarding opened this dialog (HS2-SM9PM8),
      // for every provider kind (HS2-F5HNJN).
      void refreshProviderAccounts(ticketSourceSetupProject.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.backProviderKind.selector, () => {
      cancelGitHubSignIn();
      ticketSourceSetupNavigation.value = 'pop';
      providerSetupKind.value = undefined;
      providerEditingId.value = undefined;
      providerSettingsError.value = '';
      githubAuth.value = undefined;
      providerAccountChoice.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.startGithubSignIn.selector, (_event, target) => {
      const form = target.closest<HTMLFormElement>('form');
      if (form) void startGitHubSignIn(form);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.cancelGithubSignIn.selector, () => {
      cancelGitHubSignIn();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.submitProviderSetup.selector, () =>
      document.querySelector<HTMLFormElement>('#provider-setup-form')?.requestSubmit(),
    ),
  );
  lifetime.add(
    delegate(document.body, 'submit', COMMANDS_AND_AI_ACTIONS.saveProviderConnection.selector, (event, target) => {
      event.preventDefault();
      void saveExternalProvider(target as HTMLFormElement);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
