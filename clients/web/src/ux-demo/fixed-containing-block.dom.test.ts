import { afterEach, describe, expect, it, vi } from 'vitest';

import { pointInFixedContainingBlock } from './fixed-containing-block';

const box = (left: number, top: number) => ({ left, top }) as DOMRect;

describe('pointInFixedContainingBlock (HS2-CY6A6K)', () => {
  afterEach(() => {
    document.body.replaceChildren();
    vi.restoreAllMocks();
  });

  it('keeps a viewport point when no ancestor contains fixed descendants', () => {
    const parent = document.createElement('section');
    document.body.append(parent);
    expect(pointInFixedContainingBlock(parent, 120, 340)).toEqual({ x: 120, y: 340 });
  });

  it('translates the point into the nearest transformed frame, the catalog example screen', () => {
    const outer = document.createElement('div'),
      frame = document.createElement('div'),
      parent = document.createElement('section');
    outer.style.transform = 'translate(0)';
    frame.style.transform = 'translate(0)';
    frame.append(parent);
    outer.append(frame);
    document.body.append(outer);
    vi.spyOn(outer, 'getBoundingClientRect').mockReturnValue(box(10, 20));
    vi.spyOn(frame, 'getBoundingClientRect').mockReturnValue(box(305, 208));
    expect(pointInFixedContainingBlock(parent, 1048, 395)).toEqual({ x: 743, y: 187 });
  });

  it('treats the parent itself as the containing block when it is one', () => {
    const parent = document.createElement('section');
    parent.style.filter = 'blur(0)';
    document.body.append(parent);
    vi.spyOn(parent, 'getBoundingClientRect').mockReturnValue(box(40, 60));
    expect(pointInFixedContainingBlock(parent, 50, 70)).toEqual({ x: 10, y: 10 });
  });
});
