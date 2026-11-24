import { batch, effect, type Signal } from 'kerfjs';

import { type AiToolDefaults, Api, ApiHttpError, type CommandDefinition, type ToolConnection } from '../api';
import { browserRandomId } from '../browser-id';
import type { AppTabMenuState } from '../components/project-tab-context-menu';
import type { TerminalDashboardGroup, TerminalDashboardSession } from '../components/terminal-dashboard';
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
import {
  answeredHandBacks,
  handBackKey,
  handBackOperationId,
  type PendingHandBack,
  recoveredHandBacks,
} from '../orchestration-hand-back';
import type { PendingOrchestratorAsk } from '../orchestration-validation-steps';
import { type DrawerAIChat, projectConnections } from '../project-drive';
import type { TerminalFocusRequest } from '../terminal-viewport';
import { hideNewTerminalInNamedGroups, type parseTerminalVisibilityState } from '../terminal-visibility';

type TerminalVisibilityState = ReturnType<typeof parseTerminalVisibilityState>;
type ProjectActivation = { project: Project; generation: number; cached: boolean };
export const DRAWER_CREATE_TIMEOUT_MS = 15_000;
export const DRAWER_APPEAR_TIMEOUT_MS = 10_000;

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
  appTabContextMenu: Signal<AppTabMenuState | undefined>;
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
  /** HS2-KR28FS, HS2-BPTQ5X: each hand-back still waiting, by hosted checkout and operation. */
  const pendingHandBacks = new Map<string, PendingHandBack>();
  /** HS2-BPTQ5X: the permission requests of waiting hand-backs seen pending. */
  const seenHandBackRequests = new Set<string>();
  /** HS2-BPTQ5X: the pending orchestrator requests as last seen. */
  let lastAsks: readonly PendingOrchestratorAsk[] = [];
  let drawerInputFocusGeneration = 0;
  let pendingTabSequence = 0;
  const pendingRetries = new Map<string, () => void>();
  const pendingPrevious = new Map<string, string>();
  async function withCreateDeadline<T>(name: string, current: Project, run: (signal: AbortSignal) => Promise<T>) {
    const controller = new AbortController();
    let timer: number | undefined;
    const deadline = new Promise<never>((_resolve, reject) => {
      timer = window.setTimeout(() => {
        reject(
          new Error(`${name} did not start within 15 seconds on ${current.name}. Check the server and try again.`),
        );
        controller.abort();
      }, DRAWER_CREATE_TIMEOUT_MS);
    });
    try {
      return await Promise.race([run(controller.signal), deadline]);
    } finally {
      if (timer !== undefined) window.clearTimeout(timer);
    }
  }
  function terminalAppeared(projectId: string, terminalId: string): Promise<void> {
    if (
      terminalGroups.value.some(
        (group) => group.projectId === projectId && group.sessions.some((s) => s.id === terminalId),
      )
    )
      return Promise.resolve();
    return new Promise((resolve, reject) => {
      let stop: () => void = () => undefined;
      let settled = false;
      const settle = (present: boolean) => {
        if (settled) return;
        settled = true;
        stop();
        window.clearTimeout(timer);
        if (present) resolve();
        else
          reject(new Error('The server accepted the session, but it did not appear in the terminal list. Try again.'));
      };
      const timer = window.setTimeout(() => {
        settle(false);
      }, DRAWER_APPEAR_TIMEOUT_MS);
      stop = effect(() => {
        if (
          terminalGroups.value.some(
            (group) => group.projectId === projectId && group.sessions.some((s) => s.id === terminalId),
          )
        )
          settle(true);
      });
      queueMicrotask(() => {
        if (settled) stop();
      });
    });
  }
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
    pendingPrevious.set(tab.id, previous);
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
  /** Keep the failure where it happened, with explicit recovery even after a request times out. */
  function failPendingTab(
    projectId: string,
    pending: { tab: PendingDrawerTab; previous: string },
    message: string,
    toast: boolean,
  ) {
    const current = (terminalDrawerPendingByProject.value[projectId] ?? []).find((tab) => tab.id === pending.tab.id);
    if (!current) return;
    const visibleHere =
      project()?.id === projectId &&
      (terminalDrawerSelected.value === current.id || terminalDrawerSelected.value === current.resolvedId);
    if (terminalDrawerSelected.value === current.resolvedId) selectDrawerItem(current.id, false);
    const projectName = projects.value.find((item) => item.id === projectId)?.name ?? projectId;
    updatePendingTab(projectId, pending.tab.id, {
      ...current,
      resolvedId: undefined,
      failure: message.includes(projectName) ? message : `${projectName}: ${message}`,
    });
    if (toast && !visibleHere) showToast(`${pending.tab.name} could not start: ${message}`);
  }
  function closeFailedDrawerTab(id: string) {
    const current = project();
    if (!current) return;
    const tab = (terminalDrawerPendingByProject.value[current.id] ?? []).find((item) => item.id === id);
    if (!tab?.failure) return;
    if (terminalDrawerSelected.value === id) selectDrawerItem(pendingPrevious.get(id) ?? 'grid', false);
    updatePendingTab(current.id, id, undefined);
    pendingRetries.delete(id);
    pendingPrevious.delete(id);
  }
  function retryFailedDrawerTab(id: string) {
    const retry = pendingRetries.get(id);
    if (!retry) return;
    const previous = pendingPrevious.get(id) ?? 'grid';
    retry(); // The new pending tab is selected before the failed one is removed.
    const newId = terminalDrawerSelected.value;
    if (newId !== id && isPendingDrawerTabId(newId)) pendingPrevious.set(newId, previous);
    closeFailedDrawerTab(id);
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
  async function createProjectTerminal(selection?: AiToolDefaults, requestedId = browserRandomId()) {
    const current = project();
    if (!current) return;
    const pending = beginPendingTab(current.id, selection ? aiToolLabel(selection.tool) : 'Terminal', 'terminal');
    pendingRetries.set(pending.tab.id, () => void createProjectTerminal(selection, requestedId));
    await queueTerminalCreate(async () => {
      if (project()?.id !== current.id) {
        updatePendingTab(current.id, pending.tab.id, undefined);
        pendingRetries.delete(pending.tab.id);
        pendingPrevious.delete(pending.tab.id);
        return;
      }
      terminalDashboardLoading.value = true;
      terminalDashboardMessage.value = '';
      try {
        const created = await withCreateDeadline(pending.tab.name, current, (signal) =>
          new Api(current.apiPath).createTerminal(
            {
              id: requestedId,
              cwd: current.root,
              ...(selection ? { connect: selection.tool, model: selection.model, effort: selection.effort } : {}),
            },
            signal,
          ),
        );
        if (project()?.id !== current.id) {
          updatePendingTab(current.id, pending.tab.id, undefined);
          pendingRetries.delete(pending.tab.id);
          pendingPrevious.delete(pending.tab.id);
          return;
        }
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
        await terminalAppeared(current.id, created.id);
        updatePendingTab(current.id, pending.tab.id, undefined);
        pendingRetries.delete(pending.tab.id);
        pendingPrevious.delete(pending.tab.id);
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
        const created = await withCreateDeadline(command.title, current, (signal) =>
          new Api(current.apiPath).createTerminal(
            {
              shell_command: command.command,
              cwd: command.cwd || current.root,
            },
            signal,
          ),
        );
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
    pendingRetries.set(pending.tab.id, () => void createDrawerAIChat(selection, { ...options, connectionId }));
    drivePendingByProject.value = { ...drivePendingByProject.value, [current.id]: true };
    try {
      const created = await withCreateDeadline(name, current, (signal) =>
        new Api(current.apiPath).createToolConnection(
          {
            tool: selection.tool,
            checkout: current.id,
            connection_id: connectionId,
            model: selection.model,
            effort: selection.effort,
            ...(selection.permission_mode ? { permission_mode: selection.permission_mode } : {}),
          },
          signal,
        ),
      );
      if (project()?.id !== current.id) {
        updatePendingTab(current.id, pending.tab.id, undefined);
        pendingRetries.delete(pending.tab.id);
        pendingPrevious.delete(pending.tab.id);
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
        pendingRetries.delete(pending.tab.id);
        pendingPrevious.delete(pending.tab.id);
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
  /**
   * HS2-HVKS7M: closing the tab of a session an orchestrator started here is a person's stop. It
   * goes through the hosted stop with `force`, which ends the terminal and records `person_stopped`,
   * so the orchestrator never reads it as a crash (`host_lost`). Any other terminal, or a hosted
   * stop that found nothing left to stop, is closed the ordinary way.
   */
  async function closeTerminal(current: Project, session: TerminalDashboardSession | undefined, id: string) {
    const client = new Api(current.apiPath),
      orchestrator = session?.orchestrator;
    if (orchestrator) {
      const reply = await client.stopHostedSession(orchestrator.checkout_id, orchestrator.operation_id, true);
      if (reply.stopped) return;
    }
    await client.deleteTerminal(id);
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
        await Promise.all(
          closingTerminals.map((id) =>
            closeTerminal(
              current,
              group.sessions.find((item) => item.id === id),
              id,
            ),
          ),
        );
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
  /**
   * HS2-6MNMVB: a person at this machine takes over (and optionally pauses) the orchestrator
   * session in one of this project's terminals, through the hosted target-sessions route. From
   * then on the orchestrator leaves it alone. The drawer reads the new owner from the server.
   */
  async function takeOverHostedSession(terminalId: string, pause: boolean) {
    const current = project(),
      orchestrator = terminalGroups.value
        .find((group) => group.projectId === current?.id)
        ?.sessions.find((item) => item.id === terminalId)?.orchestrator;
    if (!current || orchestrator?.owner !== 'orchestrator') return;
    try {
      await new Api(current.apiPath).takeOverHostedSession(orchestrator.checkout_id, orchestrator.operation_id, pause);
      showToast(
        pause ? 'Taken over and paused. The orchestrator leaves it alone.' : 'Taken over from the orchestrator.',
      );
    } catch (reason) {
      showToast(`Could not take over the session. ${reason instanceof Error ? reason.message : String(reason)}`);
    }
    await refreshTerminalDashboard();
  }
  /**
   * HS2-KR28FS: a person hands a session they took over or paused back to its orchestrator. It needs
   * the permission a session start needs: the first try files that request. HS2-BPTQ5X: the drawer then
   * sends it again by itself under the same operation id once the request is answered
   * (`continueHandBacks`), so an Allow finishes it and a Deny is reported. A completed hand-back or a
   * denial drops the id, so the next try asks again. The tool must be idle between turns.
   */
  async function handBackHostedSession(terminalId: string) {
    const current = project(),
      orchestrator = terminalGroups.value
        .find((group) => group.projectId === current?.id)
        ?.sessions.find((item) => item.id === terminalId)?.orchestrator;
    if (!current || orchestrator?.owner !== 'human') return;
    const key = handBackKey(orchestrator.checkout_id, orchestrator.operation_id),
      entry = pendingHandBacks.get(key) ?? {
        checkout_id: orchestrator.checkout_id,
        operation_id: orchestrator.operation_id,
        handBackOperationId: handBackOperationId(
          orchestrator.checkout_id,
          orchestrator.operation_id,
          browserRandomId(),
        ),
      };
    pendingHandBacks.set(key, entry);
    await sendHandBack(current, entry);
  }
  async function sendHandBack(current: Project, entry: PendingHandBack) {
    const key = handBackKey(entry.checkout_id, entry.operation_id);
    try {
      const reply = await new Api(current.apiPath).handBackHostedSession(
        entry.checkout_id,
        entry.operation_id,
        entry.handBackOperationId,
      );
      if (reply.refused === undefined) {
        pendingHandBacks.delete(key);
        showToast('Handed back to the orchestrator.');
      } else {
        // Not a permission question: the next try may ask again under the same id.
        pendingHandBacks.set(key, { ...entry, requestId: undefined });
        showToast(handBackRefusal(reply.refused));
      }
    } catch (reason) {
      if (reason instanceof ApiHttpError && reason.code === 'permission_required') {
        pendingHandBacks.set(key, { ...entry, requestId: reason.requestId });
        // The inbox may already show the request this answer names: it is waiting, so its
        // disappearance is the answer.
        if (reason.requestId !== undefined && lastAsks.some((ask) => ask.request_id === reason.requestId))
          seenHandBackRequests.add(reason.requestId);
        showToast('Handing back needs permission to start a session. It finishes once the request is allowed.');
      } else {
        pendingHandBacks.delete(key);
        showToast(handBackError(reason));
      }
    }
    await refreshTerminalDashboard();
  }
  /**
   * HS2-BPTQ5X: run on every change of the pending orchestrator permission requests (and of the drawer's
   * sessions). A hand-back whose request was seen pending and is pending no longer was answered, and is
   * sent again under the same operation id. After a reload, a pending request for one of this project's
   * person-owned sessions is recovered by its digest, so the answer still finishes it once.
   */
  function continueHandBacks(asks: readonly PendingOrchestratorAsk[]) {
    lastAsks = asks;
    const current = project();
    if (!current) return;
    const sessions =
      terminalGroups
        .peek()
        .find((group) => group.projectId === current.id)
        ?.sessions.flatMap((item) => (item.orchestrator?.owner === 'human' ? [item.orchestrator] : [])) ?? [];
    for (const recovered of recoveredHandBacks(asks, sessions)) {
      const key = handBackKey(recovered.checkout_id, recovered.operation_id);
      if (!pendingHandBacks.has(key)) pendingHandBacks.set(key, recovered);
    }
    const shown = new Set(asks.map((ask) => ask.request_id));
    for (const entry of answeredHandBacks([...pendingHandBacks.values()], shown, seenHandBackRequests))
      void sendHandBack(current, { ...entry, requestId: undefined });
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
    closeFailedDrawerTab,
    retryFailedDrawerTab,
    closeTerminalIds,
    closeDrawerAIChat,
    messageTerminalSession,
    takeOverHostedSession,
    handBackHostedSession,
    continueHandBacks,
  };
}

/** HS2-KR28FS: what a refused hand-back tells the person. */
export function handBackRefusal(code: string): string {
  switch (code) {
    case 'session_mid_turn':
      return 'The session is in the middle of a turn. Hand it back when it is idle.';
    case 'session_not_safe':
      return 'The host could not tell whether the session is idle. Try again in a moment.';
    case 'session_owner_remote':
      return 'Another machine orchestrates this session. Hand it back from there.';
    case 'session_not_active':
    case 'session_not_found':
      return 'The session is no longer running.';
    default:
      return `Could not hand the session back (${code}).`;
  }
}

/** HS2-KR28FS: a failed hand-back request. A denial leaves the session the person's. */
export function handBackError(reason: unknown): string {
  if (reason instanceof ApiHttpError) {
    if (reason.code === 'permission_denied')
      return 'Permission to start a session was denied. The session stays yours.';
    if (reason.code === 'policy_denied') return "The orchestration policy doesn't allow this session. It stays yours.";
    if (reason.code !== undefined && reason.code.startsWith('session_')) return handBackRefusal(reason.code);
  }
  return `Could not hand the session back. ${reason instanceof Error ? reason.message : String(reason)}`;
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
