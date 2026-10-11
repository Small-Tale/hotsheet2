import type { Signal } from 'kerfjs';

import {
  applyKnownActiveTicketExpiries,
  claimEtaPresentation,
  claimExpiryWakeDelay,
  isTicketActivelyWorkedOn,
  nextActiveTicketExpiry,
  nextClaimEtaTick,
} from '../active-ticket-work';
import type { CheckoutTicketCounts, TicketRow as WireTicketRow } from '../api';
import type { LiveClaimNoticeProps } from '../components/active-claim';
import type { Project } from '../interactions/types';

/** Live application bindings the claim clock reads (HS2-K7SYHQ). */
export interface ClaimClockDependencies {
  projects: Signal<Project[]>;
  selectedProjectId: Signal<string>;
  ticketCountsByProject: Signal<Record<string, CheckoutTicketCounts>>;
  activeTicketCount: Signal<number>;
  projectTabClaimClock: Signal<number>;
  claimEtaClock: Signal<number>;
  projectTabTicketRows: (projectId: string) => WireTicketRow[];
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  /** Refresh one project tab after one of its leases lapsed. */
  refreshProjectTab: (project: Project) => Promise<void>;
}

/**
 * Owns the local live-claim clocks: the ETA countdown tick and the lease-expiry wake-up that
 * refreshes only the projects whose claims lapsed. Both are local timers that never poll
 * (HS2-XQMDQB); extracted from the application runtime (HS2-K7SYHQ).
 */
export function createClaimClockController(dependencies: ClaimClockDependencies) {
  const {
    projects,
    selectedProjectId,
    ticketCountsByProject,
    activeTicketCount,
    projectTabClaimClock,
    claimEtaClock,
    projectTabTicketRows,
    projectTicketCounts,
    refreshProjectTab,
  } = dependencies;
  let claimLeaseExpiryTimer: number | undefined;
  let claimEtaTimer: number | undefined;
  /** The inspector/reader header's live-claim notice, on the same local ETA clock as rows (HS2-QKNQXC). */
  function liveClaimNotice(ticket: WireTicketRow): LiveClaimNoticeProps | undefined {
    const now = claimEtaClock.value;
    if (!isTicketActivelyWorkedOn(ticket, now)) return undefined;
    return {
      agentName: ticket.worker_label || ticket.claimed_by || 'AI',
      eta: claimEtaPresentation(ticket, now),
    };
  }
  /** Re-render ETA countdowns while a live claim has a future ETA; a local timer only (HS2-XQMDQB). */
  function scheduleClaimEtaTick() {
    if (claimEtaTimer !== undefined) window.clearTimeout(claimEtaTimer);
    claimEtaTimer = undefined;
    const now = Date.now(),
      delay = nextClaimEtaTick(
        projects.value.flatMap((item) => projectTabTicketRows(item.id)),
        now,
      );
    claimEtaClock.value = now;
    if (delay === undefined) return;
    claimEtaTimer = window.setTimeout(() => {
      claimEtaTimer = undefined;
      scheduleClaimEtaTick();
    }, delay);
  }
  function scheduleClaimLeaseExpiry() {
    scheduleClaimEtaTick();
    if (claimLeaseExpiryTimer !== undefined) window.clearTimeout(claimLeaseExpiryTimer);
    claimLeaseExpiryTimer = undefined;
    const now = Date.now(),
      openProjectRows = projects.value.flatMap((item) => projectTabTicketRows(item.id));
    projectTabClaimClock.value = now;
    activeTicketCount.value = projectTicketCounts(selectedProjectId.value).active;
    const next = nextActiveTicketExpiry(openProjectRows, now);
    if (next !== undefined) {
      const expiringProjects = projects.value.filter(
        (item) => nextActiveTicketExpiry(projectTabTicketRows(item.id), now) === next,
      );
      claimLeaseExpiryTimer = window.setTimeout(
        () => {
          claimLeaseExpiryTimer = undefined;
          const expiredAt = Date.now();
          // A clamped wake-up (far-future lease) arrives before anything expired: reschedule without refreshing.
          if (expiredAt < next) {
            scheduleClaimLeaseExpiry();
            return;
          }
          const adjusted = { ...ticketCountsByProject.value };
          for (const current of expiringProjects) {
            if (!Object.hasOwn(adjusted, current.id)) continue;
            adjusted[current.id] = applyKnownActiveTicketExpiries(
              adjusted[current.id],
              projectTabTicketRows(current.id),
              now,
              expiredAt,
            );
          }
          ticketCountsByProject.value = adjusted;
          projectTabClaimClock.value = expiredAt;
          void Promise.allSettled(expiringProjects.map((current) => refreshProjectTab(current))).finally(
            scheduleClaimLeaseExpiry,
          );
        },
        claimExpiryWakeDelay(next, now),
      );
    }
  }
  return { liveClaimNotice, scheduleClaimEtaTick, scheduleClaimLeaseExpiry };
}
