import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import {
  applyConversationEvent,
  beginConversationTurn,
  type ConversationState,
  EMPTY_CONVERSATION,
  reconcileConversationConnection,
} from '../ai-conversation';
import { type AiToolDefaults, Api, type ToolConnection } from '../api';
import { browserRandomId } from '../browser-id';
import {
  createConversationPersistence,
  loadConversationStates,
  saveConversationStates,
} from '../conversation-persistence';
import { createConversationRenderScheduler } from '../conversation-render-scheduler';
import { syncConversationScroll } from '../conversation-scroll';
import type { Control, Project } from '../interactions/types';
import {
  type DrawerAIChat,
  prepareProjectConversation,
  projectChatConnectionId,
  recoverProjectConnections,
  restoreDrawerAIChats,
  runProjectDrive,
  SIDEBAR_DRIVE_PROMPT,
  sidebarDriveConnectionId,
} from '../project-drive';
import type { createAiConfigurationController } from './ai-configuration';

type AiConfiguration = Pick<
  ReturnType<typeof createAiConfigurationController>,
  | 'restoreAiConfiguration'
  | 'refreshAiConfiguration'
  | 'effectiveDriveSelection'
  | 'normalizedAiSelection'
  | 'conversationAiSelection'
  | 'aiToolLabel'
>;

/** Live application bindings the drive-conversation owner reads (HS2-K7SYHQ). */
export interface DriveConversationsDependencies {
  projects: Signal<Project[]>;
  project: () => Project | undefined;
  error: Signal<string>;
  terminalDrawerChatsByProject: Signal<Record<string, DrawerAIChat[]>>;
  createDrawerAIChat: (
    selection: AiToolDefaults,
    options?: { connectionId?: string; drive?: boolean },
  ) => Promise<DrawerAIChat | undefined>;
  selectDrawerItem: (id: string) => void;
  setTerminalDrawerVisible: (visible: boolean) => void;
  /** Resolved lazily: the AI configuration owner is created after this one and reads its state. */
  aiConfiguration: () => AiConfiguration;
}

/**
 * Owns project drive and sidebar AI conversation state: tool connections per project, the
 * persisted conversation transcripts, drafts, and the start/send/stop/refresh workflows
 * extracted from the application runtime (HS2-K7SYHQ).
 */
