import type { CommandDefinition } from './api';
import { ApiHttpError } from './api';
import {
  type CommandDropTarget,
  keptCommandGroups,
  normalizeCommandGroups,
  reorderCommandsMultiple,
} from './command-order';

/** Parse the editor draft without losing the last loaded definitions to partial JSON. */
export function commandDefinitionsFromDraft(
  draft: string,
  fallback: readonly CommandDefinition[],
): CommandDefinition[] {
  try {
    const parsed: unknown = JSON.parse(draft);
    if (!Array.isArray(parsed)) return [...fallback];
    const entries: unknown[] = parsed;
    return entries.filter(
      (value): value is CommandDefinition =>
        typeof value === 'object' &&
        value !== null &&
        'id' in value &&
        typeof value.id === 'string' &&
        'title' in value &&
        typeof value.title === 'string',
    );
  } catch {
    return [...fallback];
  }
}

export function editCommandDefinition(
  definitions: readonly CommandDefinition[],
  id: string,
  field: string,
  value: string,
): CommandDefinition[] {
  return definitions.map((command) => {
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
}

export function editCommandAiSelection(
  definitions: readonly CommandDefinition[],
  id: string,
  selection: Partial<Pick<CommandDefinition, 'tool' | 'model' | 'effort'>>,
): CommandDefinition[] {
  return definitions.map((command) => {
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
}

export function nextCommandDefinition(definitions: readonly CommandDefinition[]): CommandDefinition {
  const existing = new Set(definitions.map((command) => command.id));
  let index = 1,
    id = 'command-1';
  while (existing.has(id)) id = `command-${++index}`;
  return { id, title: 'New command', kind: 'shell', command: '' };
}

export function validateCommandDefinitions(definitions: readonly CommandDefinition[]): string | undefined {
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

export interface CommandSettingsGroupState {
  definitions: CommandDefinition[];
  groups: string[];
}

export function reorderCommandSettings(
  definitions: readonly CommandDefinition[],
  groups: readonly string[],
  sourceIds: readonly string[],
  target: CommandDropTarget,
): CommandSettingsGroupState {
  const next = reorderCommandsMultiple(definitions, sourceIds, target);
  return { definitions: next, groups: keptCommandGroups(definitions, next, groups) };
}

export function commandGroupsAfterEdit(
  before: readonly CommandDefinition[],
  after: readonly CommandDefinition[],
  groups: readonly string[],
): string[] {
  return keptCommandGroups(before, after, groups);
}

export function addCommandGroupName(
  name: string,
  definitions: readonly CommandDefinition[],
  groups: readonly string[],
): { groups?: string[]; message?: string } {
  const value = name.trim();
  if (!value) return {};
  const existing = new Set([...definitions.map((command) => command.group?.trim()).filter(Boolean), ...groups]);
  if (existing.has(value)) return { message: `A group named "${value}" already exists.` };
  return { groups: normalizeCommandGroups([...groups, value]) };
}

export interface CommandSettingsSavePort {
  /** A project disappears or changes API path when its saved response is no longer relevant. */
  projectPath(projectId: string): string | undefined;
  definitions(projectId: string): CommandDefinition[];
  groups(projectId: string): string[];
  setMessage(projectId: string, message: string): void;
  commandSaved(projectId: string, definitions: CommandDefinition[]): void;
  groupsLoaded(projectId: string, groups: readonly string[]): void;
  /** HS2-WMZ6Z5: only the current group save publishes an outcome to its message owner. */
  groupsSaved?(projectId: string): void;
  groupsFailed?(projectId: string): void;
  nextWriteOrder?(): { session: string; sequence: number };
  saveCommands(
    path: string,
    definitions: CommandDefinition[],
    options: { keepalive?: boolean; writeOrder?: { session: string; sequence: number }; expectedRevision: string },
  ): Promise<CommandDefinition[] | { definitions: CommandDefinition[]; revision: string }>;
  saveGroups(path: string, groups: string[], expectedRevision: string): Promise<unknown>;
  setTimer?(callback: () => void, milliseconds: number): ReturnType<typeof setTimeout>;
  clearTimer?(timer: ReturnType<typeof setTimeout>): void;
}

/** Project-keyed save orchestration. The browser adapter owns signals and DOM; this owns transitions. */
export class CommandSettingsSaves {
  private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
  private readonly commandGenerations = new Map<string, number>();
  private readonly groupGenerations = new Map<string, number>();
  private readonly groupPending = new Map<string, number>();
  private readonly groupTail = new Map<string, Promise<unknown>>();
  private readonly commandRevisions = new Map<string, string>();
  private readonly groupRevisions = new Map<string, string>();
  private readonly conflicts = new Set<string>();
  constructor(private readonly port: CommandSettingsSavePort) {}

  loadRevisions(projectId: string, commands: string, groups: string): void {
    if (this.conflicts.has(projectId)) return;
    this.commandRevisions.set(projectId, commands);
    this.groupRevisions.set(projectId, groups);
  }

  hasConflict(projectId: string): boolean {
    return this.conflicts.has(projectId);
  }

  schedule(projectId: string): void {
    const clear = (timer: ReturnType<typeof setTimeout> | undefined) => {
        if (timer === undefined) return;
        if (this.port.clearTimer) this.port.clearTimer(timer);
        else clearTimeout(timer);
      },
      arm = (callback: () => void, milliseconds: number) =>
        this.port.setTimer ? this.port.setTimer(callback, milliseconds) : setTimeout(callback, milliseconds);
    // A draft edit supersedes an in-flight result even before its debounce fires.
    this.commandGenerations.set(projectId, (this.commandGenerations.get(projectId) ?? 0) + 1);
    clear(this.timers.get(projectId));
    this.timers.set(
      projectId,
      arm(() => {
        this.timers.delete(projectId);
        void this.save(projectId);
      }, 600),
    );
  }

  flush(): void {
    for (const [projectId, timer] of this.timers) {
      if (this.port.clearTimer) this.port.clearTimer(timer);
      else clearTimeout(timer);
      this.timers.delete(projectId);
      void this.save(projectId, { keepalive: true });
    }
  }

  async save(projectId: string, options: { keepalive?: boolean } = {}): Promise<void> {
    const path = this.port.projectPath(projectId);
    if (!path) return;
    const generation = (this.commandGenerations.get(projectId) ?? 0) + 1;
    this.commandGenerations.set(projectId, generation);
    const definitions = this.port.definitions(projectId),
      validation = validateCommandDefinitions(definitions);
    if (validation) {
      this.port.setMessage(projectId, validation);
      return;
    }
    this.port.setMessage(projectId, '');
    try {
      const saved = await this.port.saveCommands(path, definitions, {
        ...options,
        expectedRevision: this.commandRevisions.get(projectId) ?? '',
        ...(this.port.nextWriteOrder ? { writeOrder: this.port.nextWriteOrder() } : {}),
      });
      if (this.current(projectId, path, generation)) {
        if (Array.isArray(saved)) this.port.commandSaved(projectId, saved);
        else {
          this.commandRevisions.set(projectId, saved.revision);
          this.port.commandSaved(projectId, saved.definitions);
        }
      }
    } catch (reason) {
      if (this.current(projectId, path, generation)) this.saveError(projectId, reason);
    }
  }

  private current(projectId: string, path: string, generation: number): boolean {
    return this.port.projectPath(projectId) === path && this.commandGenerations.get(projectId) === generation;
  }

  async saveGroups(projectId: string): Promise<void> {
    const path = this.port.projectPath(projectId);
    if (!path) return;
    const groups = this.port.groups(projectId),
      generation = (this.groupGenerations.get(projectId) ?? 0) + 1;
    this.groupGenerations.set(projectId, generation);
    this.groupPending.set(projectId, (this.groupPending.get(projectId) ?? 0) + 1);
    const previous = this.groupTail.get(projectId) ?? Promise.resolve(),
      task = previous
        .catch(() => undefined)
        .then(async () => {
          if (this.port.projectPath(projectId) === path) {
            const saved = await this.port.saveGroups(path, groups, this.groupRevisions.get(projectId) ?? '');
            if (saved && typeof saved === 'object' && 'revision' in saved && typeof saved.revision === 'string')
              this.groupRevisions.set(projectId, saved.revision);
          }
        });
    this.groupTail.set(projectId, task);
    try {
      await task;
      if (this.port.projectPath(projectId) === path && this.groupGenerations.get(projectId) === generation)
        this.port.groupsSaved?.(projectId);
    } catch (reason) {
      if (this.port.projectPath(projectId) === path && this.groupGenerations.get(projectId) === generation) {
        this.saveError(projectId, reason);
        this.port.groupsFailed?.(projectId);
      }
    } finally {
      const remaining = (this.groupPending.get(projectId) ?? 1) - 1;
      if (remaining > 0) this.groupPending.set(projectId, remaining);
      else this.groupPending.delete(projectId);
      if (this.groupTail.get(projectId) === task) this.groupTail.delete(projectId);
    }
  }

  private saveError(projectId: string, reason: unknown): void {
    if (reason instanceof ApiHttpError && reason.status === 409) {
      this.conflicts.add(projectId);
      this.port.setMessage(
        projectId,
        'Command settings changed elsewhere. Copy your draft, then reload to review the latest settings.',
      );
      return;
    }
    this.port.setMessage(projectId, reason instanceof Error ? reason.message : String(reason));
  }

  loadGroups(projectId: string, groups: readonly string[]): void {
    if (!this.groupPending.has(projectId) && !this.conflicts.has(projectId)) this.port.groupsLoaded(projectId, groups);
  }
}
