import { describe, expect, it } from 'vitest';

import type { MediaAnnotation } from './api';
import {
  annotationBounds,
  drawGalleryAnnotation,
  galleryGestureLargeEnough,
  moveGalleryArrowVertex,
  pickGalleryAnnotation,
  resizeGalleryAnnotation,
  translateGalleryAnnotation,
} from './gallery-annotation-editor';

const base: MediaAnnotation = { id: 'one', x: 1000, y: 1000, width: 1, height: 1, text: '' };
const point = (x: number, y: number) => ({ x, y });

describe('gallery annotation geometry', () => {
  it('draws every shape with the exact persisted bounds, including edge points', () => {
    const rect = drawGalleryAnnotation(base, 'rect', point(4000, 5000), point(2000, 3000)),
      strike = drawGalleryAnnotation(base, 'strike', point(2000, 3000), point(4000, 5000)),
      arrow = drawGalleryAnnotation(base, 'arrow', point(2000, 5000), point(4000, 3000)),
      freehand = drawGalleryAnnotation(base, 'freehand', point(1000, 1000), point(3000, 1000), [point(2000, 3000)]),
      insertion = drawGalleryAnnotation(base, 'insertion', point(10_000, 10_000), point(10_000, 10_000));
    expect(rect).toMatchObject({ x: 2000, y: 3000, width: 2000, height: 2000, shape: { type: 'rect' } });
    expect(strike.shape).toEqual({ type: 'strike' });
    expect(arrow).toMatchObject({ x: 2000, y: 3000, width: 2000, height: 2000 });
    expect(freehand).toMatchObject({ x: 1000, y: 1000, width: 2000, height: 2000 });
    expect(insertion).toMatchObject({
      x: 9999,
      y: 9999,
      width: 1,
      height: 1,
      shape: { point: { x: 10_000, y: 10_000 } },
    });
  });

  it('rejects tiny gestures and freehand paths without three distinct points', () => {
    expect(galleryGestureLargeEnough('rect', [point(100, 100), point(150, 150)], 0.01, 0.01)).toBe(false);
    expect(galleryGestureLargeEnough('arrow', [point(100, 100), point(1300, 100)], 0.01, 0.01)).toBe(true);
    expect(galleryGestureLargeEnough('freehand', [point(100, 100), point(100, 100), point(700, 800)], 0.01, 0.01)).toBe(
      false,
    );
    expect(galleryGestureLargeEnough('insertion', [point(100, 100)], 0.01, 0.01)).toBe(true);
  });

  it('moves and resizes point shapes without drifting their required bounding box', () => {
    const arrow = drawGalleryAnnotation(base, 'arrow', point(9000, 8000), point(10_000, 10_000)),
      stopped = translateGalleryAnnotation(arrow, 2000, 2000),
      moved = translateGalleryAnnotation(arrow, -1000, -1000),
      vertex = moveGalleryArrowVertex(moved, 1, point(4000, 5000)),
      resized = resizeGalleryAnnotation(vertex, 'nw', -1000, -1000, 60, 60);
    expect(stopped).toEqual(arrow);
    expect(moved.shape).toEqual({ type: 'arrow', points: [point(8000, 7000), point(9000, 9000)] });
    expect(vertex).toMatchObject(annotationBounds((vertex.shape as { points: { x: number; y: number }[] }).points));
    expect(resized).toMatchObject(annotationBounds((resized.shape as { points: { x: number; y: number }[] }).points));
    const freehand = drawGalleryAnnotation(base, 'freehand', point(1000, 1000), point(3000, 3000), [point(2000, 3000)]),
      scaled = resizeGalleryAnnotation(freehand, 'se', 1400, 900, 60, 60);
    expect(scaled).toMatchObject(annotationBounds((scaled.shape as { points: { x: number; y: number }[] }).points));
  });

  it('picks a smaller overlapping shape and rejects empty arrow bounds', () => {
    const outer = drawGalleryAnnotation(base, 'rect', point(1000, 1000), point(9000, 9000)),
      inner = drawGalleryAnnotation({ ...base, id: 'inner' }, 'rect', point(4000, 4000), point(6000, 6000)),
      arrow = drawGalleryAnnotation({ ...base, id: 'arrow' }, 'arrow', point(1000, 2000), point(9000, 8000));
    expect(pickGalleryAnnotation([outer, inner], point(5000, 5000), 70, 70)?.id).toBe('inner');
    expect(pickGalleryAnnotation([arrow], point(1500, 7500), 70, 70)).toBeUndefined();
    expect(pickGalleryAnnotation([arrow], point(5000, 5000), 70, 70)?.id).toBe('arrow');
    const triangle = drawGalleryAnnotation(base, 'freehand', point(1000, 1000), point(9000, 9000), [point(1000, 9000)]);
    expect(pickGalleryAnnotation([triangle], point(8000, 2000), 70, 70)).toBeUndefined();
    expect(pickGalleryAnnotation([triangle], point(2500, 5000), 70, 70)?.id).toBe('one');
  });
});
