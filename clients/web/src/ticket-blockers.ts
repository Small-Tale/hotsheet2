import type { Capabilities, TicketRow } from './api';
import { type DuplicateTarget, duplicateTargetKey } from './ticket-close';

/**
 * Editing a ticket's dependency blockers from the Inspector (HS2-CHH28E). A blocker is a
 * ticket in the same ticket source, stored by its native id in `blocked_by`. The Inspector
 * replaces the whole set through the checkout ticket route with the ticket's concurrency
 * token, the same field `hotsheet-cli edit --add-blocked-by/--remove-blocked-by/--blocked-by`
 * and MCP `hotsheet_update blocked_by` write. The server refuses self-blocking and
 * dependency cycles.
 */

/** The ticket reference shape the blocker dialog's picker selects (shared with the close dialog). */
export type BlockerTicket = DuplicateTarget;

/** Whether the Blocked By section's Add Blocker action and Remove buttons are enabled. */
export interface BlockerAvailability {
  enabled: boolean;
  /** The truthful reason a disabled action shows; absent while enabled. */
  disabledReason?: string;
}

/** The provider (display name and capabilities) that owns the inspected ticket. */
export interface BlockerSourceProvider {
  name: string;
  capabilities: Pick<Capabilities, 'dependencies'>;
}

/**
 * Provider support comes from the structured `dependencies` capability, never from the
 * provider id. A source without it keeps the action mounted but disabled with a reason
 * (house grammar: Chrome and hierarchy); a ticket the person cannot update says so instead.
 */
export function blockerAvailability(
  source: BlockerSourceProvider | undefined,
  canUpdate: boolean,
): BlockerAvailability {
  if (!source) return { enabled: false, disabledReason: 'The ticket source is still loading.' };
  if (!source.capabilities.dependencies)
    return { enabled: false, disabledReason: `${source.name} tickets cannot record blocking dependencies.` };
  if (!canUpdate) return { enabled: false, disabledReason: 'This ticket cannot be edited.' };
  return { enabled: true };
}

/** One blocker row the Blocked By section shows. */
export interface BlockerRow {
  /** The native id stored in `blocked_by`; the Remove button's identity. */
  nativeId: string;
  /** The `@project/connection:native` reference the row opens. */
  reference: string;
  slug?: string;
  title?: string;
  /** `open` still blocks, `done` no longer blocks, `unavailable` did not resolve. */
  state: 'open' | 'done' | 'unavailable';
}

/** The fields of a resolved blocker ticket the section needs. */
export type BlockerSummary = Pick<TicketRow, 'slug' | 'title' | 'status'>;

/** Statuses whose ticket no longer blocks another (`ops::is_done` on the server). */
const FINISHED = new Set(['completed', 'verified']);

/** The qualified reference for a blocker in the inspected ticket's own source. */
export function blockerReference(projectId: string, connectionId: string, nativeId: string): string {
  return `@${projectId}/${connectionId}:${nativeId}`;
}

/** Project a ticket's `blocked_by` into rows, resolving each through `lookup`. */
export function blockerRows(
  ticket: Pick<TicketRow, 'blocked_by' | 'connection_id'>,
  projectId: string,
  lookup: (nativeId: string) => BlockerSummary | undefined,
): BlockerRow[] {
  return ticket.blocked_by.map((nativeId) => {
    const found = lookup(nativeId),
      reference = blockerReference(projectId, ticket.connection_id, nativeId);
    if (!found) return { nativeId, reference, state: 'unavailable' };
    return {
      nativeId,
      reference,
      slug: found.slug,
      title: found.title,
      state: FINISHED.has(found.status ?? '') ? 'done' : 'open',
    };
  });
}

/** The validation message that blocks submission, or an empty string when the blocker can be added. */
export function validateBlocker(
  source: BlockerTicket,
  target: BlockerTicket | undefined,
  existing: readonly string[],
): string {
  if (!target) return `Select the ticket that blocks ${source.slug}.`;
  if (duplicateTargetKey(target) === duplicateTargetKey(source)) return 'A ticket cannot block itself.';
  if (target.projectId !== source.projectId || target.connectionId !== source.connectionId)
    return `Only tickets from the same ticket source can block ${source.slug}.`;
  if (existing.includes(target.nativeId)) return `${target.slug} already blocks ${source.slug}.`;
  return '';
}

/** Only tickets the inspected ticket can depend on: same project and source, not itself. */
export function blockerCandidates(source: BlockerTicket, candidates: readonly BlockerTicket[]): BlockerTicket[] {
  return candidates.filter(
    (candidate) =>
      candidate.projectId === source.projectId &&
      candidate.connectionId === source.connectionId &&
      duplicateTargetKey(candidate) !== duplicateTargetKey(source),
  );
}

/** `blocked_by` with `nativeId` appended once. */
export function withBlocker(blockedBy: readonly string[], nativeId: string): string[] {
  return blockedBy.includes(nativeId) ? [...blockedBy] : [...blockedBy, nativeId];
}

/** `blocked_by` without `nativeId`. */
export function withoutBlocker(blockedBy: readonly string[], nativeId: string): string[] {
  return blockedBy.filter((item) => item !== nativeId);
}

/** The confirmation toast after a blocker is added. */
export function blockerAddedMessage(source: Pick<BlockerTicket, 'slug'>, target: Pick<BlockerTicket, 'slug'>): string {
  return `${target.slug} now blocks ${source.slug}.`;
}

/** The confirmation toast after a blocker is removed. */
export function blockerRemovedMessage(sourceSlug: string, blocker: string): string {
  return `${blocker} no longer blocks ${sourceSlug}.`;
}
