import { delegate, delegateCapture, effect, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  type AiToolDefaults,
  type AiToolDescriptor,
  Api,
  type CommandDefinition,
  type CommandRun,
  type FullTicket,
  type ProviderConnection,
} from '../api';
import { type CommandDropTarget } from '../command-order';
import { isConversationSurfaceLifecycleEvent } from '../components/ai-conversation';
import { COMMAND_EDITOR_DIALOG_ID } from '../components/command-settings-editor';
import { type ConversationExportDialogState } from '../components/conversation-export-dialog';
import { type ManualModelDialogState } from '../components/manual-model-dialog';
import { type ExternalProviderKind, type GithubAuthState } from '../components/provider-setup-form';
import { type SettingsCategory } from '../components/settings-navigation';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { revealContextPopupMenu } from '../context-menu-position';
import { type ConversationExportDraft } from '../conversation-export';
import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';
import { beginInteractionTiming } from '../interaction-performance';
import { chordFromEvent, saveShortcutOverrides, type ShortcutChord, shortcutDef } from '../keyboard-shortcuts';
import { loadLucideCatalog } from '../lucide-catalog';
import { toggleCollapsedCommandGroup } from '../workspace-preferences';
import { data } from './dom';
import { type Control, type Project } from './types';

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
  readonly openConversationExport: () => void;
  readonly pickConversationMessage: (connectionId: string, messageId: string) => void;
  readonly copyConversationSelection: (connectionId: string) => Promise<void>;
  readonly clearConversationSelection: (connectionId: string) => void;
  readonly conversationExportDialog: Signal<ConversationExportDialogState | undefined>;
  readonly finishConversationExport: () => Promise<void>;
  readonly updateConversationExportDraft: (update: (draft: ConversationExportDraft) => ConversationExportDraft) => void;
  readonly conversationConnectionId: Signal<string | undefined>;
  readonly conversationDrafts: Signal<Record<string, string>>;
  readonly sendConversationTurn: () => Promise<void>;
  readonly stopConversation: () => Promise<void>;
  readonly selectConversationProvider: (providerId: string) => Promise<void>;
  readonly selectConversationModel: (model: string) => void;
  readonly selectConversationEffort: (effort: string) => void;
  readonly selectedTicket: Signal<FullTicket | null>;
  readonly canAddNotes: () => boolean;
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
  draggedCommandIds: string[];
  readonly commandSettingsDefinitions: (projectId?: string) => CommandDefinition[];
  readonly selectCommandSetting: (projectId: string, id: string | undefined) => void;
  readonly clearCommandDropIndicators: () => void;
  readonly clearCommandDrag: () => void;
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
  readonly resetProgressiveTicketRendering: () => void;
  readonly viewMode: Signal<WorkspaceViewMode>;
  readonly refreshProject: (options?: { showLoading?: boolean }) => Promise<unknown>;
  readonly setSettingsCategory: (_projectId: string, category: SettingsCategory) => void;
  readonly refreshProviderConnections: (current?: Project) => Promise<void>;
  readonly refreshTerminalSettings: (current?: Project) => Promise<void>;
  readonly refreshTrashSettings: (current?: Project) => Promise<void>;
  readonly capturingShortcutId: Signal<string | undefined>;
  readonly keyboardShortcutOverrides: Signal<Record<string, ShortcutChord>>;
  readonly appleShortcutPlatform: boolean;
  readonly saveAiDefaults: (value: AiToolDefaults) => Promise<void>;
  readonly selectDefaultModel: (model: string, tool?: string) => void;
  readonly selectDefaultEffort: (effort: string, tool?: string) => void;
  readonly selectDefaultProvider: (tool: string) => void;
  readonly restoreCommandEditorAfterManualModel: (state: ManualModelDialogState | undefined) => void;
  manualModelDialogShown: boolean;
  readonly aiDefaults: Signal<AiToolDefaults>;
  readonly ticketSourceSetupProject: Signal<Project | undefined>;
  readonly providerSetupKind: Signal<ExternalProviderKind | undefined>;
  readonly providerEditingId: Signal<string | undefined>;
  readonly requestProjectSourceRemoval: (id: string) => void;
  readonly refreshProviderAccounts: (current?: Project) => Promise<void>;
  readonly signOutProviderAccount: (id: string) => Promise<void>;
  readonly useGithubAccount: (id: string) => Promise<void>;
  /** The GitLab or Jira account prefilling a new source's form (HS2-F5HNJN). */
  readonly providerAccountChoice: Signal<string | undefined>;
  readonly useProviderAccount: (id: string) => void;
  readonly setProjectDefaultSource: (id: string) => Promise<void>;
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
    refreshProject,
    setSettingsCategory,
    refreshProviderConnections,
    refreshTerminalSettings,
    refreshTrashSettings,
    capturingShortcutId,
    keyboardShortcutOverrides,
    appleShortcutPlatform,
    selectDefaultModel,
    selectDefaultEffort,
    selectDefaultProvider,
    restoreCommandEditorAfterManualModel,
    ticketSourceSetupProject,
    providerSetupKind,
    providerEditingId,
    requestProjectSourceRemoval,
    refreshProviderAccounts,
    signOutProviderAccount,
    useGithubAccount,
    providerAccountChoice,
    useProviderAccount,
    setProjectDefaultSource,
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
    delegateCapture(document.body, 'wa-hide', COMMANDS_AND_AI_TARGETS.aiConversation.selector, (event, target) => {
      if (isConversationSurfaceLifecycleEvent(event, target)) conversationOpen.value = false;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.saveConversation.selector, () => {
      openConversationExport();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event, target) => {
      if ((event.target as Element).closest('a,button,input,select,textarea') || window.getSelection()?.toString())
        return;
      const connectionId = target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId,
        messageId = data(target).messageId;
      if (connectionId && messageId) pickConversationMessage(connectionId, messageId);
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'Enter' && keyboard.key !== ' ') return;
      keyboard.preventDefault();
      (target as HTMLElement).click();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.copyConversationSelection.selector, (_event, target) => {
      const connectionId = target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId;
      if (connectionId) void copyConversationSelection(connectionId);
    }),
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
      else if (state.selectedRange)
        updateConversationExportDraft((draft) => ({ ...draft, scope: state.selectedRange! }));
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
      'wa-hide',
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
    delegate(document.body, 'input', COMMANDS_AND_AI_TARGETS.conversationDraftField.selector, (_event, target) => {
      const id = conversationConnectionId.value;
      if (id) conversationDrafts.value = { ...conversationDrafts.value, [id]: (target as HTMLTextAreaElement).value };
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', COMMANDS_AND_AI_TARGETS.conversationDraftField.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'Enter' || keyboard.shiftKey || keyboard.isComposing) return;
      keyboard.preventDefault();
      target.closest<HTMLFormElement>('form')?.requestSubmit();
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', COMMANDS_AND_AI_ACTIONS.sendConversationTurn.selector, (event) => {
      event.preventDefault();
      void sendConversationTurn();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.stopConversation.selector, () => {
      void stopConversation();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectConversationProvider.selector, (_event, target) => {
      void selectConversationProvider(data(target).value ?? '');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectConversationModel.selector, (_event, target) => {
      selectConversationModel(data(target).value ?? '');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectConversationEffort.selector, (_event, target) => {
      selectConversationEffort(data(target).value ?? '');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.openConversationManualModel.selector, () => {
      openManualModel('conversation');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.rateAiContent.selector, (_event, target) => {
      const ticket = selectedTicket.value,
        rating = data(target).aiFeedbackRating,
        targetId = data(target).aiFeedbackTarget;
      if (!ticket || !targetId || !['helpful', 'not-helpful'].includes(rating ?? '') || !canAddNotes()) return;
      const detail = window
        .prompt(
          rating === 'helpful'
            ? 'What should Hot Sheet keep doing? (optional)'
            : 'What should Hot Sheet change or stop doing? (optional)',
        )
        ?.trim();
      if (detail === undefined) return;
      const consequence =
          rating === 'helpful' ? 'Helpful — keep suggestions like this.' : 'Not helpful — stop suggestions like this.',
        note = [`AI feedback for ${targetId}: ${consequence}`, detail].filter(Boolean).join('\n\n');
      void updateSelected({ note, note_kind: 'regular' }).then((saved) => {
        if (saved) showToast('AI feedback saved as a ticket note.');
      });
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
      'close',
      '[data-component="command-run-dialog"], [data-component="command-cancellation-dialog"]',
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function openCommandEditor(id:string){commandSettingsEditingId.value=id;commandIconSearch.value='';void loadLucideCatalog();(document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control).showPopover?.()}
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.editCommandSetting.selector, (_event, target) => {
      const id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;
      if (id) openCommandEditor(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.addCommandSetting.selector, () => {
      const current = project();
      if (current) openCommandEditor(addCommandSetting(current.id));
    }),
  );
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  lifetime.add(delegate(document.body,'click',COMMANDS_AND_AI_ACTIONS.closeCommandEditor.selector,()=>{(document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control).hidePopover?.();commandSettingsEditingId.value=undefined;commandIconSearch.value=''}));
  lifetime.add(
    delegateCapture(
      document.body,
      'toggle',
      `#${COMMAND_EDITOR_DIALOG_ID}`,
      (event) => {
        if ((event as ToggleEvent).newState === 'closed' && manualModelDialog.value?.target !== 'command') {
          commandSettingsEditingId.value = undefined;
          commandIconSearch.value = '';
        }
      },
      { match: 'direct' },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.deleteCommandSetting.selector, (_event, target) => {
      const current = project(),
        id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;
      if (current && id) deleteCommandSetting(current.id, id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.addCommandGroup.selector, () => {
      const current = project();
      if (current) addCommandGroup(current.id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.deleteCommandGroup.selector, (_event, target) => {
      const current = project(),
        group = target.closest<HTMLElement>('[data-group]')?.dataset.group;
      if (current && group) deleteCommandGroup(current.id, group);
    }),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', '.command-settings-editor__row', (event, target) => {
      if ((event.target as Element).closest('.command-settings-editor__row-menu')) return;
      const id = (target as HTMLElement).dataset.commandId;
      if (id) openCommandEditor(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'contextmenu', '.command-settings-editor__row', (event, target) => {
      // Right-click opens the row's PopupMenu (a wa-dropdown root) in place of a separate context menu.
      const menu = target.querySelector<HTMLElement & { open: boolean }>(
        '.command-settings-editor__row-menu [data-component="popup-menu"]',
      );
      if (!menu) return;
      event.preventDefault();
      menu.open = true;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', '.command-settings-editor__row', (event, target) => {
      if ((event.target as Element).closest('.command-settings-editor__row-menu, .command-settings-editor__row-grip'))
        return;
      const current = project(),
        id = (target as HTMLElement).dataset.commandId;
      if (!current || !id) return;
      const mouse = event as MouseEvent;
      selectCommandRow(current.id, id, { toggle: mouse.metaKey || mouse.ctrlKey, range: mouse.shiftKey });
    }),
  );
  lifetime.add(
    delegate(document.body, 'dragstart', '.command-settings-editor__row', (event, target) => {
      const current = project(),
        element = target as HTMLElement,
        id = element.dataset.commandId;
      if (!current || !id) return;
      const selection = commandSelection(current.id);
      dependencies.draggedCommandIds =
        selection.length > 1 && selection.includes(id)
          ? commandSettingsDefinitions(current.id)
              .map((command) => command.id)
              .filter((commandId) => selection.includes(commandId))
          : [id];
      if (dependencies.draggedCommandIds.length <= 1) {
        selectCommandSetting(current.id, id);
      }
      document.querySelectorAll<HTMLElement>('.command-settings-editor__row').forEach((row) => {
        if (dependencies.draggedCommandIds.includes(row.dataset.commandId ?? '')) row.dataset.commandDragging = 'true';
      });
      const transfer = (event as DragEvent).dataTransfer;
      if (transfer) {
        transfer.effectAllowed = 'move';
        transfer.setData('text/plain', dependencies.draggedCommandIds.join(','));
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'dragover', '.command-settings-editor__list', (event) => {
      if (!dependencies.draggedCommandIds.length) return;
      const drag = event as DragEvent,
        over = drag.target as Element,
        row = over.closest<HTMLElement>('[data-command-id]');
      clearCommandDropIndicators();
      if (row && row.dataset.commandId && !dependencies.draggedCommandIds.includes(row.dataset.commandId)) {
        drag.preventDefault();
        const bounds = row.getBoundingClientRect();
        row.dataset.commandDropPosition = drag.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after';
        if (drag.dataTransfer) drag.dataTransfer.dropEffect = 'move';
        return;
      }
      const container = over.closest<HTMLElement>('[data-command-group-drop]');
      if (container) {
        drag.preventDefault();
        container.dataset.commandDropActive = 'true';
        if (drag.dataTransfer) drag.dataTransfer.dropEffect = 'move';
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'drop', '.command-settings-editor__list', (event) => {
      const sources = dependencies.draggedCommandIds,
        current = project();
      if (!sources.length) {
        clearCommandDrag();
        return;
      }
      const drag = event as DragEvent,
        over = drag.target as Element,
        row = over.closest<HTMLElement>('[data-command-id]');
      drag.preventDefault();
      let dropTarget: CommandDropTarget | undefined;
      if (row && row.dataset.commandId && !sources.includes(row.dataset.commandId)) {
        const bounds = row.getBoundingClientRect();
        dropTarget = {
          kind: 'row',
          id: row.dataset.commandId,
          position: drag.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after',
        };
      } else {
        const container = over.closest<HTMLElement>('[data-command-group-drop]');
        if (container) dropTarget = { kind: 'group', group: container.dataset.commandGroupDrop ?? '' };
      }
      clearCommandDrag();
      if (current && dropTarget) reorderCommandSettings(current.id, sources, dropTarget);
    }),
  );
  lifetime.add(delegate(document.body, 'dragend', '.command-settings-editor__row', clearCommandDrag));
  lifetime.add(
    delegate(document.body, 'input', '[data-command-field]', (_event, target) => {
      const current = project(),
        field = (target as HTMLInputElement).name,
        id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;
      if (current && id && field) updateCommandSetting(current.id, id, field, (target as HTMLInputElement).value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', COMMANDS_AND_AI_TARGETS.commandIconSearchField.selector, (_event, target) => {
      commandIconSearch.value = (target as HTMLInputElement).value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectCommandIcon.selector, (_event, target) => {
      const current = project(),
        id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId,
        name = (target as HTMLElement).dataset.iconName;
      if (current && id && name) updateCommandSetting(current.id, id, 'icon', name);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectCommandAiDefault.selector, (_event, target) => {
      const current = project(),
        id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;
      if (current && id) updateCommandAiSelection(current.id, id, {});
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectCommandAiTool.selector, (_event, target) => {
      const current = project(),
        id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId,
        tool = data(target).value,
        descriptor = aiTools.value.find((item) => item.id === tool);
      if (!current || !id || !tool || !descriptor) return;
      const selection = normalizedAiSelection({ tool });
      updateCommandAiSelection(current.id, id, selection);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectCommandAiModel.selector, (_event, target) => {
      const current = project(),
        id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId,
        model = data(target).value,
        command = id ? commandSettingsDefinitions(current?.id).find((item) => item.id === id) : undefined;
      if (!current || !id || !model || !command) return;
      const active = effectiveCommandAiSelection(command),
        selection = normalizedAiSelection({ tool: active.tool, model });
      updateCommandAiSelection(current.id, id, selection);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.selectCommandAiEffort.selector, (_event, target) => {
      const current = project(),
        id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId,
        effort = data(target).value,
        command = id ? commandSettingsDefinitions(current?.id).find((item) => item.id === id) : undefined;
      if (!current || !id || !effort || !command) return;
      updateCommandAiSelection(current.id, id, { ...effectiveCommandAiSelection(command), effort });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', COMMANDS_AND_AI_ACTIONS.openCommandManualModel.selector, (_event, target) => {
      const id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;
      if (id) openManualModel('command', id);
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
      terminalSettingsMessage.value = 'Saving…';
      void new Api(current.apiPath)
        .saveTerminalSettings({ inherit_global_shell_history: checked })
        .then((value) => {
          if (project()?.id !== current.id) return;
          inheritGlobalShellHistory.value = value.inherit_global_shell_history;
          terminalSettingsMessage.value = 'Saved locally. New terminals will use this setting.';
        })
        .catch((reason: unknown) => {
          if (project()?.id !== current.id) return;
          inheritGlobalShellHistory.value = !checked;
          terminalSettingsMessage.value = reason instanceof Error ? reason.message : String(reason);
        });
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', COMMANDS_AND_AI_ACTIONS.saveTrashSettings.selector, (event, target) => {
      event.preventDefault();
      const current = project(),
        raw = target.querySelector<Control>('[name="trash-cleanup-days"]')?.value ?? '',
        days = Number(raw);
      if (!current) return;
      if (!Number.isSafeInteger(days) || days < 1) {
        trashSettingsMessagesByProject.value = {
          ...trashSettingsMessagesByProject.value,
          [current.id]: 'Enter a positive whole number of days.',
        };
        return;
      }
      trashSettingsMessagesByProject.value = { ...trashSettingsMessagesByProject.value, [current.id]: 'Saving…' };
      void new Api(current.apiPath)
        .saveTrashSettings(current.id, { trash_cleanup_days: days })
        .then((value) => {
          if (project()?.id !== current.id) return;
          trashCleanupDaysByProject.value = {
            ...trashCleanupDaysByProject.value,
            [current.id]: value.trash_cleanup_days,
          };
          trashSettingsMessagesByProject.value = {
            ...trashSettingsMessagesByProject.value,
            [current.id]: 'Saved for this project.',
          };
          showToast('Trash retention saved.');
        })
        .catch((reason: unknown) => {
          if (project()?.id === current.id)
            trashSettingsMessagesByProject.value = {
              ...trashSettingsMessagesByProject.value,
              [current.id]: reason instanceof Error ? reason.message : String(reason),
            };
        });
    }),
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
      if (category === 'lifecycle') void refreshTrashSettings();
      if (category === 'ai' || category === 'commands') void refreshAiConfiguration(undefined, true);
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
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  lifetime.add(delegate(document.body,'click',COMMANDS_AND_AI_ACTIONS.resetShortcut.selector,(_event,target)=>{const id=data(target).shortcutId;if(!id||!keyboardShortcutOverrides.value[id])return;persistShortcutOverrides(Object.fromEntries(Object.entries(keyboardShortcutOverrides.value).filter(([entryId])=>entryId!==id)));if(capturingShortcutId.value===id)setCapturingShortcut(undefined)}));
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
    delegate(document.body, 'change', 'wa-select[name="ai-default-tool"]', (_event, target) => {
      selectDefaultProvider((target as Control).value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'wa-select[name^="ai-provider-model-"]', (_event, target) => {
      const block = target.closest<HTMLElement>('[data-ai-provider]'),
        tool = block?.dataset.aiProvider,
        model = (target as Control).value;
      if (!tool) return;
      if (block.dataset.otherModelValue === model) openManualModel('settings', undefined, tool);
      else selectDefaultModel(model, tool);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'wa-select[name^="ai-provider-effort-"]', (_event, target) => {
      const tool = target.closest<HTMLElement>('[data-ai-provider]')?.dataset.aiProvider;
      if (tool) selectDefaultEffort((target as Control).value, tool);
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
    delegateCapture(document.body, 'wa-after-show', COMMANDS_AND_AI_TARGETS.manualModelDialog.selector, () => {
      dependencies.manualModelDialogShown = true;
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'wa-hide', COMMANDS_AND_AI_TARGETS.manualModelDialog.selector, () => {
      if (dependencies.manualModelDialogShown) {
        const state = manualModelDialog.value;
        manualModelDialog.value = undefined;
        restoreCommandEditorAfterManualModel(state);
      }
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
        connection = providerConnections.value.find((item) => item.id === data(target).itemId);
      if (!current || !connection) return;
      ticketSourceSetupNavigation.value = 'none';
      ticketSourceSetupProject.value = current;
      providerSetupKind.value = connection.provider as ExternalProviderKind;
      providerEditingId.value = connection.id;
      providerRemovingId.value = undefined;
      providerSettingsError.value = '';
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
    delegate(document.body, 'change', 'wa-select[name="project-default-source"]', (_event, target) => {
      const id = (target as Control).value;
      if (id) void setProjectDefaultSource(id);
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
