import { isRecord, parseJson } from './json-value';

/**
 * Local recovery copies of in-progress ticket text edits (HS2-RE1PS6). While the user types, the
 * focus-loss autosave keeps the latest draft here together with the field value the edit started
 * from, so a crash or reload mid-edit loses nothing and the restored edit still merges against its
 * real starting point. A successful server save or an explicit cancel clears the copy.
 */
export interface StoredTicketDraft {
  /** The field value when this editing session began: the three-way merge base. */
  base: string;
  draft: string;
  /** Epoch milliseconds of the last local persist. */
  at: number;
}

const PREFIX = 'hotsheet.ticket-draft:';

/** `attachment_label` keys an attachment batch label by its batch key (HS2-0QQHSZ). */
export type TicketDraftField = 'details' | 'title' | 'blocked_reason' | 'note' | 'attachment_label';

export function ticketDraftKey(
  projectId: string,
  qualifiedId: string,
  field: TicketDraftField,
  noteId?: string,
): string {
  return `${PREFIX}${projectId}:${qualifiedId}:${field}${noteId ? `:${noteId}` : ''}`;
}

export function saveTicketDraft(
  storage: Pick<Storage, 'setItem' | 'removeItem'>,
  key: string,
  value: { base: string; draft: string },
  now = Date.now(),
): void {
  try {
    if (value.draft === value.base) storage.removeItem(key);
    else storage.setItem(key, JSON.stringify({ ...value, at: now } satisfies StoredTicketDraft));
  } catch {
    // Storage may be unavailable (private mode, quota); the draft still lives in the editor.
  }
}

export function loadTicketDraft(storage: Pick<Storage, 'getItem'>, key: string): StoredTicketDraft | undefined {
  try {
    const raw = storage.getItem(key);
    if (!raw) return undefined;
    const parsed = parseJson(raw);
    if (!isRecord(parsed) || typeof parsed.base !== 'string' || typeof parsed.draft !== 'string') return undefined;
    return { base: parsed.base, draft: parsed.draft, at: typeof parsed.at === 'number' ? parsed.at : 0 };
  } catch {
    return undefined;
  }
}

export function clearTicketDraft(storage: Pick<Storage, 'removeItem'>, key: string): void {
  try {
    storage.removeItem(key);
  } catch {
    // Nothing to recover from.
  }
}
