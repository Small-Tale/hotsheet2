import type { ActivityEvent } from './api';

/**
 * A ticket milestone the server recorded from a ticket write (HS2-G3HJZH; `docs/15` §15.6.1): a ticket
 * started with initial thoughts, a progress note, a question, a follow-up created, a block, a test
 * phase, or a completion. Its shown text is the event's structured summary; what is said is
 * `detail.spoken`, worded by the person's AI tool or the server's template, and live narration says it
 * only when the server's importance score selected it (`detail.speak`).
 */
export interface TicketMilestone {
  kind: string;
  spoken: string;
  speak: boolean;
}

/** The milestone an activity event carries, or undefined for an ordinary tool event. */
export function ticketMilestone(event: Pick<ActivityEvent, 'detail'>): TicketMilestone | undefined {
  const detail = event.detail;
  if (detail === null || typeof detail !== 'object' || Array.isArray(detail)) return undefined;
  const fields = detail as Record<string, unknown>;
  if (typeof fields.milestone !== 'string' || !fields.milestone) return undefined;
  if (typeof fields.spoken !== 'string' || !fields.spoken.trim()) return undefined;
  return { kind: fields.milestone, spoken: fields.spoken.trim(), speak: fields.speak === true };
}
