import { describe, expect, it } from 'vitest';

import { dragLeftElement } from './drag-leave';

/** A board column with a group header whose label sits inside its toggle button. */
function column() {
  const root = document.createElement('section');
  root.innerHTML = '<header>Started</header><button><span>Board</span></button>';
  document.body.replaceChildren(root);
  root.getBoundingClientRect = () => DOMRect.fromRect({ x: 100, y: 50, width: 300, height: 600 });
  return {
    root,
    header: root.querySelector('header')!,
    toggle: root.querySelector('button')!,
    label: root.querySelector('span')!,
  };
}
const leave = (relatedTarget: EventTarget | null, clientX = 0, clientY = 0) => ({ relatedTarget, clientX, clientY });

describe('HS2-1BBXER dragLeftElement', () => {
  it('stays inside while the pointer moves between the element’s children, in either direction', () => {
    const { root, header, toggle, label } = column();
    // Label onto the toggle's own padding, back onto the label, then onto the column header.
    expect(dragLeftElement(leave(toggle), root)).toBe(false);
    expect(dragLeftElement(leave(label), root)).toBe(false);
    expect(dragLeftElement(leave(header), root)).toBe(false);
    // Onto the element itself, such as the column's own padding.
    expect(dragLeftElement(leave(root), root)).toBe(false);
  });

  it('leaves when the entered node is outside the element, wherever the pointer is reported', () => {
    const { root } = column(),
      neighbor = document.body.appendChild(document.createElement('section'));
    expect(dragLeftElement(leave(neighbor, 200, 200), root)).toBe(true);
    expect(dragLeftElement(leave(document.body), root)).toBe(true);
    // A detached node is outside too.
    expect(dragLeftElement(leave(document.createElement('div'), 250, 300), root)).toBe(true);
  });

  it('falls back to the element’s box when the browser reports no entered node', () => {
    const { root } = column();
    expect(dragLeftElement(leave(null, 250, 300), root)).toBe(false);
    for (const [x, y] of [
      [99, 300],
      [401, 300],
      [250, 49],
      [250, 651],
      // A pointer on the edge is on its way out, so a neighbor's highlight never doubles this one.
      [100, 300],
      [400, 300],
      // Leaving the window reports the origin.
      [0, 0],
    ] as const)
      expect(dragLeftElement(leave(null, x, y), root)).toBe(true);
  });
});
