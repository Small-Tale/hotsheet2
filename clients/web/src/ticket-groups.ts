/**
 * Ticket groups (HS2-3XNTSD): a ticket names at most one group, and a group exists while at
 * least one ticket names it. The name rules mirror the server's (HS2-PDSHNN): trimmed, inner
 * whitespace collapsed, compared case-insensitively.
 */

/** The longest group name the server accepts. */
export const TICKET_GROUP_NAME_LIMIT = 120;

/** The stored spelling of a typed group name: trimmed, with inner whitespace runs collapsed. */
export function normalizeTicketGroupName(name: string): string {
  return name.trim().replace(/\s+/gu, ' ');
}

/** The comparison key of a group name; two names with one key are the same group. */
export function ticketGroupKey(name: string): string {
  return normalizeTicketGroupName(name).toLowerCase();
}

/** A ticket's group name, or undefined when it is ungrouped (the wire omits or nulls the field). */
export function ticketGroupName(ticket: { group?: string | null }): string | undefined {
  const name = ticket.group ? normalizeTicketGroupName(ticket.group) : '';
  return name || undefined;
}

export interface TicketGroupSummary {
  name: string;
  count: number;
}

/** Every group the given tickets name, with member counts, sorted by name. */
export function ticketGroupSummaries(tickets: readonly { group?: string | null }[]): TicketGroupSummary[] {
  const groups = new Map<string, TicketGroupSummary>();
  for (const ticket of tickets) {
    const name = ticketGroupName(ticket);
    if (!name) continue;
    const key = ticketGroupKey(name),
      existing = groups.get(key);
    if (existing) existing.count += 1;
    else groups.set(key, { name, count: 1 });
  }
  return [...groups.values()].sort((left, right) =>
    left.name.localeCompare(right.name, undefined, { sensitivity: 'base' }),
  );
}

/** The existing group a typed name refers to, in its stored spelling. */
export function existingTicketGroup(name: string, groups: readonly string[]): string | undefined {
  const key = ticketGroupKey(name);
  return key ? groups.find((group) => ticketGroupKey(group) === key) : undefined;
}

export type TicketGroupNameProblem = 'empty' | 'too-long' | 'invalid' | 'duplicate';

/**
 * Why a typed name cannot name a new group. `allow` is the group being renamed, whose own name
 * (in any casing) is not a duplicate.
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
  if (existing && (!allow || ticketGroupKey(allow) !== ticketGroupKey(existing))) return 'duplicate';
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
    const name = ticketGroupName(ticket);
    if (!name) continue;
    grouped = true;
    const key = ticketGroupKey(name),
      list = members.get(key);
    if (list) list.push(ticket);
    else members.set(key, [ticket]);
  }
  if (!grouped) return [...tickets];
  const result: T[] = [],
    placed = new Set<string>();
  for (const ticket of tickets) {
    const name = ticketGroupName(ticket);
    if (!name) {
      result.push(ticket);
      continue;
    }
    const key = ticketGroupKey(name);
    if (placed.has(key)) continue;
    placed.add(key);
    result.push(...members.get(key)!);
  }
  return result;
}

export type TicketGroupSegment<T> =
  { kind: 'ungrouped'; items: T[] } | { kind: 'group'; name: string; key: string; items: T[] };

/**
 * Split tickets into the runs a collection renders: each group once, where its first member
 * falls, and each run of ungrouped tickets between groups. The input need not be clustered.
 */
export function ticketGroupSegments<T extends { group?: string | null }>(
  tickets: readonly T[],
): TicketGroupSegment<T>[] {
  const segments: TicketGroupSegment<T>[] = [];
  for (const ticket of clusterTicketsByGroup(tickets)) {
    const name = ticketGroupName(ticket),
      key = name ? ticketGroupKey(name) : undefined,
      last = segments.at(-1);
    if (!name || !key) {
      if (last?.kind === 'ungrouped') last.items.push(ticket);
      else segments.push({ kind: 'ungrouped', items: [ticket] });
    } else if (last?.kind === 'group' && last.key === key) last.items.push(ticket);
    else segments.push({ kind: 'group', name, key, items: [ticket] });
  }
  return segments;
}

/** The collapse-preference key of one group in one project view (HS2-WZ3JGZ). */
export function ticketGroupCollapseKey(projectId: string, view: string, group: string): string {
  return JSON.stringify([projectId, view, ticketGroupKey(group)]);
}

/** Drop tickets hidden inside a collapsed group, so navigation and selection walk visible rows. */
export function withoutCollapsedGroupMembers<T extends { group?: string | null }>(
  tickets: readonly T[],
  collapsed: (group: string) => boolean,
): T[] {
  return tickets.filter((ticket) => {
    const name = ticketGroupName(ticket);
    return !name || !collapsed(name);
  });
}

/** A DOM-safe id fragment for a group's member region. */
export function ticketGroupDomId(scope: string, group: string): string {
  let hash = 0;
  for (const character of `${scope}\u0000${ticketGroupKey(group)}`)
    hash = (Math.imul(hash, 31) + character.codePointAt(0)!) >>> 0;
  return `ticket-group-${hash.toString(36)}`;
}
