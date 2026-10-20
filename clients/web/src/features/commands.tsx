import type { Signal } from 'kerfjs';
import { effect, signal } from 'kerfjs';

import { Api, type CommandDefinition, type CommandRun } from '../api';
import type { CommandDropTarget } from '../command-order';
import { normalizeCommandGroups } from '../command-order';
import {
  addCommandGroupName,
  commandDefinitionsFromDraft,
  commandGroupsAfterEdit,
  CommandSettingsSaves,
  editCommandAiSelection,
  editCommandDefinition,
  nextCommandDefinition,
  reorderCommandSettings as reorderCommandSettingsState,
} from '../command-settings-workflow';
import { commandIconNeedsCatalog } from '../components/command-icon';
import { COMMAND_EDITOR_DIALOG_ID } from '../components/command-settings-editor';
import { CommandDialogSurface } from '../components/reader-overlay-surfaces';
import type { Control, Project } from '../interactions/types';
import { loadLucideCatalog } from '../lucide-catalog';
import { projectSettingsValue, updateProjectSettingsValue } from '../project-settings-state';
import type { WorkspacePreferences } from '../workspace-preferences';

export interface CommandsDependencies {
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
  storedWorkspacePreferences: Pick<WorkspacePreferences, 'commandGroupExpanded' | 'commandGroupsCollapsed'>;
}

