import { type Signal, signal } from 'kerfjs';

import type { AttachmentMetadata } from './api';
import type { TicketAttachmentLabelConflict } from './components/ticket-attachments';
import { AUTOSAVE_DELAY_MS, createCoalescedInput, createFocusLossAutosave } from './debounced-autosave';
import {
  captureTicketDraft,
  clearTicketDraft,
  loadRestorableTicketDraft,
  loadTicketDraft,
  planTicketDraftRestore,
  prepareCommittedDraftCleanup,
  saveTicketDraft,
} from './ticket-draft-store';
import { mergedEditNotice, reconcileActiveDraft, ticketFieldLabel } from './ticket-field-reconciliation';

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

/**
 * A batch label recovery copy that overlaps a newer label and cannot be merged (HS2-9DQXGY). The copy
 * stays in storage until the user picks a version in the batch's conflict card; nothing is discarded.
 */
export interface AttachmentLabelConflict {
  /** The surface the edit was reopened on: a reader frame id, or the inspector. */
  readonly scope: string;
  /** The edited ticket's qualified id, so the card shows only on that ticket's batch. */
  readonly ticketKey?: string;
  /** The batch's grouping key (`AttachmentBatch.key`). */
  readonly key: string;
  /** The recovery copy's storage key, captured when the conflict was found. */
  readonly draftKey: string;
  readonly ids?: readonly string[];
  readonly metadata?: AttachmentMetadata;
  /** The edit-start label the copy was typed against (empty for a copy without a merge base). */
  readonly base: string;
  /** The recovery copy's text. */
  readonly mine: string;
  /** The label the provider stores now. */
  readonly theirs: string;
  /** The editable merged value the card shows; starts as `mine`. */
  readonly resolution: string;
}

