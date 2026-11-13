import {
  normalizeTicketGroupName,
  ticketGroupKey,
  ticketGroupNameKey,
  ticketGroupNameProblem,
  type TicketGroupOption,
} from './ticket-groups';

/**
 * The prompt and reply contract for Add to Group › Automatic (HS2-4C32A0). The client asks the
 * project's default AI tool, in a private one-shot drive turn, to pick one existing group for the
 * selected tickets or to name a new one, and to answer with JSON only. The client validates the
 * answer here and applies it through the ordinary group writes, so Undo reverses it.
 */

/** The first words of every Automatic prompt; a route-faithful test fake recognizes the turn by them. */
export const AUTOMATIC_GROUP_PROMPT_PREFIX = 'Choose one Hot Sheet ticket group';
/** How many tickets the prompt describes; a larger selection is described by its first ones. */
export const AUTOMATIC_GROUP_TICKET_LIMIT = 40;

export interface AutomaticGroupTicket {
  slug: string;
  title: string;
  category?: string;
}

/** The validated choice: an existing group's stored value, or a new group's normalized name. */
export type AutomaticGroupProposal = { kind: 'existing'; id: string; name: string } | { kind: 'new'; name: string };

/** A reply the client cannot use; the message is shown to the person and nothing changes. */
export class AutomaticGroupReplyError extends Error {}

/** The one-turn prompt: the instructions, then the loaded tickets' titles and categories and the groups as JSON. */
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
    ...(tickets.length > AUTOMATIC_GROUP_TICKET_LIMIT
      ? { more_tickets: tickets.length - AUTOMATIC_GROUP_TICKET_LIMIT }
      : {}),
    groups: groups.map((group) => ({ id: group.id, name: group.name })),
  };
  return [
    `${AUTOMATIC_GROUP_PROMPT_PREFIX} for all of the tickets below.`,
    'Prefer an existing group whose name fits them. Only when none fits, propose a short new group name (at most 120 characters, one line).',
    'Do not call tools, run commands, or change any data. Reply with one JSON object only, either {"group_id": "<an id from groups>"} or {"new_group": "<name>"}.',
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
 * Validate a reply against the project's groups. An id must name a listed group; a new name must
 * be a valid group name, and a name that already exists becomes that group rather than a duplicate.
 */
export function parseAutomaticGroupProposal(
  reply: string,
  groups: readonly TicketGroupOption[],
): AutomaticGroupProposal {
  const value = replyObject(reply);
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new AutomaticGroupReplyError('The AI’s answer was not the expected JSON.');
  const record = value as Record<string, unknown>,
    id = typeof record.group_id === 'string' ? record.group_id.trim() : undefined,
    name = typeof record.new_group === 'string' ? record.new_group : undefined;
  if ((id ? 1 : 0) + (name !== undefined ? 1 : 0) !== 1)
    throw new AutomaticGroupReplyError('The AI’s answer named neither one group nor one new group.');
  if (id) {
    const group = groups.find((item) => ticketGroupKey(item.id) === ticketGroupKey(id));
    if (!group) throw new AutomaticGroupReplyError('The AI chose a group that does not exist.');
    return { kind: 'existing', id: group.id, name: group.name };
  }
  // Check the raw name, so a line break is refused rather than folded into a space.
  if (ticketGroupNameProblem(name ?? '', []))
    throw new AutomaticGroupReplyError('The AI proposed a group name that is not allowed.');
  const normalized = normalizeTicketGroupName(name ?? ''),
    named = groups.filter((group) => ticketGroupNameKey(group.name) === ticketGroupNameKey(normalized));
  if (named.length > 1) throw new AutomaticGroupReplyError(`More than one group is named “${normalized}”.`);
  if (named.length === 1) return { kind: 'existing', id: named[0].id, name: named[0].name };
  return { kind: 'new', name: normalized };
}
