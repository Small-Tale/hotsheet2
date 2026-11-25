import type { TrashEmptyResult } from './api';

const plural = (count: number) => `${count} ticket${count === 1 ? '' : 's'}`;

/** Whether some Trash survived Empty Trash, so the Trash view must be shown again (GH-5). */
export function trashEmptyLeftTickets(result: TrashEmptyResult): boolean {
  return (result.retained?.length ?? 0) > 0 || (result.unavailable?.length ?? 0) > 0;
}

/**
 * The Empty Trash toast. Tickets a provider kept are named with its reason instead of being
 * counted as removed (GH-5).
 */
export function trashEmptySummary(result: TrashEmptyResult): string {
  const parts = [`Emptied Trash — ${plural(result.purged)} permanently removed.`];
  const retained = result.retained ?? [];
  if (retained.length > 0) {
    const reasons = [...new Set(retained.map((ticket) => ticket.message))].join('; ');
    parts.push(`${plural(retained.length)} stayed in Trash: ${reasons}.`);
  }
  for (const source of result.unavailable ?? []) {
    parts.push(`Could not empty ${source.connection_id}: ${source.message}.`);
  }
  return parts.join(' ');
}