/** The field-conflict card's props for the batch in `scope` and `ticketKey`, if its label conflicts. */
export function attachmentLabelConflictFor(
  conflict: AttachmentLabelConflict | undefined,
  scope: string,
  ticketKey: string | undefined,
): TicketAttachmentLabelConflict | undefined {
  if (!conflict || conflict.scope !== scope || (conflict.ticketKey ?? '') !== (ticketKey ?? '')) return undefined;
  return {
    key: conflict.key,
    resolution: conflict.resolution,
    conflict: {
      key: `attachment_label:${conflict.key}`,
      field: 'attachment_label',
      label: ticketFieldLabel('attachment_label'),
      base: conflict.base,
      mine: conflict.mine,
      theirs: conflict.theirs,
    },
  };
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
  /**
   * The unmergeable recovery copy waiting for the user's choice (HS2-9DQXGY), owned by application
   * state so the batch's conflict card survives rerenders. Defaults to a private signal.
   */
  readonly conflict?: Signal<AttachmentLabelConflict | undefined>;
  /** The edited ticket's qualified id in `scope`, scoping the conflict card to its ticket. */
  readonly ticketKey?: (scope: string) => string | undefined;
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
  captureRecovery(): void;
  /** The conflict card's merged-value field changed: coalesce it into the conflict state. */
  conflictInput(field: HTMLTextAreaElement): void;
  /** Publish a pending merged-value draft now (the field lost focus). */
  flushConflictInput(): void;
  /** Use Theirs: keep the newer label and drop the recovery copy. */
  acceptTheirs(): void;
  /** Apply Merged Value: write it once; the copy is dropped only after the write commits. */
  applyMerge(): Promise<boolean>;
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
    conflict = options.conflict ?? signal<AttachmentLabelConflict | undefined>(undefined),
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
    const copy = copyOf(edit.key),
      draft = edit.draft ?? edit.original,
      base = edit.base ?? edit.original,
      theirs = currentLabel(edit),
      next = reconcileActiveDraft(base, draft, theirs),
      // A clean merge keeps both sides; a true conflict lets the user's finished edit win.
      label = (next.kind === 'merged' ? next.draft : draft).trim();
    if (next.kind === 'adopt-remote' || label === theirs.trim()) {
      dropCopy(edit.key);
      return true;
    }
    const clearCommitted = prepareCommittedDraftCleanup(copy?.store, copy?.key, 'attachment_label', label),
      saved = await save(edit, label);
    if (saved) clearCommitted();
    return saved;
  }
  const autosave = createFocusLossAutosave(commit, {
    delay: options.delay ?? AUTOSAVE_DELAY_MS,
    prepareRecovery: (edit, previous) => {
      const key = draftKey(edit.key);
      return {
        destination: key,
        capture: () => {
          captureTicketDraft(storage(), key, {
            base: edit.base ?? edit.original,
            draft: edit.draft ?? edit.original,
            ...(key !== undefined &&
            previous?.destination === key &&
            (previous.value.draft ?? previous.value.original) !== (edit.draft ?? edit.original)
              ? { pendingIntent: { predecessor: (previous.value.draft ?? previous.value.original).trim() } }
              : {}),
          });
        },
      };
    },
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
  // The card's merged value re-renders its batch once per typing pause, never per key (HS2-9CWDSB).
  const conflictInput = createCoalescedInput<string>((resolution) => {
    const current = conflict.value;
    if (current) conflict.value = { ...current, resolution };
  });
  let applying = false;
  const settleConflict = (settled: AttachmentLabelConflict) => {
    const store = storage();
    if (store) clearTicketDraft(store, settled.draftKey);
    if (conflict.value?.draftKey === settled.draftKey) conflict.value = undefined;
  };
  return {
    begin(target) {
      const before = editing.value,
        input = beginAttachmentLabelEdit(target, editing, describe),
        edit = editing.value;
      if (!input || !edit || edit === before) return input;
      const copy = copyOf(edit.key);
      if (!copy) return input;
      // The same planner as the ticket text editors (HS2-7D3MPP, HS2-JJ3R1G): clear a redundant copy,
      // restore (three-way merged when the label changed elsewhere), or keep it for the user's choice.
      const plan = planTicketDraftRestore(loadRestorableTicketDraft(copy.store, copy.key), edit.original);
      if (plan.kind === 'none') {
        if (plan.clear) clearTicketDraft(copy.store, copy.key);
        if (conflict.value?.draftKey === copy.key) conflict.value = undefined;
        return input;
      }
      if (plan.kind === 'conflict') {
        // A true overlap keeps the copy and offers both versions in the batch's conflict card instead
        // of the editor (HS2-9DQXGY). Reopening keeps a merged value the user is already shaping.
        if (editing.value === edit) editing.value = undefined;
        const shown = conflict.value;
        if (shown?.draftKey === copy.key && shown.mine === plan.mine && shown.theirs === plan.theirs) return undefined;
        conflictInput.cancel();
        conflict.value = {
          scope: edit.scope,
          ticketKey: options.ticketKey?.(edit.scope),
          key: edit.key,
          draftKey: copy.key,
          ids: edit.ids,
          metadata: edit.metadata,
          base: plan.base,
          mine: plan.mine,
          theirs: plan.theirs,
          resolution: plan.mine,
        };
        return undefined;
      }
      if (conflict.value?.draftKey === copy.key) conflict.value = undefined;
      const restored = { draft: plan.draft, base: plan.base };
      editing.value = { ...edit, ...restored };
      input.value = restored.draft;
      autosave.schedule(editing.value);
      notify?.(
        plan.merged ? mergedEditNotice(['attachment_label'], { restored: true }) : 'Restored an unsaved label edit.',
      );
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
    captureRecovery: () => {
      autosave.captureRecovery();
    },
    conflictInput(field) {
      if (conflict.value) conflictInput.schedule(field.value);
    },
    flushConflictInput: () => {
      conflictInput.flush();
    },
    acceptTheirs() {
      conflictInput.cancel();
      const current = conflict.value;
      if (current && !applying) settleConflict(current);
    },
    async applyMerge() {
      conflictInput.flush();
      const current = conflict.value;
      if (!current || applying) return false;
      const label = current.resolution.trim();
      if (label === current.theirs.trim()) {
        settleConflict(current);
        return true;
      }
      applying = true;
      try {
        const edit: AttachmentLabelEditing = {
          scope: current.scope,
          key: current.key,
          original: current.theirs,
          ids: current.ids,
          metadata: current.metadata,
        };
        // The copy stays until the chosen label commits, so a failed write keeps both versions.
        const saved = await save(edit, label);
        if (saved) settleConflict(current);
        return saved;
      } finally {
        applying = false;
      }
    },
  };
}
