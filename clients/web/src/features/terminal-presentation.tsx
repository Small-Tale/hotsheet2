import type { Signal } from 'kerfjs';

import {
  conversationError,
  type ConversationMessage,
  type ConversationState,
  conversationTurnProgress,
  conversationTurnSummary,
  conversationUsage,
  EMPTY_CONVERSATION,
} from '../ai-conversation';
import { type ToolConnection } from '../api';
import { AIConversation, type AIConversationProps, AIConversationToolbar } from '../components/ai-conversation';
import type { ConfidenceCalibrationState } from '../components/confidence-calibration';
import { type ProjectTabBarMode } from '../components/project-tab-bar';
import { AIConversationSurface } from '../components/reader-overlay-surfaces';
import type { RepositorySizeState } from '../components/repository-size';
import { type MobileMagnifiedTerminal, type TerminalDashboardGroup } from '../components/terminal-dashboard';
import { type TerminalDrawerChatTab, type TerminalDrawerProps } from '../components/terminal-drawer';
import { type GlobalWorkspaceSurfaceProps } from '../components/workspace-composition-surfaces';
import type { Project } from '../interactions/types';
import type { MobileTerminalFocusState } from '../mobile-terminal-focus';
import { type DrawerAIChat } from '../project-drive';
import { type ServerClocks, serverClocks } from '../server-clock';
import { TERMINAL_DASHBOARD_VISIBILITY_SCOPE } from '../terminal-visibility';
import type { createAiConfigurationController } from './ai-configuration';
import type { createPermissionsController } from './permissions';

