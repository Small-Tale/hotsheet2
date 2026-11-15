/** Whether `style` makes its element the containing block of `position: fixed` descendants. */
function containsFixedDescendants(style: CSSStyleDeclaration): boolean {
  const contain = style.contain || 'none';
  return (
    (style.transform || 'none') !== 'none' ||
    (style.perspective || 'none') !== 'none' ||
    (style.filter || 'none') !== 'none' ||
    (style.containerType || 'normal') !== 'normal' ||
    /\b(layout|paint|strict|content)\b/.test(contain) ||
    /\b(transform|perspective|filter)\b/.test(style.willChange || '')
  );
}

/**
 * Translate a viewport point into the coordinate space `position: fixed` uses for a child of `parent`.
 *
 * The UX catalog's app-height example frame stands in for the screen, so Kerf gives it an identity
 * transform and it becomes the containing block of screen-fixed chrome. A production context menu
 * anchors its `position: fixed` point at viewport coordinates; inside that frame the anchor lands
 * offset by the frame's own position, which moves with the catalog's layout and scroll, and the menu
 * can open outside the viewport (HS2-CY6A6K). A demo passes its menu point through here first.
 */
export function pointInFixedContainingBlock(parent: Element, x: number, y: number): { x: number; y: number } {
  const view = parent.ownerDocument.defaultView;
  if (!view) return { x, y };
  for (let node: Element | null = parent; node; node = node.parentElement) {
    if (!containsFixedDescendants(view.getComputedStyle(node))) continue;
    const box = node.getBoundingClientRect();
    return { x: x - box.left - node.clientLeft, y: y - box.top - node.clientTop };
  }
  return { x, y };
}
