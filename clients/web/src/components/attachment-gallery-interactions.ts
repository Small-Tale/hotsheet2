import { openAnchoredSurface } from '@kerfjs/ui/anchored-surface';
import { delegate, delegateCapture, effect, type SafeHtml, type Signal } from 'kerfjs';
import type { OverlayHandle } from 'kerfjs/overlay';
import { createScope } from 'kerfjs/scope';

import {
  ATTACHMENTS_AND_GALLERY_ACTIONS,
  ATTACHMENTS_AND_GALLERY_TARGETS,
} from '../interaction-attrs/attachments-and-gallery';
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
  /** Cancel a pending one-finger gallery swipe when a second finger begins zooming. */
  readonly beginPinch: () => void;
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
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.closeAttachmentGallery.selector, () => {
      bindings.close();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.previousGalleryImage.selector, () => {
      bindings.shift(-1);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.nextGalleryImage.selector, () => {
      bindings.shift(1);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.zoomGalleryImage.selector, (_event, target) => {
      bindings.zoom((target as HTMLElement).dataset.zoomDirection === 'out' ? 'out' : 'in');
    }),
  );
  // Only gestures beginning over the scrollable media stage own gallery zoom. The surrounding
  // toolbar stays at its normal size, and one-finger stage panning remains native.
  let touchDistance: number | undefined;
  let safariGestureScale: number | undefined;
  let pinchSource: 'touch' | 'gesture' | undefined;
  const overStage = (event: Event) =>
    Boolean((event.target as Element | null)?.closest('[data-gallery-zoom-stage="true"]'));
  const distance = (touches: TouchList) =>
    Math.hypot(touches[0].clientX - touches[1].clientX, touches[0].clientY - touches[1].clientY);
  const step = (value: number, anchor: number): number => {
    if (value >= anchor * 1.18) {
      bindings.zoom('in');
      return value;
    }
    if (value <= anchor / 1.18) {
      bindings.zoom('out');
      return value;
    }
    return anchor;
  };
  root.addEventListener(
    'touchstart',
    (event) => {
      if (event.touches.length !== 2 || !overStage(event)) return;
      touchDistance = distance(event.touches);
      pinchSource = undefined;
      bindings.beginPinch();
      if (event.cancelable) event.preventDefault();
    },
    { passive: false, signal: lifetime.signal },
  );
  root.addEventListener(
    'touchmove',
    (event) => {
      if (touchDistance === undefined || event.touches.length !== 2) return;
      if (event.cancelable) event.preventDefault();
      if (pinchSource !== 'gesture') {
        pinchSource = 'touch';
        touchDistance = step(distance(event.touches), touchDistance);
      }
    },
    { passive: false, signal: lifetime.signal },
  );
  const endTouch = (event: TouchEvent) => {
    if (event.touches.length < 2) {
      touchDistance = undefined;
      pinchSource = undefined;
    }
  };
  root.addEventListener('touchend', endTouch, { signal: lifetime.signal });
  root.addEventListener('touchcancel', endTouch, { signal: lifetime.signal });
  // iOS Safari also sends gesture events; suppress its page magnification. When WebKit
  // supplies gesturechange without touchmove, use its scale as the fallback input.
  root.addEventListener(
    'gesturestart',
    (event) => {
      if (!overStage(event)) return;
      safariGestureScale = 1;
      if (touchDistance === undefined) bindings.beginPinch();
      if (event.cancelable) event.preventDefault();
    },
    { passive: false, signal: lifetime.signal },
  );
  root.addEventListener(
    'gesturechange',
    (event) => {
      if (safariGestureScale === undefined) return;
      if (event.cancelable) event.preventDefault();
      if (pinchSource === 'touch') return;
      const scale = (event as Event & { scale?: number }).scale;
      if (scale && Number.isFinite(scale)) {
        pinchSource = 'gesture';
        safariGestureScale = step(scale, safariGestureScale);
      }
    },
    { passive: false, signal: lifetime.signal },
  );
  root.addEventListener(
    'gestureend',
    () => {
      safariGestureScale = undefined;
    },
    { signal: lifetime.signal },
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryMarkup.selector, () => {
      bindings.toggleMarkup();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryDraw.selector, () => {
      bindings.toggleDraw();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryPlayback.selector, () => {
      bindings.togglePlayback();
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', ATTACHMENTS_AND_GALLERY_TARGETS.attachmentGallery.selector, (event) => {
      const keyboard = event as KeyboardEvent,
        origin = event.target as Element,
        playheadControl = origin.matches(ATTACHMENTS_AND_GALLERY_TARGETS.galleryPlayheadField.selector);
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
    }),
  );
  lifetime.add(
    delegate(root, 'input', ATTACHMENTS_AND_GALLERY_TARGETS.galleryPlayheadField.selector, (_event, target) => {
      bindings.seek(Number((target as HTMLInputElement).value), 'scrub');
    }),
  );
  lifetime.add(
    delegate(root, 'change', ATTACHMENTS_AND_GALLERY_TARGETS.galleryPlayheadField.selector, (_event, target) => {
      bindings.seek(Number((target as HTMLInputElement).value), 'commit');
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.seekGalleryAnnotation.selector, (_event, target) => {
      const element = target as HTMLElement;
      bindings.seekAnnotation(element.dataset.annotationId, Number(element.dataset.annotationTime));
    }),
  );
  const volumeOf = (target: Element) => Math.max(0, Math.min(1, Number((target as HTMLInputElement).value)));
  lifetime.add(
    delegate(root, 'input', ATTACHMENTS_AND_GALLERY_TARGETS.galleryVolumeField.selector, (_event, target) => {
      bindings.setVolume(volumeOf(target), false);
    }),
  );
  lifetime.add(
    delegate(root, 'change', ATTACHMENTS_AND_GALLERY_TARGETS.galleryVolumeField.selector, (_event, target) => {
      bindings.setVolume(volumeOf(target), true);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryVolume.selector, (event) => {
      event.stopPropagation();
      bindings.toggleVolume();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryMuted.selector, (event) => {
      event.stopPropagation();
      bindings.toggleMuted();
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', '[data-gallery-range-handle]', (event, target) => {
      const key = (event as KeyboardEvent).key;
      if (key !== 'ArrowLeft' && key !== 'ArrowRight') return;
      event.preventDefault();
      const endpoint = endpointOf(target);
      if (endpoint)
        bindings.nudgeRange((target as HTMLElement).dataset.annotationId, endpoint, key === 'ArrowLeft' ? -100 : 100);
    }),
  );
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

/**
 * Present the gallery volume popup in a Kerf dark-tone AnchoredSurface above the volume trigger while
 * `open` is true (KF-MN71AB, HS2-HTRFDS). Kerf owns the chrome, positioning, Escape/outside dismissal,
 * and top-layer hosting; `render` supplies the reactive content (`GalleryVolumePanel`). Kerf hosts the
 * surface outside the gallery, so its slider and mute row are wired on the surface itself and stop
 * propagation; production and the UX demo share this so the surface options cannot drift.
 */
export function wireGalleryVolumeSurface(
  root: ParentNode,
  open: Signal<boolean>,
  render: () => SafeHtml,
  bindings: Pick<AttachmentGalleryControlBindings, 'setVolume' | 'toggleMuted'>,
): () => void {
  let handle: OverlayHandle | undefined, surfaceScope: ReturnType<typeof createScope> | undefined;
  const close = () => {
    const current = handle;
    handle = undefined;
    surfaceScope?.dispose();
    surfaceScope = undefined;
    current?.close();
  };
  const volumeOf = (target: Element) => Math.max(0, Math.min(1, Number((target as HTMLInputElement).value)));
  const dispose = effect(() => {
    if (!open.value) {
      close();
      return;
    }
    if (handle) return;
    const trigger = root.querySelector(ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryVolume.selector);
    if (!trigger) return;
    handle = openAnchoredSurface(trigger, render, {
      label: 'Volume Controls',
      tone: 'dark',
      placement: 'top',
      align: 'end',
      dismiss: ['escape', 'outside'],
      onDismiss: () => {
        surfaceScope?.dispose();
        surfaceScope = undefined;
        handle = undefined;
        open.value = false;
      },
    });
    const surface = handle.el,
      scope = createScope();
    surfaceScope = scope;
    for (const [type, commit] of [
      ['input', false],
      ['change', true],
    ] as const)
      scope.add(
        delegate(surface, type, ATTACHMENTS_AND_GALLERY_TARGETS.galleryVolumeField.selector, (event, target) => {
          event.stopPropagation();
          bindings.setVolume(volumeOf(target), commit);
        }),
      );
    scope.add(
      delegate(surface, 'click', ATTACHMENTS_AND_GALLERY_ACTIONS.toggleGalleryMuted.selector, (event) => {
        event.stopPropagation();
        bindings.toggleMuted();
      }),
    );
  });
  return () => {
    dispose();
    close();
  };
}
