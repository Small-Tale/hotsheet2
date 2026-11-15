import type { FollowUpEvidence } from './api';
import { type DuplicateTarget, duplicateTargetKey } from './ticket-close';

/**
 * Linking an existing ticket as a follow-up from the Inspector (HS2-MCGCPQ). The inspected
 * ticket is the source. The person picks an existing target, and the client posts both as
 * qualified `@project/connection:native` references to the idempotent
 * `POST /checkouts/{target project}/follow-up-links` route. That is the same operation
 * `hotsheet-cli link-follow-up` and MCP `hotsheet_link_follow_up` call.
 */

/** The ticket reference shape the link dialog's picker selects (shared with the close dialog). */
export type FollowUpLinkTicket = DuplicateTarget;

/** Whether the Inspector's Link Existing Ticket action is enabled for a source ticket. */
export interface FollowUpLinkAvailability {
  enabled: boolean;
  /** The truthful reason a disabled action shows; absent while enabled. */
  disabledReason?: string;
}

/** The provider (kind and display name) that owns the inspected source ticket. */
export interface FollowUpLinkSourceProvider {
  provider: string;
  name: string;
}

/**
 * Only Hot Sheet Git sources store follow-up relations. The server refuses other providers
 * with 422, so the action stays mounted but disabled with that reason (house grammar: Chrome
 * and hierarchy).
 */
export function followUpLinkAvailability(source: FollowUpLinkSourceProvider | undefined): FollowUpLinkAvailability {
  if (!source) return { enabled: false, disabledReason: 'The ticket source is still loading.' };
  if (source.provider === 'git') return { enabled: true };
  return {
    enabled: false,
    disabledReason: `${source.name} tickets cannot record follow-up links. Only Hot Sheet Git tickets can.`,
  };
}

/** The exact qualified reference the follow-up route accepts: `@project/connection:native`. */
export function followUpQualifiedReference(ticket: Pick<FollowUpLinkTicket, 'projectId' | 'qualifiedId'>): string {
  return `@${ticket.projectId}/${ticket.qualifiedId}`;
}

/** The validation message that blocks submission, or an empty string when the link can be sent. */
export function validateFollowUpLink(source: FollowUpLinkTicket, target: FollowUpLinkTicket | undefined): string {
  if (!target) return 'Select the existing ticket to link as a follow-up.';
  if (duplicateTargetKey(target) === duplicateTargetKey(source)) return 'A ticket cannot be its own follow-up.';
  return '';
}

/** The `project::connection:native` keys of a source's recorded follow-ups. */
export function followUpEvidenceKeys(followUps: readonly FollowUpEvidence[] | undefined): string[] {
  return (followUps ?? []).map((item) =>
    duplicateTargetKey({
      projectId: item.target.project_id,
      qualifiedId: `${item.target.connection_id}:${item.target.native_id}`,
    }),
  );
}

/** The selected target is already recorded, so a link changes nothing (the route is idempotent). */
export function isLinkedFollowUp(existing: readonly string[], target: FollowUpLinkTicket | undefined): boolean {
  return Boolean(target && existing.includes(duplicateTargetKey(target)));
}

/** The confirmation toast after the route accepts the link. */
export function followUpLinkedMessage(
  source: FollowUpLinkTicket,
  target: FollowUpLinkTicket,
  already: boolean,
): string {
  return already
    ? `${target.slug} was already a follow-up of ${source.slug}.`
    : `${target.slug} is now a follow-up of ${source.slug}.`;
}
