import type { Signal } from 'kerfjs';
import { effect, signal } from 'kerfjs';

import { Api, type CommandDefinition, type CommandRun } from '../api';
import { type CommandDropTarget, emptyExtraGroups, reorderCommandsMultiple } from '../command-order';
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
  function isCommandSettingsDefinition(value: unknown): value is CommandDefinition {
    return (
      typeof value === 'object' &&
      value !== null &&
      'id' in value &&
      typeof value.id === 'string' &&
      'title' in value &&
      typeof value.title === 'string'
    );
  }
  function commandSettingsDefinitions(projectId = selectedProjectId.value): CommandDefinition[] {
    try {
      const parsed: unknown = JSON.parse(commandSettingsDraft(projectId));
      if (!Array.isArray(parsed)) return commandDefinitions.value;
      const entries: unknown[] = parsed;
      return entries.filter(isCommandSettingsDefinition);
    } catch {
      return commandDefinitions.value;
    }
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
    const definitions = commandSettingsDefinitions(projectId),
      next = definitions.map((command) => {
        if (command.id !== id) return command;
        const updated: CommandDefinition = { ...command };
        if (field === 'args') updated.args = value.split('\n').filter((argument) => argument.length > 0);
        else if (field === 'kind') {
          updated.kind = value as NonNullable<CommandDefinition['kind']>;
          if (value === 'program') {
            delete updated.command;
            delete updated.prompt;
            delete updated.tool;
            delete updated.model;
            delete updated.effort;
          } else if (value === 'shell') {
            delete updated.program;
            delete updated.args;
            delete updated.prompt;
            delete updated.tool;
            delete updated.model;
            delete updated.effort;
          } else {
            delete updated.program;
            delete updated.args;
            delete updated.command;
          }
        } else if (field === 'id' || field === 'title') updated[field] = value;
        else {
          const textField = (
            [
              'program',
              'group',
              'cwd',
              'confirmation',
              'command',
              'prompt',
              'tool',
              'model',
              'effort',
              'color',
              'icon',
            ] as const
          ).find((candidate) => candidate === field);
          if (textField) updated[textField] = value || undefined;
        }
        return updated;
      });
    setCommandSettingsDefinitions(projectId, next);
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
    const next = commandSettingsDefinitions(projectId).map((command) => {
      if (command.id !== id) return command;
      const updated = { ...command };
      if (selection.tool) updated.tool = selection.tool;
      else delete updated.tool;
      if (selection.model) updated.model = selection.model;
      else delete updated.model;
      if (selection.effort) updated.effort = selection.effort;
      else delete updated.effort;
      return updated;
    });
    setCommandSettingsDefinitions(projectId, next);
    scheduleCommandAutosave(projectId);
  }
  function addCommandSetting(projectId: string) {
    const existing = new Set(commandSettingsDefinitions(projectId).map((command) => command.id));
    let index = 1,
      id = 'command-1';
    while (existing.has(id)) id = `command-${++index}`;
    setCommandSettingsDefinitions(projectId, [
      ...commandSettingsDefinitions(projectId),
      { id, title: 'New command', kind: 'shell', command: '' },
    ]);
    selectCommandSetting(projectId, id);
    scheduleCommandAutosave(projectId);
    return id;
  }
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  function deleteCommandSetting(projectId:string,id:string){const definitions=commandSettingsDefinitions(projectId),index=definitions.findIndex(command=>command.id===id),next=definitions.filter(command=>command.id!==id);setCommandSettingsDefinitions(projectId,next);selectCommandSetting(projectId,next[Math.min(Math.max(index,0),next.length-1)]?.id);scheduleCommandAutosave(projectId);if(commandSettingsEditingId.value===id){commandSettingsEditingId.value=undefined;(document.querySelector(`#${COMMAND_EDITOR_DIALOG_ID}`) as Control).hidePopover?.()}}
  function commandSettingsExtraGroups(projectId = selectedProjectId.value) {
    return commandSettingsExtraGroupsByProject.value[projectId] ?? [];
  }
  function setCommandSettingsExtraGroups(projectId: string, groups: string[]) {
    commandSettingsExtraGroupsByProject.value = { ...commandSettingsExtraGroupsByProject.value, [projectId]: groups };
  }
  function reorderCommandSettings(projectId: string, sourceIds: readonly string[], target: CommandDropTarget) {
    const next = reorderCommandsMultiple(commandSettingsDefinitions(projectId), sourceIds, target);
    setCommandSettingsDefinitions(projectId, next);
    setCommandSettingsExtraGroups(projectId, emptyExtraGroups(next, commandSettingsExtraGroups(projectId)));
    scheduleCommandAutosave(projectId);
  }
  function addCommandGroup(projectId: string) {
    const name = window.prompt('New group name')?.trim();
    if (!name) return;
    const existing = new Set([
      ...commandSettingsDefinitions(projectId)
        .map((command) => command.group?.trim())
        .filter(Boolean),
      ...commandSettingsExtraGroups(projectId),
    ]);
    if (existing.has(name)) {
      setCommandSettingsMessage(projectId, `A group named "${name}" already exists.`);
      return;
    }
    setCommandSettingsExtraGroups(projectId, [...commandSettingsExtraGroups(projectId), name]);
  }
  function deleteCommandGroup(projectId: string, group: string) {
    setCommandSettingsExtraGroups(
      projectId,
      commandSettingsExtraGroups(projectId).filter((item) => item !== group),
    );
  }
  let commandAutosaveTimer: ReturnType<typeof setTimeout> | undefined;
  function scheduleCommandAutosave(projectId: string) {
    if (commandAutosaveTimer) clearTimeout(commandAutosaveTimer);
    commandAutosaveTimer = setTimeout(() => {
      commandAutosaveTimer = undefined;
      void saveCommandSettings(projectId);
    }, 600);
  }
  async function saveCommandSettings(projectId: string) {
    const current = projects.value.find((item) => item.id === projectId);
    if (!current) return;
    const definitions = commandSettingsDefinitions(projectId),
      validation = commandSettingsValidation(definitions);
    if (validation) {
      setCommandSettingsMessage(projectId, validation);
      return;
    }
    setCommandSettingsMessage(projectId, 'Saving…');
    try {
      const saved = await new Api(current.apiPath).saveCommands(definitions);
      if (!projects.value.some((item) => item.id === projectId)) return;
      if (selectedProjectId.value === projectId) commandDefinitions.value = saved;
      setCommandSettingsMessage(projectId, 'Saved.');
    } catch (reason) {
      setCommandSettingsMessage(projectId, reason instanceof Error ? reason.message : String(reason));
    }
  }
  function commandSettingsValidation(definitions: CommandDefinition[]): string | undefined {
    const ids = new Set<string>();
    for (const [index, command] of definitions.entries()) {
      const label = command.title.trim() || `Command ${index + 1}`;
      if (!command.id.trim()) return `${label} needs an identifier.`;
      if (ids.has(command.id)) return `Command identifiers must be unique: ${command.id}.`;
      ids.add(command.id);
      if (!command.title.trim()) return `${command.id} needs a button label.`;
      const type = command.kind ?? 'program';
      if (type === 'program' && !command.program?.trim()) return `${label} needs a program.`;
      if (type === 'shell' && !command.command?.trim()) return `${label} needs a shell command.`;
      if (type === 'ai' && !command.prompt?.trim()) return `${label} needs an AI prompt.`;
    }
    return undefined;
  }

  function commandRunFor(commandId: string) {
    return commandRuns.value.find((run) => run.command_id === commandId);
  }

  function showCommandDialog() {
    queueMicrotask(() => {
      const dialog = document.querySelector<HTMLDialogElement>(
        '[data-component="command-run-dialog"], [data-component="command-cancellation-dialog"]',
      );
      if (dialog && !dialog.open) dialog.showModal();
    });
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
    reorderCommandSettings,
    addCommandGroup,
    deleteCommandGroup,
    commandRunFor,
    showCommandDialog,
    commandIcon,
    commandDialogSurface,
  };
}
