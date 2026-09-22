import type { Signal } from 'kerfjs';
import { batch, signal } from 'kerfjs';

import type { Api, FullTicket, MediaAnnotation } from '../api';
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
import type { AttachmentMenu, Project } from '../interactions/types';

export interface GalleryDependencies {
  project: () => Project | undefined;
  selectedTicket: Signal<FullTicket | null>;
  api: () => Api;
  attachmentContext: (ticket: FullTicket) => AttachmentReferenceContext | undefined;
  showToast: (message: string) => void;
  error: Signal<string>;
}

export function createGalleryController(dependencies: GalleryDependencies) {
  const { project, selectedTicket, api, attachmentContext, showToast, error } = dependencies;
  const attachmentGalleryUrl = signal<string | undefined>(undefined);
  const attachmentGalleryGeometry = signal<AttachmentGalleryGeometry>({
      naturalWidth: 0,
      naturalHeight: 0,
      availableWidth: 0,
      availableHeight: 0,
    }),
    attachmentGalleryScale = signal<number | undefined>(undefined);
  const attachmentGalleryMarkup = signal(false),
    attachmentGalleryDrawMode = signal(false),
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
      }
    | undefined;
  let attachmentRangeGesture:
    { pointerId: number; annotationId: string; endpoint: 'start' | 'end'; track: DOMRect } | undefined;
  let attachmentSwipeGesture: AttachmentGallerySwipeGesture | undefined;
  let attachmentAnnotationSession:
    { projectId: string; ticketId: string; attachmentId: string; before: MediaAnnotation[] } | undefined;
  let attachmentAnnotationSave = Promise.resolve();

  let attachmentGallerySvgFrame: number | undefined, attachmentGallerySvgPreviousFrame: number | undefined;
  let attachmentGalleryLivePlayhead = 0,
    attachmentGalleryLiveVolume = 1;
  let attachmentGalleryObserver: ResizeObserver | undefined;

  const attachmentMenu = signal<AttachmentMenu | undefined>(undefined);

  function galleryImages(ticket = selectedTicket.value): AttachmentGalleryImage[] {
    const current = project();
    if (!ticket || !current) return [];
    const context = attachmentContext(ticket)!,
      images: AttachmentGalleryImage[] = ticket.attachments
        .filter((item) => isGalleryMediaAttachment(item.filename))
        .map((item) => ({
          id: item.id,
          name: item.filename,
          url: api().checkoutAttachmentUrl(current.id, ticket.id, item.id),
          thumbnailUrl: isVideoAttachment(item.filename)
            ? api().checkoutAttachmentThumbnailUrl(current.id, ticket.id, item.id)
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
      image = active ? images.find((item) => item.url === active || item.aliases?.includes(active)) : undefined;
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
                markup: attachmentGalleryMarkup.value,
                drawMode: attachmentGalleryDrawMode.value,
                selectedAnnotation: attachmentGallerySelectedAnnotation.value,
                playheadMs: attachmentGalleryLivePlayhead,
                durationMs: attachmentGalleryDuration.value,
                playing: attachmentGalleryPlaying.value,
                volume: attachmentGalleryVolume.value,
                muted: attachmentGalleryMuted.value,
                volumeOpen: attachmentGalleryVolumeOpen.value,
                annotationEnabled: Boolean(image?.attachmentId),
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

  function resetAttachmentGallery(url?: string) {
    finishGalleryAnnotationSession();
    stopGallerySvgClock();
    disposeAttachmentGalleryVideo();
    attachmentGalleryObserver?.disconnect();
    attachmentGalleryObserver = undefined;
    const image = url ? galleryImages().find((item) => item.url === url || item.aliases?.includes(url)) : undefined,
      attachment = selectedTicket.value?.attachments.find((item) => item.id === image?.attachmentId);
    batch(() => {
      attachmentGalleryUrl.value = url;
      attachmentGalleryScale.value = undefined;
      attachmentGalleryGeometry.value = { naturalWidth: 0, naturalHeight: 0, availableWidth: 0, availableHeight: 0 };
      attachmentMenu.value = undefined;
      attachmentGalleryMarkup.value = false;
      attachmentGalleryDrawMode.value = false;
      attachmentGallerySelectedAnnotation.value = undefined;
      attachmentGalleryPlayhead.value = 0;
      attachmentGalleryDuration.value = 0;
      attachmentGalleryPlaying.value = false;
      attachmentGalleryVolume.value = 1;
      attachmentGalleryMuted.value = false;
      attachmentGalleryVolumeOpen.value = false;
      attachmentGalleryAnnotations.value = attachment?.annotations?.map((item) => ({ ...item })) ?? [];
    });
    attachmentGalleryLivePlayhead = 0;
    attachmentGalleryLiveVolume = 1;
    attachmentAnnotationGesture = undefined;
    attachmentRangeGesture = undefined;
    attachmentSwipeGesture = undefined;
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
    return selectedTicket.value?.attachments.find((item) => item.id === image?.attachmentId);
  }

  function beginGalleryAnnotationSession() {
    const current = project(),
      ticket = selectedTicket.value,
      attachment = activeGalleryAttachment();
    if (!current || !ticket || !attachment) return;
    attachmentAnnotationSession = {
      projectId: current.id,
      ticketId: ticket.id,
      attachmentId: attachment.id,
      before: attachmentGalleryAnnotations.value.map((item) => ({ ...item })),
    };
  }

  function finishGalleryAnnotationSession() {
    const session = attachmentAnnotationSession;
    if (!session) return;
    attachmentAnnotationSession = undefined;
    const annotations = attachmentGalleryAnnotations.value.map((item) => ({ ...item }));
    if (JSON.stringify(session.before) === JSON.stringify(annotations)) return;
    attachmentAnnotationSave = attachmentAnnotationSave.then(async () => {
      try {
        const result = await api().updateCheckoutAttachmentAnnotations(
          session.projectId,
          session.ticketId,
          session.attachmentId,
          annotations,
        );
        if (selectedTicket.value?.id === session.ticketId) selectedTicket.value = result.ticket;
        showToast('Annotations saved.');
      } catch (reason) {
        error.value = reason instanceof Error ? reason.message : String(reason);
      }
    });
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
    attachmentGalleryDrawMode,
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
