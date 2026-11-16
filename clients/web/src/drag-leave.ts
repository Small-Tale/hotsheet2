/**
 * Whether a `dragleave` fired on `element` or one of its descendants means the drag left `element`
 * itself (HS2-1BBXER). Moving between an element's children fires `dragleave` on the child left
 * behind, so a delegated handler must not clear the element's drop state for it. The entered node
 * (`relatedTarget`) decides when the browser reports it; browsers often report none, so then only a
 * pointer outside the element's box counts as leaving.
 */
export function dragLeftElement(event: Pick<DragEvent, 'clientX' | 'clientY' | 'relatedTarget'>, element: Element) {
  const related = event.relatedTarget;
  if (related instanceof Node) return !element.contains(related);
  const { clientX: x, clientY: y } = event,
    box = element.getBoundingClientRect();
  return !(x > box.left && x < box.right && y > box.top && y < box.bottom);
}
