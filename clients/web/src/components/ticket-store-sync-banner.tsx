import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CloudAlert } from 'lucide';

import type { TicketStoreAiHelp, TicketStoreSyncHealth, TicketStoreSyncReport } from '../api';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { formatBytes } from './repository-size';
import { ShellBanner } from './shell-banner';

/** Offline passes in a row before being offline is a persistent failure (mirrors the server's
 * `PERSISTENT_OFFLINE_FAILURES`), so a momentary network blip never raises the strip. */
export const PERSISTENT_OFFLINE_FAILURES = 3;

type FailingStore = TicketStoreSyncReport['stores'][number];

/**
 * The project's ticket stores whose changes are not reaching their remote (HS2-ADHFKZ): a push the
 * remote refused, a conflict, an unfinished rebase, a required history recovery, or an outage that
 * has persisted. A local-only store or one waiting for its next pass is not failing.
 */
export function failingTicketStores(
  report: TicketStoreSyncReport | undefined,
  nowMs: number = Date.now(),
): FailingStore[] {
  return (report?.stores ?? []).filter(
    ({ health }) => isPublicationFailure(health) && !repairInProgress(health, nowMs),
  );
}

/** How long an AI repair Hot Sheet asked for keeps the warning back before a person is told. A CLI-started
 * session's end is not watched, so its ask stays `started` until the store publishes (HS2-D4TK0W). */
export const AI_REPAIR_GRACE_MS = 30 * 60 * 1000;

/**
 * Whether Hot Sheet is still repairing the stall on its own (HS2-D4TK0W): an AI session it started or the
 * running session it asked is working, within {@link AI_REPAIR_GRACE_MS}. The warning is for a person, so it
 * waits until that repair ended, failed, could not be asked, or ran past the grace period.
 */
export function repairInProgress(health: TicketStoreSyncHealth, nowMs: number): boolean {
  const help = health.ai_help;
  if (!help || (health.state !== 'conflict' && health.state !== 'rebase_in_progress')) return false;
  if (help.outcome !== 'started' && help.outcome !== 'session_asked') return false;
  const askedAt = Date.parse(help.asked_at);
  return Number.isFinite(askedAt) && nowMs - askedAt < AI_REPAIR_GRACE_MS;
}

function isPublicationFailure(health: TicketStoreSyncHealth): boolean {
  switch (health.state) {
    case 'rejected':
    case 'conflict':
    case 'rebase_in_progress':
    case 'history_recovery_required':
      return true;
    case 'offline':
      return health.consecutive_failures >= PERSISTENT_OFFLINE_FAILURES;
    case 'not_repository':
    case 'no_remote':
    case 'pending':
    case 'up_to_date':
    case 'synced':
      return false;
  }
}

/** One sentence saying why the store is not publishing, from the server's state contract. */
export function syncFailureSummary(health: TicketStoreSyncHealth): string {
  const oversized = health.oversized?.[0];
  switch (health.state) {
    case 'rejected':
      return oversized
        ? `The remote rejected the push: ${oversized.path} is ${formatBytes(oversized.bytes)}, over its file size limit.`
        : 'The remote rejected the push.';
    case 'conflict':
      return 'A ticket changed on both sides and could not be merged automatically.';
    case 'rebase_in_progress':
      return 'An unfinished rebase or merge in the store needs a decision before sync can continue.';
    case 'history_recovery_required':
      return "The remote's history was rewritten, so this copy must be recovered first.";
    case 'offline':
    case 'not_repository':
    case 'no_remote':
    case 'pending':
    case 'up_to_date':
    case 'synced':
      return `The remote could not be reached for ${health.consecutive_failures} sync attempts in a row.`;
  }
}

/** Whom Hot Sheet asked to repair the stall and how that went (HS2-QR1K8R); the server's next step says what follows. */
export function aiHelpSentence(help: TicketStoreAiHelp): string {
  const tool = help.tool ?? 'an AI session';
  switch (help.outcome) {
    case 'session_asked':
      return 'Hot Sheet asked the running AI session to repair it.';
    case 'started':
      return `Hot Sheet started ${tool} to repair it.`;
    case 'finished':
      return `The ${tool} session Hot Sheet started has ended.`;
    case 'failed':
      return `The ${tool} session Hot Sheet started ended with an error.`;
    case 'ticket_only':
      return 'Hot Sheet could not ask an AI to repair it.';
  }
}

function unpublishedSentence(count: number): string {
  if (count === 0) return 'Tickets are saved on this computer.';
  return `Tickets are saved on this computer, but ${count.toLocaleString('en-US')} local ${count === 1 ? 'change is' : 'changes are'} not published.`;
}

/**
 * A persistent project-level strip while any of the project's Git ticket stores is not publishing
 * (HS2-ADHFKZ). The server owns the state, reason, and next step; this names the store, says why in
 * one sentence, says that local tickets are saved but not remotely available, and repeats the
 * server's next step (which names the headless command). It has no dismiss: it clears when a sync
 * publishes again.
 */
export function TicketStoreSyncBanner({
  report,
  nowMs = Date.now(),
}: {
  report: TicketStoreSyncReport | undefined;
  nowMs?: number;
}) {
  const failing = failingTicketStores(report, nowMs);
  const first = failing.at(0);
  if (!first) return;
  const others = failing.length - 1;
  const title =
    others === 0
      ? `Ticket store ${first.label} is not publishing changes`
      : `${failing.length} ticket stores are not publishing changes`;
  const help = first.health.ai_help;
  const detail = [
    others === 0 ? syncFailureSummary(first.health) : `${first.label}: ${syncFailureSummary(first.health)}`,
    help ? aiHelpSentence(help) : undefined,
    unpublishedSentence(first.health.unpublished_commits),
    first.health.next_action,
    others > 0
      ? `${others} more ${others === 1 ? 'store needs' : 'stores need'} attention; run hotsheet-cli sync --status in each.`
      : undefined,
  ]
    .filter((part): part is string => Boolean(part))
    .join(' ');
  return (
    <ShellBanner
      component="ticket-store-sync-banner"
      tone="warning"
      title={title}
      detail={detail}
      icon={<LucideIcon icon={CloudAlert} name="cloud-alert" />}
      action={
        help?.ticket ? (
          <a
            href={`#ticket-${help.ticket}`}
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.openLinkedTicket.attrs}
            data-ticket-slug={help.ticket}
            title={`Open ${help.ticket}`}
          >
            {`Open ${help.ticket}`}
          </a>
        ) : undefined
      }
    />
  );
}
