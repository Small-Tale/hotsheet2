/**
 * Modifier keys during a gallery annotation drag (HS2-58BJ0P), matching the ux-review annotation
 * editor and other drawing apps: Shift constrains (a resize keeps the aspect ratio, a new box is a
 * square, an arrow end snaps to the nearest 45°, a move stays horizontal or vertical) and Option/Alt
 * works from the center (a resize moves the opposite side too, a new box grows from where the drag
 * began). All geometry is in rendered pixels, because normalized annotation axes differ in scale.
 */

export interface GalleryDragModifiers {
  /** Shift. */
  constrain: boolean;
  /** Option/Alt. */
  fromCenter: boolean;
}

export const NO_DRAG_MODIFIERS: GalleryDragModifiers = { constrain: false, fromCenter: false };

export function galleryDragModifiers(event: { shiftKey: boolean; altKey: boolean }): GalleryDragModifiers {
  return { constrain: event.shiftKey, fromCenter: event.altKey };
}

export interface PixelPoint {
  x: number;
  y: number;
}
export interface PixelRect {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface PixelBounds {
  width: number;
  height: number;
}
export type GalleryBoxHandle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

/**
 * `point` moved onto the nearest 45° line through `anchor`, at the same distance along it, and pulled
 * back inside `bounds` along that line.
 */
export function snapGalleryPoint(point: PixelPoint, anchor: PixelPoint, bounds: PixelBounds): PixelPoint {
  const dx = point.x - anchor.x,
    dy = point.y - anchor.y,
    length = Math.hypot(dx, dy);
  if (!length) return point;
  const step = Math.PI / 4,
    angle = Math.round(Math.atan2(dy, dx) / step) * step,
    unitX = Math.round(Math.cos(angle) * 1e12) / 1e12,
    unitY = Math.round(Math.sin(angle) * 1e12) / 1e12;
  let reach = length;
  if (unitX > 0) reach = Math.min(reach, (bounds.width - anchor.x) / unitX);
  if (unitX < 0) reach = Math.min(reach, -anchor.x / unitX);
  if (unitY > 0) reach = Math.min(reach, (bounds.height - anchor.y) / unitY);
  if (unitY < 0) reach = Math.min(reach, -anchor.y / unitY);
  return { x: anchor.x + unitX * reach, y: anchor.y + unitY * reach };
}

/**
 * The box a new rectangle or strike covers while dragging from `anchor` to `point`: Shift makes it a
 * square (the larger side), Option centers it on `anchor`. It stays inside `bounds`.
 */
export function drawnGalleryBox(
  anchor: PixelPoint,
  point: PixelPoint,
  modifiers: GalleryDragModifiers,
  bounds: PixelBounds,
): PixelRect {
  let dx = point.x - anchor.x,
    dy = point.y - anchor.y;
  const roomX = modifiers.fromCenter
      ? Math.min(anchor.x, bounds.width - anchor.x)
      : dx < 0
        ? anchor.x
        : bounds.width - anchor.x,
    roomY = modifiers.fromCenter
      ? Math.min(anchor.y, bounds.height - anchor.y)
      : dy < 0
        ? anchor.y
        : bounds.height - anchor.y;
  if (modifiers.constrain) {
    const length = Math.min(Math.max(Math.abs(dx), Math.abs(dy)), roomX, roomY);
    dx = (dx < 0 ? -1 : 1) * length;
    dy = (dy < 0 ? -1 : 1) * length;
  } else {
    dx = (dx < 0 ? -1 : 1) * Math.min(Math.abs(dx), roomX);
    dy = (dy < 0 ? -1 : 1) * Math.min(Math.abs(dy), roomY);
  }
  if (modifiers.fromCenter)
    return {
      x: anchor.x - Math.abs(dx),
      y: anchor.y - Math.abs(dy),
      width: Math.abs(dx) * 2,
      height: Math.abs(dy) * 2,
    };
  return {
    x: Math.min(anchor.x, anchor.x + dx),
    y: Math.min(anchor.y, anchor.y + dy),
    width: Math.abs(dx),
    height: Math.abs(dy),
  };
}

/** Shift keeps a move horizontal or vertical, whichever is larger. */
export function constrainGalleryMove(dx: number, dy: number, modifiers: GalleryDragModifiers): [number, number] {
  if (!modifiers.constrain) return [dx, dy];
  return Math.abs(dx) >= Math.abs(dy) ? [dx, 0] : [0, dy];
}

/**
 * `rect` resized by dragging `handle` to `point` with modifiers held: Option moves the opposite edges by
 * the same amount, about the center; Shift keeps `rect`'s aspect ratio (a corner scales both sides by
 * the larger change; an edge scales the other side to match, centered on it). The box stays at least
 * `minimumSide` and inside `bounds`, shrinking about its fixed point instead of sliding. Without
 * modifiers the caller keeps its plain resize.
 */
export function resizeGalleryBox(
  rect: PixelRect,
  handle: GalleryBoxHandle,
  point: PixelPoint,
  modifiers: GalleryDragModifiers,
  bounds: PixelBounds,
  minimumSide: number,
): PixelRect {
  const side = Math.min(minimumSide, bounds.width, bounds.height),
    center = { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 },
    movesMinX = handle.includes('w'),
    movesMaxX = handle.includes('e'),
    movesMinY = handle.includes('n'),
    movesMaxY = handle.includes('s'),
    movesX = movesMinX || movesMaxX,
    movesY = movesMinY || movesMaxY;
  let minX = rect.x,
    maxX = rect.x + rect.width,
    minY = rect.y,
    maxY = rect.y + rect.height;
  if (movesX) {
    if (modifiers.fromCenter) {
      const half = Math.max(Math.abs(point.x - center.x), side / 2);
      [minX, maxX] = [center.x - half, center.x + half];
    } else if (movesMinX) minX = Math.min(point.x, maxX - side);
    else maxX = Math.max(point.x, minX + side);
  }
  if (movesY) {
    if (modifiers.fromCenter) {
      const half = Math.max(Math.abs(point.y - center.y), side / 2);
      [minY, maxY] = [center.y - half, center.y + half];
    } else if (movesMinY) minY = Math.min(point.y, maxY - side);
    else maxY = Math.max(point.y, minY + side);
  }
  // The point the box grows from: the center with Option, else the opposite edge (or, along an axis the
  // handle does not move, the middle, so an edge keeps the box centered there).
  const anchor = {
    x: modifiers.fromCenter || !movesX ? center.x : movesMinX ? rect.x + rect.width : rect.x,
    y: modifiers.fromCenter || !movesY ? center.y : movesMinY ? rect.y + rect.height : rect.y,
  };
  let width = maxX - minX,
    height = maxY - minY;
  const constrain = modifiers.constrain && rect.width > 0 && rect.height > 0;
  if (!constrain) return fitBox({ x: minX, y: minY, width, height }, anchor, bounds, false);
  const aspect = rect.width / rect.height;
  if (movesX && movesY) {
    const scale = Math.max(width / rect.width, height / rect.height);
    [width, height] = [rect.width * scale, rect.height * scale];
  } else if (movesX) height = width / aspect;
  else width = height * aspect;
  const grow = Math.max(side / width, side / height, 1);
  [width, height] = [width * grow, height * grow];
  const leftShare = modifiers.fromCenter || !movesX ? 0.5 : movesMinX ? 1 : 0,
    topShare = modifiers.fromCenter || !movesY ? 0.5 : movesMinY ? 1 : 0;
  return fitBox(
    { x: anchor.x - width * leftShare, y: anchor.y - height * topShare, width, height },
    anchor,
    bounds,
    true,
  );
}

/** `box` shrunk about `anchor` until it fits `bounds`: both sides by one factor when `uniform`. */
function fitBox(box: PixelRect, anchor: PixelPoint, bounds: PixelBounds, uniform: boolean): PixelRect {
  const factor = (before: number, after: number, roomBefore: number, roomAfter: number) => {
    let result = 1;
    if (before > roomBefore && before > 0) result = Math.min(result, Math.max(roomBefore, 0) / before);
    if (after > roomAfter && after > 0) result = Math.min(result, Math.max(roomAfter, 0) / after);
    return result;
  };
  const left = anchor.x - box.x,
    right = box.x + box.width - anchor.x,
    top = anchor.y - box.y,
    bottom = box.y + box.height - anchor.y;
  let factorX = factor(left, right, anchor.x, bounds.width - anchor.x),
    factorY = factor(top, bottom, anchor.y, bounds.height - anchor.y);
  if (uniform) factorX = factorY = Math.min(factorX, factorY);
  return {
    x: anchor.x - left * factorX,
    y: anchor.y - top * factorY,
    width: (left + right) * factorX,
    height: (top + bottom) * factorY,
  };
}
