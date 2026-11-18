import {
  normalizeTicketGroupName,
  ticketGroupKey,
  ticketGroupNameKey,
  ticketGroupNameProblem,
  type TicketGroupOption,
} from './ticket-groups';

/**
 * The prompt and reply contract for Add to Group › Automatic (HS2-4C32A0). The client asks the
 * project's default AI tool, in a private one-shot drive turn, to choose a group for **each**
 * selected ticket (HS2-4H6XJ1): an existing group, or a new name, never assuming the tickets
 * belong together. The reply is one JSON object listing an assignment per ticket; the client
 * validates each here and applies them through the ordinary group writes, so Undo reverses them.
 * The server's automatic grouping of new tickets uses the same shape (HS2-FN018Q).
 */

/** The first words of every Automatic prompt; a route-faithful test fake recognizes the turn by them. */
export const AUTOMATIC_GROUP_PROMPT_PREFIX = 'Choose one Hot Sheet ticket group';
/** How many tickets one turn describes; a larger selection is asked about in several turns. */
export const AUTOMATIC_GROUP_TICKET_LIMIT = 40;

export interface AutomaticGroupTicket {
  slug: string;
  title: string;
  category?: string;
}

/** The validated choice: an existing group's stored value, or a new group's normalized name. */
export type AutomaticGroupProposal = { kind: 'existing'; id: string; name: string } | { kind: 'new'; name: string };

/** One ticket's validated choice. */
export interface AutomaticGroupAssignment {
  slug: string;
  proposal: AutomaticGroupProposal;
}

/** A reply the client cannot use; the message is shown to the person and nothing changes. */
export class AutomaticGroupReplyError extends Error {}

/** The one-turn prompt: the instructions, then the tickets' titles and categories and the groups as JSON. */
export function buildAutomaticGroupPrompt(
  tickets: readonly AutomaticGroupTicket[],
  groups: readonly TicketGroupOption[],
): string {
  const context = {
    tickets: tickets.slice(0, AUTOMATIC_GROUP_TICKET_LIMIT).map((ticket) => ({
      slug: ticket.slug,
      title: ticket.title,
      ...(ticket.category ? { category: ticket.category } : {}),
    })),
    groups: groups.map((group) => ({ id: group.id, name: group.name })),
  };
  return [
    `${AUTOMATIC_GROUP_PROMPT_PREFIX} for each of the tickets below, judging each ticket on its own; they need not share a group.`,
    'Prefer an existing group whose name fits a ticket. Only when none fits, propose a short new group name (at most 120 characters, one line); tickets that truly belong together may share a new name.',
    'Do not call tools, run commands, or change any data. Reply with one JSON object only: {"assignments": [{"slug": "<a ticket slug>", "group_id": "<an id from groups>"} or {"slug": "<a ticket slug>", "new_group": "<name>"}, ...]}. Leave out a ticket that fits no group.',
    '',
    JSON.stringify(context),
  ].join('\n');
}

/** The JSON object in a reply: the whole reply, a fenced block, or the first `{…}` span. */
function replyObject(reply: string): unknown {
  const text = reply.trim(),
    fenced = /```(?:json)?\s*([\s\S]*?)```/u.exec(text)?.[1],
    start = text.indexOf('{'),
    end = text.lastIndexOf('}');
  for (const candidate of [text, fenced, start >= 0 && end > start ? text.slice(start, end + 1) : undefined]) {
    if (!candidate) continue;
    try {
      return JSON.parse(candidate.trim()) as unknown;
    } catch {
      // Try the next shape.
    }
  }
  throw new AutomaticGroupReplyError('The AI’s answer was not the expected JSON.');
}

/**
 * One assignment's group: an id must name a listed group; a new name must be a valid group name,
 * and a name that already exists becomes that group rather than a duplicate. `undefined` when the
 * assignment is unusable (it is skipped, the others still apply).
 */
function proposalFor(
  record: Record<string, unknown>,
  groups: readonly TicketGroupOption[],
): AutomaticGroupProposal | undefined {
  const id = typeof record.group_id === 'string' ? record.group_id.trim() : undefined,
    name = typeof record.new_group === 'string' ? record.new_group : undefined;
  if ((id ? 1 : 0) + (name !== undefined ? 1 : 0) !== 1) return undefined;
  if (id) {
    const group = groups.find((item) => ticketGroupKey(item.id) === ticketGroupKey(id));
    return group && { kind: 'existing', id: group.id, name: group.name };
  }
  // Check the raw name, so a line break is refused rather than folded into a space.
  if (ticketGroupNameProblem(name ?? '', [])) return undefined;
  const normalized = normalizeTicketGroupName(name ?? ''),
    named = groups.filter((group) => ticketGroupNameKey(group.name) === ticketGroupNameKey(normalized));
  if (named.length > 1) return undefined;
  if (named.length === 1) return { kind: 'existing', id: named[0].id, name: named[0].name };
  return { kind: 'new', name: normalized };
}

/**
 * Validate a reply against the asked tickets and the project's groups (HS2-4H6XJ1). Each ticket
 * gets its own choice; an assignment for a ticket not asked about, a repeat, an unknown group, or
 * an invalid name is skipped on its own. A reply without an assignments list is an error.
 */
export function parseAutomaticGroupAssignments(
  reply: string,
  tickets: readonly AutomaticGroupTicket[],
  groups: readonly TicketGroupOption[],
): AutomaticGroupAssignment[] {
  const value = replyObject(reply);
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new AutomaticGroupReplyError('The AI’s answer was not the expected JSON.');
  const list = (value as Record<string, unknown>).assignments;
  if (!Array.isArray(list)) throw new AutomaticGroupReplyError('The AI’s answer had no assignments list.');
  const asked = new Set(tickets.map((ticket) => ticket.slug)),
    seen = new Set<string>(),
    assignments: AutomaticGroupAssignment[] = [];
  for (const item of list as unknown[]) {
    if (!item || typeof item !== 'object' || Array.isArray(item)) continue;
    const record = item as Record<string, unknown>,
      slug = typeof record.slug === 'string' ? record.slug.trim() : '';
    if (!asked.has(slug) || seen.has(slug)) continue;
    const proposal = proposalFor(record, groups);
    if (!proposal) continue;
    seen.add(slug);
    assignments.push({ slug, proposal });
  }
  return assignments;
}
