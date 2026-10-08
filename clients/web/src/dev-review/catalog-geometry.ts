/** Development-only geometry overlay for rendered catalog components. */
const SVG_NS = 'http://www.w3.org/2000/svg';

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export function positiveMarginBands(box: Box, margins: readonly number[]): Box[] {
  const [top, right, bottom, left] = margins.map((value) => Math.max(0, value));
  return [
    { left: box.left - left, top: box.top - top, right: box.right + right, bottom: box.top },
    { left: box.left - left, top: box.bottom, right: box.right + right, bottom: box.bottom + bottom },
    { left: box.left - left, top: box.top, right: box.left, bottom: box.bottom },
    { left: box.right, top: box.top, right: box.right + right, bottom: box.bottom },
  ].filter(({ left, top, right, bottom }) => right > left && bottom > top);
}

export function installCatalogGeometryInspection(doc: Document, host: HTMLElement): { destroy(): void } {
  const view = doc.defaultView ?? window;
  const overlay = doc.createElementNS(SVG_NS, 'svg');
  overlay.classList.add('hs-dev-review__geometry');
  overlay.setAttribute('aria-hidden', 'true');
  overlay.setAttribute('data-hotsheet-dev-review', 'true');
  host.append(overlay);
  let frame: number | undefined;

  const appendBox = (box: Box, kind: 'bounds' | 'margin') => {
    const rect = doc.createElementNS(SVG_NS, 'rect');
    rect.setAttribute('x', String(box.left));
    rect.setAttribute('y', String(box.top));
    rect.setAttribute('width', String(box.right - box.left));
    rect.setAttribute('height', String(box.bottom - box.top));
    rect.setAttribute('data-kind', kind);
    overlay.append(rect);
  };
  const draw = () => {
    frame = undefined;
    overlay.replaceChildren();
    overlay.setAttribute('viewBox', `0 0 ${view.innerWidth} ${view.innerHeight}`);
    for (const element of doc.querySelectorAll<HTMLElement>('[data-catalog-example-stack] [data-component]')) {
      if (host.contains(element) || element.closest('[data-catalog-geometry-skip]')) continue;
      const bounds = element.getBoundingClientRect();
      if (!bounds.width || !bounds.height) continue;
      const style = view.getComputedStyle(element);
      if (style.visibility === 'hidden' || style.display === 'none') continue;
      const box = { left: bounds.left, top: bounds.top, right: bounds.right, bottom: bounds.bottom };
      const margins = [style.marginTop, style.marginRight, style.marginBottom, style.marginLeft].map(parseFloat);
      for (const margin of positiveMarginBands(box, margins)) appendBox(margin, 'margin');
      appendBox(box, 'bounds');
    }
  };
  const schedule = () => {
    if (frame === undefined) frame = view.requestAnimationFrame(draw);
  };
  const observer = new view.MutationObserver((records) => {
    if (records.some((record) => !host.contains(record.target))) schedule();
  });
  observer.observe(doc.body, { childList: true, attributes: true, subtree: true });
  doc.addEventListener('scroll', schedule, true);
  view.addEventListener('resize', schedule);
  doc.addEventListener('transitionend', schedule, true);
  schedule();
  return {
    destroy() {
      observer.disconnect();
      doc.removeEventListener('scroll', schedule, true);
      view.removeEventListener('resize', schedule);
      doc.removeEventListener('transitionend', schedule, true);
      if (frame !== undefined) view.cancelAnimationFrame(frame);
      overlay.remove();
    },
  };
}
