import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { attachmentGalleryKeyboardAction } from './attachment-gallery';

export type GalleryRangeEndpoint = 'start' | 'end';

/**
 * Side effects for the AttachmentGallery's viewer controls (HS2-1KM6MB). Production passes the gallery
 * feature's signals and video element operations; the UX demo passes fixture callbacks, so both resolve
 * the same navigation, zoom, markup, playback, keyboard, volume, and range-handle gestures.
 */
export interface AttachmentGalleryControlBindings {
  readonly close: () => void;
  readonly shift: (delta: -1 | 1) => void;
  readonly zoom: (direction: 'in' | 'out') => void;
  readonly toggleMarkup: () => void;
  readonly toggleDraw: () => void;
  readonly togglePlayback: () => void;
  /** The live playhead and duration, read when a playback key arrives. */
  readonly playback: () => { playheadMs: number; durationMs: number };
  /**
   * Move the playhead. `keyboard` is a paused seek from a playback key, `scrub` a live playhead-slider
   * input, and `commit` the slider's settled change.
   */
  readonly seek: (milliseconds: number, source: 'keyboard' | 'scrub' | 'commit') => void;
  /** A timeline annotation marker was clicked: seek to its time and select it while marking up. */
  readonly seekAnnotation: (annotationId: string | undefined, milliseconds: number) => void;
  /** Set the volume, already clamped to 0…1; `commit` is the slider's settled change. */
  readonly setVolume: (volume: number, commit: boolean) => void;
  readonly toggleVolume: () => void;
  readonly toggleMuted: () => void;
  /** Start dragging a range handle; return false to ignore it (for example an unselected annotation). */
  readonly beginRange: (annotationId: string | undefined, endpoint: GalleryRangeEndpoint) => boolean;
  /** Move a range endpoint to `fraction` (0…1, unclamped) of the timeline track during a drag. */
  readonly dragRange: (annotationId: string | undefined, endpoint: GalleryRangeEndpoint, fraction: number) => void;
  /** The drag ended; `cancelled` is a pointer cancel that should restore the drag-start ranges. */
  readonly finishRange: (cancelled: boolean) => void;
  /** Arrow-key nudge of a range endpoint by `deltaMs`. */
  readonly nudgeRange: (annotationId: string | undefined, endpoint: GalleryRangeEndpoint, deltaMs: number) => void;
}

const endpointOf = (target: Element): GalleryRangeEndpoint | undefined => {
  const endpoint = (target as HTMLElement).dataset.galleryRangeHandle;
  return endpoint === 'start' || endpoint === 'end' ? endpoint : undefined;
};

/** Wire the viewer controls of every AttachmentGallery under `root`. */
export function wireAttachmentGalleryControls(
  root: HTMLElement,
  bindings: AttachmentGalleryControlBindings,
): () => void {
  const lifetime = createScope(),
    on = (event: string, selector: string, handle: (event: Event, target: Element) => void) => {
      lifetime.add(delegate(root, event, selector, handle));
    };
  on('click', '[data-action="close-attachment-gallery"]', () => {
    bindings.close();
  });
  on('click', '[data-action="previous-gallery-image"]', () => {
    bindings.shift(-1);
  });
  on('click', '[data-action="next-gallery-image"]', () => {
    bindings.shift(1);
  });
  on('click', '[data-action="zoom-gallery-image"]', (_event, target) => {
    bindings.zoom((target as HTMLElement).dataset.zoomDirection === 'out' ? 'out' : 'in');
  });
  on('click', '[data-action="toggle-gallery-markup"]', () => {
    bindings.toggleMarkup();
  });
  on('click', '[data-action="toggle-gallery-draw"]', () => {
    bindings.toggleDraw();
  });
  on('click', '[data-action="toggle-gallery-playback"]', () => {
    bindings.togglePlayback();
  });
  on('keydown', '[data-component="attachment-gallery"]', (event) => {
    const keyboard = event as KeyboardEvent,
      origin = event.target as Element,
      playheadControl = origin.matches('input[name="gallery-playhead"]');
    if (
      !root.querySelector('.attachment-gallery video') ||
      (!playheadControl && origin.closest('button,input,textarea,select,[contenteditable="true"]'))
    )
      return;
    const { playheadMs, durationMs } = bindings.playback(),
      action = attachmentGalleryKeyboardAction(keyboard.key, playheadMs, durationMs, keyboard.shiftKey);
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    if (action.kind === 'toggle-playback') bindings.togglePlayback();
    else bindings.seek(action.playheadMs, 'keyboard');
  });
  on('input', 'input[name="gallery-playhead"]', (_event, target) => {
    bindings.seek(Number((target as HTMLInputElement).value), 'scrub');
  });
  on('change', 'input[name="gallery-playhead"]', (_event, target) => {
    bindings.seek(Number((target as HTMLInputElement).value), 'commit');
  });
  on('click', '[data-action="seek-gallery-annotation"]', (_event, target) => {
    const element = target as HTMLElement;
    bindings.seekAnnotation(element.dataset.annotationId, Number(element.dataset.annotationTime));
  });
  const volumeOf = (target: Element) => Math.max(0, Math.min(1, Number((target as HTMLInputElement).value)));
  on('input', 'input[name="gallery-volume"]', (_event, target) => {
    bindings.setVolume(volumeOf(target), false);
  });
  on('change', 'input[name="gallery-volume"]', (_event, target) => {
    bindings.setVolume(volumeOf(target), true);
  });
  on('click', '[data-action="toggle-gallery-volume"]', (event) => {
    event.stopPropagation();
    bindings.toggleVolume();
  });
  on('click', '[data-action="toggle-gallery-muted"]', (event) => {
    event.stopPropagation();
    bindings.toggleMuted();
  });
  on('keydown', '[data-gallery-range-handle]', (event, target) => {
    const key = (event as KeyboardEvent).key;
    if (key !== 'ArrowLeft' && key !== 'ArrowRight') return;
    event.preventDefault();
    const endpoint = endpointOf(target);
    if (endpoint)
      bindings.nudgeRange((target as HTMLElement).dataset.annotationId, endpoint, key === 'ArrowLeft' ? -100 : 100);
  });
  let gesture:
    { pointerId: number; annotationId: string | undefined; endpoint: GalleryRangeEndpoint; track: DOMRect } | undefined;
  lifetime.add(
    delegateCapture(root, 'pointerdown', '[data-gallery-range-handle]', (event, target) => {
      const endpoint = endpointOf(target),
        annotationId = (target as HTMLElement).dataset.annotationId,
        track = target.closest<HTMLElement>('.attachment-gallery__timeline-track')?.getBoundingClientRect();
      if (!endpoint || !track || !bindings.beginRange(annotationId, endpoint)) return;
      event.preventDefault();
      event.stopPropagation();
      gesture = { pointerId: (event as PointerEvent).pointerId, annotationId, endpoint, track };
    }),
  );
  document.addEventListener(
    'pointermove',
    (event) => {
      if (!gesture || event.pointerId !== gesture.pointerId) return;
      event.preventDefault();
      bindings.dragRange(
        gesture.annotationId,
        gesture.endpoint,
        (event.clientX - gesture.track.left) / gesture.track.width,
      );
    },
    { signal: lifetime.signal },
  );
  const finish = (cancelled: boolean) => (event: PointerEvent) => {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    gesture = undefined;
    if (!cancelled) event.preventDefault();
    bindings.finishRange(cancelled);
  };
  document.addEventListener('pointerup', finish(false), { signal: lifetime.signal });
  document.addEventListener('pointercancel', finish(true), { signal: lifetime.signal });
  return () => {
    lifetime.dispose();
  };
}
