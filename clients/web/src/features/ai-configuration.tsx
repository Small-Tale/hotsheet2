import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import { type ConversationState, EMPTY_CONVERSATION } from '../ai-conversation';
import { type AiToolDefaults, type AiToolDescriptor, Api, type CommandDefinition, type ToolConnection } from '../api';
import { COMMAND_EDITOR_DIALOG_ID } from '../components/command-settings-editor';
import { type ManualModelDialogState } from '../components/manual-model-dialog';
import { conversationTranscriptMarkdown } from '../conversation-export';
import { syncConversationScroll } from '../conversation-scroll';
import type { Control, Project } from '../interactions/types';
import { compatibleAiEffort, type DrawerAIChat } from '../project-drive';

export interface AiConfigurationDependencies {
  selectedProjectId: Signal<string>;
  project: () => Project | undefined;
  conversationConnectionId: Signal<string | undefined>;
  conversationSelections: Signal<Record<string, { model?: string; effort?: string }>>;
  terminalDrawerChatsByProject: Signal<Record<string, DrawerAIChat[]>>;
  driveConnectionsByProject: Signal<Record<string, ToolConnection[]>>;
  conversationStates: Signal<Record<string, ConversationState>>;
  conversationOpen: Signal<boolean>;
  createDrawerAIChat: (selection: AiToolDefaults) => Promise<{ connectionId: string } | undefined>;
  showToast: (message: string) => void;
  beginConversation: (connectionId: string, content: string) => void;
  updateConversation: (connectionId: string, update: (state: ConversationState) => ConversationState) => void;
  error: Signal<string>;
  commandSettingsDefinitions: (projectId?: string) => CommandDefinition[];
  commandSettingsEditingId: Signal<string | undefined>;
}

export function createAiConfigurationController(dependencies: AiConfigurationDependencies) {
  const {
    selectedProjectId,
    project,
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
  } = dependencies;
  const aiTools = signal<AiToolDescriptor[]>([]),
    aiDefaults = signal<AiToolDefaults>({ tool: 'codex' }),
    aiSettingsLoading = signal(false),
    aiSettingsMessage = signal(''),
    driveOptionsOpen = signal(false),
    driveOverridesByProject = signal<Record<string, Partial<AiToolDefaults>>>({}),
    manualModelDialog = signal<ManualModelDialogState | undefined>(undefined);
  let manualModelDialogShown = false;
  let aiConfigurationProjectId = '';

  function aiToolLabel(tool: string) {
    return (
      aiTools.value.find((item) => item.id === tool)?.display_name ??
      `${tool.slice(0, 1).toUpperCase()}${tool.slice(1)}`
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

  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function conversationAiSelection(connectionId:string){const current=project(),connection=current?(driveConnectionsByProject.value[current.id]??[]).find(item=>item.id===connectionId):undefined,chat=current?(terminalDrawerChatsByProject.value[current.id]??[]).find(item=>item.connectionId===connectionId):undefined,override=conversationSelections.value[connectionId],tool=chat?.tool??connection?.tool??aiDefaults.value.tool,descriptor=aiTools.value.find(item=>item.id===tool),model=override?.model??chat?.model??connection?.model??descriptor?.default_model,modelDescriptor=descriptor?.models.find(item=>item.id===model),efforts=modelDescriptor?.effort_levels??[],effort=compatibleAiEffort(efforts,override?.effort,chat?.effort,connection?.effort,descriptor?.default_effort);return{tool,descriptor,model,effort,efforts}}

  return {
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
    aiToolOptions,
    selectConversationProvider,
    openManualModel,
    restoreCommandEditorAfterManualModel,
    aiLaunchConfiguration,
    refreshAiConfiguration,
    saveAiDefaults,
    conversationAiSelection,
    get manualModelDialogShown() {
      return manualModelDialogShown;
    },
    set manualModelDialogShown(value: typeof manualModelDialogShown) {
      manualModelDialogShown = value;
    },
    get aiConfigurationProjectId() {
      return aiConfigurationProjectId;
    },
    set aiConfigurationProjectId(value: typeof aiConfigurationProjectId) {
      aiConfigurationProjectId = value;
    },
  };
}
