import { batch, type Signal } from 'kerfjs';

import { type AiToolDefaults, Api, ApiHttpError, type CommandDefinition, type ToolConnection } from '../api';
import { browserRandomId } from '../browser-id';
import type { AppTabKind } from '../components/project-tab-context-menu';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import {
  drawerInputFocusRequestStillOwned,
  drawerTabFocusRequestStillOwned,
  drawerTabSelectionAfterClose,
  isPendingDrawerTabId,
  PENDING_DRAWER_TAB_PREFIX,
  type PendingDrawerTab,
  selectedDrawerInput,
} from '../drawer-tab-order';
import type { Project } from '../interactions/types';
import { automaticInputFocusAllowed } from '../mobile-layout';
import { type DrawerAIChat, projectConnections } from '../project-drive';
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
  /** Requested drawer items still being created, shown as pending tabs (HS2-AQ40G7). */
  terminalDrawerPendingByProject: Signal<Record<string, PendingDrawerTab[]>>;
  terminalDashboardLoading: Signal<boolean>;
  terminalDashboardMessage: Signal<string>;
  terminalVisibility: Signal<TerminalVisibilityState>;
  terminalContextMenu: Signal<{ key: string; x: number; y: number } | undefined>;
  appTabContextMenu: Signal<
    { x: number; y: number; kind: AppTabKind; id: string; direction: 'left' | 'right' } | undefined
  >;
  error: Signal<string>;
  showToast: (message: string) => void;
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
    terminalDrawerPendingByProject,
    terminalDashboardLoading,
    terminalDashboardMessage,
    showToast,
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
  let pendingTabSequence = 0;
  /**
   * Select a new pending tab at once, so the drawer shows a busy placeholder rather than falling back
   * to the Project grid (which would mount and resize every terminal) while the create resolves
   * (HS2-AQ40G7). Returns the tab and the selection to restore if the create fails.
   */
  function beginPendingTab(projectId: string, name: string, kind: PendingDrawerTab['kind']) {
    const tab: PendingDrawerTab = { id: `${PENDING_DRAWER_TAB_PREFIX}${++pendingTabSequence}`, name, kind },
      previous = terminalDrawerSelected.value;
    terminalDrawerPendingByProject.value = {
      ...terminalDrawerPendingByProject.value,
      [projectId]: [...(terminalDrawerPendingByProject.value[projectId] ?? []), tab],
    };
    if (previous !== tab.id) exitMobileTerminalFocus();
    // Not remembered: a reload never restores a placeholder.
    terminalDrawerSelected.value = tab.id;
    return { tab, previous };
  }
  function updatePendingTab(projectId: string, id: string, next: PendingDrawerTab | undefined) {
    const tabs = terminalDrawerPendingByProject.value[projectId] ?? [];
    if (!tabs.some((tab) => tab.id === id)) return;
    terminalDrawerPendingByProject.value = {
      ...terminalDrawerPendingByProject.value,
      [projectId]: next ? tabs.map((tab) => (tab.id === id ? next : tab)) : tabs.filter((tab) => tab.id !== id),
    };
  }
  /** Whether the person still has the pending tab selected, so its result may take the selection. */
  function pendingTabSelected(projectId: string, tab: PendingDrawerTab) {
    return project()?.id === projectId && terminalDrawerSelected.value === tab.id;
  }
  /**
   * A failed create removes its placeholder and returns to where the person was. The caller records
   * the reason where it persists; `toast` also announces it when that record may be out of view.
   */
  function failPendingTab(
    projectId: string,
    pending: { tab: PendingDrawerTab; previous: string },
    message: string,
    toast: boolean,
  ) {
    // Select first: removing a still-selected placeholder would leave the selection unknown (the grid).
    if (pendingTabSelected(projectId, pending.tab)) selectDrawerItem(pending.previous, false);
    updatePendingTab(projectId, pending.tab.id, undefined);
    if (toast) showToast(`${pending.tab.name} could not start: ${message}`);
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
                  ?.querySelector<HTMLElement>('.kui-tab-bar-item__select');
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
    if (id === 'grid' || isPendingDrawerTabId(id)) return;
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
      if (!isPendingDrawerTabId(id))
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
    const pending = beginPendingTab(current.id, selection ? aiToolLabel(selection.tool) : 'Terminal', 'terminal');
    await queueTerminalCreate(async () => {
      if (project()?.id !== current.id) {
        updatePendingTab(current.id, pending.tab.id, undefined);
        return;
      }
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
        // The placeholder stays under the created id until the refreshed snapshot carries the
        // terminal, then the live tab replaces it in place (HS2-AQ40G7).
        updatePendingTab(current.id, pending.tab.id, { ...pending.tab, resolvedId: created.id });
        const selected = pendingTabSelected(current.id, pending.tab);
        if (selected) {
          pendingTerminalFocus = { projectId: current.id, terminalId: created.id };
          selectDrawerItem(created.id);
        }
        await refreshTerminalDashboard();
        updatePendingTab(current.id, pending.tab.id, undefined);
        if (selected) focusDrawerTab(current.id, created.id);
      } catch (reason) {
        const message = reason instanceof Error ? reason.message : String(reason);
        pendingTerminalFocus = undefined;
        terminalDashboardMessage.value = message;
        terminalDashboardLoading.value = false;
        // The drawer message persists in the Project grid; the toast reaches a person on another tab.
        failPendingTab(current.id, pending, message, true);
      }
    });
  }
  /** Open a shell command's terminal; resolves to the new terminal's id once it exists (HS2-PWNPM2). */
  async function createShellCommandTerminal(command: CommandDefinition, current: Project) {
    if (!command.command) return undefined;
    let createdId: string | undefined;
    await queueTerminalCreate(async () => {
      if (project()?.id !== current.id) return;
      terminalDashboardLoading.value = true;
      terminalDashboardMessage.value = '';
      try {
        const created = await new Api(current.apiPath).createTerminal({
          shell_command: command.command,
          cwd: command.cwd || current.root,
        });
        createdId = created.id;
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
    return createdId;
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
    const name = options.drive ? `${aiToolLabel(selection.tool)} Drive` : `${aiToolLabel(selection.tool)} chat`,
      pending = beginPendingTab(current.id, name, 'ai-chat');
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
      if (project()?.id !== current.id) {
        updatePendingTab(current.id, pending.tab.id, undefined);
        return;
      }
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
        name,
        model: selection.model,
        effort: selection.effort,
        permissionMode: created.permission_mode ?? selection.permission_mode,
        drive: options.drive,
      };
      // The chat tab joins the drawer, takes the selection, and retires its placeholder in one
      // update, so no render sees the selection unknown (HS2-AQ40G7).
      batch(() => {
        terminalDrawerChatsByProject.value = {
          ...terminalDrawerChatsByProject.value,
          [current.id]: [...(terminalDrawerChatsByProject.value[current.id] ?? []), tab],
        };
        selectDrawerItem(tab.id);
        updatePendingTab(current.id, pending.tab.id, undefined);
      });
      persistTerminalVisibility(hideNewTerminalInNamedGroups(terminalVisibility.value, `${current.id}:${tab.id}`));
      setTerminalDrawerVisible(true);
      return tab;
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : String(reason);
      error.value = message;
      failPendingTab(current.id, pending, message, false);
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
    const previousSelected = terminalDrawerSelected.value,
      nextSelected = drawerTabSelectionAfterClose(order, previousSelected, closing),
      selectionChanges = nextSelected !== previousSelected;
    // Select the neighbor before the closed item leaves the drawer's snapshot, so the drawer goes
    // straight to it instead of showing the Project grid in between (HS2-AQ40G7).
    if (selectionChanges) {
      // Closing from the tab bar keeps keyboard focus on the tabs, so repeated Delete keeps
      // closing; do not also schedule input focus into the newly selected item.
      drawerInputFocusGeneration += 1;
      selectDrawerItem(nextSelected, false);
      pendingTerminalFocus = undefined;
      focusDrawerTab(current.id, nextSelected);
    }
    if (closingTerminals.length) {
      terminalDashboardLoading.value = true;
      try {
        await Promise.all(closingTerminals.map((id) => new Api(current.apiPath).deleteTerminal(id)));
        await refreshTerminalDashboard();
      } catch (reason) {
        terminalDashboardMessage.value = reason instanceof Error ? reason.message : String(reason);
        terminalDashboardLoading.value = false;
        // The tab stays open, so return to it unless the person has moved on meanwhile.
        if (selectionChanges && terminalDrawerSelected.value === nextSelected)
          selectDrawerItem(previousSelected, false);
        return;
      }
      // Removing the closed tab re-renders the strip; keep keyboard focus on the selected neighbor.
      if (selectionChanges && terminalDrawerSelected.value === nextSelected) focusDrawerTab(current.id, nextSelected);
    }
    const before = terminalDrawerChatsByProject.value[current.id] ?? [];
    terminalDrawerChatsByProject.value = {
      ...terminalDrawerChatsByProject.value,
      [current.id]: before.filter((item) => !requested.has(item.id)),
    };
    persistDrawerTabOrder(
      current.id,
      order.filter((id) => !requested.has(id)),
    );
  }
  async function closeTerminalIds(ids: readonly string[]) {
    await closeDrawerTabIds(ids);
  }
  function closeDrawerAIChat(id: string) {
    void closeDrawerTabIds([id]);
  }
  async function messageTerminalSession(id: string, text: string) {
    const current = project();
    if (
      !current ||
      !terminalGroups.value.find((group) => group.projectId === current.id)?.sessions.some((item) => item.id === id)
    ) {
      showToast('This terminal is no longer open in this project.');
      return;
    }
    try {
      const client = new Api(current.apiPath),
        connections = projectConnections(await client.activeToolConnections(), current);
      if (!connections.some((connection) => connection.id === id && connection.actions?.includes('message'))) {
        showToast('This AI session cannot receive messages right now.');
        return;
      }
      await client.messageToolConnection(id, text);
      showToast('Message delivered to the AI session.');
    } catch (reason) {
      showToast(terminalSessionMessageError(reason));
    }
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
    messageTerminalSession,
  };
}

export function terminalSessionMessageError(reason: unknown): string {
  if (reason instanceof ApiHttpError) {
    if (reason.status === 404) return 'This AI session is no longer connected. Reopen it and try again.';
    if (reason.status === 409) return 'This AI session has no message inbox. Restart the session and try again.';
    if (reason.status === 422) return 'This AI tool or session cannot receive messages yet.';
    if (reason.status === 502) return 'The AI session could not be reached. Check that it is still running.';
  }
  return reason instanceof Error ? reason.message : 'The message could not be delivered.';
}
