import type { Signal } from 'kerfjs';

import type { AttachmentMetadata } from './api';
import { AUTOSAVE_DELAY_MS, createFocusLossAutosave } from './debounced-autosave';
import { clearTicketDraft, loadTicketDraft, saveTicketDraft } from './ticket-draft-store';
import { reconcileActiveDraft } from './ticket-field-reconciliation';

/**
 * The one attachment batch label being edited, held in application state so a rerender of the
 * owning surface (a live ticket update, background metadata) keeps the editor open instead of
 * morphing away a DOM-only flag (HS2-SG0AZY). The label is ticket text, so the record also carries
 * the controlled draft and the value it merges against (HS2-0QQHSZ).
 */
export interface AttachmentLabelEditing {
  /** The surface: a reader frame id, or {@link INSPECTOR_ATTACHMENT_LABEL_SCOPE}. */
  readonly scope: string;
  /** The batch's grouping key (`AttachmentBatch.key`). */
  readonly key: string;
  /** The label when editing began, restored by Escape. */
  readonly original: string;
  /** The controlled draft the editor shows; keystrokes update it. Defaults to `original`. */
  readonly draft?: string;
  /** The label the draft merges against: `original`, or a restored recovery copy's edit-start label. */
  readonly base?: string;
  /** The batch's attachment ids, the metadata write's target. */
  readonly ids?: readonly string[];
  /** The batch's other metadata (id, purpose, actor) captured when editing began. */
  readonly metadata?: AttachmentMetadata;
}

/** The scope of the sidebar inspector (and any surface outside a ticket reader frame). */
export const INSPECTOR_ATTACHMENT_LABEL_SCOPE = 'inspector';

/** The editing scope owning `target`: its ticket reader frame, else the inspector. */
export function attachmentLabelEditScope(target: Element): string {
  return (
    target.closest<HTMLElement>('[data-component="ticket-reader"]')?.dataset.readerFrameId ??
    INSPECTOR_ATTACHMENT_LABEL_SCOPE
  );
}

/** The batch key being edited in `scope`, for a surface's `editingLabelBatch` prop. */
export function attachmentLabelEditingKey(
  editing: AttachmentLabelEditing | undefined,
  scope: string,
): string | undefined {
  return editing?.scope === scope ? editing.key : undefined;
}

/** The controlled draft of the label being edited in `scope`, for a surface's `editingLabelDraft` prop. */
export function attachmentLabelEditingDraft(
  editing: AttachmentLabelEditing | undefined,
  scope: string,
): string | undefined {
  return editing?.scope === scope ? (editing.draft ?? editing.original) : undefined;
}

/**
 * Open the label editor for the batch owning `target` (its title button). Returns the editor
 * input to focus, or `undefined` when the batch cannot be relabelled. Repeating the gesture on the
 * batch already being edited keeps that edit and its draft.
 */
export function beginAttachmentLabelEdit(
  target: Element,
  editing: Signal<AttachmentLabelEditing | undefined>,
  describe?: (batch: HTMLElement) => AttachmentMetadata,
): HTMLInputElement | undefined {
  const batch = target.closest<HTMLElement>('[data-attachment-batch-key]'),
    input = batch?.querySelector<HTMLInputElement>('[name="attachment-batch-label"]'),
    key = batch?.dataset.attachmentBatchKey;
  if (!batch || !input || input.disabled || key === undefined) return undefined;
  const scope = attachmentLabelEditScope(target),
    current = editing.value;
  if (current?.scope === scope && current.key === key) return input;
  editing.value = {
    scope,
    key,
    original: input.value,
    draft: input.value,
    base: input.value,
    ids: (batch.dataset.attachmentIds ?? '').split(',').filter(Boolean),
    metadata: describe?.(batch),
  };
  return input;
}

/** Escape: put the edit-start label back into the editor before it closes. */
export function restoreAttachmentLabelEdit(
  input: HTMLInputElement,
  editing: Signal<AttachmentLabelEditing | undefined>,
): void {
  const current = editing.value;
  if (current?.scope !== attachmentLabelEditScope(input)) return;
  input.value = current.original;
  editing.value = { ...current, draft: current.original };
}

/**
 * Close the editor owning `input` (blur). A scope edits one label at a time, so this also clears
 * an edit whose batch key a rename just replaced; another surface's edit is left alone.
 */
export function endAttachmentLabelEdit(input: Element, editing: Signal<AttachmentLabelEditing | undefined>): void {
  if (editing.value?.scope === attachmentLabelEditScope(input)) editing.value = undefined;
}

type DraftStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>;

export interface AttachmentLabelEditorOptions {
  readonly editing: Signal<AttachmentLabelEditing | undefined>;
  /** The recovery-copy key for a batch of the edited ticket (project, ticket, and batch), if any. */
  readonly draftKey: (batchKey: string) => string | undefined;
  /** The label the provider currently stores for the edited batch: the merge's "theirs". */
  readonly currentLabel: (edit: AttachmentLabelEditing) => string;
  /** Captures the batch's other metadata when editing begins. */
  readonly describe?: (batch: HTMLElement) => AttachmentMetadata;
  /** The one server write for a finished edit; resolves whether it committed. */
  readonly save: (edit: AttachmentLabelEditing, label: string) => Promise<boolean>;
  readonly storage?: () => DraftStorage | undefined;
  readonly notify?: (message: string) => void;
  readonly delay?: number;
}

