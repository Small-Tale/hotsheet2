import type { Signal } from 'kerfjs';

import { Api, type TerminalInfo } from '../api';
import type { TerminalDashboardGroup } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import {
  createTerminalNameWriteQueue,
  reconcileLocalTerminalNames,
  restoreDefaultTerminalTitle,
  retitleTerminal,
  terminalNameKey,
  withoutTerminalName,
} from '../terminal-names';

/** Live application bindings the terminal-name owner reads (HS2-K7SYHQ). */
export interface TerminalNamesDependencies {
  projects: Signal<Project[]>;
  terminalNames: Signal<Record<string, string>>;
  terminalGroups: Signal<TerminalDashboardGroup[]>;
  showToast: (message: string) => void;
  terminalGroupLoaded: (projectId: string) => boolean;
  refreshTerminalDashboard: () => Promise<unknown>;
}

/**
 * Owns terminal tab names: the browser-local in-flight copy, serialized server writes, resets,
 * reconciliation after a refresh, and live `terminal_renamed` events (HS2-89FPV1, HS2-2Q7KTX);
 * extracted from the application runtime (HS2-K7SYHQ).
 */
export function createTerminalNamesController(dependencies: TerminalNamesDependencies) {
  const { projects, terminalNames, terminalGroups, showToast, terminalGroupLoaded, refreshTerminalDashboard } =
    dependencies;
  /** Project-scoped keys of renames whose server write is still in flight (HS2-89FPV1). */
  // Serializes each terminal's name writes so the last rename or reset always lands last (HS2-0E7Q6E).
  const pendingTerminalRenames = createTerminalNameWriteQueue();
  function persistLocalTerminalNames(names: Record<string, string>) {
    terminalNames.value = names;
    localStorage.setItem('hotsheet.terminals.names', JSON.stringify(names));
  }
  /**
   * Save a terminal's tab name on the project's server so it survives reloads and restores and
   * reaches every other client (HS2-89FPV1). The browser-local copy covers the request in flight
   * (and a reload during it) and is dropped once the server has the name; a failed write stays
   * local and is retried by the next terminal refresh.
   */
  function saveTerminalName(projectId: string, terminalId: string, name: string) {
    const trimmed = name.trim(),
      key = terminalNameKey(projectId, terminalId);
    if (!trimmed) return;
    persistLocalTerminalNames({ ...terminalNames.value, [key]: trimmed });
    terminalGroups.value = retitleTerminal(terminalGroups.value, projectId, terminalId, trimmed);
    uploadTerminalName(projectId, terminalId, trimmed);
  }
  function uploadTerminalName(projectId: string, terminalId: string, name: string) {
    const target = projects.value.find((item) => item.id === projectId),
      key = terminalNameKey(projectId, terminalId);
    if (!target) return;
    void pendingTerminalRenames.enqueue(key, () =>
      new Api(target.apiPath).renameTerminal(terminalId, name).then(
        () => {
          if (terminalNames.value[key] === name)
            persistLocalTerminalNames(withoutTerminalName(terminalNames.value, key));
        },
        (reason: unknown) => {
          showToast(
            `The terminal name could not be saved: ${reason instanceof Error ? reason.message : String(reason)}`,
          );
        },
      ),
    );
  }
  /**
   * Return a renamed terminal to its derived default name (HS2-2Q7KTX): retitle the tab at once,
   * forget any browser-local copy (an in-flight or legacy rename), and clear the server's saved
   * name so every client follows through the `terminal_renamed` event.
   */
  function resetTerminalName(projectId: string, terminalId: string) {
    const target = projects.value.find((item) => item.id === projectId),
      key = terminalNameKey(projectId, terminalId);
    if (!target) return;
    if (Object.hasOwn(terminalNames.value, key))
      persistLocalTerminalNames(withoutTerminalName(terminalNames.value, key));
    terminalGroups.value = restoreDefaultTerminalTitle(terminalGroups.value, projectId, terminalId);
    void pendingTerminalRenames.enqueue(key, () =>
      new Api(target.apiPath).renameTerminal(terminalId, null).catch((reason: unknown) => {
        showToast(`The terminal name could not be reset: ${reason instanceof Error ? reason.message : String(reason)}`);
        if (terminalGroupLoaded(projectId)) void refreshTerminalDashboard();
      }),
    );
  }
  /** Upload settled browser-local names the server lacks and drop ones it supersedes. */
  function reconcileTerminalNames(current: Project, sessions: readonly TerminalInfo[]) {
    const { upload, drop } = reconcileLocalTerminalNames(
      current.id,
      sessions,
      terminalNames.value,
      pendingTerminalRenames,
    );
    if (drop.length)
      persistLocalTerminalNames(drop.reduce((names, key) => withoutTerminalName(names, key), terminalNames.value));
    for (const item of upload) uploadTerminalName(current.id, item.terminalId, item.name);
  }
  /** Another client (or this one) renamed a terminal on the server: retitle the tab live. */
  function applyTerminalRenamed(current: Project, terminalId: string, name: string | undefined) {
    const key = terminalNameKey(current.id, terminalId);
    if (pendingTerminalRenames.has(key)) return;
    if (Object.hasOwn(terminalNames.value, key))
      persistLocalTerminalNames(withoutTerminalName(terminalNames.value, key));
    if (name) {
      terminalGroups.value = retitleTerminal(terminalGroups.value, current.id, terminalId, name);
      return;
    }
    // A cleared name returns the tab to the default this client already derived (HS2-2Q7KTX);
    // only a terminal it has not listed yet needs the list refetched.
    const known = terminalGroups.value
      .find((group) => group.projectId === current.id)
      ?.sessions.find((session) => session.id === terminalId);
    if (known?.defaultTitle)
      terminalGroups.value = restoreDefaultTerminalTitle(terminalGroups.value, current.id, terminalId);
    else if (terminalGroupLoaded(current.id)) void refreshTerminalDashboard();
  }
  return { saveTerminalName, resetTerminalName, reconcileTerminalNames, applyTerminalRenamed };
}
