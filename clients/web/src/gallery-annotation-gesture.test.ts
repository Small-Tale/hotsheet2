import { describe, expect, it } from 'vitest';

import type { MediaAnnotation } from './api';
import {
  drawGalleryAnnotation,
  moveGalleryArrowVertex,
  resizeGalleryAnnotation,
  translateGalleryAnnotation,
} from './gallery-annotation-editor';
import { type GalleryAnnotationGestureState, galleryGestureAnnotation } from './gallery-annotation-gesture';
import { NO_DRAG_MODIFIERS } from './gallery-drag-modifiers';

// A 1000×500 px surface at (100, 50): one normalized unit is 0.1 px across and 0.05 px down.
const surface = { left: 100, top: 50, width: 1000, height: 500 },
  shift = { constrain: true, fromCenter: false },
  option = { constrain: false, fromCenter: true },
  base: MediaAnnotation = { id: 'a', x: 2000, y: 2000, width: 1, height: 1, text: '' };
const gesture = (overrides: Partial<GalleryAnnotationGestureState>): GalleryAnnotationGestureState => ({
  kind: 'draw',
  startX: 300,
  startY: 150,
  surface,
  annotation: base,
  tool: 'rect',
  samples: [],
  startPoint: { x: 2000, y: 2000 },
  ...overrides,
});
const pixelAspect = (annotation: MediaAnnotation | undefined) =>
  ((annotation!.width / 10_000) * surface.width) / ((annotation!.height / 10_000) * surface.height);

describe('HS2-58BJ0P gallery annotation gesture', () => {
  it('matches the plain draw, move, resize and vertex drag without modifiers', () => {
    expect(galleryGestureAnnotation(gesture({}), 400, 250, NO_DRAG_MODIFIERS)).toEqual(
      drawGalleryAnnotation(base, 'rect', { x: 2000, y: 2000 }, { x: 3000, y: 4000 }),
    );
    const rect: MediaAnnotation = { ...base, x: 2000, y: 2000, width: 2000, height: 2000, shape: { type: 'rect' } };
    expect(galleryGestureAnnotation(gesture({ kind: 'move', annotation: rect }), 330, 160, NO_DRAG_MODIFIERS)).toEqual(
      translateGalleryAnnotation(rect, 300, 200),
    );
    expect(
      galleryGestureAnnotation(
        gesture({ kind: 'resize', annotation: rect, handle: 'se' }),
        330,
        160,
        NO_DRAG_MODIFIERS,
      ),
    ).toEqual(resizeGalleryAnnotation(rect, 'se', 300, 200, 60, 120));
    const arrow: MediaAnnotation = {
      ...base,
      x: 1000,
      y: 1000,
      width: 2000,
      height: 2000,
      shape: {
        type: 'arrow',
        points: [
          { x: 1000, y: 1000 },
          { x: 3000, y: 3000 },
        ],
      },
    };
    expect(
      galleryGestureAnnotation(
        gesture({ kind: 'resize', annotation: arrow, handle: 'point-1' }),
        500,
        200,
        NO_DRAG_MODIFIERS,
      ),
    ).toEqual(moveGalleryArrowVertex(arrow, 1, { x: 4000, y: 3000 }));
    expect(galleryGestureAnnotation(gesture({ tool: 'select' }), 400, 250, NO_DRAG_MODIFIERS)).toBeUndefined();
  });

  it('draws a straight arrow and a square box with Shift, a centered box with Option', () => {
    const arrow = galleryGestureAnnotation(gesture({ tool: 'arrow' }), 500, 160, shift);
    expect(arrow?.shape).toEqual({
      type: 'arrow',
      points: [{ x: 2000, y: 2000 }, expect.objectContaining({ y: 2000 })],
    });
    expect(pixelAspect(galleryGestureAnnotation(gesture({}), 400, 160, shift))).toBeCloseTo(1, 1);
    const centered = galleryGestureAnnotation(gesture({}), 400, 250, option);
    expect(centered).toMatchObject({ x: 1000, y: 0, width: 2000, height: 4000 });
  });

  it('keeps a move horizontal and a resize at its aspect ratio with Shift', () => {
    const rect: MediaAnnotation = { ...base, x: 2000, y: 2000, width: 2000, height: 2000, shape: { type: 'rect' } };
    expect(galleryGestureAnnotation(gesture({ kind: 'move', annotation: rect }), 400, 160, shift)).toMatchObject({
      x: 3000,
      y: 2000,
    });
    const freehand: MediaAnnotation = {
      ...rect,
      shape: {
        type: 'freehand',
        closed: true,
        points: [
          { x: 2000, y: 2000 },
          { x: 4000, y: 3000 },
          { x: 3000, y: 4000 },
        ],
      },
    };
    const before = pixelAspect(freehand),
      resized = galleryGestureAnnotation(
        gesture({ kind: 'resize', annotation: freehand, handle: 'se' }),
        400,
        160,
        shift,
      );
    expect(resized!.width).toBeGreaterThan(freehand.width);
    expect(pixelAspect(resized)).toBeCloseTo(before, 1);
  });

  it('snaps a dragged arrow end to 45° from the point it joins with Shift', () => {
    const arrow: MediaAnnotation = {
      ...base,
      x: 1000,
      y: 1000,
      width: 2000,
      height: 2000,
      shape: {
        type: 'arrow',
        points: [
          { x: 1000, y: 1000 },
          { x: 3000, y: 3000 },
        ],
      },
    };
    // The start sits at (200, 100) px; a pointer at (499, 110) px snaps onto its horizontal line.
    const moved = galleryGestureAnnotation(
      gesture({ kind: 'resize', annotation: arrow, handle: 'point-1' }),
      599,
      160,
      shift,
    );
    expect(moved?.shape).toEqual({
      type: 'arrow',
      points: [{ x: 1000, y: 1000 }, expect.objectContaining({ y: 1000 })],
    });
  });
});
