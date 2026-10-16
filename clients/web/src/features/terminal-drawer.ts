import type { Signal } from 'kerfjs';

import { type AiToolDefaults, Api, type CommandDefinition, type ToolConnection } from '../api';
import { browserRandomId } from '../browser-id';
import type { AppTabKind } from '../components/project-tab-context-menu';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import {
  drawerInputFocusRequestStillOwned,
  drawerTabFocusRequestStillOwned,
  drawerTabSelectionAfterClose,
  selectedDrawerInput,
} from '../drawer-tab-order';
import type { Project } from '../interactions/types';
import { automaticInputFocusAllowed } from '../mobile-layout';
import type { DrawerAIChat } from '../project-drive';
import type { TerminalFocusRequest } from '../terminal-viewport';
import { hideNewTerminalInNamedGroups, type parseTerminalVisibilityState } from '../terminal-visibility';

type TerminalVisibilityState = ReturnType<typeof parseTerminalVisibilityState>;
type ProjectActivation = { project: Project; generation: number; cached: boolean };

/** Live application bindings the terminal drawer owner reads and writes (HS2-BSSWGV). */
export interface TerminalDrawerDependencies {
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
  terminalGroups: Signal<TerminalDashboardGroup[]>;
  terminalDrawerVisible: Signal<boolean>;
  terminalDrawerMounted: Signal<boolean>;
  terminalDrawerTransitioning: Signal<boolean>;
  terminalDrawerSelected: Signal<string>;
  terminalDrawerChatsByProject: Signal<Record<string, DrawerAIChat[]>>;
  terminalDashboardLoading: Signal<boolean>;
  terminalDashboardMessage: Signal<string>;
  terminalVisibility: Signal<TerminalVisibilityState>;
  terminalContextMenu: Signal<{ key: string; x: number; y: number } | undefined>;
  appTabContextMenu: Signal<
    { x: number; y: number; kind: AppTabKind; id: string; direction: 'left' | 'right' } | undefined
  >;
  error: Signal<string>;
  project: () => Project | undefined;
  conversationConnectionId: Signal<string | undefined>;
  driveConnectionsByProject: Signal<Record<string, ToolConnection[]>>;
  drivePendingByProject: Signal<Record<string, boolean>>;
  aiToolLabel: (tool: string) => string;
  exitMobileTerminalFocus: () => void;
  settleTerminalDrawerGeometry: () => void;
  observeTerminalDrawer: () => void;
  disconnectTerminalDrawerSize: () => void;
  refreshTerminalDashboard: () => Promise<void>;
  terminalSession: (key: string) => { projectId: string; id: string } | undefined;
  currentDrawerTabIds: (projectId: string) => string[];
  persistDrawerTabOrder: (projectId: string, ids: readonly string[]) => void;
  persistTerminalVisibility: (next: TerminalVisibilityState) => void;
  saveTerminalName: (projectId: string, terminalId: string, name: string) => void;
  activateOpenProject: (projectId: string) => ProjectActivation | undefined;
  refreshActivatedProject: (activation: ProjectActivation, includeTerminals: boolean) => Promise<void>;
  refreshProject: (options: { showLoading: boolean }) => Promise<void>;
  refreshCommands: () => Promise<void>;
  setShellMode: (mode: 'project') => void;
}

/**
 * Owns the project terminal drawer's visibility, selection, and tab/input focus requests, the
 * serialized terminal / shell-command / AI-chat creation queue, and closing drawer tabs. The drawer
 * signals stay with the runtime, which renders them. Extracted from the application runtime without
 * behavior changes (HS2-BSSWGV).
 */
