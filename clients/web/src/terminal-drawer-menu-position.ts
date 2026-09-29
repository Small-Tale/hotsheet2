export interface DrawerCreateMenuPosition {
  side: 'above' | 'below';
  left: number;
  maxHeight: number;
}

/** Place the drawer's create menu beside its rail button without crossing viewport edges. */
export function drawerCreateMenuPosition(
  trigger: Pick<DOMRect, 'top' | 'bottom' | 'left'>,
  viewport: { width: number; height: number },
  menuWidth = 224,
  margin = 8,
  gap = 8,
): DrawerCreateMenuPosition {
  const above = Math.max(0, trigger.top - margin - gap),
    below = Math.max(0, viewport.height - trigger.bottom - margin - gap),
    side = above >= below ? 'above' : 'below',
    width = Math.min(menuWidth, Math.max(0, viewport.width - margin * 2)),
    absoluteLeft = Math.max(margin, Math.min(trigger.left, viewport.width - margin - width));
  return { side, left: absoluteLeft - trigger.left, maxHeight: side === 'above' ? above : below };
}
