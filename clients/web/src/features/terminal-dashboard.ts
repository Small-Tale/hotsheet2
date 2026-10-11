import type { Signal } from 'kerfjs';

import { Api } from '../api';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import { loadDrawerTabOrder, orderedDrawerTabIds, saveDrawerTabOrder } from '../drawer-tab-order';
import type { Project } from '../interactions/types';
import type { DrawerAIChat } from '../project-drive';
import { applyRememberedTabOrder } from '../tab-order';
import { deriveAiConnectionStates } from '../terminal-ai-connection';
import { defaultTerminalNames, terminalNameKey, terminalTitle } from '../terminal-names';
import { terminalProjectOwner } from '../terminal-project-scope';
import { sameTerminalDashboardSnapshot, TerminalSnapshotRefresh } from '../terminal-snapshot-refresh';
import {
  activeTerminalVisibilityGroup,
  type parseTerminalVisibilityState,
  TERMINAL_DASHBOARD_VISIBILITY_SCOPE,
  TERMINAL_VISIBILITY_STORAGE_KEY,
  terminalProjectVisibilityScope,
  terminalVisibilityItems,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import type { createDriveConversationsController } from './drive-conversations';
import type { createTerminalNamesController } from './terminal-names';

type TerminalVisibilityState = ReturnType<typeof parseTerminalVisibilityState>;

/** Live application bindings the terminal dashboard owner reads and writes (HS2-ZJ67VE). */
export interface TerminalDashboardDependencies {
  projects: Signal<Project[]>;
  selectedProjectId: () => string | undefined;
  terminalGroups: Signal<TerminalDashboardGroup[]>;
  terminalDashboardLoading: Signal<boolean>;
  terminalDashboardMessage: Signal<string>;
  terminalVisibility: Signal<TerminalVisibilityState>;
  terminalVisibilityDialogScope: () => string | undefined;
  terminalVisibilityFilter: () => readonly TerminalVisibilityType[];
  terminalDrawerOrderByProject: Signal<Record<string, string[]>>;
  terminalDrawerChatsByProject: () => Record<string, DrawerAIChat[]>;
  terminalNames: () => Record<string, string>;
  /** Whether a project's drive connections were already loaded (background projects load them here). */
  hasDriveConnections: (projectId: string) => boolean;
  refreshDriveConnections: ReturnType<typeof createDriveConversationsController>['refreshDriveConnections'];
  reconcileTerminalNames: ReturnType<typeof createTerminalNamesController>['reconcileTerminalNames'];
  /** The workspace's visible terminal groups; built after this owner, so read lazily. */
  workspaceTerminalGroups: () => Parameters<typeof terminalVisibilityItems>[0];
  aiToolLabel: (tool: string) => string;
  showToast: (message: string) => void;
  error: Signal<string>;
}

/**
 * Owns the terminal dashboard snapshot: per-project terminal fetches merged by version, AI
 * connection grace states, terminal visibility scopes and persistence, and the remembered
 * drawer tab order. Extracted from the application runtime without behavior changes
 * (HS2-ZJ67VE); `dispose()` cancels the pending AI-connection re-derive timer.
 */
export function createTerminalDashboardController(dependencies: TerminalDashboardDependencies) {
  const {
    projects,
    selectedProjectId,
    terminalGroups,
    terminalDashboardLoading,
    terminalDashboardMessage,
    terminalVisibility,
    terminalVisibilityDialogScope,
    terminalVisibilityFilter,
    terminalDrawerOrderByProject,
    terminalDrawerChatsByProject,
    terminalNames,
    hasDriveConnections,
    refreshDriveConnections,
    reconcileTerminalNames,
    workspaceTerminalGroups,
    aiToolLabel,
    showToast,
    error,
  } = dependencies;
  let terminalDashboardGeneration = 0;
  const terminalSnapshotRefresh = new TerminalSnapshotRefresh();
  function terminalSession(key?: string) {
    return terminalGroups.value
      .flatMap((group) => group.sessions)
      .find((session) => `${session.projectId}:${session.id}` === key);
  }
  function terminalHiddenKeys(scope: string) {
    return activeTerminalVisibilityGroup(terminalVisibility.value, scope).hiddenKeys;
  }
  function terminalHiddenCount(scope: string, projectId?: string) {
    const live = new Set(
      terminalVisibilityItems(
        workspaceTerminalGroups(),
        projectId ? terminalProjectVisibilityScope(projectId) : scope,
      ).flatMap((group) => group.items.map((item) => item.key)),
    );
    return terminalHiddenKeys(scope).filter((key) => live.has(key)).length;
  }
  function persistTerminalVisibility(next: TerminalVisibilityState) {
    terminalVisibility.value = next;
    localStorage.setItem(TERMINAL_VISIBILITY_STORAGE_KEY, JSON.stringify(next));
  }
  function terminalVisibilityScopeFor(target: Element) {
    return (
      target.closest<HTMLElement>('[data-visibility-scope]')?.dataset.visibilityScope ??
      TERMINAL_DASHBOARD_VISIBILITY_SCOPE
    );
  }
  function terminalKeysForVisibilityDialog() {
    return terminalVisibilityItems(
      workspaceTerminalGroups(),
      terminalVisibilityDialogScope() ?? TERMINAL_DASHBOARD_VISIBILITY_SCOPE,
      terminalVisibilityFilter(),
    ).flatMap((group) => group.items.map((item) => item.key));
  }
  function drawerTabOrder(projectId: string) {
    return terminalDrawerOrderByProject.value[projectId] ?? loadDrawerTabOrder(localStorage, projectId);
  }
  function currentDrawerTabIds(projectId: string) {
    const terminalIds =
        terminalGroups.value.find((group) => group.projectId === projectId)?.sessions.map((session) => session.id) ??
        [],
      chatIds = (terminalDrawerChatsByProject()[projectId] ?? []).map((chat) => chat.id);
    return orderedDrawerTabIds(terminalIds, chatIds, drawerTabOrder(projectId));
  }
  function persistDrawerTabOrder(projectId: string, ids: readonly string[]) {
    const order = saveDrawerTabOrder(localStorage, projectId, ids);
    terminalDrawerOrderByProject.value = { ...terminalDrawerOrderByProject.value, [projectId]: order };
    terminalGroups.value = terminalGroups.value.map((group) =>
      group.projectId === projectId
        ? { ...group, sessions: applyRememberedTabOrder(group.sessions, (item) => item.id, order) }
        : group,
    );
  }
  // When each terminal first appeared, for the AI-connection grace period (HS2-EV1XK3).
  const terminalFirstSeen = new Map<string, number>();
  let aiConnectionTimer: ReturnType<typeof setTimeout> | undefined;
  /** Mark each terminal connected to Hot Sheet or not; re-derives locally (no request) when a grace ends. */
  function applyAiConnectionStates(snapshot = terminalGroups.peek()) {
    clearTimeout(aiConnectionTimer);
    aiConnectionTimer = undefined;
    const { groups, nextCheckInMs } = deriveAiConnectionStates(snapshot, terminalFirstSeen, Date.now());
    if (!sameTerminalDashboardSnapshot(terminalGroups.peek(), groups)) terminalGroups.value = groups;
    if (nextCheckInMs !== undefined) aiConnectionTimer = setTimeout(applyAiConnectionStates, nextCheckInMs);
  }
  async function refreshTerminalDashboard(quiet = false, targetProject?: Project) {
    const generation = quiet ? terminalDashboardGeneration : ++terminalDashboardGeneration,
      openProjects = [...projects.value],
      fetchProjects = targetProject ? openProjects.filter((project) => project.id === targetProject.id) : openProjects,
      versions = fetchProjects.map((project) => terminalSnapshotRefresh.begin(project.id));
    if (!quiet) {
      terminalDashboardLoading.value = true;
      terminalDashboardMessage.value = '';
    }
    const results: Array<TerminalDashboardGroup | undefined> = await Promise.all(
      fetchProjects.map(async (current) => {
        try {
          const [infos] = await Promise.all([
              new Api(current.apiPath, '', { trackBusy: !quiet }).terminals(),
              ...(current.id !== selectedProjectId() && !hasDriveConnections(current.id)
                ? [refreshDriveConnections(current, true, quiet)]
                : []),
            ]),
            owned = infos.filter((session) => terminalProjectOwner(openProjects, session.cwd) === current.id),
            defaultNames = defaultTerminalNames(owned, aiToolLabel),
            sessions = owned.map((session, index) => {
              const localName = terminalNames()[terminalNameKey(current.id, session.id)];
              return {
                ...session,
                scrollback: '',
                projectId: current.id,
                projectName: current.name,
                title: terminalTitle(localName, session.name, defaultNames[index]),
                defaultTitle: defaultNames[index],
                named: Boolean(localName || session.name),
              };
            });
          reconcileTerminalNames(current, owned);
          return {
            projectId: current.id,
            projectName: current.name,
            sessions: applyRememberedTabOrder(sessions, (item) => item.id, drawerTabOrder(current.id)),
          } satisfies TerminalDashboardGroup;
        } catch {
          return undefined;
        }
      }),
    );
    if (!quiet && generation !== terminalDashboardGeneration) return;
    // A failed fetch is not a resumed session. Keep the last snapshot for still-open projects.
    const merged = terminalSnapshotRefresh.merge(
      terminalGroups.peek(),
      results.map((group, index) => ({ projectId: fetchProjects[index].id, version: versions[index], group })),
      projects.value.map((project) => project.id),
    );
    applyAiConnectionStates(merged);
    if (!quiet) {
      terminalDashboardMessage.value =
        openProjects.length > 0 && terminalGroups.value.length === 0 ? 'Terminal snapshots could not be loaded.' : '';
      terminalDashboardLoading.value = false;
    } else if (terminalGroups.peek().length > 0) terminalDashboardMessage.value = '';
  }
  async function clearTerminalHalt(key: string) {
    const session = terminalSession(key),
      current = projects.value.find((item) => item.id === session?.projectId);
    if (!session?.halt || !current) return;
    try {
      await new Api(current.apiPath).clearTerminalHalt(session.id, session.halt.at);
      await refreshTerminalDashboard(true, current);
      if (!terminalSession(key)?.halt) showToast('Stopped state cleared.');
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  function dispose() {
    clearTimeout(aiConnectionTimer);
    aiConnectionTimer = undefined;
  }
  return {
    terminalSession,
    terminalHiddenKeys,
    terminalHiddenCount,
    persistTerminalVisibility,
    terminalVisibilityScopeFor,
    terminalKeysForVisibilityDialog,
    drawerTabOrder,
    currentDrawerTabIds,
    persistDrawerTabOrder,
    applyAiConnectionStates,
    refreshTerminalDashboard,
    clearTerminalHalt,
    dispose,
  };
}
