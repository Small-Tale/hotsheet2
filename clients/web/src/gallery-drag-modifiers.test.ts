import { describe, expect, it } from 'vitest';

import {
  constrainGalleryMove,
  drawnGalleryBox,
  galleryDragModifiers,
  NO_DRAG_MODIFIERS,
  resizeGalleryBox,
  snapGalleryPoint,
} from './gallery-drag-modifiers';

const bounds = { width: 800, height: 400 },
  shift = { constrain: true, fromCenter: false },
  option = { constrain: false, fromCenter: true },
  both = { constrain: true, fromCenter: true };

describe('HS2-58BJ0P gallery drag modifiers', () => {
  it('reads Shift as constrain and Option/Alt as from-center', () => {
    expect(galleryDragModifiers({ shiftKey: true, altKey: false })).toEqual(shift);
    expect(galleryDragModifiers({ shiftKey: false, altKey: true })).toEqual(option);
    expect(galleryDragModifiers({ shiftKey: false, altKey: false })).toEqual(NO_DRAG_MODIFIERS);
  });

  it('snaps a line end to the nearest 45° and keeps it inside the media', () => {
    expect(snapGalleryPoint({ x: 300, y: 110 }, { x: 100, y: 100 }, bounds)).toEqual({
      x: 100 + Math.hypot(200, 10),
      y: 100,
    });
    const diagonal = snapGalleryPoint({ x: 200, y: 190 }, { x: 100, y: 100 }, bounds);
    expect(diagonal.x - 100).toBeCloseTo(diagonal.y - 100, 6);
    const vertical = snapGalleryPoint({ x: 104, y: 10 }, { x: 100, y: 100 }, bounds);
    expect(vertical.x).toBeCloseTo(100, 6);
    // A 45° end past the bottom edge is pulled back along the line, not bent.
    const clipped = snapGalleryPoint({ x: 700, y: 700 }, { x: 100, y: 300 }, bounds);
    expect(clipped).toEqual({ x: 200, y: 400 });
    expect(snapGalleryPoint({ x: 100, y: 100 }, { x: 100, y: 100 }, bounds)).toEqual({ x: 100, y: 100 });
  });

  it('draws a square with Shift, a centered box with Option, and both together', () => {
    expect(drawnGalleryBox({ x: 100, y: 100 }, { x: 160, y: 120 }, NO_DRAG_MODIFIERS, bounds)).toEqual({
      x: 100,
      y: 100,
      width: 60,
      height: 20,
    });
    expect(drawnGalleryBox({ x: 100, y: 100 }, { x: 160, y: 120 }, shift, bounds)).toEqual({
      x: 100,
      y: 100,
      width: 60,
      height: 60,
    });
    expect(drawnGalleryBox({ x: 100, y: 100 }, { x: 40, y: 90 }, shift, bounds)).toEqual({
      x: 40,
      y: 40,
      width: 60,
      height: 60,
    });
    expect(drawnGalleryBox({ x: 100, y: 100 }, { x: 160, y: 120 }, option, bounds)).toEqual({
      x: 40,
      y: 80,
      width: 120,
      height: 40,
    });
    expect(drawnGalleryBox({ x: 100, y: 100 }, { x: 160, y: 120 }, both, bounds)).toEqual({
      x: 40,
      y: 40,
      width: 120,
      height: 120,
    });
    // A square stops at the nearest media edge.
    expect(drawnGalleryBox({ x: 700, y: 350 }, { x: 790, y: 395 }, shift, bounds)).toEqual({
      x: 700,
      y: 350,
      width: 50,
      height: 50,
    });
  });

  it('keeps a move horizontal or vertical with Shift', () => {
    expect(constrainGalleryMove(30, 10, shift)).toEqual([30, 0]);
    expect(constrainGalleryMove(-5, 12, shift)).toEqual([0, 12]);
    expect(constrainGalleryMove(30, 10, NO_DRAG_MODIFIERS)).toEqual([30, 10]);
  });

  it('keeps the aspect ratio from a corner and an edge with Shift', () => {
    const rect = { x: 100, y: 100, width: 200, height: 100 };
    // The corner scales both sides by the larger change, anchored at the opposite corner.
    expect(resizeGalleryBox(rect, 'se', { x: 340, y: 210 }, shift, bounds, 6)).toEqual({
      x: 100,
      y: 100,
      width: 240,
      height: 120,
    });
    expect(resizeGalleryBox(rect, 'nw', { x: 60, y: 100 }, shift, bounds, 6)).toEqual({
      x: 60,
      y: 80,
      width: 240,
      height: 120,
    });
    // An edge scales the other side to match, centered on it.
    expect(resizeGalleryBox(rect, 'e', { x: 400, y: 0 }, shift, bounds, 6)).toEqual({
      x: 100,
      y: 75,
      width: 300,
      height: 150,
    });
  });

  it('resizes about the center with Option and shrinks to fit the media instead of sliding', () => {
    const rect = { x: 100, y: 100, width: 200, height: 100 };
    expect(resizeGalleryBox(rect, 'e', { x: 320, y: 0 }, option, bounds, 6)).toEqual({
      x: 80,
      y: 100,
      width: 240,
      height: 100,
    });
    // Growing past the bottom edge with Shift shrinks both sides by one factor about the anchor.
    const fitted = resizeGalleryBox(rect, 'se', { x: 700, y: 600 }, shift, bounds, 6);
    expect(fitted.y + fitted.height).toBeCloseTo(400, 6);
    expect(fitted.width / fitted.height).toBeCloseTo(2, 6);
    expect(fitted.x).toBe(100);
    // The minimum side holds when dragged past the opposite edge.
    const tiny = resizeGalleryBox(rect, 'se', { x: 0, y: 0 }, shift, bounds, 6);
    expect(Math.min(tiny.width, tiny.height)).toBeGreaterThanOrEqual(6 - 1e-9);
  });
});
