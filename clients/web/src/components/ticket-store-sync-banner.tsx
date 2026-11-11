import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CloudAlert } from 'lucide';

import type { TicketStoreSyncHealth, TicketStoreSyncReport } from '../api';
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
export function failingTicketStores(report: TicketStoreSyncReport | undefined): FailingStore[] {
  return (report?.stores ?? []).filter(({ health }) => isPublicationFailure(health));
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
      return 'A rebase or merge was left unfinished in the store.';
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
export function TicketStoreSyncBanner({ report }: { report: TicketStoreSyncReport | undefined }) {
  const failing = failingTicketStores(report);
  const first = failing.at(0);
  if (!first) return;
  const others = failing.length - 1;
  const title =
    others === 0
      ? `Ticket store ${first.label} is not publishing changes`
      : `${failing.length} ticket stores are not publishing changes`;
  const detail = [
    others === 0 ? syncFailureSummary(first.health) : `${first.label}: ${syncFailureSummary(first.health)}`,
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
    />
  );
}
