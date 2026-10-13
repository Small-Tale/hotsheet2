import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { AttachmentLabelEditor } from '../attachment-label-editing';

const ATTACHMENT_DROP_TARGET = '[data-attachment-drop-target="true"]';
const GROUP_DROP_TARGETS = '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target]';
const LABEL_FIELD = '[name="attachment-batch-label"]';

/**
 * Side effects for TicketAttachments' add, drop, batch-label, purpose, and regroup gestures (HS2-1KM6MB).
 * Production passes the attachment API and the ticket's label editor; the UX demo passes fixture
 * callbacks, so both resolve the same drop highlighting, label editing focus, and drag-to-regroup.
 */
export interface TicketAttachmentActionBindings {
  /** Add picked or dropped files; `origin` is the file input or drop target they arrived on. */
  readonly addFiles: (files: FileList, origin: HTMLElement) => void;
  /** Extra selectors that accept a file drop without carrying the drop highlight. */
  readonly extraDropTargets?: string;
  /** The batch-label autosave editor shared by the inspector, reader, and demo. */
  readonly labelEditor: AttachmentLabelEditor;
  /** A batch's purpose select changed. */
  readonly changePurpose?: (batch: HTMLElement, ids: string[]) => void;
  /**
   * Move a dragged attachment from its `source` batch into `destination`, or into a new group when
   * `destination` is undefined.
   */
  readonly regroup: (id: string, source: HTMLElement | undefined, destination: HTMLElement | undefined) => void;
}

/** Wire the attachment gestures of every TicketAttachments surface and drop target under `root`. */
export function wireTicketAttachmentActions(root: HTMLElement, bindings: TicketAttachmentActionBindings): () => void {
  const lifetime = createScope(),
    on = (event: string, selector: string, handle: (event: Event, target: Element) => void) => {
      lifetime.add(delegate(root, event, selector, handle));
    },
    dropTargets = bindings.extraDropTargets
      ? `${ATTACHMENT_DROP_TARGET}, ${bindings.extraDropTargets}`
      : ATTACHMENT_DROP_TARGET;
  on('change', 'input[name="ticket-attachments"]', (_event, target) => {
    const input = target as HTMLInputElement;
    if (input.files?.length) bindings.addFiles(input.files, input);
    input.value = '';
  });
  on('dragover', dropTargets, (event, target) => {
    event.preventDefault();
    if (target.matches(ATTACHMENT_DROP_TARGET)) (target as HTMLElement).dataset.draggingAttachment = 'true';
  });
  on('dragleave', ATTACHMENT_DROP_TARGET, (_event, target) => {
    delete (target as HTMLElement).dataset.draggingAttachment;
  });
  on('drop', dropTargets, (event, target) => {
    event.preventDefault();
    const element = target as HTMLElement,
      files = (event as DragEvent).dataTransfer?.files;
    delete element.dataset.draggingAttachment;
    if (files?.length) bindings.addFiles(files, element);
  });
  if (bindings.changePurpose) {
    const changePurpose = bindings.changePurpose;
    on('change', '[name="attachment-batch-purpose"]', (_event, target) => {
      const batch = target.closest<HTMLElement>('[data-attachment-ids]'),
        ids = batch?.dataset.attachmentIds?.split(',').filter(Boolean);
      if (batch && ids?.length) changePurpose(batch, ids);
    });
  }
  const { labelEditor } = bindings;
  // The in-flight write of the most recently blurred label editor, so Enter restores focus to the
  // title only after the editor closes.
  let labelFinish: Promise<boolean> | undefined;
  on('input', LABEL_FIELD, (_event, target) => {
    labelEditor.input(target as HTMLInputElement);
  });
  on('dblclick', '[data-action="edit-attachment-batch-label"]', (_event, target) => {
    const input = labelEditor.begin(target);
    if (!input) return;
    queueMicrotask(() => {
      input.focus();
      input.select();
    });
  });
  on('keydown', LABEL_FIELD, (event, target) => {
    const input = target as HTMLInputElement,
      key = (event as KeyboardEvent).key;
    if (key !== 'Escape' && key !== 'Enter') return;
    const ids = input.closest<HTMLElement>('[data-attachment-ids]')?.dataset.attachmentIds;
    if (key === 'Escape') labelEditor.escape(input);
    input.blur();
    // The title button replaces the editor only once the blur's save settles, which can take longer
    // than a couple of frames under load; restore focus after that, not on a frame count (HS2-VRBDPV).
    const settled = labelFinish ?? Promise.resolve(true);
    const restore = () =>
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (ids)
            root
              .querySelector<HTMLElement>(
                `[data-component="ticket-attachments"] [data-attachment-ids="${CSS.escape(ids)}"] [data-action="edit-attachment-batch-label"]`,
              )
              ?.focus();
        }),
      );
    settled.then(restore, restore);
  });
  lifetime.add(
    delegateCapture(root, 'blur', LABEL_FIELD, (_event, target) => {
      const pending = labelEditor.finish(target);
      labelFinish = pending;
      const clear = () => {
        if (labelFinish === pending) labelFinish = undefined;
      };
      pending.then(clear, clear);
    }),
  );
  let dragged: string | undefined;
  const clearDrag = (surface: HTMLElement | null) => {
    dragged = undefined;
    if (!surface) return;
    delete surface.dataset.draggingGroupAttachment;
    for (const target of surface.querySelectorAll<HTMLElement>('[data-drag-over]')) delete target.dataset.dragOver;
  };
  on('dragstart', '[data-drag-attachment-id]', (event, target) => {
    dragged = (target as HTMLElement).dataset.dragAttachmentId;
    const transfer = (event as DragEvent).dataTransfer;
    if (transfer && dragged) {
      transfer.effectAllowed = 'move';
      transfer.setData('application/x-hotsheet-attachment', dragged);
    }
    const surface = target.closest<HTMLElement>('[data-component="ticket-attachments"]');
    if (surface) surface.dataset.draggingGroupAttachment = 'true';
  });
  on('dragend', '[data-drag-attachment-id]', (_event, target) => {
    clearDrag(target.closest<HTMLElement>('[data-component="ticket-attachments"]'));
  });
  on('dragover', GROUP_DROP_TARGETS, (event, target) => {
    if (!dragged) return;
    event.preventDefault();
    event.stopPropagation();
    (target as HTMLElement).dataset.dragOver = 'true';
    const transfer = (event as DragEvent).dataTransfer;
    if (transfer) transfer.dropEffect = 'move';
  });
  on('dragleave', GROUP_DROP_TARGETS, (event, target) => {
    const related = (event as DragEvent).relatedTarget;
    if (related instanceof Node && target.contains(related)) return;
    delete (target as HTMLElement).dataset.dragOver;
  });
  on('drop', GROUP_DROP_TARGETS, (event, target) => {
    if (!dragged) return;
    event.preventDefault();
    event.stopPropagation();
    const id = dragged,
      surface = target.closest<HTMLElement>('[data-component="ticket-attachments"]'),
      source =
        surface
          ?.querySelector<HTMLElement>(`[data-drag-attachment-id="${CSS.escape(id)}"]`)
          ?.closest<HTMLElement>('[data-attachment-ids]') ?? undefined,
      newGroup = target.matches('[data-attachment-new-group-drop-target]'),
      destination = target.closest<HTMLElement>('[data-attachment-group-drop-target]') ?? undefined;
    clearDrag(surface);
    if (newGroup) bindings.regroup(id, source, undefined);
    else if (destination) bindings.regroup(id, source, destination);
  });
  return () => {
    lifetime.dispose();
  };
}
