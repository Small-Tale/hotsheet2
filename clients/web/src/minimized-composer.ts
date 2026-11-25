import { isRecord, tryParseJson } from './json-value';
import { loadMinimizedRecords, saveMinimizedRecords } from './minimized-dialogs';
import { MINIMIZED_DRAFT_LABELS } from './minimized-readers';

/**
 * The minimized new-ticket composer (HS2-3JR0ZH). A project tab has at most one composer, so it has
 * at most one minimized record. The record is only a marker with the summary the Minimized Tickets
 * control lists: the draft itself (title, details, category, Up Next, priority, Backlog, and staged
 * attachments) stays in the composer signals and the project's session (`workspace-session.ts`),
 * which already keep it across project switches and reloads.
 */
export interface MinimizedComposer {
  /** `new-ticket:<project id>`, unique across every minimized dialog kind. */
  id: string;
  /** The project tab the composer was minimized from; only that tab lists it. */
  tabProjectId: string;
  /** The draft title when it was minimized; the composer cannot change while minimized. */
  title: string;
  /** Whether closing it would discard a title, details, or staged files. */
  hasDraft: boolean;
  minimizedAt: number;
}

/** The fields that decide whether a composer holds a draft worth confirming before discarding. */
export interface ComposerDraftSnapshot {
  title: string;
  details: string;
  attachmentCount: number;
}

export const MINIMIZED_COMPOSERS_STORAGE_KEY = 'hotsheet.minimized-composers.v1';

export function minimizedComposerId(projectId: string): string {
  return `new-ticket:${projectId}`;
}

/** Whether closing this composer would lose typed text or staged files. */
export function composerHasDraft(draft: ComposerDraftSnapshot): boolean {
  return draft.title.trim().length > 0 || draft.details.trim().length > 0 || draft.attachmentCount > 0;
}

/** The menu label of a minimized composer: "New ticket: <title>", or "New ticket: Untitled". */
export function minimizedComposerLabel(title: string): string {
  return `New ticket: ${title.trim() || 'Untitled'}`;
}

export function minimizedComposerDraftLabel(entry: MinimizedComposer): string | undefined {
  return entry.hasDraft ? MINIMIZED_DRAFT_LABELS.newTicket : undefined;
}

/** Add a minimized composer, replacing an earlier record of the same tab. */
export function addMinimizedComposer(
  list: readonly MinimizedComposer[],
  entry: MinimizedComposer,
): MinimizedComposer[] {
  return [...list.filter((item) => item.tabProjectId !== entry.tabProjectId), entry];
}

/** Drop the records of these project tabs. */
export function withoutMinimizedComposers(
  list: readonly MinimizedComposer[],
  projectIds: ReadonlySet<string>,
): MinimizedComposer[] {
  return list.filter((item) => !projectIds.has(item.tabProjectId));
}

export function serializeMinimizedComposers(list: readonly MinimizedComposer[]): string {
  return JSON.stringify(
    list.map(({ id, tabProjectId, title, hasDraft, minimizedAt }) => ({
      id,
      tabProjectId,
      title,
      hasDraft,
      minimizedAt,
    })),
  );
}

/** Read the stored list, dropping malformed records and a second record of one tab. */
export function parseMinimizedComposers(raw: string | null | undefined): MinimizedComposer[] {
  if (!raw) return [];
  const parsed = tryParseJson(raw);
  if (!Array.isArray(parsed)) return [];
  const seen = new Set<string>();
  return parsed.flatMap((value): MinimizedComposer[] => {
    if (!isRecord(value) || typeof value.tabProjectId !== 'string' || !value.tabProjectId) return [];
    const tabProjectId = value.tabProjectId;
    if (seen.has(tabProjectId)) return [];
    seen.add(tabProjectId);
    return [
      {
        id: minimizedComposerId(tabProjectId),
        tabProjectId,
        title: typeof value.title === 'string' ? value.title : '',
        hasDraft: value.hasDraft === true,
        minimizedAt: typeof value.minimizedAt === 'number' ? value.minimizedAt : 0,
      },
    ];
  });
}

export function loadMinimizedComposers(storage: Pick<Storage, 'getItem'> | undefined): MinimizedComposer[] {
  return loadMinimizedRecords(storage, MINIMIZED_COMPOSERS_STORAGE_KEY, parseMinimizedComposers);
}

export function saveMinimizedComposers(
  storage: Pick<Storage, 'setItem' | 'removeItem'> | undefined,
  list: readonly MinimizedComposer[],
): void {
  saveMinimizedRecords(storage, MINIMIZED_COMPOSERS_STORAGE_KEY, list, serializeMinimizedComposers);
}
