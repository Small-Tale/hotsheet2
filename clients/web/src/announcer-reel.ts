import { announcerSpokenText } from './announcer-speech';
import type { ActivityEvent, CheckoutActivityEvent } from './api';

export interface AnnouncerReelItem {
  key: string;
  projectId: string;
  projectName: string;
  storeId: string;
  event: ActivityEvent;
  /** What the display shows: the event's own summary, rendered as Markdown (HS2-YJK0G9). */
  narration: string;
  /** What speech says: a summary that never reads a literal command (HS2-R5Q2GY). */
  spoken: string;
  change?: { path: string; operation?: string; fileCount?: number };
}

const MAX_REEL_ITEMS = 120;

function changeDetail(value: unknown): AnnouncerReelItem['change'] {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return undefined;
  const detail = value as Record<string, unknown>;
  if (typeof detail.path !== 'string' || !detail.path.trim()) return undefined;
  const operation = typeof detail.operation === 'string' ? detail.operation : undefined;
  const fileCount =
    Number.isSafeInteger(detail.file_count) && (detail.file_count as number) > 0
      ? (detail.file_count as number)
      : undefined;
  return { path: detail.path, operation, fileCount };
}

export function announcerItem(
  projectId: string,
  projectName: string,
  storeId: string,
  event: ActivityEvent,
): AnnouncerReelItem {
  return {
    key: `${storeId}:${event.id}`,
    projectId,
    projectName,
    storeId,
    event,
    narration: event.summary.trim() || `${event.tool} reported ${event.kind.replaceAll('_', ' ')}.`,
    spoken: announcerSpokenText(event),
    change: event.kind === 'edit' ? changeDetail(event.detail) : undefined,
  };
}

/** Merge retained project windows and live stream events. Shared stores keep one event, and
 * replayed stream cursors cannot repeat narration (HS2-5PSQJQ). */
export function mergeAnnouncerReel(
  current: readonly AnnouncerReelItem[],
  incoming: readonly AnnouncerReelItem[],
  limit = MAX_REEL_ITEMS,
): AnnouncerReelItem[] {
  const byKey = new Map<string, AnnouncerReelItem>();
  for (const item of [...current, ...incoming]) if (!byKey.has(item.key)) byKey.set(item.key, item);
  return [...byKey.values()]
    .sort((left, right) => left.event.ts.localeCompare(right.event.ts) || left.key.localeCompare(right.key))
    .slice(-Math.max(0, limit));
}

export function retainedAnnouncerReel(
  projects: readonly { id: string; name: string; events: readonly CheckoutActivityEvent[] }[],
): AnnouncerReelItem[] {
  return mergeAnnouncerReel(
    [],
    projects.flatMap((project) =>
      project.events.map(({ store_id, event }) => announcerItem(project.id, project.name, store_id, event)),
    ),
  );
}

/** A digest favors important events, keeps recording order, and has a deliberate upper bound. */
export function announcerDigest(reel: readonly AnnouncerReelItem[], limit = 12): AnnouncerReelItem[] {
  const selected = reel.filter((item) => item.event.importance !== 'low');
  return selected.slice(-Math.max(0, limit));
}