export interface AttachmentLabelEditor {
  /** Double-click on a batch title: open its editor, restoring an unsaved local copy. */
  begin(target: Element): HTMLInputElement | undefined;
  /** A keystroke in the editor: update the controlled draft and refresh the recovery copy. */
  input(field: HTMLInputElement): void;
  /** Escape: restore the edit-start label and drop the edit's recovery copy. */
  escape(field: HTMLInputElement): void;
  /** Focus left the editor: write the draft once, then close it. A failed write keeps it open. */
  finish(field: Element): Promise<boolean>;
  /** Page hide or an explicit save: write any unsaved draft now. */
  flush(): Promise<boolean>;
}

const defaultStorage = (): DraftStorage | undefined => (typeof localStorage === 'undefined' ? undefined : localStorage);

/**
 * The attachment batch label editor's autosave contract (HS2-0QQHSZ), shared by the production
 * inspector and reader and the UX demo. Keystrokes update the controlled draft and, after a 150 ms
 * debounce, a local recovery copy keyed by project, ticket, and batch with the label the edit
 * started from. The single server write happens when focus leaves the editor (Enter blurs it) or
 * the page hides, three-way merged against that edit-start label so the user's own typing never
 * conflicts. Reopening the batch restores an unsaved copy.
 */
export function createAttachmentLabelEditor(options: AttachmentLabelEditorOptions): AttachmentLabelEditor {
  const { editing, draftKey, currentLabel, describe, save, notify } = options,
    storage = options.storage ?? defaultStorage,
    copyOf = (batchKey: string) => {
      const store = storage(),
        key = draftKey(batchKey);
      return store && key ? { store, key } : undefined;
    },
    dropCopy = (batchKey: string, committed?: string) => {
      const copy = copyOf(batchKey);
      if (!copy) return;
      const stored = loadTicketDraft(copy.store, copy.key);
      // A newer copy typed while the write was in flight stays for the next save.
      if (committed === undefined || !stored || stored.draft.trim() === committed.trim())
        clearTicketDraft(copy.store, copy.key);
    };
  async function commit(edit: AttachmentLabelEditing): Promise<boolean> {
    const draft = edit.draft ?? edit.original,
      base = edit.base ?? edit.original,
      theirs = currentLabel(edit),
      next = reconcileActiveDraft(base, draft, theirs),
      // A clean merge keeps both sides; a true conflict lets the user's finished edit win.
      label = (next.kind === 'merged' ? next.draft : draft).trim();
    if (next.kind === 'adopt-remote' || label === theirs.trim()) {
      dropCopy(edit.key);
      return true;
    }
    const saved = await save(edit, label);
    if (saved) dropCopy(edit.key, label);
    return saved;
  }
  const autosave = createFocusLossAutosave(commit, {
    delay: options.delay ?? AUTOSAVE_DELAY_MS,
    persist: (edit) => {
      const copy = copyOf(edit.key);
      if (copy)
        saveTicketDraft(copy.store, copy.key, {
          base: edit.base ?? edit.original,
          draft: edit.draft ?? edit.original,
        });
    },
  });
  const owns = (field: Element) => editing.value?.scope === attachmentLabelEditScope(field);
  return {
    begin(target) {
      const before = editing.value,
        input = beginAttachmentLabelEdit(target, editing, describe),
        edit = editing.value;
      if (!input || !edit || edit === before) return input;
      const copy = copyOf(edit.key),
        stored = copy && loadTicketDraft(copy.store, copy.key);
      if (!copy || !stored) return input;
      if (stored.draft === stored.base || stored.draft === edit.original) {
        clearTicketDraft(copy.store, copy.key);
        return input;
      }
      const next = reconcileActiveDraft(stored.base, stored.draft, edit.original);
      if (next.kind === 'conflict') {
        clearTicketDraft(copy.store, copy.key);
        notify?.('An unsaved label edit could not be merged with a newer label and was discarded.');
        return input;
      }
      const restored =
        next.kind === 'unchanged' ? { draft: stored.draft, base: stored.base } : { draft: next.draft, base: next.base };
      editing.value = { ...edit, ...restored };
      input.value = restored.draft;
      autosave.schedule(editing.value);
      notify?.('Restored an unsaved label edit.');
      return input;
    },
    input(field) {
      const edit = editing.value;
      if (!edit || !owns(field)) return;
      editing.value = { ...edit, draft: field.value };
      autosave.schedule(editing.value);
    },
    escape(field) {
      const edit = editing.value;
      if (!edit || !owns(field)) return;
      autosave.cancel();
      dropCopy(edit.key);
      restoreAttachmentLabelEdit(field, editing);
    },
    async finish(field) {
      const edit = editing.value;
      if (!edit || !owns(field)) return true;
      const saved = await autosave.flush();
      const current = editing.value;
      if (saved && current?.scope === edit.scope && current.key === edit.key) editing.value = undefined;
      return saved;
    },
    flush: () => autosave.flush(),
  };
}