export function createTerminalDrawerController(dependencies: TerminalDrawerDependencies) {
  const {
    projects,
    selectedProjectId,
    terminalGroups,
    terminalDrawerVisible,
    terminalDrawerMounted,
    terminalDrawerTransitioning,
    terminalDrawerSelected,
    terminalDrawerChatsByProject,
    terminalDashboardLoading,
    terminalDashboardMessage,
    terminalVisibility,
    terminalContextMenu,
    appTabContextMenu,
    error,
    project,
    conversationConnectionId,
    driveConnectionsByProject,
    drivePendingByProject,
    aiToolLabel,
    exitMobileTerminalFocus,
    settleTerminalDrawerGeometry,
    observeTerminalDrawer,
    disconnectTerminalDrawerSize,
    refreshTerminalDashboard,
    terminalSession,
    currentDrawerTabIds,
    persistDrawerTabOrder,
    persistTerminalVisibility,
    saveTerminalName,
    activateOpenProject,
    refreshActivatedProject,
    refreshProject,
    refreshCommands,
    setShellMode,
  } = dependencies;
  let terminalCreateChain: Promise<unknown> = Promise.resolve();
  let terminalDrawerTransitionTimer: number | undefined;
  let pendingTerminalFocus: TerminalFocusRequest | undefined;
  let drawerInputFocusGeneration = 0;
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
    if (!automaticInputFocusAllowed(window.innerWidth)) return;
    const scheduled = document.activeElement,
      generation = ++drawerInputFocusGeneration;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const drawer = [...document.querySelectorAll<HTMLElement>('[data-component="terminal-drawer"]')].find(
            (item) => item.dataset.projectId === projectId,
          ),
          input = drawer ? selectedDrawerInput(drawer) : undefined;
        if (
          input &&
          drawerInputFocusRequestStillOwned(
            generation,
            drawerInputFocusGeneration,
            scheduled,
            document.activeElement,
            document.body,
          )
        )
          input.focus();
      }),
    );
  }
  function requestDrawerInputFocus(projectId: string, id: string, chat: DrawerAIChat | undefined) {
    if (id === 'grid') return;
    if (!chat) pendingTerminalFocus = { projectId, terminalId: id };
    focusDrawerInput(projectId);
  }
  function setTerminalDrawerVisible(visible: boolean, refresh = true) {
    if (!visible) exitMobileTerminalFocus();
    if (visible === terminalDrawerVisible.value) {
      if (visible) {
        terminalDrawerMounted.value = true;
        if (refresh) void refreshTerminalDashboard();
        observeTerminalDrawer();
        settleTerminalDrawerGeometry();
      }
      return;
    }
    if (terminalDrawerTransitionTimer !== undefined) window.clearTimeout(terminalDrawerTransitionTimer);
    if (visible) {
      const current = project(),
        chat = current
          ? // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
            terminalDrawerChatsByProject.value[current.id]?.find((item) => item.id === terminalDrawerSelected.value)
          : undefined;
      if (current) requestDrawerInputFocus(current.id, terminalDrawerSelected.value, chat);
      terminalDrawerMounted.value = true;
    }
    terminalDrawerTransitioning.value = true;
    terminalDrawerVisible.value = visible;
    localStorage.setItem('hotsheet.terminals.drawer-open', String(visible));
    terminalDrawerTransitionTimer = window.setTimeout(() => {
      terminalDrawerTransitionTimer = undefined;
      terminalDrawerTransitioning.value = false;
      if (!terminalDrawerVisible.value) terminalDrawerMounted.value = false;
    }, 220);
    if (visible) {
      if (refresh) void refreshTerminalDashboard();
      observeTerminalDrawer();
    } else disconnectTerminalDrawerSize();
  }
  function selectDrawerItem(id: string, focusInput = true) {
    if (id !== terminalDrawerSelected.value) exitMobileTerminalFocus();
    const current = project(),
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      chat = current ? terminalDrawerChatsByProject.value[current.id]?.find((item) => item.id === id) : undefined;
    if (chat) conversationConnectionId.value = chat.connectionId;
    terminalDrawerSelected.value = id;
    if (current) {
      localStorage.setItem(`hotsheet.project.${current.id}.terminal-drawer-selection`, id);
      if (focusInput) requestDrawerInputFocus(current.id, id, chat);
    }
  }
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
  function openGridAIChat(projectId: string, chatId: string) {
    const target = projects.value.find((item) => item.id === projectId),
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      chat = terminalDrawerChatsByProject.value[projectId]?.find((item) => item.id === chatId);
    if (!target || !chat) return;
    const activated = projectId === selectedProjectId.value ? undefined : activateOpenProject(projectId);
    setShellMode('project');
    selectDrawerItem(chat.id);
    setTerminalDrawerVisible(true);
    if (activated) void refreshActivatedProject(activated, false);
    else void Promise.all([refreshProject({ showLoading: false }), refreshCommands()]);
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
        ...(selection.permission_mode ? { permission_mode: selection.permission_mode } : {}),
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
        permissionMode: created.permission_mode ?? selection.permission_mode,
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
      // Closing from the tab bar keeps keyboard focus on the tabs, so repeated Delete keeps
      // closing; do not also schedule input focus into the newly selected item.
      drawerInputFocusGeneration += 1;
      selectDrawerItem(nextSelected, false);
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
  return {
    /** The terminal the next viewport mount focuses, consumed by the terminal viewport owner. */
    get pendingTerminalFocus() {
      return pendingTerminalFocus;
    },
    set pendingTerminalFocus(value: TerminalFocusRequest | undefined) {
      pendingTerminalFocus = value;
    },
    focusDrawerTab,
    focusDrawerInput,
    setTerminalDrawerVisible,
    selectDrawerItem,
    openTerminalInProject,
    openGridAIChat,
    queueTerminalCreate,
    createProjectTerminal,
    createShellCommandTerminal,
    createDrawerAIChat,
    closeDrawerTabIds,
    closeTerminalIds,
    closeDrawerAIChat,
  };
}
