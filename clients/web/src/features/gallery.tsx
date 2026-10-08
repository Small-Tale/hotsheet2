import type { Signal } from 'kerfjs';
import { batch, signal } from 'kerfjs';

import type { Api, Attachment, FullTicket, MediaAnnotation } from '../api';
import {
  type AttachmentReferenceContext,
  attachmentReferences,
  attachmentReferenceUrl,
  isGalleryMediaAttachment,
  isVideoAttachment,
} from '../attachment-references';
import {
  attachmentGalleryAnnotationVisible,
  type AttachmentGalleryGeometry,
  type AttachmentGalleryImage,
  attachmentGalleryShiftUrl,
  type AttachmentGallerySwipeGesture,
  releaseAttachmentGalleryVideo,
} from '../components/attachment-gallery';
import type { AttachmentContextMenuSurface } from '../components/reader-overlay-surfaces';
import { GallerySurface } from '../components/reader-overlay-surfaces';
import type { GalleryAnnotationTool } from '../gallery-annotation-editor';
import { croppableImageHeader, projectGalleryAnnotations, restoreGalleryAnnotations } from '../gallery-crop';
import type { AttachmentMenu, GallerySource, Project } from '../interactions/types';

export interface GalleryDependencies {
  project: () => Project | undefined;
  selectedTicket: Signal<FullTicket | null>;
  api: () => Api;
  attachmentContext: (ticket: FullTicket, project?: Project) => AttachmentReferenceContext | undefined;
  showToast: (message: string) => void;
  error: Signal<string>;
  /**
   * Whether the ticket's provider can edit existing attachments (`attachment_edit`); an
   * append-only provider has no annotations or generated video posters (HS2-HSA64D).
   */
  attachmentsEditable?: (ticket: FullTicket) => boolean;
  attachmentsCroppable?: (ticket: FullTicket) => boolean;
}

