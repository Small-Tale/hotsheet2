import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { AttachmentLabelEditor } from '../attachment-label-editing';
import { routeFilePicker } from '../file-picker-routes';
import {
  ATTACHMENTS_AND_GALLERY_ACTIONS,
  ATTACHMENTS_AND_GALLERY_TARGETS,
} from '../interaction-attrs/attachments-and-gallery';
import { TICKET_ATTACHMENTS_PICKER } from './ticket-attachments';

const ATTACHMENT_DROP_TARGET = ATTACHMENTS_AND_GALLERY_TARGETS.attachmentDropTarget.selector;
const GROUP_DROP_TARGETS = '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target]';
/** The batch heading Kerf ListHeader renders for an editable batch (HS2-K9HSXZ). */
const BATCH_TITLE = '[data-attachment-batch-title]';
const LABEL_FIELD = ATTACHMENTS_AND_GALLERY_TARGETS.attachmentBatchLabelField.selector;

/**
 * Side effects for TicketAttachments' add, drop, batch-label, purpose, and regroup gestures (HS2-1KM6MB).
 * Production passes the attachment API and the ticket's label editor; the UX demo passes fixture
 * callbacks, so both resolve the same drop highlighting, label editing focus, and drag-to-regroup.
 */
export interface TicketAttachmentActionBindings {
  /** Add picked or dropped files; `origin` is the file input or drop target they arrived on. */
  readonly addFiles: (files: FileList | File[], origin: HTMLElement) => void;
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
  const lifetime = createScope();
  // Kerf's scoped FileDropZone reports browsed files and drops anywhere on the attachments surface.
  lifetime.add(
    routeFilePicker(root, TICKET_ATTACHMENTS_PICKER, (files, selection) => {
      bindings.addFiles(files, selection.picker);
    }),
  );
  if (bindings.extraDropTargets) {
    // The inspector rail also accepts drops outside the attachments panel. A drop inside the panel
    // bubbles through the rail, but Kerf has already reported it through the scoped zone.
    const railDrop = bindings.extraDropTargets;
    lifetime.add(
      delegate(root, 'dragover', railDrop, (event) => {
        if (!(event.target as Element).closest(ATTACHMENT_DROP_TARGET)) event.preventDefault();
      }),
    );
    lifetime.add(
      delegate(root, 'drop', railDrop, (event, target) => {
        if ((event.target as Element).closest(ATTACHMENT_DROP_TARGET)) return;
        event.preventDefault();
        const files = (event as DragEvent).dataTransfer?.files;
        if (files?.length) bindings.addFiles(files, target as HTMLElement);
      }),
    );
  }
  if (bindings.changePurpose) {
    const changePurpose = bindings.changePurpose;
    lifetime.add(
      delegate(
        root,
        'change',
        ATTACHMENTS_AND_GALLERY_TARGETS.attachmentBatchPurposeField.selector,
        (_event, target) => {
          const batch = target.closest<HTMLElement>('[data-attachment-ids]'),
            ids = batch?.dataset.attachmentIds?.split(',').filter(Boolean);
          if (batch && ids?.length) changePurpose(batch, ids);
        },
      ),
    );
  }
  const { labelEditor } = bindings;
  // The in-flight write of the most recently blurred label editor, so Enter restores focus to the
  // title only after the editor closes.
  let labelFinish: Promise<boolean> | undefined;
  lifetime.add(
    delegate(root, 'input', LABEL_FIELD, (_event, target) => {
      labelEditor.input(target as HTMLInputElement);
    }),
  );
  const beginLabelEdit = (target: Element) => {
    const input = labelEditor.begin(target);
    if (!input) return;
    queueMicrotask(() => {
      input.focus();
      input.select();
    });
  };
  // The batch heading's Edit action opens the label editor; double-clicking the heading itself stays a
  // shortcut (HS2-K9HSXZ). Repeating either gesture keeps the open edit and its draft.
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.editAttachmentBatchLabel.selector, (_event, target) => {
      beginLabelEdit(target);
    }),
  );
  lifetime.add(
    delegate(root, 'dblclick', BATCH_TITLE, (_event, target) => {
      beginLabelEdit(target);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', LABEL_FIELD, (event, target) => {
      const input = target as HTMLInputElement,
        key = (event as KeyboardEvent).key;
      if (key !== 'Escape' && key !== 'Enter') return;
      const ids = input.closest<HTMLElement>('[data-attachment-ids]')?.dataset.attachmentIds;
      if (key === 'Escape') labelEditor.escape(input);
      // The editor is a Web Awesome input: blurring before it renders the restored value would let
      // its native change event write the discarded draft back (HS2-K9HSXZ).
      const rendered =
        key === 'Escape' ? (input as Partial<{ updateComplete: Promise<unknown> }>).updateComplete : undefined;
      if (rendered)
        void rendered.then(() => {
          finishWithKey(input, ids);
        });
      else finishWithKey(input, ids);
    }),
  );
  function finishWithKey(input: HTMLInputElement, ids: string | undefined) {
    input.blur();
    // The title and its Edit action replace the editor only once the blur's save settles, which can take longer
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
  }
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
  lifetime.add(
    delegate(root, 'dragstart', '[data-drag-attachment-id]', (event, target) => {
      dragged = (target as HTMLElement).dataset.dragAttachmentId;
      const transfer = (event as DragEvent).dataTransfer;
      if (transfer && dragged) {
        transfer.effectAllowed = 'move';
        transfer.setData('application/x-hotsheet-attachment', dragged);
      }
      const surface = target.closest<HTMLElement>('[data-component="ticket-attachments"]');
      if (surface) surface.dataset.draggingGroupAttachment = 'true';
    }),
  );
  lifetime.add(
    delegate(root, 'dragend', '[data-drag-attachment-id]', (_event, target) => {
      clearDrag(target.closest<HTMLElement>('[data-component="ticket-attachments"]'));
    }),
  );
  lifetime.add(
    delegate(root, 'dragover', GROUP_DROP_TARGETS, (event, target) => {
      if (!dragged) return;
      event.preventDefault();
      event.stopPropagation();
      (target as HTMLElement).dataset.dragOver = 'true';
      const transfer = (event as DragEvent).dataTransfer;
      if (transfer) transfer.dropEffect = 'move';
    }),
  );
  lifetime.add(
    delegate(root, 'dragleave', GROUP_DROP_TARGETS, (event, target) => {
      const related = (event as DragEvent).relatedTarget;
      if (related instanceof Node && target.contains(related)) return;
      delete (target as HTMLElement).dataset.dragOver;
    }),
  );
  lifetime.add(
    delegate(root, 'drop', GROUP_DROP_TARGETS, (event, target) => {
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
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
