import type { CheckoutTicketCounts, TicketRow } from './api';
import { isUpNextTicket } from './ticket-views';

type ClaimState = Pick<TicketRow, 'claimed_by' | 'claim_lease_expires_at'> & Pick<Partial<TicketRow>, 'status'>;
type ClaimEtaState = ClaimState & Pick<Partial<TicketRow>, 'claim_eta_at' | 'claim_started_at'>;

export function isTicketActivelyWorkedOn(ticket: ClaimState, now = Date.now()): boolean {
  if (ticket.status && ticket.status !== 'not_started' && ticket.status !== 'started') return false;
  if (!ticket.claimed_by || !ticket.claim_lease_expires_at) return false;
  const expiry = Date.parse(ticket.claim_lease_expires_at);
  return Number.isFinite(expiry) && expiry > now;
}

export function nextActiveTicketExpiry(tickets: ClaimState[], now = Date.now()): number | undefined {
  const expiries = tickets
    .filter((ticket) => isTicketActivelyWorkedOn(ticket, now))
    .map((ticket) => Date.parse(ticket.claim_lease_expires_at!));
  return expiries.length ? Math.min(...expiries) : undefined;
}

/** Longest delay `setTimeout` honors; larger values wrap to 0 and fire immediately. */
const MAX_TIMER_DELAY_MS = 2_147_483_647;

/** Delay until a claim expiry check should wake, clamped so a far-future lease (beyond ~24.8 days) cannot wrap
 * `setTimeout` to an immediate wake-up and spin a tight refresh loop (HS2-43Z35K). A clamped wake-up must
 * re-check the expiry before treating the lease as expired. */
export function claimExpiryWakeDelay(nextExpiry: number, now = Date.now()): number {
  return Math.min(Math.max(1, nextExpiry - now + 25), MAX_TIMER_DELAY_MS);
}

export function projectTabTicketState(
  tickets: readonly TicketRow[],
  now = Date.now(),
): { upNextCount: number; activeTicketCount: number } {
  let upNextCount = 0;
  let activeTicketCount = 0;
  for (const ticket of tickets) {
    if (isUpNextTicket(ticket)) upNextCount += 1;
    if (isTicketActivelyWorkedOn(ticket, now)) activeTicketCount += 1;
  }
  return { upNextCount, activeTicketCount };
}

/** Keep an authoritative aggregate honest when known cached claims expire without a server event. */
export function applyKnownActiveTicketExpiries(
  counts: CheckoutTicketCounts,
  tickets: readonly TicketRow[],
  before: number,
  now: number,
): CheckoutTicketCounts {
  const previous = projectTabTicketState(tickets, before).activeTicketCount,
    current = projectTabTicketState(tickets, now).activeTicketCount;
  return current === previous ? counts : { ...counts, active: Math.max(0, counts.active + current - previous) };
}

/**
 * How a live claim's ETA reads (HS2-XQMDQB). Before the ETA, a determinate `percent` of the time
 * from the claim's start to its ETA plus the remaining time; once it passes, "Soon" until the
 * worker re-estimates or the claim ends. Undefined without a live claim or a valid ETA.
 */
export type ClaimEtaPresentation =
  | { kind: 'estimate'; percent: number; label: string; title: string }
  | { kind: 'overrun'; label: string; title: string };

const MINUTE = 60_000;

function approximateDuration(ms: number): string {
  const minutes = Math.round(ms / MINUTE);
  if (minutes < 1) return '<1m';
  if (minutes < 60) return `~${minutes}m`;
  const hours = Math.floor(minutes / 60),
    rest = minutes % 60;
  if (hours < 24) return rest ? `~${hours}h ${rest}m` : `~${hours}h`;
  return `~${Math.round(minutes / 1440)}d`;
}

function etaClockTime(eta: number): string {
  return new Intl.DateTimeFormat(undefined, { dateStyle: 'short', timeStyle: 'short' }).format(new Date(eta));
}

export function claimEtaPresentation(ticket: ClaimEtaState, now = Date.now()): ClaimEtaPresentation | undefined {
  if (!isTicketActivelyWorkedOn(ticket, now) || !ticket.claim_eta_at) return undefined;
  const eta = Date.parse(ticket.claim_eta_at);
  if (!Number.isFinite(eta)) return undefined;
  if (eta <= now)
    return {
      kind: 'overrun',
      label: 'Soon',
      title: `Past its estimate (${etaClockTime(eta)}) by ${approximateDuration(now - eta).replace('~', 'about ')}`,
    };
  const start = Date.parse(ticket.claim_started_at ?? ''),
    span = eta - start,
    elapsed = Number.isFinite(start) && span > 0 ? (now - start) / span : 0;
  return {
    kind: 'estimate',
    percent: Math.min(99, Math.max(0, Math.round(elapsed * 100))),
    label: `${approximateDuration(eta - now)} left`,
    title: `Estimated to finish ${etaClockTime(eta)}`,
  };
}

/** Longest gap between ETA countdown refreshes; labels are minute-granular. */
const CLAIM_ETA_TICK_MS = 30_000;

/**
 * Delay until the next local ETA countdown refresh (HS2-XQMDQB), or undefined when no live claim
 * has a future ETA. Wakes just after the nearest ETA so the row switches to "Soon" on time. A
 * local render tick only: it must never trigger network requests.
 */
export function nextClaimEtaTick(tickets: readonly ClaimEtaState[], now = Date.now()): number | undefined {
  let delay: number | undefined;
  for (const ticket of tickets) {
    if (!isTicketActivelyWorkedOn(ticket, now) || !ticket.claim_eta_at) continue;
    const eta = Date.parse(ticket.claim_eta_at);
    if (!Number.isFinite(eta) || eta <= now) continue;
    delay = Math.min(delay ?? CLAIM_ETA_TICK_MS, CLAIM_ETA_TICK_MS, eta - now + 25);
  }
  return delay;
}
