/**
 * One step of a gallery annotation drag (draw, move, or resize), computed from the pointer and the
 * modifier keys held (HS2-58BJ0P). The interaction layer calls it on every pointer move and again when
 * Shift or Option/Alt is pressed or released mid-drag, so the shape reshapes at once.
 */
import type { MediaAnnotation } from './api';
import {
  type AnnotationHandle,
  type AnnotationPoint,
  clampAnnotationCoordinate,
  drawGalleryAnnotation,
  fitGalleryAnnotationToBox,
  type GalleryAnnotationTool,
  moveGalleryArrowVertex,
  resizeGalleryAnnotation,
  translateGalleryAnnotation,
} from './gallery-annotation-editor';
import {
  constrainGalleryMove,
  drawnGalleryBox,
  type GalleryDragModifiers,
  type PixelPoint,
  resizeGalleryBox,
  snapGalleryPoint,
} from './gallery-drag-modifiers';

const EXTENT = 10_000;
/** The smallest box side a drag keeps, in rendered pixels. */
const MINIMUM_SIDE = 6;

export interface GalleryAnnotationGestureState {
  kind: 'draw' | 'move' | 'resize';
  startX: number;
  startY: number;
  surface: { left: number; top: number; width: number; height: number };
  annotation: MediaAnnotation;
  handle?: string;
  tool: GalleryAnnotationTool;
  samples: AnnotationPoint[];
  startPoint: AnnotationPoint;
}

/**
 * The annotation after the pointer reached `clientX`/`clientY` with `modifiers` held, or undefined when
 * the gesture changes nothing (a select-tool draw).
 */
export function galleryGestureAnnotation(
  gesture: GalleryAnnotationGestureState,
  clientX: number,
  clientY: number,
  modifiers: GalleryDragModifiers,
): MediaAnnotation | undefined {
  const { surface, annotation: base } = gesture,
    bounds = { width: surface.width, height: surface.height },
    toPixel = (source: AnnotationPoint): PixelPoint => ({
      x: (source.x * surface.width) / EXTENT,
      y: (source.y * surface.height) / EXTENT,
    }),
    toNormalized = (source: PixelPoint): AnnotationPoint => ({
      x: clampAnnotationCoordinate((source.x * EXTENT) / surface.width),
      y: clampAnnotationCoordinate((source.y * EXTENT) / surface.height),
    }),
    pointer = { x: clientX - surface.left, y: clientY - surface.top },
    clamped = {
      x: Math.max(0, Math.min(surface.width, pointer.x)),
      y: Math.max(0, Math.min(surface.height, pointer.y)),
    },
    point = toNormalized(clamped),
    modified = modifiers.constrain || modifiers.fromCenter;
  if (gesture.kind === 'draw') {
    if (gesture.tool === 'select') return undefined;
    if (modified && (gesture.tool === 'rect' || gesture.tool === 'strike')) {
      const box = drawnGalleryBox(toPixel(gesture.startPoint), clamped, modifiers, bounds);
      return drawGalleryAnnotation(
        base,
        gesture.tool,
        toNormalized({ x: box.x, y: box.y }),
        toNormalized({ x: box.x + box.width, y: box.y + box.height }),
      );
    }
    const end =
      modifiers.constrain && gesture.tool === 'arrow'
        ? toNormalized(snapGalleryPoint(clamped, toPixel(gesture.startPoint), bounds))
        : point;
    return drawGalleryAnnotation(base, gesture.tool, gesture.startPoint, end, gesture.samples, surface);
  }
  const [moveX, moveY] = constrainGalleryMove(clientX - gesture.startX, clientY - gesture.startY, modifiers),
    dx = (moveX * EXTENT) / surface.width,
    dy = (moveY * EXTENT) / surface.height;
  if (gesture.kind === 'move') return translateGalleryAnnotation(base, dx, dy);
  const handle = gesture.handle ?? '',
    vertex = handle.startsWith('point-') ? Number(handle.slice(6)) : -1;
  if (vertex >= 0) {
    const points = base.shape?.type === 'arrow' ? base.shape.points : [],
      neighborIndex = vertex > 0 ? vertex - 1 : vertex + 1,
      neighbor = neighborIndex < points.length ? points[neighborIndex] : undefined;
    return moveGalleryArrowVertex(
      base,
      vertex,
      modifiers.constrain && neighbor ? toNormalized(snapGalleryPoint(clamped, toPixel(neighbor), bounds)) : point,
    );
  }
  const boxHandle = handle as AnnotationHandle;
  if (!modified)
    return resizeGalleryAnnotation(
      base,
      boxHandle,
      dx,
      dy,
      (MINIMUM_SIDE * EXTENT) / surface.width,
      (MINIMUM_SIDE * EXTENT) / surface.height,
    );
  // The dragged handle starts on the box edge and moves with the pointer.
  const origin = toPixel({ x: base.x, y: base.y }),
    size = toPixel({ x: base.width, y: base.height }),
    rect = { x: origin.x, y: origin.y, width: size.x, height: size.y },
    target = {
      x: (boxHandle.includes('w') ? rect.x : rect.x + rect.width) + (clientX - gesture.startX),
      y: (boxHandle.includes('n') ? rect.y : rect.y + rect.height) + (clientY - gesture.startY),
    },
    next = resizeGalleryBox(
      rect,
      boxHandle,
      { x: Math.max(0, Math.min(surface.width, target.x)), y: Math.max(0, Math.min(surface.height, target.y)) },
      modifiers,
      bounds,
      MINIMUM_SIDE,
    ),
    start = toNormalized({ x: next.x, y: next.y }),
    end = toNormalized({ x: next.x + next.width, y: next.y + next.height });
  return fitGalleryAnnotationToBox(base, {
    x: start.x,
    y: start.y,
    width: Math.max(1, end.x - start.x),
    height: Math.max(1, end.y - start.y),
  });
}
