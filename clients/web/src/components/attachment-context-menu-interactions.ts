import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type ContextMenuPosition, viewportSafeContextMenuPosition } from '../context-menu-position';
import {
  ATTACHMENTS_AND_GALLERY_ACTIONS,
  ATTACHMENTS_AND_GALLERY_TARGETS,
} from '../interaction-attrs/attachments-and-gallery';
import { ATTACHMENT_CONTEXT_MENU_HEIGHT, type AttachmentContextMenuKind } from './attachment-context-menu';

export const ATTACHMENT_CONTEXT_MENU_WIDTH = 224;

/** Where an attachment menu was opened: an attachment row, the media gallery, or a Markdown reference. */
export type AttachmentContextMenuOrigin = 'row' | 'gallery' | 'reference';

export interface AttachmentContextMenuRequest extends ContextMenuPosition {
  readonly kind: AttachmentContextMenuKind;
  readonly origin: AttachmentContextMenuOrigin;
  /** The ellipsis/gallery trigger that was clicked, or the right-clicked `[data-attachment-url]` element. */
  readonly target: HTMLElement;
}

export interface AttachmentContextMenuBindings {
  /** Open the menu for `request`; the caller resolves the attachment from `request.target`. */
  readonly open: (request: AttachmentContextMenuRequest) => void;
  /** Whether a right-click may replace the native menu (false on read-only providers). */
  readonly canOpenFromPointer?: (target: HTMLElement) => boolean;
}

/** Keep the attachment menu inside the viewport when opened at `(x, y)` (HS2-1RQA1E). */
export function attachmentContextMenuPosition(
  x: number,
  y: number,
  viewportWidth = window.innerWidth,
  viewportHeight = window.innerHeight,
): ContextMenuPosition {
  return viewportSafeContextMenuPosition(x, y, viewportWidth, viewportHeight, {
    width: ATTACHMENT_CONTEXT_MENU_WIDTH,
    height: ATTACHMENT_CONTEXT_MENU_HEIGHT,
  });
}

/** The menu kind and origin a right-clicked `[data-attachment-url]` element declares. */
export function attachmentContextMenuPointerTarget(target: HTMLElement): {
  kind: AttachmentContextMenuKind;
  origin: AttachmentContextMenuOrigin;
} {
  if (target.dataset.attachmentMenuKind === 'item') return { kind: 'item', origin: 'row' };
  return {
    kind: 'host',
    origin: target.closest(ATTACHMENTS_AND_GALLERY_TARGETS.attachmentGallery.selector) ? 'gallery' : 'reference',
  };
}

/**
 * Open/position wiring shared by production and the ticket-metadata UX demo (HS2-1RQA1E): the row
 * ellipsis and gallery menu buttons anchor below their trigger's right edge; a right-click on any
 * `[data-attachment-url]` (row, gallery media, Markdown reference) opens at the pointer. Every
 * position is clamped to the viewport by `attachmentContextMenuPosition`.
 */
export function wireAttachmentContextMenuOpeners(
  root: HTMLElement,
  bindings: AttachmentContextMenuBindings,
): () => void {
  const lifetime = createScope();
  const fromTrigger = (target: Element, kind: AttachmentContextMenuKind, origin: AttachmentContextMenuOrigin) => {
    const rect = target.getBoundingClientRect();
    bindings.open({
      ...attachmentContextMenuPosition(rect.right, rect.bottom),
      kind,
      origin,
      target: target as HTMLElement,
    });
  };
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.openGalleryAttachmentMenu.selector, (_event, target) => {
      fromTrigger(target, 'host', 'gallery');
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.openAttachmentMenu.selector, (event, target) => {
      event.stopPropagation();
      fromTrigger(target, 'item', 'row');
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', '[data-attachment-url]', (event, target) => {
      const element = target as HTMLElement;
      if (!(bindings.canOpenFromPointer?.(element) ?? true)) return;
      event.preventDefault();
      const pointer = event as MouseEvent;
      bindings.open({
        ...attachmentContextMenuPosition(pointer.clientX, pointer.clientY),
        ...attachmentContextMenuPointerTarget(element),
        target: element,
      });
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
