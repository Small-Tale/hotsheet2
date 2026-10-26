import { isRecord, parseJson } from './json-value';
import { type DraftReconciliation, reconcileActiveDraft } from './ticket-field-reconciliation';

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
  /** Native-close intent captured while this exact older value is in flight. */
  pendingIntent?: { predecessor: string };
}

const PREFIX = 'hotsheet.ticket-draft:';

/** `attachment_label` keys an attachment batch label by its batch key (HS2-0QQHSZ). */
export type TicketDraftField =
  'details' | 'title' | 'blocked_reason' | 'note' | 'attachment_label' | 'attachment_filename';

export function ticketDraftKey(
  projectId: string,
  qualifiedId: string,
  field: TicketDraftField,
  noteId?: string,
): string {
  return `${PREFIX}${projectId}:${qualifiedId}:${field}${noteId ? `:${noteId}` : ''}`;
}

export function saveTicketDraft(
  storage: Pick<Storage, 'setItem' | 'removeItem'> & Partial<Pick<Storage, 'getItem'>>,
  key: string,
  value: { base: string; draft: string },
  now = Date.now(),
): void {
  try {
    // Native teardown still emits ordinary pagehide. Keep an identical strict copy's
    // predecessor evidence rather than erasing it with that repeated persist (HS2-E0H7S0).
    const previous = storage.getItem ? loadTicketDraft({ getItem: storage.getItem.bind(storage) }, key) : undefined,
      pendingIntent =
        previous?.base === value.base && previous.draft === value.draft ? previous.pendingIntent : undefined;
    if (value.draft === value.base && !pendingIntent) storage.removeItem(key);
    else
      storage.setItem(
        key,
        JSON.stringify({ ...value, at: now, ...(pendingIntent ? { pendingIntent } : {}) } satisfies StoredTicketDraft),
      );
  } catch {
    // Storage may be unavailable (private mode, quota); the draft still lives in the editor.
  }
}

/** HS2-E0H7S0: controlled close cannot rely on an editor that is about to be destroyed. */
export function captureTicketDraft(
  storage: Pick<Storage, 'setItem' | 'removeItem'> | undefined,
  key: string | undefined,
  value: { base: string; draft: string; pendingIntent?: { predecessor: string } },
  now = Date.now(),
): void {
  if (!storage || !key) throw new Error('Local recovery destination is unavailable.');
  if (value.draft === value.base && !value.pendingIntent) storage.removeItem(key);
  else storage.setItem(key, JSON.stringify({ ...value, at: now } satisfies StoredTicketDraft));
}

export function loadTicketDraft(storage: Pick<Storage, 'getItem'>, key: string): StoredTicketDraft | undefined {
  try {
    const raw = storage.getItem(key);
    if (!raw) return undefined;
    const parsed = parseJson(raw);
    if (!isRecord(parsed) || typeof parsed.base !== 'string' || typeof parsed.draft !== 'string') return undefined;
    if (
      parsed.pendingIntent !== undefined &&
      (!isRecord(parsed.pendingIntent) ||
        typeof parsed.pendingIntent.predecessor !== 'string' ||
        Object.keys(parsed.pendingIntent).length !== 1)
    )
      return undefined;
    return {
      base: parsed.base,
      draft: parsed.draft,
      at: typeof parsed.at === 'number' ? parsed.at : 0,
      ...(parsed.pendingIntent !== undefined
        ? { pendingIntent: { predecessor: (parsed.pendingIntent as { predecessor: string }).predecessor } }
        : {}),
    };
  } catch {
    return undefined;
  }
}

/** Native revert intent may rebase over only its exact older submitted value, never unrelated edits. */
export function reconcileStoredTicketDraft(stored: StoredTicketDraft, current: string): DraftReconciliation {
  if (!stored.pendingIntent) return reconcileActiveDraft(stored.base, stored.draft, current);
  if (current === stored.draft) return { kind: 'converged', base: current, draft: current };
  if (current === stored.pendingIntent.predecessor) return { kind: 'unchanged', base: current, draft: stored.draft };
  return stored.draft === stored.base
    ? { kind: 'conflict', base: current, draft: stored.draft }
    : reconcileActiveDraft(stored.base, stored.draft, current);
}

export function clearTicketDraft(storage: Pick<Storage, 'removeItem'>, key: string): void {
  try {
    storage.removeItem(key);
  } catch {
    // Nothing to recover from.
  }
}

/** Capture cleanup ownership before a server await; Markdown whitespace is meaningful (HS2-E0H7S0). */
export function prepareCommittedDraftCleanup(
  storage: Pick<Storage, 'getItem' | 'removeItem'> | undefined,
  key: string | undefined,
  field: TicketDraftField,
  committed: string,
): () => void {
  return () => {
    if (!storage || !key) return;
    const stored = loadTicketDraft(storage, key);
    if (!stored) return;
    const exact = field === 'details' || field === 'note';
    if (exact ? stored.draft === committed : stored.draft.trim() === committed.trim()) clearTicketDraft(storage, key);
  };
}
