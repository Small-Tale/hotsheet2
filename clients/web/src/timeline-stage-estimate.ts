import { approximateDuration, claimEtaPresentation, isTicketActivelyWorkedOn } from './active-ticket-work';
import type { TicketRow } from './api';

type StageEstimateTicket = Pick<
  Partial<TicketRow>,
  'status' | 'started_phase' | 'started_phase_since' | 'claim_eta_at' | 'claim_started_at'
> &
  Pick<TicketRow, 'claimed_by' | 'claim_lease_expires_at'>;

/** The in-progress timeline stage's estimate: a short label and an explanatory title (HS2-ST6RN1). */
export interface TimelineStageEstimate {
  label?: string;
  title: string;
  /** Estimated time left in the stage; 0 once overrun, absent without an estimate (HS2-TE86WX). */
  remainingMs?: number;
}

/**
 * The current stage of a ticket someone is actively working on (HS2-ST6RN1). The estimate is this
 * project's median past duration of the current Started phase minus the time already spent in it,
 * "Soon" once overrun; without phase history it falls back to the live claim's ETA; with neither it
 * has no label and the stage only shows that work is in progress. Undefined unless the ticket is
 * Started with a live claim.
 */
export function timelineStageEstimate(
  ticket: StageEstimateTicket,
  typicalByPhase: Readonly<Record<string, number>> | undefined,
  now = Date.now(),
): TimelineStageEstimate | undefined {
  if (ticket.status !== 'started' || !isTicketActivelyWorkedOn(ticket, now)) return undefined;
  const phase = ticket.started_phase,
    typical = phase ? typicalByPhase?.[phase] : undefined,
    since = Date.parse(ticket.started_phase_since ?? '');
  if (typical !== undefined && Number.isFinite(typical) && typical > 0 && Number.isFinite(since)) {
    const deadline = since + typical,
      usual = approximateDuration(typical).replace('~', 'about ');
    return deadline <= now
      ? { label: 'Soon', title: `Past this phase's typical duration (${usual})`, remainingMs: 0 }
      : {
          label: `${approximateDuration(deadline - now)} left`,
          title: `This phase typically takes ${usual}`,
          remainingMs: deadline - now,
        };
  }
  const eta = claimEtaPresentation(ticket, now);
  if (eta)
    return {
      label: eta.label,
      title: eta.title,
      remainingMs: eta.kind === 'estimate' ? Math.max(0, Date.parse(ticket.claim_eta_at ?? '') - now) : 0,
    };
  return { title: 'In progress; no estimate yet' };
}

/** Each phase's median wall time from a phase report, keyed by phase id (HS2-ST6RN1). */
export function typicalPhaseDurations(report: {
  phases: readonly { phase: string; wall: { median_ms: number } }[];
}): Record<string, number> {
  return Object.fromEntries(report.phases.map((row) => [row.phase, row.wall.median_ms]));
}
