/**
 * Ticket groups (HS2-3XNTSD, HS2-9KZSD4): a ticket stores at most one group value, and a group
 * exists while at least one ticket stores it. The value is normally a group id whose display
 * name lives in the project's group registry; an unmigrated ("legacy") ticket may still store a
 * plain name. Ids key groups exactly, so two ids with one display name stay two groups; legacy
 * names key by the server's name rules (HS2-PDSHNN): trimmed, inner whitespace collapsed,
 * compared case-insensitively.
 */

/** The longest group name the server accepts. */
export const TICKET_GROUP_NAME_LIMIT = 120;

/** A group id: 10 uppercase Crockford base32 characters (HS2-9KZSD4). */
const GROUP_ID_PATTERN = /^[0-9A-HJKMNP-TV-Z]{10}$/u;

/** Whether a stored group value is a registry id rather than a legacy name. */
export function isTicketGroupId(value: string): boolean {
  return GROUP_ID_PATTERN.test(value);
}

/** The stored spelling of a typed group name: trimmed, with inner whitespace runs collapsed. */
export function normalizeTicketGroupName(name: string): string {
  return name.trim().replace(/\s+/gu, ' ');
}

/** The comparison key of a group display name; two names with one key read as the same name. */
export function ticketGroupNameKey(name: string): string {
  return normalizeTicketGroupName(name).toLowerCase();
}

/**
 * The identity key of a stored group value: an id is its own key, and a legacy name keys by
 * its name key, so `Board` and `board ` are one legacy group.
 */
export function ticketGroupKey(value: string): string {
  const trimmed = value.trim();
  return isTicketGroupId(trimmed) ? trimmed : ticketGroupNameKey(trimmed);
}

/** A ticket's stored group value, or undefined when it is ungrouped (the wire omits or nulls the field). */
export function ticketGroupValue(ticket: { group?: string | null }): string | undefined {
  const value = ticket.group ? normalizeTicketGroupName(ticket.group) : '';
  return value || undefined;
}

/** The name shown for an id with no registry entry in this project (HS2-9KZSD4). */
export function ticketGroupPlaceholderName(id: string): string {
  return `Group ${id}`;
}

/** A group as the project lists it: its stored value and display name. */
export interface TicketGroupOption {
  /** The value member tickets store: an id, or a legacy group's name. */
  id: string;
  name: string;
  /** Another listed group has the same name key (HS2-9KZSD4). */
  duplicate?: boolean;
}

/**
 * The display name of a stored group value: the registry summary's name for it; otherwise a
 * legacy value shows itself and an unknown id shows `Group <id>`.
 */
export function ticketGroupDisplayName(value: string, groups: readonly TicketGroupOption[] = []): string {
  const key = ticketGroupKey(value),
    listed = groups.find((group) => ticketGroupKey(group.id) === key);
  if (listed) return listed.name;
  const normalized = normalizeTicketGroupName(value);
  return isTicketGroupId(normalized) ? ticketGroupPlaceholderName(normalized) : normalized;
}

export interface TicketGroupSummary {
  /** The stored value the members share. */
  id: string;
  name: string;
  count: number;
}

/** Every group the given tickets store, with display names and member counts, sorted by name. */
export function ticketGroupSummaries(
  tickets: readonly { group?: string | null }[],
  groups: readonly TicketGroupOption[] = [],
): TicketGroupSummary[] {
  const found = new Map<string, TicketGroupSummary>();
  for (const ticket of tickets) {
    const value = ticketGroupValue(ticket);
    if (!value) continue;
    const key = ticketGroupKey(value),
      existing = found.get(key);
    if (existing) existing.count += 1;
    else found.set(key, { id: value, name: ticketGroupDisplayName(value, groups), count: 1 });
  }
  return [...found.values()].sort(
    (left, right) =>
      left.name.localeCompare(right.name, undefined, { sensitivity: 'base' }) || left.id.localeCompare(right.id),
  );
}

/** The existing display name a typed name refers to, in its stored spelling. */
export function existingTicketGroup(name: string, groups: readonly string[]): string | undefined {
  const key = ticketGroupNameKey(name);
  return key ? groups.find((group) => ticketGroupNameKey(group) === key) : undefined;
}

export type TicketGroupNameProblem = 'empty' | 'too-long' | 'invalid' | 'duplicate';

/**
 * Why a typed name cannot name a new group. `groups` are the project's display names; `allow`
 * is the renamed group's own display name, which (in any casing) is not a duplicate.
 */