export function createDriveConversationsController(dependencies: DriveConversationsDependencies) {
  const {
    projects,
    project,
    error,
    terminalDrawerChatsByProject,
    createDrawerAIChat,
    selectDrawerItem,
    setTerminalDrawerVisible,
  } = dependencies;
  const effectiveDriveSelection: AiConfiguration['effectiveDriveSelection'] = (...args) =>
      dependencies.aiConfiguration().effectiveDriveSelection(...args),
    normalizedAiSelection: AiConfiguration['normalizedAiSelection'] = (...args) =>
      dependencies.aiConfiguration().normalizedAiSelection(...args),
    conversationAiSelection: AiConfiguration['conversationAiSelection'] = (...args) =>
      dependencies.aiConfiguration().conversationAiSelection(...args),
    aiToolLabel: AiConfiguration['aiToolLabel'] = (...args) => dependencies.aiConfiguration().aiToolLabel(...args);
  const driveConnectionsByProject = signal<Record<string, ToolConnection[]>>({}),
    drivePendingByProject = signal<Record<string, boolean>>({});

  let conversationStartGeneration = 0;
  const pendingConversationStarts = new Set<string>();
  const conversationStates = signal<Record<string, ConversationState>>(loadConversationStates(localStorage)),
    conversationDrafts = signal<Record<string, string>>({}),
    conversationSelections = signal<Record<string, { model?: string; effort?: string }>>({}),
    conversationConnectionId = signal<string | undefined>(undefined),
    conversationOpen = signal(false);
  const conversationRenderRevision = signal(0);
  const conversationRenderScheduler = createConversationRenderScheduler(() => {
    conversationRenderRevision.value += 1;
  });
  const conversationPersistence = createConversationPersistence(() => {
    try {
      saveConversationStates(localStorage, conversationStates.peek());
    } catch {
      /* storage quota/privacy mode must not interrupt a live turn */
    }
  });

  async function refreshDriveConnections(current = project(), restoreDrawerTabs = false, quiet = false) {
    if (!current) return;
    if (project()?.id === current.id && !dependencies.aiConfiguration().restoreAiConfiguration(current))
      void dependencies.aiConfiguration().refreshAiConfiguration(current);
    try {
      const startGeneration = conversationStartGeneration,
        pendingAtRequest = new Set(pendingConversationStarts),
        client = new Api(current.apiPath, '', { trackBusy: !quiet }),
        [active, sessions] = await Promise.all([client.activeToolConnections(), client.toolSessions().catch(() => [])]),
        activeIds = new Set(active.map((connection) => connection.id)),
        connections = await recoverProjectConnections(client, active, sessions, current.id, current.root);
      if (startGeneration === conversationStartGeneration && projects.value.some((item) => item.id === current.id)) {
        for (const connection of connections)
          if (
            // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Persisted per-connection state may be missing at this runtime boundary.
            conversationStates.peek()[connection.id]?.activeAssistantId &&
            !pendingAtRequest.has(connection.id) &&
            !pendingConversationStarts.has(connection.id)
          )
            updateConversation(connection.id, (state) =>
              !state.activeAssistantId
                ? state
                : !activeIds.has(connection.id)
                  ? applyConversationEvent(state, { type: 'done', reason: 'interrupted' })
                  : reconcileConversationConnection(state, connection),
            );
        driveConnectionsByProject.value = { ...driveConnectionsByProject.value, [current.id]: connections };
        if (restoreDrawerTabs)
          terminalDrawerChatsByProject.value = {
            ...terminalDrawerChatsByProject.value,
            [current.id]: restoreDrawerAIChats(
              connections,
              current.id,
              terminalDrawerChatsByProject.value[current.id],
              aiToolLabel,
            ),
          };
      }
    } catch {
      /* retain the last event-projected state while a project server reconnects */
    }
  }
  function replaceConversationStates(states: Record<string, ConversationState>, streamed = false) {
    conversationStates.value = states;
    conversationPersistence.schedule();
    if (streamed) conversationRenderScheduler.schedule();
    else conversationRenderScheduler.immediate();
  }
  function updateConversation(
    connectionId: string,
    update: (state: ConversationState) => ConversationState,
    streamed = false,
  ) {
    const conversations = conversationStates.peek();
    replaceConversationStates(
      {
        ...conversations,
        [connectionId]: update(conversations[connectionId] ?? EMPTY_CONVERSATION),
      },
      streamed,
    );
  }
  function conversationForActivity(current: Project, tool: string, session?: string) {
    const conversations = conversationStates.peek(),
      connections = (driveConnectionsByProject.value[current.id] ?? []).filter(
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
        (item) => item.tool.toLowerCase() === tool.toLowerCase() && conversations[item.id],
      );
    return (
      connections.find((item) => session && (item.session_id === session || item.id === session)) ??
      (connections.length === 1 ? connections[0] : undefined)
    );
  }
  function beginConversation(connectionId: string, content: string) {
    conversationStartGeneration += 1;
    pendingConversationStarts.add(connectionId);
    updateConversation(connectionId, (state) => beginConversationTurn(state, browserRandomId(), content));
    return () => {
      pendingConversationStarts.delete(connectionId);
    };
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
    const finishStart = beginConversation(connectionId, SIDEBAR_DRIVE_PROMPT);
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
      finishStart();
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
  async function sendConversationTurn() {
    const current = project(),
      connectionId = conversationConnectionId.value,
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      draft = connectionId ? conversationDrafts.value[connectionId]?.trim() : '';
    if (!current || !connectionId || !draft) return;
    const connection = (driveConnectionsByProject.value[current.id] ?? []).find((item) => item.id === connectionId),
      selection = conversationAiSelection(connectionId),
      turnSelection = {
        ...(selection.descriptor?.actions?.includes('change_model') && selection.model
          ? { model: selection.model }
          : {}),
        ...(selection.descriptor?.actions?.includes('change_effort') && selection.effort
          ? { effort: selection.effort }
          : {}),
      };
    if (!connection?.actions?.includes('send_turn') || connection.busy) return;
    const finishStart = beginConversation(connectionId, draft);
    conversationDrafts.value = { ...conversationDrafts.value, [connectionId]: '' };
    const composer = document.querySelector<HTMLTextAreaElement>('[name="conversation-draft"]');
    if (composer) composer.value = '';
    requestAnimationFrame(() => {
      syncConversationScroll(document, true);
    });
    try {
      const updated = await new Api(current.apiPath).sendToolTurn(
        connectionId,
        draft,
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
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      updateConversation(connectionId, (state) => ({
        ...state,
        activeAssistantId: undefined,
        progress: undefined,
        error: message,
        messages: state.messages.map((item) =>
          item.id === state.activeAssistantId
            ? { ...item, status: 'failed', content: item.content || 'The message could not be sent.' }
            : item,
        ),
      }));
    } finally {
      finishStart();
    }
  }
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
  return {
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
  };
}