/** Own project command drafts, selection, autosave and run-dialog projection. */
export function createCommandsController({
  projects,
  selectedProjectId,
  storedWorkspacePreferences,
}: CommandsDependencies) {
  const commandDefinitions = signal<CommandDefinition[]>([]),
    commandRuns = signal<CommandRun[]>([]),
    commandGroupExpanded = signal(storedWorkspacePreferences.commandGroupExpanded),
    commandGroupsCollapsed = signal(storedWorkspacePreferences.commandGroupsCollapsed),
    commandDialogId = signal<string | undefined>(undefined),
    commandStopConfirmation = signal(false),
    commandSettingsEditingId = signal<string | undefined>(undefined),
    commandSettingsDraftsByProject = signal<Record<string, string>>({}),
    commandSettingsMessagesByProject = signal<Record<string, string>>({}),
    commandSettingsSelectedByProject = signal<Record<string, string[]>>({}),
    commandSettingsExtraGroupsByProject = signal<Record<string, string[]>>({}),
    commandIconSearch = signal('');
  let commandSelectionAnchor: string | undefined;
  // Lazily load the full Lucide catalog once a project's commands reference an icon outside the
  // bundled popular set, so the sidebar can render those custom icons without a picker being opened.
  effect(() => {
    if (commandDefinitions.value.some((command) => commandIconNeedsCatalog(command.icon))) void loadLucideCatalog();
  });
  const commandSettingsDraft = (projectId = selectedProjectId.value) =>
    projectSettingsValue(
      commandSettingsDraftsByProject.value,
      projectId,
      JSON.stringify(commandDefinitions.value, null, 2),
    );
  const commandSettingsMessage = (projectId = selectedProjectId.value) =>
    projectSettingsValue(commandSettingsMessagesByProject.value, projectId, '');
  function setCommandSettingsDraft(projectId: string, draft: string) {
    commandSettingsDraftsByProject.value = updateProjectSettingsValue(
      commandSettingsDraftsByProject.value,
      projectId,
      draft,
    );
  }
  function setCommandSettingsMessage(projectId: string, message: string) {
    commandSettingsMessagesByProject.value = updateProjectSettingsValue(
      commandSettingsMessagesByProject.value,
      projectId,
      message,
    );
  }
  function commandSettingsDefinitions(projectId = selectedProjectId.value): CommandDefinition[] {
    return commandDefinitionsFromDraft(commandSettingsDraft(projectId), commandDefinitions.value);
  }
  function setCommandSettingsDefinitions(projectId: string, definitions: CommandDefinition[]) {
    setCommandSettingsDraft(projectId, JSON.stringify(definitions, null, 2));
    setCommandSettingsMessage(projectId, '');
  }
  function commandSelection(projectId = selectedProjectId.value): string[] {
    return commandSettingsSelectedByProject.value[projectId] ?? [];
  }
  function setCommandSelection(projectId: string, ids: readonly string[]) {
    commandSettingsSelectedByProject.value = { ...commandSettingsSelectedByProject.value, [projectId]: [...ids] };
  }
  function selectCommandSetting(projectId: string, id: string | undefined) {
    setCommandSelection(projectId, id ? [id] : []);
    commandSelectionAnchor = id;
  }
  /** Apply a click on a command row to the multi-selection, honoring toggle (Cmd/Ctrl) and range (Shift) intent. */
  function selectCommandRow(projectId: string, id: string, intent: { toggle?: boolean; range?: boolean }) {
    const ordered = commandSettingsDefinitions(projectId).map((command) => command.id),
      current = commandSelection(projectId);
    if (intent.range && commandSelectionAnchor) {
      const from = ordered.indexOf(commandSelectionAnchor),
        to = ordered.indexOf(id);
      if (from >= 0 && to >= 0) {
        const [lo, hi] = from < to ? [from, to] : [to, from];
        setCommandSelection(projectId, ordered.slice(lo, hi + 1));
        return;
      }
    }
    if (intent.toggle) {
      const next = current.includes(id) ? current.filter((item) => item !== id) : [...current, id];
      setCommandSelection(projectId, next);
      commandSelectionAnchor = id;
      return;
    }
    selectCommandSetting(projectId, id);
  }
  function updateCommandSetting(projectId: string, id: string, field: string, value: string) {
    setCommandSettingsDefinitions(
      projectId,
      editCommandDefinition(commandSettingsDefinitions(projectId), id, field, value),
    );
    if (field === 'id') {
      selectCommandSetting(projectId, value);
      if (commandSettingsEditingId.value === id) commandSettingsEditingId.value = value;
    }
    scheduleCommandAutosave(projectId);
  }
  function updateCommandAiSelection(
    projectId: string,
    id: string,
    selection: Partial<Pick<CommandDefinition, 'tool' | 'model' | 'effort'>>,
  ) {
    setCommandSettingsDefinitions(
      projectId,
      editCommandAiSelection(commandSettingsDefinitions(projectId), id, selection),
    );
    scheduleCommandAutosave(projectId);
  }
  function addCommandSetting(projectId: string) {
    const definitions = commandSettingsDefinitions(projectId),
      next = nextCommandDefinition(definitions),
      id = next.id;
    setCommandSettingsDefinitions(projectId, [...definitions, next]);
    selectCommandSetting(projectId, id);
    scheduleCommandAutosave(projectId);
    return id;
  }
  function deleteCommandSetting(projectId: string, id: string) {
    const definitions = commandSettingsDefinitions(projectId),
      index = definitions.findIndex((command) => command.id === id),
      next = definitions.filter((command) => command.id !== id);
    setCommandSettingsDefinitions(projectId, next);
    keepEmptiedCommandGroups(projectId, definitions, next);
    selectCommandSetting(projectId, next[Math.min(Math.max(index, 0), next.length - 1)]?.id);
    scheduleCommandAutosave(projectId);
    if (commandSettingsEditingId.value === id) {
      commandSettingsEditingId.value = undefined;
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      (document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control).hidePopover?.();
    }
  }
  function commandSettingsExtraGroups(projectId = selectedProjectId.value) {
    return commandSettingsExtraGroupsByProject.value[projectId] ?? [];
  }
  function setCommandSettingsExtraGroups(projectId: string, groups: readonly string[]) {
    commandSettingsExtraGroupsByProject.value = {
      ...commandSettingsExtraGroupsByProject.value,
      [projectId]: normalizeCommandGroups(groups),
    };
  }
  const commandSettingsSaves = new CommandSettingsSaves({
    projectPath: (projectId) => projects.value.find((item) => item.id === projectId)?.apiPath,
    definitions: commandSettingsDefinitions,
    groups: commandSettingsExtraGroups,
    setMessage: setCommandSettingsMessage,
    commandSaved: (projectId, definitions) => {
      if (selectedProjectId.value === projectId) commandDefinitions.value = definitions;
    },
    groupsLoaded: setCommandSettingsExtraGroups,
    saveCommands: (path, definitions, options) => new Api(path).saveCommands(definitions, options),
    saveGroups: (path, groups) => new Api(path).saveCommandGroups(groups),
  });
  function changeCommandGroups(projectId: string, groups: readonly string[]) {
    const before = commandSettingsExtraGroups(projectId);
    setCommandSettingsExtraGroups(projectId, groups);
    if (JSON.stringify(before) !== JSON.stringify(commandSettingsExtraGroups(projectId)))
      void commandSettingsSaves.saveGroups(projectId);
  }
  /** Keep any group a command edit emptied, so emptying a group never silently deletes it. */
  function keepEmptiedCommandGroups(
    projectId: string,
    before: readonly CommandDefinition[],
    after: readonly CommandDefinition[],
  ) {
    changeCommandGroups(projectId, commandGroupsAfterEdit(before, after, commandSettingsExtraGroups(projectId)));
  }
  /** Adopt the server's kept command groups unless a local group save is still in flight. */
  function loadCommandGroups(projectId: string, groups: readonly string[]) {
    commandSettingsSaves.loadGroups(projectId, groups);
  }
  function reorderCommandSettings(projectId: string, sourceIds: readonly string[], target: CommandDropTarget) {
    const next = reorderCommandSettingsState(
      commandSettingsDefinitions(projectId),
      commandSettingsExtraGroups(projectId),
      sourceIds,
      target,
    );
    setCommandSettingsDefinitions(projectId, next.definitions);
    changeCommandGroups(projectId, next.groups);
    scheduleCommandAutosave(projectId);
  }
  function addCommandGroup(projectId: string) {
    const name = window.prompt('New group name');
    if (!name) return;
    const result = addCommandGroupName(
      name,
      commandSettingsDefinitions(projectId),
      commandSettingsExtraGroups(projectId),
    );
    if (result.message) setCommandSettingsMessage(projectId, result.message);
    if (result.groups) changeCommandGroups(projectId, result.groups);
  }
  function deleteCommandGroup(projectId: string, group: string) {
    changeCommandGroups(
      projectId,
      commandSettingsExtraGroups(projectId).filter((item) => item !== group),
    );
  }
  function scheduleCommandAutosave(projectId: string) {
    commandSettingsSaves.schedule(projectId);
  }
  /**
   * Save every pending command edit now (HS2-25HAK3). Called when the page hides, so a reload or
   * closed tab inside the debounce window keeps the edit; `keepalive` lets the write outlive the page.
   */
  function flushCommandAutosaves() {
    commandSettingsSaves.flush();
  }

  function commandRunFor(commandId: string) {
    return commandRuns.value.find((run) => run.command_id === commandId);
  }

  function commandIcon(command: CommandDefinition): string {
    return (
      command.icon ??
      (command.kind === 'ai' ||
      command.program?.includes('hotsheet') ||
      command.args?.some((value) => value.includes('trigger'))
        ? 'send'
        : command.id.includes('test') || command.title.toLowerCase().includes('test')
          ? 'test'
          : 'build')
    );
  }

  function commandDialogSurface() {
    const id = commandDialogId.value,
      command = commandDefinitions.value.find((item) => item.id === id),
      run = id ? commandRunFor(id) : undefined;
    return <CommandDialogSurface command={command} run={run} confirmStop={commandStopConfirmation.value} />;
  }

  return {
    commandDefinitions,
    commandRuns,
    commandGroupExpanded,
    commandGroupsCollapsed,
    commandDialogId,
    commandStopConfirmation,
    commandSettingsEditingId,
    commandSettingsDraftsByProject,
    commandSettingsMessagesByProject,
    commandSettingsSelectedByProject,
    commandSettingsExtraGroupsByProject,
    commandIconSearch,
    commandSettingsMessage,
    setCommandSettingsDraft,
    setCommandSettingsMessage,
    commandSettingsDefinitions,
    commandSelection,
    selectCommandSetting,
    selectCommandRow,
    updateCommandSetting,
    updateCommandAiSelection,
    addCommandSetting,
    deleteCommandSetting,
    commandSettingsExtraGroups,
    loadCommandGroups,
    reorderCommandSettings,
    addCommandGroup,
    deleteCommandGroup,
    flushCommandAutosaves,
    commandRunFor,
    commandIcon,
    commandDialogSurface,
  };
}