export function ticketGroupNameProblem(
  name: string,
  groups: readonly string[],
  allow?: string,
): TicketGroupNameProblem | undefined {
  const normalized = normalizeTicketGroupName(name);
  if (!normalized) return 'empty';
  // eslint-disable-next-line no-control-regex -- HS2-PDSHNN: the server rejects control characters in a group name.
  if (/[\u0000-\u001f\u007f]/u.test(name.trim())) return 'invalid';
  if (normalized.length > TICKET_GROUP_NAME_LIMIT) return 'too-long';
  const existing = existingTicketGroup(normalized, groups);
  if (existing && (allow === undefined || ticketGroupNameKey(allow) !== ticketGroupNameKey(existing)))
    return 'duplicate';
  return undefined;
}

/** The inline error shown for a group name problem; an empty field shows none. */
export function ticketGroupNameError(problem: TicketGroupNameProblem | undefined, name: string): string | undefined {
  if (problem === 'duplicate') return `A group named “${normalizeTicketGroupName(name)}” already exists.`;
  if (problem === 'too-long') return `Use ${TICKET_GROUP_NAME_LIMIT} characters or fewer.`;
  if (problem === 'invalid') return 'Use a single line of text.';
  return undefined;
}

/**
 * Reorder sorted tickets so each group's members are contiguous: a group sits where its first
 * member sorts, and its members keep their sorted order. Ungrouped tickets keep their places.
 */
export function clusterTicketsByGroup<T extends { group?: string | null }>(tickets: readonly T[]): T[] {
  const members = new Map<string, T[]>();
  let grouped = false;
  for (const ticket of tickets) {
    const value = ticketGroupValue(ticket);
    if (!value) continue;
    grouped = true;
    const key = ticketGroupKey(value),
      list = members.get(key);
    if (list) list.push(ticket);
    else members.set(key, [ticket]);
  }
  if (!grouped) return [...tickets];
  const result: T[] = [],
    placed = new Set<string>();
  for (const ticket of tickets) {
    const value = ticketGroupValue(ticket);
    if (!value) {
      result.push(ticket);
      continue;
    }
    const key = ticketGroupKey(value);
    if (placed.has(key)) continue;
    placed.add(key);
    result.push(...members.get(key)!);
  }
  return result;
}

export type TicketGroupSegment<T> =
  { kind: 'ungrouped'; items: T[] } | { kind: 'group'; group: string; name: string; key: string; items: T[] };

/**
 * Split tickets into the runs a collection renders: each group once, where its first member
 * falls, and each run of ungrouped tickets between groups. The input need not be clustered. A
 * row's `groupName` is its group's display name; without one the stored value's fallback name
 * shows (HS2-9KZSD4).
 */
export function ticketGroupSegments<T extends { group?: string | null; groupName?: string }>(
  tickets: readonly T[],
): TicketGroupSegment<T>[] {
  const segments: TicketGroupSegment<T>[] = [];
  for (const ticket of clusterTicketsByGroup(tickets)) {
    const group = ticketGroupValue(ticket),
      key = group ? ticketGroupKey(group) : undefined,
      last = segments.at(-1);
    if (!group || !key) {
      if (last?.kind === 'ungrouped') last.items.push(ticket);
      else segments.push({ kind: 'ungrouped', items: [ticket] });
    } else if (last?.kind === 'group' && last.key === key) last.items.push(ticket);
    else
      segments.push({
        kind: 'group',
        group,
        name: ticket.groupName ?? ticketGroupDisplayName(group),
        key,
        items: [ticket],
      });
  }
  return segments;
}

/** The collapse-preference key of one group in one project view (HS2-WZ3JGZ), by its identity key. */
export function ticketGroupCollapseKey(projectId: string, view: string, group: string): string {
  return JSON.stringify([projectId, view, ticketGroupKey(group)]);
}

/** Drop tickets hidden inside a collapsed group, so navigation and selection walk visible rows. */
export function withoutCollapsedGroupMembers<T extends { group?: string | null }>(
  tickets: readonly T[],
  collapsed: (group: string) => boolean,
): T[] {
  return tickets.filter((ticket) => {
    const value = ticketGroupValue(ticket);
    return !value || !collapsed(value);
  });
}

/** A DOM-safe id fragment for a group's member region. */
export function ticketGroupDomId(scope: string, group: string): string {
  let hash = 0;
  for (const character of `${scope}\u0000${ticketGroupKey(group)}`)
    hash = (Math.imul(hash, 31) + character.codePointAt(0)!) >>> 0;
  return `ticket-group-${hash.toString(36)}`;
}