export function createGalleryController(dependencies: GalleryDependencies) {
  const {
    project,
    selectedTicket,
    api,
    attachmentContext,
    showToast,
    error,
    attachmentsEditable = () => true,
    attachmentsCroppable = attachmentsEditable,
  } = dependencies;
  const attachmentGalleryUrl = signal<string | undefined>(undefined),
    gallerySource = signal<GallerySource | undefined>(undefined);
  // Without an explicit source the gallery belongs to the workspace selection.
  const sourceTicket = () => gallerySource.value?.ticket ?? selectedTicket.value,
    sourceProject = () => gallerySource.value?.project ?? project();
  const attachmentGalleryGeometry = signal<AttachmentGalleryGeometry>({
      naturalWidth: 0,
      naturalHeight: 0,
      availableWidth: 0,
      availableHeight: 0,
    }),
    attachmentGalleryScale = signal<number | undefined>(undefined);
  const attachmentGalleryMarkup = signal(false),
    attachmentGalleryCropMode = signal(false),
    attachmentGalleryCrop = signal<Attachment['crop']>(undefined),
    attachmentGalleryCropAvailable = signal(false),
    attachmentGalleryPreviewUrl = signal<string | undefined>(undefined),
    attachmentGalleryOriginalSize = signal({ width: 0, height: 0 }),
    attachmentGalleryRevision = signal(0),
    attachmentGalleryDrawMode = signal(false),
    attachmentGalleryTool = signal<GalleryAnnotationTool>('select'),
    attachmentGalleryAnnotations = signal<MediaAnnotation[]>([]),
    attachmentGallerySelectedAnnotation = signal<string | undefined>(undefined),
    attachmentGalleryPlayhead = signal(0),
    attachmentGalleryDuration = signal(0),
    attachmentGalleryPlaying = signal(false),
    attachmentGalleryVolume = signal(1),
    attachmentGalleryMuted = signal(false),
    attachmentGalleryVolumeOpen = signal(false);
  let attachmentAnnotationGesture:
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
  let attachmentRangeGesture:
    | { pointerId: number; annotationId: string; endpoint: 'start' | 'end'; track: DOMRect; before: MediaAnnotation[] }
    | undefined;
  let attachmentSwipeGesture: AttachmentGallerySwipeGesture | undefined;
  let attachmentAnnotationSession:
    | {
        projectId: string;
        ticketId: string;
        qualifiedId: string;
        attachmentId: string;
        before: MediaAnnotation[];
        original: MediaAnnotation[];
        crop?: Attachment['crop'];
        originalSize: { width: number; height: number };
      }
    | undefined;
  const attachmentGallerySaveState = signal<'idle' | 'saving' | 'failed'>('idle');
  let pendingGalleryReset: { url?: string; source?: GallerySource } | undefined;

  let attachmentGallerySvgFrame: number | undefined, attachmentGallerySvgPreviousFrame: number | undefined;
  let attachmentGalleryLivePlayhead = 0,
    attachmentGalleryLiveVolume = 1;
  let attachmentGalleryObserver: ResizeObserver | undefined;

  const attachmentMenu = signal<AttachmentMenu | undefined>(undefined);

  function galleryImages(ticket = sourceTicket(), current = sourceProject()): AttachmentGalleryImage[] {
    if (!ticket || !current) return [];
    const context = attachmentContext(ticket, current)!,
      posters = attachmentsEditable(ticket),
      images: AttachmentGalleryImage[] = ticket.attachments
        .filter((item) => isGalleryMediaAttachment(item.filename))
        .map((item) => ({
          id: item.id,
          name: item.filename,
          url: api().checkoutAttachmentUrl(current.id, ticket.qualified_id, item.id),
          thumbnailUrl:
            posters && isVideoAttachment(item.filename)
              ? api().checkoutAttachmentThumbnailUrl(current.id, ticket.qualified_id, item.id)
              : undefined,
          aliases: [attachmentReferenceUrl(context, { filename: item.filename })],
          ticket: ticket.slug,
          attachmentId: item.id,
        })),
      seen = new Set(images.map((image) => `${ticket.slug}\0${image.name}`));
    for (const note of ticket.notes)
      for (const reference of attachmentReferences(note.text, context)) {
        if (!isGalleryMediaAttachment(reference.filename)) continue;
        const referencedTicket = reference.ticket ?? ticket.slug,
          key = `${referencedTicket}\0${reference.filename}`,
          url = attachmentReferenceUrl(context, reference);
        if (!seen.has(key)) {
          seen.add(key);
          images.push({
            id: `${referencedTicket}:${reference.filename}`,
            name: reference.filename,
            url,
            ticket: referencedTicket,
          });
        }
      }
    return images;
  }

  function gallerySurface() {
    const active = attachmentGalleryUrl.value,
      images = galleryImages(),
      image = active ? images.find((item) => item.url === active || item.aliases?.includes(active)) : undefined,
      displayUrl = image?.url ?? active ?? '',
      ticket = sourceTicket(),
      attachmentIndex = ticket?.attachments.findIndex((item) => item.id === image?.attachmentId) ?? -1,
      annotationNumberOffset =
        ticket && attachmentIndex > 0
          ? ticket.attachments
              .slice(0, attachmentIndex)
              .reduce((count, attachment) => count + (attachment.annotations?.length ?? 0), 0)
          : 0;
    return (
      <GallerySurface
        gallery={
          active
            ? {
                images,
                activeUrl: active,
                geometry: attachmentGalleryGeometry.value,
                selectedScale: attachmentGalleryScale.value,
                annotations: attachmentGalleryAnnotations.value,
                imageUrl:
                  attachmentGalleryCropMode.value && image?.attachmentId
                    ? api().checkoutAttachmentOriginalUrl(sourceProject()!.id, ticket!.qualified_id, image.attachmentId)
                    : (attachmentGalleryPreviewUrl.value ??
                      `${displayUrl}${image?.attachmentId && attachmentGalleryRevision.value > 0 ? `${displayUrl.includes('?') ? '&' : '?'}revision=${attachmentGalleryRevision.value}` : ''}`),
                cropMode: attachmentGalleryCropMode.value,
                crop: attachmentGalleryCrop.value,
                cropEnabled: Boolean(
                  image?.attachmentId &&
                  /\.(png|jpe?g|webp)$/i.test(image.name) &&
                  attachmentGalleryCropAvailable.value &&
                  ticket &&
                  attachmentsCroppable(ticket) &&
                  !gallerySource.value?.readOnly,
                ),
                originalWidth: attachmentGalleryOriginalSize.value.width,
                originalHeight: attachmentGalleryOriginalSize.value.height,
                annotationNumberOffset,
                markup: attachmentGalleryMarkup.value,
                saveState: attachmentGallerySaveState.value,
                drawMode: attachmentGalleryDrawMode.value,
                tool: attachmentGalleryTool.value,
                selectedAnnotation: attachmentGallerySelectedAnnotation.value,
                playheadMs: attachmentGalleryLivePlayhead,
                durationMs: attachmentGalleryDuration.value,
                playing: attachmentGalleryPlaying.value,
                volume: attachmentGalleryVolume.value,
                muted: attachmentGalleryMuted.value,
                volumeOpen: attachmentGalleryVolumeOpen.value,
                annotationEnabled:
                  Boolean(image?.attachmentId) &&
                  annotationsWritable() &&
                  (!activeGalleryAttachment()?.crop || attachmentGalleryOriginalSize.value.width > 0),
              }
            : undefined
        }
        menu={attachmentMenuSurfaceProps({ insideGallery: true })}
      />
    );
  }

  function stopGallerySvgClock() {
    if (attachmentGallerySvgFrame !== undefined) cancelAnimationFrame(attachmentGallerySvgFrame);
    attachmentGallerySvgFrame = undefined;
    attachmentGallerySvgPreviousFrame = undefined;
  }

  function galleryTimeLabel(milliseconds: number) {
    const seconds = Math.max(0, Math.floor(milliseconds / 1000));
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
  }

  function updateGalleryPlaybackPresentation(milliseconds: number) {
    const next = Math.max(0, Math.min(attachmentGalleryDuration.value, Math.round(milliseconds)));
    attachmentGalleryLivePlayhead = next;
    const gallery = document.querySelector<HTMLElement>('[data-component="attachment-gallery"]');
    if (!gallery) return;
    const slider = gallery.querySelector<HTMLInputElement>('input[name="gallery-playhead"]');
    if (slider && slider.value !== String(next)) slider.value = String(next);
    const current = gallery.querySelector<HTMLElement>('[data-gallery-current-time="true"]');
    if (current) current.textContent = galleryTimeLabel(next);
    for (const annotation of gallery.querySelectorAll<HTMLElement>(
      '.attachment-gallery__annotation[data-annotation-start]',
    )) {
      const start = Number(annotation.dataset.annotationStart),
        end = Number(annotation.dataset.annotationEnd ?? annotation.dataset.annotationStart);
      annotation.hidden = !attachmentGalleryAnnotationVisible(
        { id: 'presentation', x: 0, y: 0, width: 0, height: 0, start_ms: start, end_ms: end, text: '' },
        next,
        attachmentGalleryDuration.value,
      );
    }
  }

  function gallerySvgClock(timestamp: number) {
    if (!attachmentGalleryPlaying.value || !attachmentGalleryDuration.value) {
      stopGallerySvgClock();
      return;
    }
    const elapsed = attachmentGallerySvgPreviousFrame === undefined ? 0 : timestamp - attachmentGallerySvgPreviousFrame;
    attachmentGallerySvgPreviousFrame = timestamp;
    updateGalleryPlaybackPresentation((attachmentGalleryLivePlayhead + elapsed) % attachmentGalleryDuration.value);
    attachmentGallerySvgFrame = requestAnimationFrame(gallerySvgClock);
  }

  async function detectAnimatedGallerySvg(url: string) {
    try {
      const response = await fetch(url);
      if (!response.ok || attachmentGalleryUrl.value !== url) return;
      const document = new DOMParser().parseFromString(await response.text(), 'image/svg+xml'),
        animations = [...document.querySelectorAll('animate, animateMotion, animateTransform, set')],
        duration = Math.max(0, ...animations.map((node) => svgDurationMilliseconds(node.getAttribute('dur') ?? '')));
      if (!animations.length || attachmentGalleryUrl.value !== url) return;
      attachmentGalleryDuration.value = duration || 5000;
      attachmentGalleryPlaying.value = true;
      attachmentGallerySvgFrame = requestAnimationFrame(gallerySvgClock);
    } catch {
      /* the image remains viewable even if animation metadata cannot be inspected */
    }
  }

  function activeAttachmentGalleryVideo(target?: EventTarget | null) {
    const video = document.querySelector<HTMLVideoElement>('.attachment-gallery video');
    return video && (!target || target === video) ? video : undefined;
  }

  function disposeAttachmentGalleryVideo() {
    const video = activeAttachmentGalleryVideo();
    if (video) releaseAttachmentGalleryVideo(video);
  }

  /** Show `url` (or close without one). A `source` opens it for that ticket; shifting keeps the current one. */
  function resetAttachmentGallery(url?: string, source?: GallerySource) {
    if (attachmentAnnotationSession) {
      pendingGalleryReset = { url, source };
      if (attachmentGallerySaveState.value !== 'saving') finishGalleryAnnotationSession();
      return;
    }
    performGalleryReset(url, source);
  }

  function performGalleryReset(url?: string, source?: GallerySource) {
    if (attachmentGalleryPreviewUrl.value?.startsWith('blob:')) URL.revokeObjectURL(attachmentGalleryPreviewUrl.value);
    if (!url) gallerySource.value = undefined;
    else if (source) gallerySource.value = source;
    stopGallerySvgClock();
    disposeAttachmentGalleryVideo();
    attachmentGalleryObserver?.disconnect();
    attachmentGalleryObserver = undefined;
    const image = url ? galleryImages().find((item) => item.url === url || item.aliases?.includes(url)) : undefined,
      attachment = sourceTicket()?.attachments.find((item) => item.id === image?.attachmentId);
    batch(() => {
      attachmentGalleryUrl.value = url;
      attachmentGalleryScale.value = undefined;
      attachmentGalleryGeometry.value = { naturalWidth: 0, naturalHeight: 0, availableWidth: 0, availableHeight: 0 };
      attachmentMenu.value = undefined;
      attachmentGalleryMarkup.value = false;
      attachmentGalleryCropMode.value = false;
      attachmentGalleryCrop.value = attachment?.crop;
      attachmentGalleryCropAvailable.value = Boolean(attachment?.crop);
      attachmentGalleryPreviewUrl.value = undefined;
      attachmentGalleryOriginalSize.value = { width: 0, height: 0 };
      attachmentGalleryDrawMode.value = false;
      attachmentGalleryTool.value = 'select';
      attachmentGallerySelectedAnnotation.value = undefined;
      attachmentGalleryPlayhead.value = 0;
      attachmentGalleryDuration.value = 0;
      attachmentGalleryPlaying.value = false;
      attachmentGalleryVolume.value = 1;
      attachmentGalleryMuted.value = false;
      attachmentGalleryVolumeOpen.value = false;
      attachmentGalleryAnnotations.value = attachment?.crop
        ? []
        : (attachment?.annotations?.map((item) => ({ ...item })) ?? []);
    });
    attachmentGalleryLivePlayhead = 0;
    attachmentGalleryLiveVolume = 1;
    attachmentAnnotationGesture = undefined;
    attachmentRangeGesture = undefined;
    attachmentSwipeGesture = undefined;
    if (attachment && image && !attachment.crop && /\.(png|jpe?g|webp)$/i.test(image.name)) {
      const requested = url,
        originalUrl = api().checkoutAttachmentOriginalUrl(
          sourceProject()!.id,
          sourceTicket()!.qualified_id,
          attachment.id,
        );
      void (async () => {
        try {
          const response = await fetch(originalUrl, { headers: { Range: 'bytes=0-65535' } });
          if (!response.ok) return;
          const reader = response.body?.getReader();
          if (!reader) return;
          const chunks: Uint8Array[] = [];
          let length = 0;
          while (length < 65536) {
            const part = await reader.read();
            if (part.done) break;
            chunks.push(part.value);
            length += part.value.length;
          }
          await reader.cancel();
          const bytes = new Uint8Array(Math.min(length, 65536));
          let offset = 0;
          for (const chunk of chunks) {
            bytes.set(chunk.subarray(0, bytes.length - offset), offset);
            offset += Math.min(chunk.length, bytes.length - offset);
          }
          if (attachmentGalleryUrl.value === requested)
            attachmentGalleryCropAvailable.value = croppableImageHeader(image.name, bytes);
        } catch {
          /* The original remains viewable even if a crop format check cannot complete. */
        }
      })();
    }
    if (attachment?.crop && image && sourceProject()) {
      const original = new Image(),
        requested = url,
        originalUrl = api().checkoutAttachmentOriginalUrl(
          sourceProject()!.id,
          sourceTicket()!.qualified_id,
          attachment.id,
        );
      original.onload = () => {
        if (attachmentGalleryUrl.value !== requested) return;
        attachmentGalleryOriginalSize.value = { width: original.naturalWidth, height: original.naturalHeight };
        if (!attachmentGalleryCropMode.value)
          attachmentGalleryAnnotations.value = projectGalleryAnnotations(
            attachment.annotations ?? [],
            attachment.crop,
            original.naturalWidth,
            original.naturalHeight,
          );
      };
      original.src = originalUrl;
    }
    if (image?.name.toLowerCase().endsWith('.svg')) void detectAnimatedGallerySvg(url!);
  }

  function syncAttachmentGalleryMeasurement() {
    attachmentGalleryObserver?.disconnect();
    attachmentGalleryObserver = undefined;
    const gallery = document.querySelector<HTMLDialogElement>('dialog[data-component="attachment-gallery"]');
    if (gallery && !gallery.open) gallery.showModal();
    const stage = document.querySelector<HTMLElement>('[data-gallery-zoom-stage="true"]'),
      media = document.querySelector<HTMLImageElement | HTMLVideoElement>('[data-gallery-media="true"]');
    if (!stage || !media) return;
    const update = () => {
      const canvas = stage.firstElementChild instanceof HTMLElement ? stage.firstElementChild : undefined,
        style = canvas ? getComputedStyle(canvas) : undefined,
        horizontal =
          (Number.parseFloat(style?.paddingLeft ?? '0') || 0) + (Number.parseFloat(style?.paddingRight ?? '0') || 0),
        vertical =
          (Number.parseFloat(style?.paddingTop ?? '0') || 0) + (Number.parseFloat(style?.paddingBottom ?? '0') || 0),
        naturalWidth = media instanceof HTMLVideoElement ? media.videoWidth : media.naturalWidth,
        naturalHeight = media instanceof HTMLVideoElement ? media.videoHeight : media.naturalHeight,
        next = {
          naturalWidth,
          naturalHeight,
          availableWidth: Math.max(1, stage.clientWidth - horizontal),
          availableHeight: Math.max(1, stage.clientHeight - vertical),
        },
        previous = attachmentGalleryGeometry.value;
      if (
        Object.keys(next).some(
          (key) => next[key as keyof AttachmentGalleryGeometry] !== previous[key as keyof AttachmentGalleryGeometry],
        )
      )
        attachmentGalleryGeometry.value = next;
    };
    update();
    attachmentGalleryObserver = new ResizeObserver(update);
    attachmentGalleryObserver.observe(stage);
  }

  // The menu renders inside whichever surface owns the trigger. When it was opened from within the
  // modal ticket reader (menu.reader = that reader's frame id) it must render as a descendant of that
  // dialog, or the reader's modality leaves it inert and painted beneath (HS2-EZ10RS); otherwise it
  // renders at the app root (the side inspector or the media gallery).
  function attachmentMenuSurfaceProps({
    insideGallery = false,
    readerScope,
  }: { insideGallery?: boolean; readerScope?: string } = {}): Parameters<
    typeof AttachmentContextMenuSurface
  >[0]['menu'] {
    const menu = attachmentMenu.value;
    if (!menu) return;
    if (readerScope !== undefined) {
      if (menu.reader !== readerScope) return;
    } else if (menu.reader || Boolean(attachmentGalleryUrl.value) !== insideGallery) return;
    const reveal = /Mac/i.test(navigator.userAgent)
      ? 'Show in Finder'
      : /Win/i.test(navigator.userAgent)
        ? 'Show in File Explorer'
        : 'Show in file manager';
    return { x: menu.x, y: menu.y, kind: menu.kind, revealLabel: reveal };
  }

  function activeGalleryAttachment() {
    const active = attachmentGalleryUrl.value,
      image = active
        ? galleryImages().find((item) => item.url === active || item.aliases?.includes(active))
        : undefined;
    return sourceTicket()?.attachments.find((item) => item.id === image?.attachmentId);
  }

  function annotationsWritable() {
    const ticket = sourceTicket();
    return !gallerySource.value?.readOnly && Boolean(ticket && attachmentsEditable(ticket));
  }

  function beginGalleryAnnotationSession() {
    if (!annotationsWritable()) return;
    const current = sourceProject(),
      ticket = sourceTicket(),
      attachment = activeGalleryAttachment();
    if (!current || !ticket || !attachment) return;
    attachmentAnnotationSession = {
      projectId: current.id,
      ticketId: ticket.id,
      qualifiedId: ticket.qualified_id,
      attachmentId: attachment.id,
      before: attachmentGalleryAnnotations.value.map((item) => ({ ...item })),
      original: structuredClone(attachment.annotations ?? []),
      crop: attachment.crop,
      originalSize: attachmentGalleryOriginalSize.value,
    };
    attachmentGallerySaveState.value = 'idle';
  }

  async function beginGalleryCrop() {
    if (!attachmentGalleryMarkup.value || attachmentGalleryCropMode.value) return;
    const attachment = activeGalleryAttachment(),
      current = sourceProject(),
      ticket = sourceTicket();
    if (!attachment || !current || !ticket) return;
    const size = attachmentGalleryOriginalSize.value;
    const enter = (width: number, height: number) => {
      if (!attachmentGalleryMarkup.value) return;
      const canonical = restoreGalleryAnnotations(
        attachmentAnnotationSession?.original ?? attachment.annotations ?? [],
        attachmentGalleryAnnotations.value,
        attachmentGalleryCrop.value,
        width,
        height,
      );
      if (attachmentAnnotationSession) {
        attachmentAnnotationSession.original = canonical;
        attachmentAnnotationSession.originalSize = { width, height };
      }
      attachmentGalleryOriginalSize.value = { width, height };
      attachmentGalleryAnnotations.value = canonical;
      attachmentGalleryCropMode.value = true;
      attachmentGallerySelectedAnnotation.value = undefined;
      attachmentGalleryTool.value = 'select';
      attachmentGalleryDrawMode.value = false;
      attachmentGalleryScale.value = undefined;
    };
    if (size.width && size.height) enter(size.width, size.height);
    else {
      const original = new Image();
      original.src = api().checkoutAttachmentOriginalUrl(current.id, ticket.qualified_id, attachment.id);
      try {
        await original.decode();
        enter(original.naturalWidth, original.naturalHeight);
      } catch {
        error.value = 'Could not load the original image for cropping.';
      }
    }
  }

  async function restoreGalleryCrop() {
    if (!attachmentGalleryCropMode.value) await beginGalleryCrop();
    if (!attachmentGalleryCropMode.value) return;
    attachmentGalleryCrop.value = undefined;
  }

  async function finishGalleryCrop() {
    if (!attachmentGalleryCropMode.value) return;
    const attachment = activeGalleryAttachment(),
      current = sourceProject(),
      ticket = sourceTicket();
    if (!attachment || !current || !ticket) return;
    const crop = attachmentGalleryCrop.value,
      size = attachmentGalleryOriginalSize.value,
      originalUrl = api().checkoutAttachmentOriginalUrl(current.id, ticket.qualified_id, attachment.id),
      oldPreview = attachmentGalleryPreviewUrl.value;
    if (crop) {
      const original = new Image();
      original.src = originalUrl;
      try {
        await original.decode();
        const canvas = document.createElement('canvas');
        canvas.width = crop.width;
        canvas.height = crop.height;
        canvas
          .getContext('2d')
          ?.drawImage(original, crop.x, crop.y, crop.width, crop.height, 0, 0, crop.width, crop.height);
        const blob = await new Promise<Blob | null>((resolve) => {
          canvas.toBlob(resolve, 'image/png');
        });
        if (!blob) throw new Error('Could not preview the crop.');
        attachmentGalleryPreviewUrl.value = URL.createObjectURL(blob);
      } catch (reason) {
        error.value = reason instanceof Error ? reason.message : String(reason);
        return;
      }
    } else attachmentGalleryPreviewUrl.value = originalUrl;
    if (oldPreview?.startsWith('blob:')) URL.revokeObjectURL(oldPreview);
    attachmentGalleryAnnotations.value = projectGalleryAnnotations(
      attachmentAnnotationSession?.original ?? attachmentGalleryAnnotations.value,
      crop,
      size.width,
      size.height,
    );
    attachmentGalleryCropMode.value = false;
    attachmentGalleryScale.value = undefined;
    attachmentGallerySelectedAnnotation.value = undefined;
    requestAnimationFrame(syncAttachmentGalleryMeasurement);
  }

  function finishGalleryAnnotationSession() {
    const session = attachmentAnnotationSession;
    if (!session || attachmentGallerySaveState.value === 'saving') return;
    const annotations = attachmentGalleryAnnotations.value.map((item) => ({ ...item }));
    const crop = attachmentGalleryCrop.value,
      cropChanged = JSON.stringify(session.crop) !== JSON.stringify(crop),
      changed = JSON.stringify(session.before) !== JSON.stringify(annotations) || cropChanged;
    if (!changed) {
      attachmentAnnotationSession = undefined;
      attachmentGallerySaveState.value = 'idle';
      const reset = pendingGalleryReset;
      pendingGalleryReset = undefined;
      if (reset) performGalleryReset(reset.url, reset.source);
      else if (attachmentGalleryCropMode.value && attachmentGalleryUrl.value)
        performGalleryReset(attachmentGalleryUrl.value);
      return;
    }
    const canonical = attachmentGalleryCropMode.value
      ? annotations
      : restoreGalleryAnnotations(
          session.original,
          annotations,
          crop,
          session.originalSize.width,
          session.originalSize.height,
        );
    attachmentGallerySaveState.value = 'saving';
    void (async () => {
      let result;
      try {
        result = cropChanged
          ? await api().updateCheckoutAttachmentMarkup(
              session.projectId,
              session.qualifiedId,
              session.attachmentId,
              canonical,
              crop,
            )
          : await api().updateCheckoutAttachmentAnnotations(
              session.projectId,
              session.qualifiedId,
              session.attachmentId,
              canonical,
            );
      } catch (reason) {
        error.value = reason instanceof Error ? reason.message : String(reason);
        attachmentGallerySaveState.value = 'failed';
        attachmentGalleryMarkup.value = true;
        return;
      }
      const source = gallerySource.value;
      if (source?.ticket.id === session.ticketId && source.project.id === session.projectId) {
        gallerySource.value = { ...source, ticket: result.ticket };
        source.update?.(result.ticket);
      }
      if (selectedTicket.value?.id === session.ticketId && project()?.id === session.projectId)
        selectedTicket.value = result.ticket;
      attachmentAnnotationSession = undefined;
      attachmentGallerySaveState.value = 'idle';
      error.value = '';
      attachmentGalleryRevision.value++;
      const reset = pendingGalleryReset;
      pendingGalleryReset = undefined;
      if (reset) performGalleryReset(reset.url, reset.source);
      else if (attachmentGalleryUrl.value && activeGalleryAttachment()?.id === session.attachmentId)
        performGalleryReset(attachmentGalleryUrl.value);
      showToast(cropChanged ? 'Image crop saved.' : 'Annotations saved.');
    })();
  }

  function discardGalleryAnnotationSession() {
    if (!attachmentAnnotationSession || attachmentGallerySaveState.value === 'saving') return;
    attachmentAnnotationSession = undefined;
    attachmentGallerySaveState.value = 'idle';
    error.value = '';
    const reset = pendingGalleryReset;
    pendingGalleryReset = undefined;
    if (reset) performGalleryReset(reset.url, reset.source);
    else performGalleryReset(attachmentGalleryUrl.value);
  }

  function shiftGallery(delta: number) {
    const active = attachmentGalleryUrl.value;
    if (!active) return;
    const url = attachmentGalleryShiftUrl(galleryImages(), active, delta);
    if (url) resetAttachmentGallery(url);
  }
  const svgDurationMilliseconds = (value: string) => {
    const match = value.trim().match(/^(\d+(?:\.\d+)?)(ms|s)$/);
    return match ? Number(match[1]) * (match[2] === 's' ? 1000 : 1) : 0;
  };

  return {
    attachmentGalleryUrl,
    attachmentGalleryGeometry,
    attachmentGalleryScale,
    attachmentGalleryMarkup,
    attachmentGallerySaveState,
    attachmentGalleryCropMode,
    attachmentGalleryCrop,
    attachmentGalleryCropAvailable,
    attachmentGalleryPreviewUrl,
    attachmentGalleryOriginalSize,
    beginGalleryCrop,
    restoreGalleryCrop,
    finishGalleryCrop,
    attachmentGalleryDrawMode,
    attachmentGalleryTool,
    attachmentGalleryAnnotations,
    attachmentGallerySelectedAnnotation,
    attachmentGalleryPlayhead,
    attachmentGalleryDuration,
    attachmentGalleryPlaying,
    attachmentGalleryVolume,
    attachmentGalleryMuted,
    attachmentGalleryVolumeOpen,
    attachmentMenu,
    galleryImages,
    gallerySurface,
    stopGallerySvgClock,
    updateGalleryPlaybackPresentation,
    gallerySvgClock,
    activeAttachmentGalleryVideo,
    resetAttachmentGallery,
    syncAttachmentGalleryMeasurement,
    attachmentMenuSurfaceProps,
    beginGalleryAnnotationSession,
    finishGalleryAnnotationSession,
    discardGalleryAnnotationSession,
    shiftGallery,
    get attachmentAnnotationGesture() {
      return attachmentAnnotationGesture;
    },
    set attachmentAnnotationGesture(value: typeof attachmentAnnotationGesture) {
      attachmentAnnotationGesture = value;
    },
    get attachmentRangeGesture() {
      return attachmentRangeGesture;
    },
    set attachmentRangeGesture(value: typeof attachmentRangeGesture) {
      attachmentRangeGesture = value;
    },
    get attachmentSwipeGesture() {
      return attachmentSwipeGesture;
    },
    set attachmentSwipeGesture(value: typeof attachmentSwipeGesture) {
      attachmentSwipeGesture = value;
    },
    get attachmentGalleryLivePlayhead() {
      return attachmentGalleryLivePlayhead;
    },
    set attachmentGalleryLivePlayhead(value: typeof attachmentGalleryLivePlayhead) {
      attachmentGalleryLivePlayhead = value;
    },
    get attachmentGalleryLiveVolume() {
      return attachmentGalleryLiveVolume;
    },
    set attachmentGalleryLiveVolume(value: typeof attachmentGalleryLiveVolume) {
      attachmentGalleryLiveVolume = value;
    },
    get attachmentGallerySvgPreviousFrame() {
      return attachmentGallerySvgPreviousFrame;
    },
    set attachmentGallerySvgPreviousFrame(value: typeof attachmentGallerySvgPreviousFrame) {
      attachmentGallerySvgPreviousFrame = value;
    },
    get attachmentGallerySvgFrame() {
      return attachmentGallerySvgFrame;
    },
    set attachmentGallerySvgFrame(value: typeof attachmentGallerySvgFrame) {
      attachmentGallerySvgFrame = value;
    },
  };
}