export interface TerminalPresentationDependencies {
  projects: Signal<Project[]>;
  project: () => Project | undefined;
  shellMode: Signal<ProjectTabBarMode>;
  statsProjectId: Signal<string | undefined>;
  /** Calibration report state per project for the stats view (HS2-Q1WCCY). */
  confidenceReportByProject: Signal<Record<string, ConfidenceCalibrationState>>;
  /** Repository size state per project for the stats view (HS2-QF44T7). */
  repositorySizeByProject: Signal<Record<string, RepositorySizeState>>;
  canGiveFeedback: () => boolean;
  terminals: {
    terminalGroups: Signal<TerminalDashboardGroup[]>;
    terminalGridFullscreen: Signal<boolean>;
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
    mobileTerminalFocus: Signal<MobileTerminalFocusState>;
    /** Phone-only magnified-terminal chrome inputs; undefined on wider viewports (HS2-WMN626). */
    mobileMagnifiedTerminal: () => MobileMagnifiedTerminal | undefined;
    /** The app's phone layout, which the drawer's dedicated terminals present for (HS2-DR549A). */
    viewportMobile: Signal<boolean>;
    /** Whether the primary pointer is coarse (touch-first device), for desktop-width Copy/Paste (HS2-5DHHPV). */
    coarsePointer: Signal<boolean>;
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
    'aiToolLabel' | 'conversationAiSelection' | 'aiToolOptions' | 'aiTools' | 'effectiveDriveSelection'
  >;
  permissions: Pick<ReturnType<typeof createPermissionsController>, 'pendingPermissions' | 'permissionPopupSurface'>;
  /** Server-clock estimate for remote clients' turn timing (HS2-GA5ASE); defaults to the shared one. */
  serverClocks?: Pick<ServerClocks, 'now'>;
}
/** Project terminal/chat surfaces from live feature owners during the root render. */
export function createTerminalPresentation(dependencies: TerminalPresentationDependencies) {
  const {
    projects,
    project,
    shellMode,
    statsProjectId,
    confidenceReportByProject,
    repositorySizeByProject,
    canGiveFeedback,
  } = dependencies;
  const {
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
  } = dependencies.terminals;
  // Phones already carry Copy/Paste in their own chrome; a touch-first desktop layout adds them (HS2-5DHHPV).
  const touchClipboard = () => coarsePointer.value && !viewportMobile.value;
  const {
    conversationStates,
    driveConnectionsByProject,
    terminalDrawerChatsByProject,
    conversationDrafts,
    conversationOpen,
    conversationConnectionId,
    conversationSelectedMessages,
  } = dependencies.conversations;
  const { aiToolLabel, conversationAiSelection, aiToolOptions, aiTools, effectiveDriveSelection } = dependencies.ai;
  const { pendingPermissions, permissionPopupSurface } = dependencies.permissions;
  const clocks = dependencies.serverClocks ?? serverClocks;

  /**
   * A chat's running-turn progress (HS2-XV1VCR) at the local render clock's `now`; an open
   * permission request means the turn waits on a person, not that it is stuck.
   */
  function chatTurnProgress(
    projectId: string,
    connectionId: string,
    state: ConversationState,
    connection: ToolConnection | undefined,
  ) {
    // The server's clock, not this device's, measures server-stamped turn times (HS2-GA5ASE).
    return conversationTurnProgress(state, connection, clocks.now(projectId), {
      waitingForPermission: pendingPermissions().some(
        (item) => item.projectId === projectId && item.connection === connectionId,
      ),
    });
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
            error: conversationError(state, connection),
            summary:
              conversationTurnSummary(chatTurnProgress(group.projectId, chat.connectionId, state, connection)) ??
              state.progress ??
              state.messages.at(-1)?.content ??
              state.activity?.at(-1)?.summary,
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
          fullscreen: terminalGridFullscreen.value,
          width: terminalDashboardSize.value.width,
          height: terminalDashboardSize.value.height,
          fitAcross: terminalFitAcross.value,
          fitHigh: terminalFitHigh.value,
          grouping: 'flow',
          magnifiedKey: magnifiedTerminalKey.value,
          mobileMagnified: magnifiedTerminalKey.value ? mobileMagnifiedTerminal() : undefined,
          touchClipboard: touchClipboard(),
          hiddenKeys: terminalHiddenKeys(TERMINAL_DASHBOARD_VISIBILITY_SCOPE),
          loading: terminalDashboardLoading.value,
          message: terminalDashboardMessage.value,
          contextMenu: terminalContextMenu.value,
        },
      };
    }
    const statsProject = projects.value.find((item) => item.id === statsProjectId.value);
    return {
      kind: 'stats',
      projectName: statsProject?.name,
      calibration: statsProject ? confidenceReportByProject.value[statsProject.id] : undefined,
      repositorySize: statsProject ? repositorySizeByProject.value[statsProject.id] : undefined,
    };
  }

  function projectTerminalDrawerProps(): TerminalDrawerProps | undefined {
    const current = project();
    if (!current) return;
    const conversations = conversationStates.peek(),
      group = terminalGroups.value.find((item) => item.projectId === current.id),
      messageable = new Set(
        (driveConnectionsByProject.value[current.id] ?? [])
          .filter((connection) => connection.actions?.includes('message'))
          .map((connection) => connection.id),
      ),
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
          ),
          turnProgress = chatTurnProgress(current.id, chat.connectionId, state, connection),
          conversation: AIConversationProps = {
            open: true,
            presentation: 'embedded',
            // The drawer content the chat fills sizes its floating form (HS2-SZZGC6).
            availableHeight: terminalDrawerBounds.value.height,
            tool,
            sessionId: connection?.session_id ?? chat.sourceSessionId,
            selectionId: chat.connectionId,
            selectedMessageIds,
            messages: state.messages,
            skippedBefore: state.skippedBefore,
            draft: conversationDrafts.value[chat.connectionId] ?? '',
            busy: connection?.busy ?? false,
            progress: state.progress,
            turnProgress,
            interruptible: Boolean(connection?.actions?.includes('interrupt')),
            permissions: pendingPermissions().filter(
              (item) => item.projectId === current.id && item.connection === chat.connectionId,
            ),
            activity: state.activity,
            totalUsage: conversationUsage(state),
            error: conversationError(state, connection),
            providerId: chat.tool,
            providers: aiToolOptions(),
            canChangeProvider: !chat.readOnly && aiTools.value.length > 1,
            model: selection.model,
            effort: selection.effort,
            models: descriptor?.models,
            efforts: selection.efforts,
            canChangeModel: descriptor?.actions?.includes('change_model'),
            canChangeEffort: descriptor?.actions?.includes('change_effort'),
            permissionMode: selection.permissionMode,
            permissionModes: selection.permissionModes,
            canChangePermissionMode: descriptor?.actions?.includes('change_permission_mode'),
            readOnly: chat.readOnly,
            savedSource: chat.savedSource,
          };
        return {
          id: chat.id,
          name: chat.name,
          tool,
          busy: connection?.busy ?? false,
          error: conversationError(state, connection),
          summary:
            conversationTurnSummary(turnProgress) ??
            state.progress ??
            state.messages.at(-1)?.content ??
            state.activity?.at(-1)?.summary,
          permissionMode: activePermissionMode(selection.permissionModes, selection.permissionMode),
          content: <AIConversation {...conversation} />,
          toolbar: <AIConversationToolbar {...conversation} />,
        };
      });
    const focused = mobileTerminalFocus.value,
      focusMode = focused.active && focused.terminalId === terminalDrawerSelected.value;
    return {
      projectId: current.id,
      projectName: current.name,
      sessions: (group?.sessions ?? []).map((session) => ({ ...session, canMessage: messageable.has(session.id) })),
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
      focusMode,
      focusViewport: focused.viewport,
      // The phone focus-mode text-size control uses the same mobile geometry + columns as the
      // magnified terminal; undefined off phones so the control never renders there (HS2-ZSFAHF).
      focusTextSize: focusMode ? mobileMagnifiedTerminal() : undefined,
      // The drawer grid's tiles carry the same More actions button as the dashboard (HS2-V2CCN6).
      contextMenu: terminalContextMenu.value,
      aiProviders: aiToolOptions().map((tool) => ({ id: tool.id, name: tool.label })),
      defaultAiProvider: effectiveDriveSelection(current.id).tool,
      mobile: viewportMobile.value,
      touchClipboard: touchClipboard(),
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
          skippedBefore: state.skippedBefore,
          draft: conversationDrafts.value[connectionId] ?? '',
          busy: connection.busy,
          progress: state.progress,
          turnProgress: chatTurnProgress(current.id, connectionId, state, connection),
          interruptible: Boolean(connection.actions?.includes('interrupt')),
          permissions: pendingPermissions().filter(
            (item) => item.projectId === current.id && item.connection === connectionId,
          ),
          activity: state.activity,
          totalUsage: conversationUsage(state),
          error: conversationError(state, connection),
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
          permissionMode: selection.permissionMode,
          permissionModes: selection.permissionModes,
          canChangePermissionMode: descriptor?.actions?.includes('change_permission_mode'),
        }}
      />
    );
  }

  return { workspaceTerminalGroups, globalWorkspaceSurfaceProps, projectTerminalDrawerProps, aiConversationSurface };
}

/** The chat tab's permission indicator: the active declared mode (HS2-NHRDMW). */
function activePermissionMode(
  modes: readonly { id: string; label: string; warning?: string }[],
  current: string | undefined,
): { label: string; warning?: string } | undefined {
  const mode = modes.find((item) => item.id === current);
  return mode ? { label: mode.label, ...(mode.warning ? { warning: mode.warning } : {}) } : undefined;
}
