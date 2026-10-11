import type { Signal } from 'kerfjs';

import { Api, type CommandDefinition, type CommandRun } from '../api';
import type { ProjectRestoreFailure } from '../components/project-restore-error';
import type { Project } from '../interactions/types';
import { disposeTicketReaderFrames, type TicketReaderFrame } from '../ticket-reader-stack';

/**
 * Pure helpers behind closing project tabs (HS2-DCTS07): dropping every per-project record for the
 * closing ids and choosing which remaining tab takes the selection. The orchestration (closing server
 * sessions, activating the next project, refreshing) is `createProjectTeardown` below (HS2-BSSWGV).
 */

/** Replace each per-project record with a copy that omits the closing project ids, in order. */
export function pruneProjectRecords(
  records: ReadonlyArray<Signal<Record<string, unknown>>>,
  closing: ReadonlySet<string>,
): void {
  for (const record of records)
    record.value = Object.fromEntries(Object.entries(record.value).filter(([id]) => !closing.has(id)));
}

/**
 * The project tab that takes the selection after the selected one closes: the nearest surviving tab
 * to its right, then the nearest to its left, else the first remaining tab, else none (`''`).
 */
export function nextProjectAfterClose<T extends { id: string }>(
  before: readonly T[],
  remaining: readonly T[],
  selectedIndex: number,
): string {
  return (
    remaining.find((item) => before.indexOf(item) > selectedIndex)?.id ??
    [...remaining].reverse().find((item) => before.indexOf(item) < selectedIndex)?.id ??
    remaining.at(0)?.id ??
    ''
  );
}

type ProjectActivation = { project: Project; generation: number; cached: boolean };
interface Cancelable {
  cancel(): void;
}

/** Live application bindings the project teardown orchestration reads and writes (HS2-BSSWGV). */
export interface ProjectTeardownDependencies {
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
  statsProjectId: Signal<string | undefined>;
  projectRestoreFailures: Signal<ProjectRestoreFailure[]>;
  selectedProjectRestoreRoot: Signal<string>;
  terminalDrawerVisible: Signal<boolean>;
  commandDialogId: Signal<string | undefined>;
  commandSettingsEditingId: Signal<string | undefined>;
  commandDefinitions: Signal<CommandDefinition[]>;
  commandRuns: Signal<CommandRun[]>;
  readerOpen: Signal<boolean>;
  linkedReaderStack: Signal<TicketReaderFrame[]>;
  /** Per-project records dropped as soon as their projects leave the tab list. */
  projectRecords: ReadonlyArray<Signal<Record<string, unknown>>>;
  /** Per-project source and tool-connection records, dropped after the next tab is chosen. */
  connectionRecords: ReadonlyArray<Signal<Record<string, unknown>>>;
  histories: Map<string, unknown>;
  linkedReaderAutosaves: Map<string, { details: Cancelable; note: Cancelable; blocked: Cancelable }>;
  readerReturnFocus: Map<string, unknown>;
  readerClosing: Set<string>;
  readerApprovedClose: Set<string>;
  readerDetailsAutosave: Cancelable;
  readerNoteAutosave: Cancelable;
  readerBlockedReasonAutosave: Cancelable;
  projectTabRefresh: { cancel(projectId: string): void };
  pendingCreatedTickets: { forgetProject(projectId: string): void };
  projectsPendingActivation: Set<string>;
  warmProjects: { forget(projectId: string): void };
  forgetAiConfiguration: (projectId: string) => void;
  project: () => Project | undefined;
  resetTicketComposer: () => void;
  activateOpenProject: (projectId: string) => ProjectActivation | undefined;
  refreshActivatedProject: (activation: ProjectActivation, includeTerminals: boolean) => Promise<void>;
  currentRememberedProjectRoots: () => string[];
  syncProjectChangeStreams: () => void;
  refreshProject: () => Promise<void>;
  refreshCommands: () => Promise<void>;
  refreshCustomViews: () => Promise<void>;
  refreshDriveConnections: () => Promise<void>;
}

/**
 * Closing project tabs (HS2-BSSWGV): disposing their ticket readers and server sessions, dropping
 * every per-project record, activating the next tab, and refreshing what remains. Extracted from the
 * application runtime without behavior changes.
 */
