import type { MediaAnnotation } from './api';

export type GalleryAnnotationTool = 'select' | 'rect' | 'freehand' | 'arrow' | 'insertion' | 'strike';
export type AnnotationPoint = { x: number; y: number };
export type AnnotationHandle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

const extent = 10_000;
export const clampAnnotationCoordinate = (value: number) => Math.max(0, Math.min(extent, Math.round(value)));
const point = (x: number, y: number): AnnotationPoint => ({
  x: clampAnnotationCoordinate(x),
  y: clampAnnotationCoordinate(y),
});

export function annotationBounds(points: readonly AnnotationPoint[]) {
  const xs = points.map((item) => item.x),
    ys = points.map((item) => item.y),
    left = Math.min(Math.min(...xs), extent - 1),
    top = Math.min(Math.min(...ys), extent - 1);
  return { x: left, y: top, width: Math.max(1, Math.max(...xs) - left), height: Math.max(1, Math.max(...ys) - top) };
}

export function drawGalleryAnnotation(
  base: MediaAnnotation,
  tool: Exclude<GalleryAnnotationTool, 'select'>,
  start: AnnotationPoint,
  current: AnnotationPoint,
  samples: readonly AnnotationPoint[] = [],
): MediaAnnotation {
  if (tool === 'insertion')
    return { ...base, ...annotationBounds([start]), shape: { type: 'insertion', point: start } };
  if (tool === 'freehand') {
    const points = [start, ...samples, current].map((sample) => point(sample.x, sample.y));
    return { ...base, ...annotationBounds(points), shape: { type: 'freehand', points, closed: true } };
  }
  if (tool === 'arrow') {
    const points = [start, current].map((sample) => point(sample.x, sample.y));
    return { ...base, ...annotationBounds(points), shape: { type: 'arrow', points } };
  }
  return {
    ...base,
    x: Math.min(start.x, current.x),
    y: Math.min(start.y, current.y),
    width: Math.max(1, Math.abs(current.x - start.x)),
    height: Math.max(1, Math.abs(current.y - start.y)),
    shape: { type: tool },
  };
}

export function galleryGestureLargeEnough(
  tool: Exclude<GalleryAnnotationTool, 'select'>,
  points: readonly AnnotationPoint[],
  pixelsPerNormalizedX: number,
  pixelsPerNormalizedY: number,
): boolean {
  if (tool === 'insertion') return true;
  if (points.length < (tool === 'freehand' ? 3 : 2)) return false;
  const bounds = annotationBounds(points),
    width = bounds.width * pixelsPerNormalizedX,
    height = bounds.height * pixelsPerNormalizedY;
  if (tool === 'arrow') return Math.hypot(width, height) >= 12;
  if (tool === 'freehand')
    return new Set(points.map((sample) => `${sample.x},${sample.y}`)).size >= 3 && (width >= 6 || height >= 6);
  return width >= 6 && height >= 6;
}

export function translateGalleryAnnotation(annotation: MediaAnnotation, dx: number, dy: number): MediaAnnotation {
  const movedX = Math.max(-annotation.x, Math.min(extent - annotation.x - annotation.width, Math.round(dx))),
    movedY = Math.max(-annotation.y, Math.min(extent - annotation.y - annotation.height, Math.round(dy))),
    translate = (source: AnnotationPoint) => point(source.x + movedX, source.y + movedY),
    shape = annotation.shape;
  const result: MediaAnnotation = {
    ...annotation,
    x: annotation.x + movedX,
    y: annotation.y + movedY,
    shape:
      shape?.type === 'freehand' || shape?.type === 'arrow'
        ? { ...shape, points: shape.points.map(translate) }
        : shape?.type === 'insertion'
          ? { ...shape, point: translate(shape.point) }
          : shape,
  };
  if (result.shape?.type === 'freehand' || result.shape?.type === 'arrow')
    return { ...result, ...annotationBounds(result.shape.points) };
  if (result.shape?.type === 'insertion') return { ...result, ...annotationBounds([result.shape.point]) };
  return result;
}

