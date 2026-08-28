export interface ReviewRect { id: string; x: number; y: number; width: number; height: number }
export type ResizeHandle = 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w' | 'nw';

export function normalizeRect(id: string, startX: number, startY: number, endX: number, endY: number): ReviewRect {
  return { id, x: Math.min(startX, endX), y: Math.min(startY, endY), width: Math.abs(endX - startX), height: Math.abs(endY - startY) };
}

export function resizeRect(rect: ReviewRect, handle: ResizeHandle, clientX: number, clientY: number, minSize = 24): ReviewRect {
  const right = rect.x + rect.width;
  const bottom = rect.y + rect.height;
  const leftEdge = handle.includes('w') ? Math.min(clientX, right - minSize) : rect.x;
  const rightEdge = handle.includes('e') ? Math.max(clientX, rect.x + minSize) : right;
  const topEdge = handle.includes('n') ? Math.min(clientY, bottom - minSize) : rect.y;
  const bottomEdge = handle.includes('s') ? Math.max(clientY, rect.y + minSize) : bottom;
  return { id: rect.id, x: leftEdge, y: topEdge, width: rightEdge - leftEdge, height: bottomEdge - topEdge };
}

export function clampRectToViewport(rect: ReviewRect, viewportWidth: number, viewportHeight: number): ReviewRect {
  const x = Math.max(0, Math.min(rect.x, viewportWidth - 1));
  const y = Math.max(0, Math.min(rect.y, viewportHeight - 1));
  return { ...rect, x, y, width: Math.max(1, Math.min(rect.width, viewportWidth - x)), height: Math.max(1, Math.min(rect.height, viewportHeight - y)) };
}
