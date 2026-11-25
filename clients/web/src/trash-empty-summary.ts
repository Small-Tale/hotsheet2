import type { TrashEmptyResult } from './api';

const plural = (count: number) => `${count} ticket${count === 1 ? '' : 's'}`;

/** At most this many kept tickets are named; the rest are counted (GH-5). */
const NAMED_TICKETS = 3;
/** The default toast lifetime, kept for a plain Empty Trash result. */
const DEFAULT_TOAST_MS = 2_500;
/** Reading time per word for a toast that explains what stayed in Trash (GH-5 visual QA). */
const READING_MS_PER_WORD = 300;
const MAX_TOAST_MS = 20_000;

/** Whether some Trash survived Empty Trash, so the Trash view must be shown again (GH-5). */
export function trashEmptyLeftTickets(result: TrashEmptyResult): boolean {
  return (result.retained?.length ?? 0) > 0 || (result.unavailable?.length ?? 0) > 0;
}

function namedTickets(slugs: string[]): string {
  const named = slugs.slice(0, NAMED_TICKETS).join(', ');
  const rest = slugs.length - NAMED_TICKETS;
  return rest > 0 ? `${named} and ${rest} more` : named;
}

/**
 * The Empty Trash toast. Tickets a provider kept are named, with its reason, instead of being
 * counted as removed (GH-5).
 */
export function trashEmptySummary(result: TrashEmptyResult): string {
  const parts = [`Emptied Trash — ${plural(result.purged)} permanently removed.`];
  const retained = result.retained ?? [];
  if (retained.length > 0) {
    const reasons = [...new Set(retained.map((ticket) => ticket.message))].join('; ');
    const names = namedTickets(retained.map((ticket) => ticket.slug));
    parts.push(`${plural(retained.length)} stayed in Trash (${names}): ${reasons}.`);
  }
  for (const source of result.unavailable ?? []) {
    parts.push(`Could not empty ${source.connection_id}: ${source.message}.`);
  }
  return parts.join(' ');
}

/**
 * How long the Empty Trash toast stays up. A result that left tickets in Trash explains why in
 * several sentences, which the default 2.5 seconds cannot fit, so it gets reading time per
 * word, capped; the toast can still be dismissed at any time (GH-5 visual QA).
 */
export function trashEmptyToastDuration(result: TrashEmptyResult): number {
  if (!trashEmptyLeftTickets(result)) return DEFAULT_TOAST_MS;
  const words = trashEmptySummary(result).split(/\s+/).filter(Boolean).length;
  return Math.min(MAX_TOAST_MS, Math.max(DEFAULT_TOAST_MS, words * READING_MS_PER_WORD));
}
