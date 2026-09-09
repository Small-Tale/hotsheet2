import { describe, expect, it, vi } from 'vitest';

import { devReviewRequested, promoteDevReviewPopover } from './request';

describe('devReviewRequested', () => {
  it('defaults on in development and honors only the explicit false value', () => {
    expect(devReviewRequested('http://localhost/?dev-review=1', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?project=demo&dev-review=1', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?dev-review=0', true)).toBe(true);
    expect(devReviewRequested('http://localhost/', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?dev-review=false', true)).toBe(false);
    expect(devReviewRequested('https://example.test/?dev-review=1', false)).toBe(false);
  });

  it('promotes a supported toolbar popover above a newly opened dialog', () => {
    const showPopover=vi.fn(),hidePopover=vi.fn();
    const toolbar={matches:vi.fn(()=>true),showPopover,hidePopover};
    promoteDevReviewPopover(toolbar);
    expect(hidePopover).toHaveBeenCalledOnce();
    expect(showPopover).toHaveBeenCalledOnce();
    promoteDevReviewPopover({matches:vi.fn(()=>false)});
  });
});
