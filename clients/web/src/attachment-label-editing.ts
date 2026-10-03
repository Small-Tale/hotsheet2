import type { Signal } from 'kerfjs';

/**
 * The one attachment batch label being edited, held in application state so a rerender of the
 * owning surface (a live ticket update, background metadata) keeps the editor open instead of
 * morphing away a DOM-only flag (HS2-SG0AZY).
 */
export interface AttachmentLabelEditing {
  /** The surface: a reader frame id, or {@link INSPECTOR_ATTACHMENT_LABEL_SCOPE}. */
  readonly scope: string;
  /** The batch's grouping key (`AttachmentBatch.key`). */
  readonly key: string;
  /** The label when editing began, restored by Escape. */
  readonly original: string;
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

/**
 * Open the label editor for the batch owning `target` (its title button). Returns the editor
 * input to focus, or `undefined` when the batch cannot be relabelled.
 */
export function beginAttachmentLabelEdit(
  target: Element,
  editing: Signal<AttachmentLabelEditing | undefined>,
): HTMLInputElement | undefined {
  const batch = target.closest<HTMLElement>('[data-attachment-batch-key]'),
    input = batch?.querySelector<HTMLInputElement>('[name="attachment-batch-label"]'),
    key = batch?.dataset.attachmentBatchKey;
  if (!batch || !input || input.disabled || key === undefined) return undefined;
  editing.value = { scope: attachmentLabelEditScope(target), key, original: input.value };
  return input;
}

/** Escape: put the edit-start label back into the editor before it closes. */
export function restoreAttachmentLabelEdit(
  input: HTMLInputElement,
  editing: Signal<AttachmentLabelEditing | undefined>,
): void {
  const current = editing.value;
  if (current?.scope === attachmentLabelEditScope(input)) input.value = current.original;
}

/**
 * Close the editor owning `input` (blur). A scope edits one label at a time, so this also clears
 * an edit whose batch key a rename just replaced; another surface's edit is left alone.
 */
export function endAttachmentLabelEdit(input: Element, editing: Signal<AttachmentLabelEditing | undefined>): void {
  if (editing.value?.scope === attachmentLabelEditScope(input)) editing.value = undefined;
}
