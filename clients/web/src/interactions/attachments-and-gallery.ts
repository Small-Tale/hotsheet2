import { delegate, delegateCapture, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { annotationDefaultIntent } from '../annotation-intents';
import {
  type Api,
  type AttachmentMetadata,
  type AttachmentPurpose,
  type FullTicket,
  type MediaAnnotation,
  type TicketRow,
} from '../api';
import { type AttachmentLabelEditing, createAttachmentLabelEditor } from '../attachment-label-editing';
import { browserRandomId } from '../browser-id';
import { ATTACHMENT_CONTEXT_MENU_HEIGHT, type AttachmentContextMenuKind } from '../components/attachment-context-menu';
import {
  attachmentGalleryAnnotationVisible,
  attachmentGalleryDefaultRange,
  type AttachmentGalleryGeometry,
  type AttachmentGalleryImage,
  attachmentGalleryImageIndex,
  attachmentGalleryKeyboardAction,
  attachmentGallerySelectionUrl,
  attachmentGallerySwipeDirection,
  type AttachmentGallerySwipeGesture,
  attachmentGallerySwipeGesture,
  attachmentGalleryZoomModel,
} from '../components/attachment-gallery';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { copyText } from '../copy-text';
import {
  clampAnnotationCoordinate,
  drawGalleryAnnotation,
  type GalleryAnnotationTool,
  galleryGestureLargeEnough,
  moveGalleryArrowVertex,
  pickGalleryAnnotation,
  resizeGalleryAnnotation,
  translateGalleryAnnotation,
} from '../gallery-annotation-editor';
import { galleryCropFromDrag } from '../gallery-crop';
import {
  ATTACHMENTS_AND_GALLERY_ACTIONS,
  ATTACHMENTS_AND_GALLERY_TARGETS,
} from '../interaction-attrs/attachments-and-gallery';
import { ticketDraftKey } from '../ticket-draft-store';
import { data } from './dom';
import { type AttachmentMenu, type GallerySource, type Project } from './types';

/** Live application bindings used by this handler group. */
const ATTACHMENT_DROP_TARGET = '[data-attachment-drop-target="true"]';
/** The shell's right rail while its panel shows the ticket inspector (not the terminal rail's list). */
export const INSPECTOR_RAIL_DROP_TARGET =
  '#app-right-rail:has(> .kui-workbench__panel-content > .kui-pane > .kui-pane__header [data-component="ticket-inspector-header"])';

export interface AttachmentAndGalleryInteractionsDependencies {
  readonly selectedTicket: Signal<FullTicket | null>;
  /**
   * Whether the selected ticket's provider can act on existing attachments (`attachment_edit`).
   * The attachment menu's open/reveal/rename/remove actions are store operations, so an
   * append-only provider keeps the browser's own context menu instead (HS2-HSA64D).
   */
  readonly canEditAttachments?: () => boolean;
  readonly tickets: Signal<TicketRow[]>;
  readonly addAttachments: (slug: string, files: FileList | File[]) => Promise<void>;
  readonly project: () => Project | undefined;
  readonly api: () => Api;
  readonly attachmentMessage: Signal<string>;
  /** The open batch label editor, owned by application state (HS2-SG0AZY). */
  readonly attachmentLabelEditing: Signal<AttachmentLabelEditing | undefined>;
  readonly showToast: (message: string) => void;
  readonly refreshProject: ({ showLoading }?: { showLoading?: boolean }) => Promise<void>;
  draggedGroupedAttachmentId: string | undefined;
  readonly galleryImages: (ticket?: FullTicket | null, project?: Project) => AttachmentGalleryImage[];
  readonly resetAttachmentGallery: (url?: string, source?: GallerySource) => void;
  /** The stacked reader's ticket and project owning a gallery opener, if it is not the workspace reader (HS2-97E0QR). */
  readonly gallerySourceFor: (target: Element) => GallerySource | undefined;
  readonly shiftGallery: (delta: number) => void;
  readonly attachmentGalleryGeometry: Signal<AttachmentGalleryGeometry>;
  readonly attachmentGalleryScale: Signal<number | undefined>;
  readonly attachmentGalleryUrl: Signal<string | undefined>;
  readonly attachmentMenu: Signal<AttachmentMenu | undefined>;
  readonly syncAttachmentGalleryMeasurement: () => void;
  readonly activeAttachmentGalleryVideo: (target?: EventTarget | null) => HTMLVideoElement | undefined;
  readonly attachmentGalleryDuration: Signal<number>;
  readonly error: Signal<string>;
  readonly attachmentGalleryMarkup: Signal<boolean>;
  readonly attachmentGallerySaveState?: Signal<'idle' | 'saving' | 'failed'>;
  readonly attachmentGalleryCropMode: Signal<boolean>;
  readonly attachmentGalleryCrop: Signal<FullTicket['attachments'][number]['crop']>;
  readonly attachmentGalleryOriginalSize: Signal<{ width: number; height: number }>;
  readonly beginGalleryCrop: () => Promise<void>;
  readonly restoreGalleryCrop: () => Promise<void>;
  readonly finishGalleryCrop: () => Promise<void>;
  readonly finishGalleryAnnotationSession: () => void;
  readonly discardGalleryAnnotationSession?: () => void;
  readonly beginGalleryAnnotationSession: () => void;
  readonly attachmentGalleryDrawMode: Signal<boolean>;
  readonly attachmentGalleryTool: Signal<GalleryAnnotationTool>;
  readonly attachmentGallerySelectedAnnotation: Signal<string | undefined>;
  readonly attachmentGalleryAnnotations: Signal<MediaAnnotation[]>;
  readonly updateGalleryPlaybackPresentation: (milliseconds: number) => void;
  readonly attachmentGalleryPlayhead: Signal<number>;
  attachmentRangeGesture:
    | { pointerId: number; annotationId: string; endpoint: 'start' | 'end'; track: DOMRect; before: MediaAnnotation[] }
    | undefined;
  attachmentAnnotationGesture:
    | {
        kind: 'draw' | 'move' | 'resize';
        pointerId: number;
        startX: number;
        startY: number;
        surface: DOMRect;
        annotation: MediaAnnotation;
        handle?: string;
        tool: GalleryAnnotationTool;
        samples: { x: number; y: number }[];
        startPoint: { x: number; y: number };
        before: MediaAnnotation[];
      }
    | undefined;
  attachmentGalleryLivePlayhead: number;
  readonly attachmentGalleryPlaying: Signal<boolean>;
  attachmentGallerySvgPreviousFrame: number | undefined;
  attachmentGallerySvgFrame: number | undefined;
  readonly gallerySvgClock: (timestamp: number) => void;
  readonly stopGallerySvgClock: () => void;
  readonly attachmentGalleryVolumeOpen: Signal<boolean>;
  attachmentGalleryLiveVolume: number;
  readonly attachmentGalleryMuted: Signal<boolean>;
  readonly attachmentGalleryVolume: Signal<number>;
  attachmentSwipeGesture: AttachmentGallerySwipeGesture | undefined;
  readonly canUseAttachments: () => boolean;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireAttachmentAndGalleryInteractions(dependencies: AttachmentAndGalleryInteractionsDependencies) {
  const lifetime = createScope();
  const {
    selectedTicket,
    tickets,
    addAttachments,
    project,
    api,
    attachmentMessage,
    attachmentLabelEditing,
    showToast,
    refreshProject,
    galleryImages,
    resetAttachmentGallery,
    gallerySourceFor,
    shiftGallery,
    attachmentGalleryGeometry,
    attachmentGalleryScale,
    attachmentGalleryUrl,
    attachmentMenu,
    syncAttachmentGalleryMeasurement,
    activeAttachmentGalleryVideo,
    attachmentGalleryDuration,
    error,
    attachmentGalleryMarkup,
    attachmentGallerySaveState,
    attachmentGalleryCropMode,
    attachmentGalleryCrop,
    attachmentGalleryOriginalSize,
    beginGalleryCrop,
    restoreGalleryCrop,
    finishGalleryCrop,
    finishGalleryAnnotationSession,
    discardGalleryAnnotationSession,
    beginGalleryAnnotationSession,
    attachmentGalleryDrawMode,
    attachmentGalleryTool,
    attachmentGallerySelectedAnnotation,
    attachmentGalleryAnnotations,
    updateGalleryPlaybackPresentation,
    attachmentGalleryPlayhead,
    attachmentGalleryPlaying,
    gallerySvgClock,
    stopGallerySvgClock,
    attachmentGalleryVolumeOpen,
    attachmentGalleryMuted,
    attachmentGalleryVolume,
    canUseAttachments,
  } = dependencies;
  const routedTicketId = (slug: string) =>
    (selectedTicket.value?.slug === slug ? selectedTicket.value.qualified_id : undefined) ??
    tickets.value.find((ticket) => ticket.slug === slug)?.qualified_id ??
    slug;
  lifetime.add(
    delegate(document.body, 'change', 'input[name="ticket-attachments"]', (_event, target) => {
      const input = target as HTMLInputElement,
        slug = input.closest<HTMLElement>('[data-ticket-slug]')?.dataset.ticketSlug ?? selectedTicket.value?.slug;
      if (slug && input.files?.length) void addAttachments(slug, input.files);
      input.value = '';
    }),
  );
  // The shell's right rail is a Workbench panel whose toolbar and padding Kerf owns (HS2-QQW6CT), so a
  // drop anywhere on the rail while it shows a ticket inspector attaches to that ticket; the app-owned
  // header and body wrappers carry the highlight.
  const dropTargets = `${ATTACHMENT_DROP_TARGET}, ${INSPECTOR_RAIL_DROP_TARGET}`;
  lifetime.add(
    delegate(document.body, 'dragover', dropTargets, (event, target) => {
      event.preventDefault();
      if ((target as HTMLElement).matches(ATTACHMENT_DROP_TARGET))
        (target as HTMLElement).dataset.draggingAttachment = 'true';
    }),
  );
  lifetime.add(
    delegate(document.body, 'dragleave', ATTACHMENT_DROP_TARGET, (_event, target) => {
      delete (target as HTMLElement).dataset.draggingAttachment;
    }),
  );
  lifetime.add(
    delegate(document.body, 'drop', dropTargets, (event, target) => {
      event.preventDefault();
      const element = target as HTMLElement;
      delete element.dataset.draggingAttachment;
      const slug = element.closest<HTMLElement>('[data-ticket-slug]')?.dataset.ticketSlug ?? selectedTicket.value?.slug,
        files = (event as DragEvent).dataTransfer?.files;
      if (slug && files?.length) void addAttachments(slug, files);
    }),
  );
  async function openSelectedAttachment(id: string) {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket) return;
    await api().checkoutAttachmentAction(current.id, ticket.qualified_id, id, 'open');
  }
  function metadataForBatch(
    batch: HTMLElement,
    batch_id = batch.dataset.attachmentBatch || browserRandomId(),
  ): AttachmentMetadata {
    const role = batch.dataset.attachmentActorRole as 'human' | 'ai' | 'system' | 'unknown' | undefined;
    return {
      batch_id,
      batch_label: batch.querySelector<HTMLInputElement>('[name="attachment-batch-label"]')?.value.trim() || undefined,
      purpose: (batch.querySelector<HTMLSelectElement>('[name="attachment-batch-purpose"]')?.value || undefined) as
        AttachmentPurpose | undefined,
      actor: role
        ? {
            role,
            identity: batch.dataset.attachmentActorIdentity || undefined,
            display_name: batch.dataset.attachmentActorName || undefined,
          }
        : undefined,
    };
  }
  async function persistAttachmentMetadata(ids: readonly string[], metadata: AttachmentMetadata): Promise<boolean> {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket || !ids.length) return false;
    attachmentMessage.value = 'Updating attachment group…';
    try {
      const result = await api().updateCheckoutAttachmentMetadata(current.id, ticket.qualified_id, [...ids], metadata);
      selectedTicket.value = result.ticket;
      attachmentMessage.value = '';
      showToast('Attachment group updated.');
      await refreshProject();
      return true;
    } catch (reason) {
      attachmentMessage.value = `Update failed: ${reason instanceof Error ? reason.message : String(reason)}`;
      return false;
    }
  }
  // The batch label is ticket text (HS2-0QQHSZ): a controlled draft with a local recovery copy keyed by
  // project, ticket, and batch, written once when focus leaves the editor or the page hides.
  const labelEditor = createAttachmentLabelEditor({
    editing: attachmentLabelEditing,
    draftKey: (batchKey) => {
      const current = project(),
        ticket = selectedTicket.value;
      return current && ticket
        ? ticketDraftKey(current.id, ticket.qualified_id, 'attachment_label', batchKey)
        : undefined;
    },
    currentLabel: (edit) =>
      selectedTicket.value?.attachments.find((item) => edit.ids?.includes(item.id))?.batch_label ?? '',
    describe: (batch) => metadataForBatch(batch),
    save: (edit, label) =>
      persistAttachmentMetadata(edit.ids ?? [], { ...edit.metadata, batch_label: label || undefined }),
    notify: showToast,
  });
  // The in-flight write of the most recently blurred label editor, so Enter can restore focus to the
  // title only after the editor closes.
  let labelFinish: Promise<boolean> | undefined;
  const flushLabelOnHide = () => {
    void labelEditor.flush();
  };
  document.defaultView?.addEventListener('pagehide', flushLabelOnHide, { signal: lifetime.signal });
  document.addEventListener(
    'visibilitychange',
    () => {
      if (document.visibilityState === 'hidden') flushLabelOnHide();
    },
    { signal: lifetime.signal },
  );
  lifetime.add(
    delegate(
      document.body,
      'change',
      ATTACHMENTS_AND_GALLERY_TARGETS.attachmentBatchPurposeField.selector,
      (_event, target) => {
        const batch = target.closest<HTMLElement>('[data-attachment-ids]'),
          ids = batch?.dataset.attachmentIds?.split(',').filter(Boolean);
        if (batch && ids?.length) void persistAttachmentMetadata(ids, metadataForBatch(batch));
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'input',
      ATTACHMENTS_AND_GALLERY_TARGETS.attachmentBatchLabelField.selector,
      (_event, target) => {
        labelEditor.input(target as HTMLInputElement);
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'dblclick',
      ATTACHMENTS_AND_GALLERY_ACTIONS.editAttachmentBatchLabel.selector,
      (_event, target) => {
        const input = labelEditor.begin(target);
        if (!input) return;
        queueMicrotask(() => {
          input.focus();
          input.select();
        });
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'keydown',
      ATTACHMENTS_AND_GALLERY_TARGETS.attachmentBatchLabelField.selector,
      (event, target) => {
        const input = target as HTMLInputElement,
          key = (event as KeyboardEvent).key;
        if (key !== 'Escape' && key !== 'Enter') return;
        const ids = input.closest<HTMLElement>('[data-attachment-ids]')?.dataset.attachmentIds;
        if (key === 'Escape') labelEditor.escape(input);
        input.blur();
        // The title button replaces the editor only once the blur's save settles, which can take
        // longer than a couple of frames under load; restore focus after that, not on a frame count
        // (HS2-VRBDPV).
        const settled = labelFinish ?? Promise.resolve(true);
        const restore = () =>
          requestAnimationFrame(() =>
            requestAnimationFrame(() => {
              if (ids)
                document.body
                  .querySelector<HTMLElement>(
                    `[data-component="ticket-attachments"] [data-attachment-ids="${CSS.escape(ids)}"] [data-action="edit-attachment-batch-label"]`,
                  )
                  ?.focus();
            }),
          );
        settled.then(restore, restore);
      },
    ),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'blur',
      ATTACHMENTS_AND_GALLERY_TARGETS.attachmentBatchLabelField.selector,
      (_event, target) => {
        const pending = labelEditor.finish(target);
        labelFinish = pending;
        const clear = () => {
          if (labelFinish === pending) labelFinish = undefined;
        };
        pending.then(clear, clear);
      },
    ),
  );
  function clearGroupedAttachmentDrag(surface?: HTMLElement) {
    dependencies.draggedGroupedAttachmentId = undefined;
    if (surface) {
      delete surface.dataset.draggingGroupAttachment;
      for (const target of surface.querySelectorAll<HTMLElement>('[data-drag-over]')) delete target.dataset.dragOver;
    }
  }
  lifetime.add(
    delegate(document.body, 'dragstart', '[data-drag-attachment-id]', (event, target) => {
      dependencies.draggedGroupedAttachmentId = data(target).dragAttachmentId;
      const transfer = (event as DragEvent).dataTransfer;
      if (transfer && dependencies.draggedGroupedAttachmentId) {
        transfer.effectAllowed = 'move';
        transfer.setData('application/x-hotsheet-attachment', dependencies.draggedGroupedAttachmentId);
      }
      const surface = target.closest<HTMLElement>('[data-component="ticket-attachments"]');
      if (surface) surface.dataset.draggingGroupAttachment = 'true';
    }),
  );
  lifetime.add(
    delegate(document.body, 'dragend', '[data-drag-attachment-id]', (_event, target) => {
      clearGroupedAttachmentDrag(target.closest<HTMLElement>('[data-component="ticket-attachments"]') ?? undefined);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'dragover',
      '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target]',
      (event, target) => {
        if (!dependencies.draggedGroupedAttachmentId) return;
        event.preventDefault();
        event.stopPropagation();
        (target as HTMLElement).dataset.dragOver = 'true';
        const transfer = (event as DragEvent).dataTransfer;
        if (transfer) transfer.dropEffect = 'move';
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'dragleave',
      '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target]',
      (event, target) => {
        const related = (event as DragEvent).relatedTarget;
        if (related instanceof Node && target.contains(related)) return;
        delete (target as HTMLElement).dataset.dragOver;
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'drop',
      '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target]',
      (event, target) => {
        if (!dependencies.draggedGroupedAttachmentId) return;
        event.preventDefault();
        event.stopPropagation();
        const id = dependencies.draggedGroupedAttachmentId,
          surface = target.closest<HTMLElement>('[data-component="ticket-attachments"]'),
          source = surface
            ?.querySelector<HTMLElement>(`[data-drag-attachment-id="${CSS.escape(id)}"]`)
            ?.closest<HTMLElement>('[data-attachment-ids]'),
          destination = target.closest<HTMLElement>('[data-attachment-group-drop-target]'),
          newGroup = target.matches('[data-attachment-new-group-drop-target]');
        clearGroupedAttachmentDrag(surface ?? undefined);
        if (!source) return;
        if (newGroup) {
          const role = source.dataset.attachmentActorRole as 'human' | 'ai' | 'system' | 'unknown' | undefined;
          void persistAttachmentMetadata([id], {
            batch_id: browserRandomId(),
            batch_label: 'New group',
            actor: role
              ? {
                  role,
                  identity: source.dataset.attachmentActorIdentity || undefined,
                  display_name: source.dataset.attachmentActorName || undefined,
                }
              : undefined,
          });
          return;
        }
        if (destination && destination !== source) void persistAttachmentMetadata([id], metadataForBatch(destination));
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', ATTACHMENTS_AND_GALLERY_ACTIONS.openAttachmentRow.selector, (event, target) => {
      if ((event.target as Element).closest('button, input, a')) return;
      void openSelectedAttachment(data(target).attachmentActionId!);
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      ATTACHMENTS_AND_GALLERY_ACTIONS.openAttachmentGallery.selector,
      (_event, target) => {
        // Media in a stacked reader belongs to that reader's ticket, not the workspace selection.
        const source = gallerySourceFor(target),
          selection = data(target),
          url = attachmentGallerySelectionUrl(galleryImages(source?.ticket, source?.project), {
            url: selection.attachmentUrl,
            ticket: selection.attachmentTicket,
            name: selection.attachmentName,
            attachmentId: selection.galleryAttachmentId,
          });
        if (url) resetAttachmentGallery(url, source);
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.closeAttachmentGallery.selector, () => {
      resetAttachmentGallery();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.previousGalleryImage.selector, () => {
      shiftGallery(-1);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.nextGalleryImage.selector, () => {
      shiftGallery(1);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.zoomGalleryImage.selector, (_event, target) => {
      const model = attachmentGalleryZoomModel(attachmentGalleryGeometry.value, attachmentGalleryScale.value),
        direction = data(target).zoomDirection === 'out' ? -1 : 1,
        next = model.stops[model.index + direction];
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      if (next !== undefined) attachmentGalleryScale.value = next;
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      ATTACHMENTS_AND_GALLERY_ACTIONS.openGalleryAttachmentMenu.selector,
      (_event, target) => {
        const active = attachmentGalleryUrl.value,
          images = galleryImages(),
          index = active ? attachmentGalleryImageIndex(images, active) : -1,
          image = index >= 0 ? images[index] : undefined;
        if (!image?.ticket) return;
        const rect = target.getBoundingClientRect(),
          position = viewportSafeContextMenuPosition(rect.right, rect.bottom, window.innerWidth, window.innerHeight, {
            width: 224,
            height: ATTACHMENT_CONTEXT_MENU_HEIGHT,
          });
        attachmentMenu.value = {
          ...position,
          ticket: image.ticket,
          name: image.name,
          url: image.url,
          id: image.attachmentId,
          kind: 'host',
        };
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.openAttachmentMenu.selector, (event, target) => {
      event.stopPropagation();
      const item = target.closest<HTMLElement>('[data-component="ticket-attachment-item"]'),
        ticket = selectedTicket.value,
        name = item?.dataset.attachmentName,
        url = item?.dataset.attachmentUrl,
        id = item?.dataset.attachmentActionId;
      if (!ticket || !name || !url || !id) return;
      const rect = target.getBoundingClientRect(),
        position = viewportSafeContextMenuPosition(rect.right, rect.bottom, window.innerWidth, window.innerHeight, {
          width: 224,
          height: ATTACHMENT_CONTEXT_MENU_HEIGHT,
        });
      attachmentMenu.value = {
        ...position,
        ticket: ticket.slug,
        name,
        url,
        id,
        kind: 'item',
        reader: target.closest<HTMLElement>('[data-component="ticket-reader"]')?.dataset.readerFrameId,
      };
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'load', ATTACHMENTS_AND_GALLERY_TARGETS.galleryImage.selector, () => {
      syncAttachmentGalleryMeasurement();
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'loadedmetadata',
      ATTACHMENTS_AND_GALLERY_TARGETS.galleryMedia.selector,
      (event, target) => {
        syncAttachmentGalleryMeasurement();
        if (target instanceof HTMLVideoElement && activeAttachmentGalleryVideo(event.target))
          attachmentGalleryDuration.value = Math.round(target.duration * 1000);
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      ATTACHMENTS_AND_GALLERY_ACTIONS.openReferencedAttachment.selector,
      (event, target) => {
        event.preventDefault();
        const current = project(),
          ticket = data(target).attachmentTicket,
          name = data(target).attachmentName;
        if (current && ticket && name)
          void api()
            .checkoutAttachmentByNameAction(current.id, routedTicketId(ticket), name, 'open')
            .catch((reason: unknown) => {
              error.value = reason instanceof Error ? reason.message : String(reason);
            });
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'contextmenu', '[data-attachment-url]', (event, target) => {
      if (!(dependencies.canEditAttachments?.() ?? true)) return;
      event.preventDefault();
      const ticket = data(target).attachmentTicket ?? selectedTicket.value?.slug,
        name = data(target).attachmentName,
        url = data(target).attachmentUrl,
        pointer = event as MouseEvent,
        kind: AttachmentContextMenuKind = data(target).attachmentMenuKind === 'item' ? 'item' : 'host',
        id = data(target).attachmentActionId ?? data(target).galleryAttachmentId;
      if (ticket && name && url)
        attachmentMenu.value = {
          ...viewportSafeContextMenuPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight, {
            width: 224,
            height: ATTACHMENT_CONTEXT_MENU_HEIGHT,
          }),
          ticket,
          name,
          url,
          id,
          kind,
          reader:
            (target as HTMLElement).closest('[data-component="ticket-reader"]')?.getAttribute('data-reader-frame-id') ??
            undefined,
        };
    }),
  );
  async function attachmentMenuHostAction(menu: AttachmentMenu, action: 'open' | 'reveal' | 'path') {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket) return undefined;
    return menu.id && menu.ticket === ticket.slug
      ? api().checkoutAttachmentAction(current.id, ticket.qualified_id, menu.id, action)
      : api().checkoutAttachmentByNameAction(current.id, routedTicketId(menu.ticket), menu.name, action);
  }
  lifetime.add(
    delegate(
      document.body,
      'click',
      ATTACHMENTS_AND_GALLERY_ACTIONS.attachmentMenuAction.selector,
      (_event, target) => {
        const menu = attachmentMenu.value,
          action = data(target).itemId;
        if (!menu) return;
        attachmentMenu.value = undefined;
        if (action === 'download') {
          const link = document.createElement('a');
          link.href = menu.url;
          link.download = menu.name;
          link.click();
          return;
        }
        if (action === 'copy-reference') {
          const local = menu.ticket === selectedTicket.value?.slug;
          void copyText(`attachment:${local ? '' : `[${menu.ticket}]`}${menu.name}`)
            .then(() => {
              showToast('Attachment reference copied to clipboard.');
            })
            .catch((reason: unknown) => {
              error.value = `Copy failed: ${reason instanceof Error ? reason.message : String(reason)}`;
            });
          return;
        }
        if (action === 'copy-path') {
          void attachmentMenuHostAction(menu, 'path')
            .then(
              (result) =>
                result &&
                copyText(result.path).then(() => {
                  showToast('Attachment path copied to clipboard.');
                }),
            )
            .catch((reason: unknown) => {
              error.value = reason instanceof Error ? reason.message : String(reason);
            });
          return;
        }
        if (action === 'rename') {
          const current = project(),
            ticket = selectedTicket.value,
            filename = window.prompt('Attachment filename', menu.name);
          if (current && ticket && menu.id && filename?.trim())
            void api()
              .renameCheckoutAttachment(current.id, ticket.qualified_id, menu.id, filename.trim())
              .then((result) => {
                selectedTicket.value = result.ticket;
                showToast('Attachment renamed.');
                return refreshProject();
              })
              .catch((reason: unknown) => {
                error.value = reason instanceof Error ? reason.message : String(reason);
              });
          return;
        }
        if (action === 'remove') {
          if (menu.kind === 'host') resetAttachmentGallery();
          void removeSelectedAttachment(menu.id);
          return;
        }
        if (action === 'open' || action === 'reveal')
          void attachmentMenuHostAction(menu, action)
            .then(() => {
              showToast(action === 'open' ? 'Opened attachment.' : 'Opened attachment location.');
            })
            .catch((reason: unknown) => {
              error.value = reason instanceof Error ? reason.message : String(reason);
            });
      },
    ),
  );
  const snapshotAnnotations = () => structuredClone(attachmentGalleryAnnotations.value),
    snapshotMarkup = () => ({
      annotations: snapshotAnnotations(),
      crop: attachmentGalleryCrop.value,
      cropMode: attachmentGalleryCropMode.value,
    }),
    undoAnnotations: ReturnType<typeof snapshotMarkup>[] = [],
    redoAnnotations: ReturnType<typeof snapshotMarkup>[] = [];
  let editGroup: string | undefined;
  function recordAnnotationChange(before: MediaAnnotation[], group?: string) {
    if (JSON.stringify(before) === JSON.stringify(attachmentGalleryAnnotations.value)) return;
    if (!group || editGroup !== group) undoAnnotations.push({ ...snapshotMarkup(), annotations: before });
    redoAnnotations.length = 0;
    editGroup = group;
  }
  function recordCropChange(before: FullTicket['attachments'][number]['crop']) {
    if (JSON.stringify(before) === JSON.stringify(attachmentGalleryCrop.value)) return;
    undoAnnotations.push({ ...snapshotMarkup(), crop: before, cropMode: true });
    redoAnnotations.length = 0;
    editGroup = undefined;
  }
  function setGalleryTool(tool: GalleryAnnotationTool) {
    attachmentGalleryTool.value = tool;
    attachmentGalleryDrawMode.value = tool !== 'select';
    editGroup = undefined;
  }
  function annotationPoint(event: PointerEvent, surface: DOMRect) {
    return {
      x: clampAnnotationCoordinate(((event.clientX - surface.left) * 10_000) / surface.width),
      y: clampAnnotationCoordinate(((event.clientY - surface.top) * 10_000) / surface.height),
    };
  }
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryMarkup.selector, () => {
      if (attachmentGallerySaveState?.value === 'saving') return;
      if (attachmentGalleryMarkup.value) {
        attachmentGalleryMarkup.value = false;
        finishGalleryAnnotationSession();
      } else {
        beginGalleryAnnotationSession();
        attachmentGalleryMarkup.value = true;
        undoAnnotations.length = 0;
        redoAnnotations.length = 0;
      }
      setGalleryTool('select');
      attachmentGallerySelectedAnnotation.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.retryGallerySave.selector, () => {
      finishGalleryAnnotationSession();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.discardGallerySave.selector, () => {
      discardGalleryAnnotationSession?.();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryCrop.selector, () => {
      if (attachmentGalleryCropMode.value) void finishGalleryCrop();
      else void beginGalleryCrop();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.restoreGalleryCrop.selector, () => {
      const before = attachmentGalleryCrop.value;
      void restoreGalleryCrop().then(() => {
        recordCropChange(before);
      });
    }),
  );
  let cropGesture:
    | {
        pointerId: number;
        start: { x: number; y: number };
        before: FullTicket['attachments'][number]['crop'];
        surface: DOMRect;
      }
    | undefined;
  lifetime.add(
    delegateCapture(
      document.body,
      'pointerdown',
      ATTACHMENTS_AND_GALLERY_TARGETS.galleryCropSurface.selector,
      (event, target) => {
        if (!attachmentGalleryMarkup.value || !attachmentGalleryCropMode.value) return;
        const pointer = event as PointerEvent,
          surface = target.getBoundingClientRect();
        if (!surface.width || !surface.height) return;
        event.preventDefault();
        event.stopPropagation();
        event.stopImmediatePropagation();
        const start = annotationPoint(pointer, surface);
        cropGesture = { pointerId: pointer.pointerId, start, before: attachmentGalleryCrop.value, surface };
      },
    ),
  );
  document.addEventListener(
    'pointermove',
    (event) => {
      if (!cropGesture || event.pointerId !== cropGesture.pointerId) return;
      event.preventDefault();
      const { width, height } = attachmentGalleryOriginalSize.value;
      attachmentGalleryCrop.value = galleryCropFromDrag(
        cropGesture.start,
        annotationPoint(event, cropGesture.surface),
        width,
        height,
      );
    },
    { signal: lifetime.signal },
  );
  document.addEventListener(
    'pointerup',
    (event) => {
      if (!cropGesture || event.pointerId !== cropGesture.pointerId) return;
      event.preventDefault();
      const { width, height } = attachmentGalleryOriginalSize.value;
      attachmentGalleryCrop.value =
        galleryCropFromDrag(cropGesture.start, annotationPoint(event, cropGesture.surface), width, height) ??
        cropGesture.before;
      recordCropChange(cropGesture.before);
      cropGesture = undefined;
    },
    { signal: lifetime.signal },
  );
  document.addEventListener(
    'pointercancel',
    (event) => {
      if (!cropGesture || event.pointerId !== cropGesture.pointerId) return;
      attachmentGalleryCrop.value = cropGesture.before;
      cropGesture = undefined;
    },
    { signal: lifetime.signal },
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryDraw.selector, () => {
      setGalleryTool(attachmentGalleryTool.value === 'rect' ? 'select' : 'rect');
      attachmentGallerySelectedAnnotation.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.selectGalleryTool.selector, (_event, target) => {
      const tool = data(target).tool;
      if (tool === 'select' || tool === 'freehand' || tool === 'arrow' || tool === 'insertion' || tool === 'strike') {
        setGalleryTool(tool);
        attachmentGallerySelectedAnnotation.value = undefined;
      }
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      ATTACHMENTS_AND_GALLERY_ACTIONS.selectGalleryAnnotation.selector,
      (event, target) => {
        event.stopPropagation();
        attachmentGallerySelectedAnnotation.value = data(target).annotationId;
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'dblclick',
      ATTACHMENTS_AND_GALLERY_ACTIONS.editGalleryAnnotation.selector,
      (event, target) => {
        event.stopPropagation();
        const id = data(target).annotationId;
        attachmentGallerySelectedAnnotation.value = id;
        requestAnimationFrame(() =>
          document.querySelector<HTMLTextAreaElement>('.attachment-gallery__editor textarea')?.focus(),
        );
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'input', ATTACHMENTS_AND_GALLERY_ACTIONS.editGalleryNote.selector, (event) => {
      const id = attachmentGallerySelectedAnnotation.value,
        field = event.target as HTMLTextAreaElement;
      if (!id) return;
      const before = snapshotAnnotations();
      attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.map((item) =>
        item.id === id ? { ...item, text: field.value } : item,
      );
      recordAnnotationChange(before, `note:${id}`);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryIntent.selector, (_event, target) => {
      const id = attachmentGallerySelectedAnnotation.value,
        intent = data(target).intent,
        order = ['comment', 'bug', 'change', 'insert', 'remove', 'move', 'question'];
      if (!id || !intent || !order.includes(intent)) return;
      const before = snapshotAnnotations();
      attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.map((item) => {
        if (item.id !== id) return item;
        const fallback = annotationDefaultIntent(item),
          selected = new Set(item.intents?.length ? item.intents : [fallback]);
        if (selected.has(intent)) selected.delete(intent);
        else selected.add(intent);
        const intents = order.filter((candidate) => selected.has(candidate));
        return {
          ...item,
          intents: intents.length === 0 || (intents.length === 1 && intents[0] === fallback) ? [] : intents,
        };
      });
      recordAnnotationChange(before);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryClosed.selector, (event) => {
      const id = attachmentGallerySelectedAnnotation.value,
        checked = (event.target as HTMLInputElement).checked;
      if (!id) return;
      const before = snapshotAnnotations();
      attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.map((item) =>
        item.id === id && item.shape?.type === 'freehand'
          ? { ...item, shape: { ...item.shape, closed: checked } }
          : item,
      );
      recordAnnotationChange(before);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.deleteGalleryAnnotation.selector, () => {
      const id = attachmentGallerySelectedAnnotation.value;
      if (!id || !window.confirm('Delete this annotation?')) return;
      const before = snapshotAnnotations();
      attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.filter((item) => item.id !== id);
      attachmentGallerySelectedAnnotation.value = undefined;
      recordAnnotationChange(before);
    }),
  );
  function setGalleryPlayhead(milliseconds: number, commit = true) {
    const next = Math.max(0, Math.min(attachmentGalleryDuration.value, Math.round(milliseconds))),
      video = activeAttachmentGalleryVideo();
    updateGalleryPlaybackPresentation(next);
    if (commit) attachmentGalleryPlayhead.value = next;
    if (video) video.currentTime = next / 1000;
  }
  function setGalleryAnnotationEndpoint(annotationId: string, endpoint: 'start' | 'end', milliseconds: number) {
    attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.map((item) => {
      if (item.id !== annotationId) return item;
      const next = Math.max(0, Math.min(attachmentGalleryDuration.value, Math.round(milliseconds))),
        start = item.start_ms ?? next,
        end = item.end_ms ?? start;
      return endpoint === 'start'
        ? { ...item, start_ms: Math.min(next, end) }
        : { ...item, end_ms: Math.max(start, next) };
    });
  }
  lifetime.add(
    delegate(
      document.body,
      'click',
      ATTACHMENTS_AND_GALLERY_ACTIONS.seekGalleryAnnotation.selector,
      (_event, target) => {
        const annotationId = data(target).annotationId,
          milliseconds = Number(data(target).annotationTime);
        if (!annotationId || !Number.isFinite(milliseconds)) return;
        setGalleryPlayhead(milliseconds);
        if (attachmentGalleryMarkup.value) attachmentGallerySelectedAnnotation.value = annotationId;
      },
    ),
  );
  lifetime.add(
    delegateCapture(document.body, 'pointerdown', '[data-gallery-range-handle]', (event, target) => {
      const pointer = event as PointerEvent,
        track = target.closest<HTMLElement>('.attachment-gallery__timeline-track')?.getBoundingClientRect(),
        annotationId = data(target).annotationId,
        endpoint = data(target).galleryRangeHandle;
      if (
        !track ||
        !annotationId ||
        annotationId !== attachmentGallerySelectedAnnotation.value ||
        (endpoint !== 'start' && endpoint !== 'end')
      )
        return;
      event.preventDefault();
      event.stopPropagation();
      dependencies.attachmentRangeGesture = {
        pointerId: pointer.pointerId,
        annotationId,
        endpoint,
        track,
        before: snapshotAnnotations(),
      };
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', '[data-gallery-range-handle]', (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'ArrowLeft' && keyboard.key !== 'ArrowRight') return;
      event.preventDefault();
      const annotationId = data(target).annotationId,
        endpoint = data(target).galleryRangeHandle,
        annotation = attachmentGalleryAnnotations.value.find((item) => item.id === annotationId);
      if (
        !annotation ||
        annotation.id !== attachmentGallerySelectedAnnotation.value ||
        (endpoint !== 'start' && endpoint !== 'end')
      )
        return;
      const current = endpoint === 'start' ? annotation.start_ms : annotation.end_ms;
      const before = snapshotAnnotations();
      setGalleryAnnotationEndpoint(
        annotation.id,
        endpoint,
        (current ?? 0) + (keyboard.key === 'ArrowLeft' ? -100 : 100),
      );
      recordAnnotationChange(before, `range:${annotation.id}:${endpoint}`);
    }),
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'pointerdown',
      ATTACHMENTS_AND_GALLERY_TARGETS.galleryAnnotationSurface.selector,
      (event, target) => {
        if (!attachmentGalleryMarkup.value) return;
        const pointer = event as PointerEvent,
          surface = target.getBoundingClientRect(),
          point = annotationPoint(pointer, surface),
          handleElement = (pointer.target as Element).closest<HTMLElement>('[data-annotation-handle]'),
          handle = handleElement?.dataset.annotationHandle,
          annotation = handle
            ? attachmentGalleryAnnotations.value.find((item) => item.id === attachmentGallerySelectedAnnotation.value)
            : pickGalleryAnnotation(
                attachmentGalleryAnnotations.value.filter((item) =>
                  attachmentGalleryAnnotationVisible(
                    item,
                    dependencies.attachmentGalleryLivePlayhead,
                    attachmentGalleryDuration.value,
                  ),
                ),
                point,
                (7 * 10_000) / surface.width,
                (7 * 10_000) / surface.height,
              );
        if (annotation && attachmentGalleryTool.value === 'select') {
          event.preventDefault();
          event.stopPropagation();
          attachmentGallerySelectedAnnotation.value = annotation.id;
          dependencies.attachmentAnnotationGesture = {
            kind: handle ? 'resize' : 'move',
            pointerId: pointer.pointerId,
            startX: pointer.clientX,
            startY: pointer.clientY,
            surface,
            annotation: { ...annotation },
            handle,
            tool: 'select',
            samples: [],
            startPoint: point,
            before: snapshotAnnotations(),
          };
          return;
        }
        attachmentGallerySelectedAnnotation.value = undefined;
        const tool = attachmentGalleryTool.value;
        if (tool === 'select') return;
        event.preventDefault();
        event.stopPropagation();
        const timed = attachmentGalleryDuration.value > 0,
          base: MediaAnnotation = {
            id: browserRandomId(),
            x: point.x,
            y: point.y,
            width: 1,
            height: 1,
            text: '',
            ...(timed
              ? attachmentGalleryDefaultRange(
                  dependencies.attachmentGalleryLivePlayhead,
                  attachmentGalleryDuration.value,
                )
              : {}),
          },
          created = drawGalleryAnnotation(base, tool, point, point);
        const before = snapshotAnnotations();
        attachmentGalleryAnnotations.value = [...attachmentGalleryAnnotations.value, created];
        attachmentGallerySelectedAnnotation.value = created.id;
        dependencies.attachmentAnnotationGesture = {
          kind: 'draw',
          pointerId: pointer.pointerId,
          startX: pointer.clientX,
          startY: pointer.clientY,
          surface,
          annotation: created,
          tool,
          samples: [],
          startPoint: point,
          before,
        };
      },
    ),
  );
  document.addEventListener(
    'pointermove',
    (event) => {
      const gesture = dependencies.attachmentAnnotationGesture;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      event.preventDefault();
      const dx = ((event.clientX - gesture.startX) * 10_000) / gesture.surface.width,
        dy = ((event.clientY - gesture.startY) * 10_000) / gesture.surface.height,
        base = gesture.annotation;
      let next = { ...base };
      if (gesture.kind === 'draw') {
        const point = annotationPoint(event, gesture.surface);
        if (gesture.tool === 'select') return;
        next = drawGalleryAnnotation(base, gesture.tool, gesture.startPoint, point, gesture.samples, gesture.surface);
        if (gesture.tool === 'freehand') gesture.samples.push(point);
      } else if (gesture.kind === 'move') {
        next = translateGalleryAnnotation(base, dx, dy);
      } else {
        const handle = gesture.handle ?? '',
          vertex = handle.startsWith('point-') ? Number(handle.slice(6)) : -1;
        next =
          vertex >= 0
            ? moveGalleryArrowVertex(base, vertex, annotationPoint(event, gesture.surface))
            : resizeGalleryAnnotation(
                base,
                handle as 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w',
                dx,
                dy,
                (6 * 10_000) / gesture.surface.width,
                (6 * 10_000) / gesture.surface.height,
              );
      }
      attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.map((item) =>
        item.id === base.id ? next : item,
      );
    },
    { signal: lifetime.signal },
  );
  document.addEventListener(
    'pointermove',
    (event) => {
      const gesture = dependencies.attachmentRangeGesture;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      event.preventDefault();
      setGalleryAnnotationEndpoint(
        gesture.annotationId,
        gesture.endpoint,
        ((event.clientX - gesture.track.left) * attachmentGalleryDuration.value) / gesture.track.width,
      );
    },
    { signal: lifetime.signal },
  );
  document.addEventListener(
    'pointerup',
    (event) => {
      const gesture = dependencies.attachmentAnnotationGesture;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      dependencies.attachmentAnnotationGesture = undefined;
      event.preventDefault();
      const annotation = attachmentGalleryAnnotations.value.find((item) => item.id === gesture.annotation.id);
      if (!annotation) return;
      if (gesture.kind === 'draw') {
        const points =
          annotation.shape?.type === 'freehand' || annotation.shape?.type === 'arrow'
            ? annotation.shape.points
            : [
                { x: annotation.x, y: annotation.y },
                { x: annotation.x + annotation.width, y: annotation.y + annotation.height },
              ];
        if (
          gesture.tool === 'select' ||
          !galleryGestureLargeEnough(
            gesture.tool,
            points,
            gesture.surface.width / 10_000,
            gesture.surface.height / 10_000,
          )
        ) {
          attachmentGalleryAnnotations.value = gesture.before;
          attachmentGallerySelectedAnnotation.value = undefined;
          return;
        }
        setGalleryTool('select');
        requestAnimationFrame(() =>
          document.querySelector<HTMLTextAreaElement>('.attachment-gallery__editor textarea')?.focus(),
        );
      }
      recordAnnotationChange(gesture.before);
    },
    { signal: lifetime.signal },
  );
  document.addEventListener(
    'pointercancel',
    (event) => {
      const gesture = dependencies.attachmentAnnotationGesture;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      attachmentGalleryAnnotations.value = gesture.before;
      dependencies.attachmentAnnotationGesture = undefined;
      attachmentGallerySelectedAnnotation.value = undefined;
    },
    { signal: lifetime.signal },
  );
  function insertDefaultGalleryAnnotation(tool: Exclude<GalleryAnnotationTool, 'select'>) {
    const surface = document.querySelector<HTMLElement>('[data-gallery-annotation-surface="true"]');
    if (!surface) return;
    const rect = surface.getBoundingClientRect(),
      stage = surface.closest<HTMLElement>('[data-gallery-zoom-stage="true"]')?.getBoundingClientRect(),
      centerX = stage
        ? Math.max(
            rect.left,
            Math.min(rect.right, (Math.max(rect.left, stage.left) + Math.min(rect.right, stage.right)) / 2),
          )
        : rect.left + rect.width / 2,
      centerY = stage
        ? Math.max(
            rect.top,
            Math.min(rect.bottom, (Math.max(rect.top, stage.top) + Math.min(rect.bottom, stage.bottom)) / 2),
          )
        : rect.top + rect.height / 2,
      center = {
        x: clampAnnotationCoordinate(((centerX - rect.left) * 10_000) / rect.width),
        y: clampAnnotationCoordinate(((centerY - rect.top) * 10_000) / rect.height),
      },
      size = Math.min(rect.width, rect.height) / 5,
      radiusX = (size * 5_000) / rect.width,
      radiusY = (size * 5_000) / rect.height,
      start = { x: clampAnnotationCoordinate(center.x - radiusX), y: clampAnnotationCoordinate(center.y - radiusY) },
      end = { x: clampAnnotationCoordinate(center.x + radiusX), y: clampAnnotationCoordinate(center.y + radiusY) },
      base: MediaAnnotation = {
        id: browserRandomId(),
        x: center.x,
        y: center.y,
        width: 1,
        height: 1,
        text: '',
        ...(attachmentGalleryDuration.value > 0
          ? attachmentGalleryDefaultRange(dependencies.attachmentGalleryLivePlayhead, attachmentGalleryDuration.value)
          : {}),
      },
      samples =
        tool === 'freehand'
          ? Array.from({ length: 10 }, (_, index) => ({
              x: clampAnnotationCoordinate(center.x + radiusX * Math.cos(((index + 1) * Math.PI * 2) / 12)),
              y: clampAnnotationCoordinate(center.y + radiusY * Math.sin(((index + 1) * Math.PI * 2) / 12)),
            }))
          : [],
      before = snapshotAnnotations(),
      created =
        tool === 'insertion'
          ? drawGalleryAnnotation(base, tool, center, center)
          : tool === 'arrow'
            ? drawGalleryAnnotation(base, tool, { x: start.x, y: end.y }, { x: end.x, y: start.y })
            : tool === 'freehand'
              ? drawGalleryAnnotation(
                  base,
                  tool,
                  { x: center.x + radiusX, y: center.y },
                  { x: center.x + radiusX, y: center.y },
                  samples,
                )
              : drawGalleryAnnotation(base, tool, start, end);
    attachmentGalleryAnnotations.value = [...attachmentGalleryAnnotations.value, created];
    attachmentGallerySelectedAnnotation.value = created.id;
    recordAnnotationChange(before);
    setGalleryTool('select');
  }
  document.addEventListener(
    'keydown',
    (event) => {
      if (!attachmentGalleryMarkup.value || !attachmentGalleryUrl.value || event.defaultPrevented) return;
      const target = event.target as HTMLElement;
      if (target.closest('input, textarea, [contenteditable="true"], [data-gallery-range-handle]')) return;
      const key = event.key.toLowerCase(),
        selectedId = attachmentGallerySelectedAnnotation.value,
        selected = attachmentGalleryAnnotations.value.find((item) => item.id === selectedId),
        meta = event.metaKey || event.ctrlKey;
      let handled = true;
      if (meta && key === 'z') {
        const source = event.shiftKey ? redoAnnotations : undoAnnotations,
          destination = event.shiftKey ? undoAnnotations : redoAnnotations,
          previous = source.pop();
        if (previous) {
          const previewFromCrop = attachmentGalleryCropMode.value && !previous.cropMode;
          destination.push(snapshotMarkup());
          attachmentGalleryCrop.value = previous.crop;
          if (previewFromCrop)
            void finishGalleryCrop().then(() => {
              if (!attachmentGalleryCropMode.value) attachmentGalleryAnnotations.value = previous.annotations;
            });
          else {
            attachmentGalleryAnnotations.value = previous.annotations;
            attachmentGalleryCropMode.value = previous.cropMode;
          }
          if (selectedId && !previous.annotations.some((item) => item.id === selectedId))
            attachmentGallerySelectedAnnotation.value = undefined;
          editGroup = undefined;
        }
      } else if (meta && key === 'd' && selected) {
        const before = snapshotAnnotations(),
          duplicate = translateGalleryAnnotation({ ...selected, id: browserRandomId() }, 200, 200);
        attachmentGalleryAnnotations.value = [...attachmentGalleryAnnotations.value, duplicate];
        attachmentGallerySelectedAnnotation.value = duplicate.id;
        recordAnnotationChange(before);
      } else if (!meta && !event.altKey && key in { v: 1, r: 1, f: 1, a: 1, i: 1, s: 1 }) {
        setGalleryTool(
          ({ v: 'select', r: 'rect', f: 'freehand', a: 'arrow', i: 'insertion', s: 'strike' } as const)[
            key as 'v' | 'r' | 'f' | 'a' | 'i' | 's'
          ],
        );
      } else if (key === 'escape') {
        const gesture = dependencies.attachmentAnnotationGesture;
        if (gesture) {
          attachmentGalleryAnnotations.value = gesture.before;
          dependencies.attachmentAnnotationGesture = undefined;
        } else if (attachmentGalleryTool.value !== 'select') setGalleryTool('select');
        else if (selectedId) attachmentGallerySelectedAnnotation.value = undefined;
        else handled = false;
      } else if (key === 'tab') {
        const visible = attachmentGalleryAnnotations.value.filter((item) =>
          attachmentGalleryAnnotationVisible(
            item,
            dependencies.attachmentGalleryLivePlayhead,
            attachmentGalleryDuration.value,
          ),
        );
        const inCanvas = Boolean(target.closest('[data-gallery-zoom-stage="true"]')),
          focusedAnnotation = Boolean(target.closest('.attachment-gallery__annotation'));
        if (visible.length && inCanvas && (selectedId || !focusedAnnotation)) {
          const index = visible.findIndex((item) => item.id === selectedId),
            next = (index + (event.shiftKey ? -1 : 1) + visible.length) % visible.length;
          attachmentGallerySelectedAnnotation.value = visible[next].id;
          document
            .querySelector<HTMLElement>(
              `[data-action="select-gallery-annotation"][data-annotation-id="${visible[next].id}"]`,
            )
            ?.focus();
        } else handled = false;
      } else if (key === 'enter') {
        if (attachmentGalleryTool.value !== 'select') insertDefaultGalleryAnnotation(attachmentGalleryTool.value);
        else if (selected) document.querySelector<HTMLTextAreaElement>('.attachment-gallery__editor textarea')?.focus();
        else handled = false;
      } else if ((key === 'delete' || key === 'backspace') && selected) {
        const before = snapshotAnnotations();
        attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.filter(
          (item) => item.id !== selected.id,
        );
        attachmentGallerySelectedAnnotation.value = undefined;
        recordAnnotationChange(before);
      } else if (key.startsWith('arrow') && selected) {
        const distance = event.shiftKey ? 10 : 1,
          surface = document
            .querySelector<HTMLElement>('[data-gallery-annotation-surface="true"]')
            ?.getBoundingClientRect();
        if (surface) {
          const dx =
              key === 'arrowleft'
                ? (-distance * 10_000) / surface.width
                : key === 'arrowright'
                  ? (distance * 10_000) / surface.width
                  : 0,
            dy =
              key === 'arrowup'
                ? (-distance * 10_000) / surface.height
                : key === 'arrowdown'
                  ? (distance * 10_000) / surface.height
                  : 0,
            before = snapshotAnnotations();
          attachmentGalleryAnnotations.value = attachmentGalleryAnnotations.value.map((item) =>
            item.id === selected.id ? translateGalleryAnnotation(item, dx, dy) : item,
          );
          recordAnnotationChange(before, `nudge:${selected.id}`);
        }
      } else handled = false;
      if (handled) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    { capture: true, signal: lifetime.signal },
  );
  document.addEventListener(
    'pointerup',
    (event) => {
      const gesture = dependencies.attachmentRangeGesture;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      dependencies.attachmentRangeGesture = undefined;
      event.preventDefault();
      recordAnnotationChange(gesture.before);
    },
    { signal: lifetime.signal },
  );
  document.addEventListener(
    'pointercancel',
    (event) => {
      const gesture = dependencies.attachmentRangeGesture;
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      attachmentGalleryAnnotations.value = gesture.before;
      dependencies.attachmentRangeGesture = undefined;
    },
    { signal: lifetime.signal },
  );
  function toggleGalleryPlayback() {
    const video = document.querySelector<HTMLVideoElement>('.attachment-gallery video');
    if (video) {
      if (video.paused) void video.play();
      else video.pause();
      return;
    }
    attachmentGalleryPlaying.value = !attachmentGalleryPlaying.value;
    if (attachmentGalleryPlaying.value) {
      dependencies.attachmentGallerySvgPreviousFrame = undefined;
      dependencies.attachmentGallerySvgFrame = requestAnimationFrame(gallerySvgClock);
    } else {
      attachmentGalleryPlayhead.value = dependencies.attachmentGalleryLivePlayhead;
      stopGallerySvgClock();
    }
  }
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryPlayback.selector, () => {
      toggleGalleryPlayback();
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', ATTACHMENTS_AND_GALLERY_TARGETS.attachmentGallery.selector, (event) => {
      const keyboard = event as KeyboardEvent,
        origin = event.target as Element,
        playheadControl = origin.matches('input[name="gallery-playhead"]');
      if (
        !document.querySelector('.attachment-gallery video') ||
        (!playheadControl && origin.closest('button,input,textarea,select,[contenteditable="true"]'))
      )
        return;
      const action = attachmentGalleryKeyboardAction(
        keyboard.key,
        dependencies.attachmentGalleryLivePlayhead,
        attachmentGalleryDuration.value,
        keyboard.shiftKey,
      );
      if (!action) return;
      event.preventDefault();
      event.stopPropagation();
      if (action.kind === 'toggle-playback') {
        toggleGalleryPlayback();
        return;
      }
      document.querySelector<HTMLVideoElement>('.attachment-gallery video')?.pause();
      setGalleryPlayhead(action.playheadMs);
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', 'input[name="gallery-playhead"]', (_event, target) => {
      setGalleryPlayhead(Number((target as HTMLInputElement).value), false);
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="gallery-playhead"]', (_event, target) => {
      setGalleryPlayhead(Number((target as HTMLInputElement).value));
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryVolume.selector, (event) => {
      event.stopPropagation();
      attachmentGalleryVolumeOpen.value = !attachmentGalleryVolumeOpen.value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'input', 'input[name="gallery-volume"]', (_event, target) => {
      const video = document.querySelector<HTMLVideoElement>('.attachment-gallery video'),
        volume = Math.max(0, Math.min(1, Number((target as HTMLInputElement).value)));
      dependencies.attachmentGalleryLiveVolume = volume;
      if (attachmentGalleryMuted.value) attachmentGalleryMuted.value = false;
      if (video) {
        video.volume = volume;
        video.muted = false;
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', 'input[name="gallery-volume"]', (_event, target) => {
      attachmentGalleryVolume.value = Math.max(0, Math.min(1, Number((target as HTMLInputElement).value)));
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryMuted.selector, (event) => {
      event.stopPropagation();
      const video = document.querySelector<HTMLVideoElement>('.attachment-gallery video'),
        unmute = attachmentGalleryMuted.value || dependencies.attachmentGalleryLiveVolume === 0;
      if (unmute && dependencies.attachmentGalleryLiveVolume === 0) {
        dependencies.attachmentGalleryLiveVolume = 0.5;
        attachmentGalleryVolume.value = 0.5;
        if (video) video.volume = 0.5;
      }
      attachmentGalleryMuted.value = !unmute;
      if (video) video.muted = !unmute;
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'timeupdate', '.attachment-gallery video', (event) => {
      const video = activeAttachmentGalleryVideo(event.target),
        slider = document.querySelector<HTMLInputElement>('.attachment-gallery input[name="gallery-playhead"]');
      if (video && (!video.paused || document.activeElement !== slider))
        updateGalleryPlaybackPresentation(Math.round(video.currentTime * 1000));
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'play', '.attachment-gallery video', (event) => {
      if (activeAttachmentGalleryVideo(event.target)) attachmentGalleryPlaying.value = true;
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'pause', '.attachment-gallery video', (event) => {
      if (activeAttachmentGalleryVideo(event.target)) {
        attachmentGalleryPlayhead.value = dependencies.attachmentGalleryLivePlayhead;
        attachmentGalleryPlaying.value = false;
      }
    }),
  );
  document.addEventListener(
    'click',
    (event) => {
      if (attachmentGalleryVolumeOpen.value && !(event.target as Element).closest('.attachment-gallery__volume'))
        attachmentGalleryVolumeOpen.value = false;
    },
    { signal: lifetime.signal },
  );
  lifetime.add(
    delegateCapture(
      document.body,
      'pointerdown',
      ATTACHMENTS_AND_GALLERY_TARGETS.attachmentGallery.selector,
      (event) => {
        const pointer = event as PointerEvent,
          origin = event.target instanceof Element ? event.target : undefined,
          stage = origin?.closest<HTMLElement>('[data-gallery-zoom-stage="true"]');
        dependencies.attachmentSwipeGesture = attachmentGallerySwipeGesture({
          pointerId: pointer.pointerId,
          clientX: pointer.clientX,
          clientY: pointer.clientY,
          button: pointer.button,
          markup: attachmentGalleryMarkup.value,
          stage: Boolean(stage),
          interactive: Boolean(origin?.closest('button,input,textarea,select,a,[contenteditable="true"]')),
          horizontallyScrollable: Boolean(stage && stage.scrollWidth > stage.clientWidth + 1),
        });
      },
    ),
  );
  lifetime.add(
    delegateCapture(document.body, 'pointerup', ATTACHMENTS_AND_GALLERY_TARGETS.attachmentGallery.selector, (event) => {
      const pointer = event as PointerEvent,
        direction = attachmentGallerySwipeDirection(
          dependencies.attachmentSwipeGesture,
          pointer.pointerId,
          pointer.clientX,
          pointer.clientY,
        );
      dependencies.attachmentSwipeGesture = undefined;
      if (direction) shiftGallery(direction);
    }),
  );
  document.addEventListener(
    'pointercancel',
    (event) => {
      if (event.pointerId === dependencies.attachmentSwipeGesture?.pointerId)
        dependencies.attachmentSwipeGesture = undefined;
    },
    { signal: lifetime.signal },
  );
  async function removeSelectedAttachment(id?: string) {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket || !id || !canUseAttachments()) return;
    attachmentMessage.value = 'Removing attachment…';
    try {
      const result = await api().deleteCheckoutAttachment(current.id, ticket.qualified_id, id);
      selectedTicket.value = result.ticket;
      attachmentMessage.value = '';
      showToast('Attachment removed.');
      await refreshProject();
    } catch (reason) {
      attachmentMessage.value = `Remove failed: ${reason instanceof Error ? reason.message : String(reason)}`;
    }
  }
  return () => {
    lifetime.dispose();
  };
}
