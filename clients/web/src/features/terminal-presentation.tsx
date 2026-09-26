import type { Signal } from 'kerfjs';

import {
  type ConversationMessage,
  type ConversationState,
  conversationUsage,
  EMPTY_CONVERSATION,
} from '../ai-conversation';
import { type ToolConnection } from '../api';
import { AIConversation } from '../components/ai-conversation';
import { type ProjectTabBarMode } from '../components/project-tab-bar';
import { AIConversationSurface } from '../components/reader-overlay-surfaces';
import { type MobileMagnifiedTerminal, type TerminalDashboardGroup } from '../components/terminal-dashboard';
import { type TerminalDrawerChatTab, type TerminalDrawerProps } from '../components/terminal-drawer';
import { type GlobalWorkspaceSurfaceProps } from '../components/workspace-composition-surfaces';
import type { Project } from '../interactions/types';
import type { MobileTerminalFocusState } from '../mobile-terminal-focus';
import { type DrawerAIChat } from '../project-drive';
import { TERMINAL_DASHBOARD_VISIBILITY_SCOPE } from '../terminal-visibility';
import type { createAiConfigurationController } from './ai-configuration';
import type { createPermissionsController } from './permissions';

export interface TerminalPresentationDependencies {
  projects: Signal<Project[]>;
  project: () => Project | undefined;
  shellMode: Signal<ProjectTabBarMode>;
  statsProjectId: Signal<string | undefined>;
  canGiveFeedback: () => boolean;
  terminals: {
    terminalGroups: Signal<TerminalDashboardGroup[]>;
    drawerTabOrder: (projectId: string) => string[];
    terminalDashboardSize: Signal<{ width: number; height: number }>;
    terminalFitAcross: Signal<number>;
    terminalFitHigh: Signal<number>;
    magnifiedTerminalKey: Signal<string | undefined>;
    terminalHiddenKeys: (scope: string) => string[];
    terminalDashboardLoading: Signal<boolean>;
    terminalDashboardMessage: Signal<string>;
    terminalContextMenu: Signal<{ key: string; x: number; y: number } | undefined>;
    terminalDrawerBounds: Signal<{ width: number; height: number }>;
    terminalDrawerFitAcross: Signal<number>;
    terminalDrawerFitHigh: Signal<number>;
    terminalDrawerSelected: Signal<string>;
    terminalDrawerMaximized: Signal<boolean>;
    terminalDrawerCreateMenuOpen: Signal<boolean>;
    mobileTerminalFocus: Signal<MobileTerminalFocusState>;
    /** Phone-only magnified-terminal chrome inputs; undefined on wider viewports (HS2-WMN626). */
    mobileMagnifiedTerminal: () => MobileMagnifiedTerminal | undefined;
  };
  conversations: {
    conversationStates: Signal<Record<string, ConversationState>>;
    driveConnectionsByProject: Signal<Record<string, ToolConnection[]>>;
    terminalDrawerChatsByProject: Signal<Record<string, DrawerAIChat[]>>;
    conversationDrafts: Signal<Record<string, string>>;
    conversationOpen: Signal<boolean>;
    conversationConnectionId: Signal<string | undefined>;
    conversationSelectedMessages: (
      connectionId: string,
      messages: readonly ConversationMessage[],
    ) => ConversationMessage[];
  };
  ai: Pick<
    ReturnType<typeof createAiConfigurationController>,
    'aiToolLabel' | 'conversationAiSelection' | 'aiToolOptions' | 'aiTools'
  >;
  permissions: Pick<ReturnType<typeof createPermissionsController>, 'pendingPermissions' | 'permissionPopupSurface'>;
}
/** Project terminal/chat surfaces from live feature owners during the root render. */
export function createTerminalPresentation(dependencies: TerminalPresentationDependencies) {
  const { projects, project, shellMode, statsProjectId, canGiveFeedback } = dependencies;
  const {
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
  } = dependencies.terminals;
  const {
    conversationStates,
    driveConnectionsByProject,
    terminalDrawerChatsByProject,
    conversationDrafts,
    conversationOpen,
    conversationConnectionId,
    conversationSelectedMessages,
  } = dependencies.conversations;
  const { aiToolLabel, conversationAiSelection, aiToolOptions, aiTools } = dependencies.ai;
  const { pendingPermissions, permissionPopupSurface } = dependencies.permissions;

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
          mobileMagnified: magnifiedTerminalKey.value ? mobileMagnifiedTerminal() : undefined,
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
    const focused = mobileTerminalFocus.value,
      focusMode = focused.active && focused.terminalId === terminalDrawerSelected.value;
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
      mobileMagnified: magnifiedTerminalKey.value ? mobileMagnifiedTerminal() : undefined,
      loading: terminalDashboardLoading.value,
      message: terminalDashboardMessage.value,
      maximized: terminalDrawerMaximized.value,
      createMenuOpen: terminalDrawerCreateMenuOpen.value,
      focusMode,
      focusViewport: focused.viewport,
      // The phone focus-mode text-size control uses the same mobile geometry + columns as the
      // magnified terminal; undefined off phones so the control never renders there (HS2-ZSFAHF).
      focusTextSize: focusMode ? mobileMagnifiedTerminal() : undefined,
      // The drawer grid's tiles carry the same More actions button as the dashboard (HS2-V2CCN6).
      contextMenu: terminalContextMenu.value,
    };
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
          feedbackAvailable: canGiveFeedback(),
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

  return { workspaceTerminalGroups, globalWorkspaceSurfaceProps, projectTerminalDrawerProps, aiConversationSurface };
}
