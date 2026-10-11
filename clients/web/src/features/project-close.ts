import type { Signal } from 'kerfjs';

import { conversationError, type ConversationState, conversationUsage, EMPTY_CONVERSATION } from '../ai-conversation';
import { Api, type ToolConnection } from '../api';
import {
  type ProjectCloseDialogState,
  type ProjectCloseResource,
  projectCloseResourceKey,
  selectedProjectCloseResource,
} from '../components/project-close-dialog';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import type { DrawerAIChat } from '../project-drive';
import { TERMINAL_DRAWER_RESIZE_END_EVENT } from '../terminal-viewport';

/** Live application bindings the project-close flow reads (HS2-3JGWTV). */
export interface ProjectCloseDependencies {
  projects: Signal<Project[]>;
  projectCloseDialog: Signal<ProjectCloseDialogState | undefined>;
  terminalGroups: Signal<TerminalDashboardGroup[]>;
  terminalDrawerChatsByProject: Signal<Record<string, DrawerAIChat[]>>;
  driveConnectionsByProject: Signal<Record<string, ToolConnection[]>>;
  conversationStates: Signal<Record<string, ConversationState>>;
  aiToolLabel: (tool: string) => string;
  /** Tear down the closed projects' runtime state (owned by the application runtime). */
  closeProjectIds: (ids: readonly string[]) => void;
}

/**
 * Owns the project close flow: the queue of projects awaiting confirmation, the live-resource
 * inventory each confirmation dialog presents, confirm / close-everything / cancel, and the
 * borrowed drawer terminal hand-back. Extracted from the application runtime without behavior
 * changes (HS2-3JGWTV).
 */
export function createProjectCloseController(dependencies: ProjectCloseDependencies) {
  const {
    projects,
    projectCloseDialog,
    terminalGroups,
    terminalDrawerChatsByProject,
    driveConnectionsByProject,
    conversationStates,
    aiToolLabel,
    closeProjectIds,
  } = dependencies;
  let pendingProjectCloseIds: string[] = [];
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
            error: conversationError(state, connection),
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
  async function closeAllProjectResources() {
    const state = projectCloseDialog.value,
      target = state && projects.value.find((item) => item.id === state.projectId);
    if (!state || !target || state.operation) return;
    projectCloseDialog.value = { ...state, operation: 'closing-all', error: '' };
    try {
      await Promise.all(
        state.resources.map((resource) =>
          resource.kind === 'terminal'
            ? new Api(target.apiPath).deleteTerminal(resource.id)
            : new Api(target.apiPath).deleteToolConnection(target.id, resource.id),
        ),
      );
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      if (projectCloseDialog.value?.projectId !== state.projectId) return;
      closeProjectIds([state.projectId]);
      pendingProjectCloseIds = pendingProjectCloseIds.filter((id) => id !== state.projectId);
      projectCloseDialog.value = undefined;
      queueMicrotask(presentNextProjectClose);
    } catch (reason) {
      if (projectCloseDialog.value?.projectId === state.projectId)
        projectCloseDialog.value = { ...state, error: reason instanceof Error ? reason.message : String(reason) };
    }
  }
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
  return {
    requestProjectClose,
    confirmProjectClose,
    closeAllProjectResources,
    restoreBorrowedProjectCloseTerminal,
    cancelProjectClose,
  };
}