export function createProjectTeardown(dependencies: ProjectTeardownDependencies) {
  const {
    projects,
    selectedProjectId,
    statsProjectId,
    projectRestoreFailures,
    selectedProjectRestoreRoot,
    terminalDrawerVisible,
    commandDialogId,
    commandSettingsEditingId,
    commandDefinitions,
    commandRuns,
    readerOpen,
    linkedReaderStack,
    projectRecords,
    connectionRecords,
    histories,
    linkedReaderAutosaves,
    readerReturnFocus,
    readerClosing,
    readerApprovedClose,
    readerDetailsAutosave,
    readerNoteAutosave,
    readerBlockedReasonAutosave,
    projectTabRefresh,
    pendingCreatedTickets,
    projectsPendingActivation,
    warmProjects,
    forgetAiConfiguration,
    project,
    resetTicketComposer,
    activateOpenProject,
    refreshActivatedProject,
    currentRememberedProjectRoots,
    syncProjectChangeStreams,
    refreshProject,
    refreshCommands,
    refreshCustomViews,
    refreshDriveConnections,
  } = dependencies;
  function disposeProjectTicketReaders(ids: readonly string[]) {
    const closing = new Set(ids),
      result = disposeTicketReaderFrames(linkedReaderStack.value, closing);
    // Undo callbacks may capture complete tickets and attachments from a closed checkout.
    for (const id of ids) histories.delete(id);
    for (const frame of result.disposed) {
      const saves = linkedReaderAutosaves.get(frame.id);
      saves?.details.cancel();
      saves?.note.cancel();
      saves?.blocked.cancel();
      linkedReaderAutosaves.delete(frame.id);
      readerReturnFocus.delete(frame.id);
      readerClosing.delete(frame.id);
      readerApprovedClose.delete(frame.id);
    }
    linkedReaderStack.value = result.retained;
    if (closing.has(selectedProjectId.value)) {
      readerDetailsAutosave.cancel();
      readerNoteAutosave.cancel();
      readerBlockedReasonAutosave.cancel();
      readerOpen.value = false;
      readerReturnFocus.delete('workspace-reader');
      readerClosing.delete('workspace-reader');
      readerApprovedClose.delete('workspace-reader');
    }
  }
  function closeProjectIds(ids: readonly string[]) {
    disposeProjectTicketReaders(ids);
    for (const item of projects.value)
      if (ids.includes(item.id)) void new Api(item.apiPath).closeProjectSession().catch(() => undefined);
    /* Lets the server stop hosting what no open project needs (HS2-ARJ9J1). */ const closing = new Set(ids),
      before = projects.value,
      selectedIndex = before.findIndex((item) => item.id === selectedProjectId.value);
    let activation: ReturnType<typeof activateOpenProject>;
    for (const id of ids) {
      projectTabRefresh.cancel(id);
      pendingCreatedTickets.forgetProject(id);
      projectsPendingActivation.delete(id);
      warmProjects.forget(id);
      forgetAiConfiguration(id);
    }
    projects.value = before.filter((item) => !closing.has(item.id));
    pruneProjectRecords(projectRecords, closing);
    if (statsProjectId.value && closing.has(statsProjectId.value)) statsProjectId.value = undefined;
    if (closing.has(selectedProjectId.value)) {
      resetTicketComposer();
      const next = nextProjectAfterClose(before, projects.value, selectedIndex);
      if (next) activation = activateOpenProject(next);
      else selectedProjectId.value = '';
      if (!selectedProjectId.value && projectRestoreFailures.value.length)
        selectedProjectRestoreRoot.value = projectRestoreFailures.value[0].root;
    }
    pruneProjectRecords(connectionRecords, closing);
    localStorage.setItem('hotsheet.open-projects', JSON.stringify(currentRememberedProjectRoots()));
    syncProjectChangeStreams();
    commandDialogId.value = undefined;
    commandSettingsEditingId.value = undefined;
    if (activation) void refreshActivatedProject(activation, terminalDrawerVisible.value);
    else if (project())
      void Promise.all([refreshProject(), refreshCommands(), refreshCustomViews(), refreshDriveConnections()]);
    else {
      commandDefinitions.value = [];
      commandRuns.value = [];
    }
  }
  return { closeProjectIds, disposeProjectTicketReaders };
}