export function resizeGalleryAnnotation(
  annotation: MediaAnnotation,
  handle: AnnotationHandle,
  dx: number,
  dy: number,
  minimumX: number,
  minimumY: number,
): MediaAnnotation {
  const old = annotation,
    right = old.x + old.width,
    bottom = old.y + old.height;
  let left = old.x,
    top = old.y,
    newRight = right,
    newBottom = bottom;
  if (handle.includes('w')) left = Math.max(0, Math.min(right - minimumX, Math.round(old.x + dx)));
  if (handle.includes('e')) newRight = Math.min(extent, Math.max(old.x + minimumX, Math.round(right + dx)));
  if (handle.includes('n')) top = Math.max(0, Math.min(bottom - minimumY, Math.round(old.y + dy)));
  if (handle.includes('s')) newBottom = Math.min(extent, Math.max(old.y + minimumY, Math.round(bottom + dy)));
  const next = { ...old, x: left, y: top, width: newRight - left, height: newBottom - top },
    scale = (source: AnnotationPoint) =>
      point(
        left + ((source.x - old.x) / old.width) * next.width,
        top + ((source.y - old.y) / old.height) * next.height,
      );
  if (old.shape?.type === 'freehand' || old.shape?.type === 'arrow') {
    const points = old.shape.points.map(scale);
    return { ...next, ...annotationBounds(points), shape: { ...old.shape, points } };
  }
  if (old.shape?.type === 'insertion') {
    const insertion = scale(old.shape.point);
    return { ...next, ...annotationBounds([insertion]), shape: { type: 'insertion', point: insertion } };
  }
  return next;
}

export function moveGalleryArrowVertex(
  annotation: MediaAnnotation,
  index: number,
  target: AnnotationPoint,
): MediaAnnotation {
  if (annotation.shape?.type !== 'arrow' || index < 0 || index >= annotation.shape.points.length) return annotation;
  const points = annotation.shape.points.map((source, position) => (position === index ? target : source));
  return { ...annotation, ...annotationBounds(points), shape: { type: 'arrow', points } };
}

const pointSegmentDistance = (point: AnnotationPoint, start: AnnotationPoint, end: AnnotationPoint) => {
  const dx = end.x - start.x,
    dy = end.y - start.y,
    length = dx * dx + dy * dy,
    ratio = length ? Math.max(0, Math.min(1, ((point.x - start.x) * dx + (point.y - start.y) * dy) / length)) : 0;
  return Math.hypot(point.x - start.x - ratio * dx, point.y - start.y - ratio * dy);
};
const pointInPolygon = (target: AnnotationPoint, points: readonly AnnotationPoint[]) => {
  let inside = false;
  for (let index = 0, previous = points.length - 1; index < points.length; previous = index++) {
    const left = points[index],
      right = points[previous];
    if (
      left.y > target.y !== right.y > target.y &&
      target.x < ((right.x - left.x) * (target.y - left.y)) / (right.y - left.y) + left.x
    )
      inside = !inside;
  }
  return inside;
};

/** Hit test in normalized coordinates. The tolerance comes from the rendered screen size. */
export function pickGalleryAnnotation(
  annotations: readonly MediaAnnotation[],
  target: AnnotationPoint,
  toleranceX: number,
  toleranceY: number,
): MediaAnnotation | undefined {
  const screen = (source: AnnotationPoint) => ({ x: (source.x * 7) / toleranceX, y: (source.y * 7) / toleranceY }),
    targetScreen = screen(target),
    candidates = annotations.flatMap((annotation, index) => {
      const shape = annotation.shape,
        x = annotation.x,
        y = annotation.y,
        right = x + annotation.width,
        bottom = y + annotation.height,
        inside = target.x >= x && target.x <= right && target.y >= y && target.y <= bottom;
      let distance: number;
      let filled = false;
      if (shape?.type === 'insertion')
        distance = Math.hypot(targetScreen.x - screen(shape.point).x, targetScreen.y - screen(shape.point).y);
      else if (shape?.type === 'freehand' || shape?.type === 'arrow') {
        const points = shape.points.map(screen);
        distance = Math.min(
          ...points.slice(1).map((item, offset) => pointSegmentDistance(targetScreen, points[offset], item)),
          shape.type === 'freehand' && shape.closed !== false && points.length > 2
            ? pointSegmentDistance(targetScreen, points.at(-1)!, points[0])
            : Infinity,
        );
        filled = shape.type === 'freehand' && shape.closed !== false && pointInPolygon(target, shape.points);
      } else {
        distance = inside
          ? Math.min(
              ((target.x - x) * 7) / toleranceX,
              ((right - target.x) * 7) / toleranceX,
              ((target.y - y) * 7) / toleranceY,
              ((bottom - target.y) * 7) / toleranceY,
            )
          : Math.hypot(
              (Math.max(x - target.x, 0, target.x - right) * 7) / toleranceX,
              (Math.max(y - target.y, 0, target.y - bottom) * 7) / toleranceY,
            );
        filled = inside;
      }
      return filled || distance <= 7
        ? [{ annotation, index, distance, area: annotation.width * annotation.height }]
        : [];
    });
  candidates.sort(
    (left, right) => left.distance - right.distance || left.area - right.area || right.index - left.index,
  );
  return candidates[0]?.annotation;
}
