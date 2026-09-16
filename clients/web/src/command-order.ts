import type { CommandDefinition } from './api';

/** Whether a dropped row lands before or after the row it was dropped onto. */
export type CommandDropPosition = 'before' | 'after';

/** A drag drop target: onto another row (relative position) or into a group's open area. */
export type CommandDropTarget =
  | { kind: 'row'; id: string; position: CommandDropPosition }
  | { kind: 'group'; group: string };

/** The normalized group key of a command; blank means ungrouped. */
export function commandGroupKey(command: CommandDefinition): string {
  return command.group?.trim() || '';
}

function withGroup(command: CommandDefinition, group: string): CommandDefinition {
  return { ...command, group: group || undefined };
}

/**
 * Move `sourceId` to a drop target, updating both its order in the flat definition list and its
 * group. Dropping onto a row places the source before/after it and adopts that row's group; dropping
 * into a group's area appends it after that group's last command (or last overall when empty).
 */
export function reorderCommands(
  commands: readonly CommandDefinition[],
  sourceId: string,
  target: CommandDropTarget,
): CommandDefinition[] {
  const source = commands.find(command => command.id === sourceId);
  if (!source) return [...commands];
  const targetGroup = target.kind === 'row'
    ? commandGroupKey(commands.find(command => command.id === target.id) ?? source)
    : target.group;
  const moved = withGroup(source, targetGroup);
  const remaining = commands.filter(command => command.id !== sourceId);
  if (target.kind === 'row') {
    if (target.id === sourceId) return [...commands];
    const index = remaining.findIndex(command => command.id === target.id);
    if (index < 0) return [...commands];
    remaining.splice(index + (target.position === 'after' ? 1 : 0), 0, moved);
    return remaining;
  }
  let lastIndex = -1;
  remaining.forEach((command, index) => { if (commandGroupKey(command) === targetGroup) lastIndex = index; });
  if (lastIndex >= 0) remaining.splice(lastIndex + 1, 0, moved);
  else remaining.push(moved);
  return remaining;
}

/** One rendered section of the editor list: the ungrouped section (blank group) or a named group. */
export interface CommandGroupSection {
  group: string;
  commands: CommandDefinition[];
}

/**
 * Group commands for display: the ungrouped section first (only when non-empty), then named groups in
 * first-seen order, then any still-empty `extraGroups` (added via "Add group") appended at the end.
 */
export function commandGroupSections(
  commands: readonly CommandDefinition[],
  extraGroups: readonly string[] = [],
): CommandGroupSection[] {
  const order: string[] = [];
  const buckets = new Map<string, CommandDefinition[]>();
  const ensure = (group: string): CommandDefinition[] => {
    let bucket = buckets.get(group);
    if (!bucket) { bucket = []; buckets.set(group, bucket); if (group !== '') order.push(group); }
    return bucket;
  };
  for (const command of commands) ensure(commandGroupKey(command)).push(command);
  for (const group of extraGroups) { const key = group.trim(); if (key) ensure(key); }
  const sections: CommandGroupSection[] = [];
  const ungrouped = buckets.get('');
  if (ungrouped?.length) sections.push({ group: '', commands: ungrouped });
  for (const group of order) sections.push({ group, commands: buckets.get(group) ?? [] });
  return sections;
}

/** The still-empty groups from `extraGroups` (those with no commands), preserving their order. */
export function emptyExtraGroups(
  commands: readonly CommandDefinition[],
  extraGroups: readonly string[],
): string[] {
  const populated = new Set(commands.map(commandGroupKey));
  const seen = new Set<string>();
  const result: string[] = [];
  for (const group of extraGroups) {
    const key = group.trim();
    if (key && !populated.has(key) && !seen.has(key)) { seen.add(key); result.push(key); }
  }
  return result;
}
