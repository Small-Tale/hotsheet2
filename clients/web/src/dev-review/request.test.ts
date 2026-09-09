import { describe, expect, it } from 'vitest';

import { devReviewRequested } from './request';

describe('devReviewRequested', () => {
  it('defaults on in development and honors only the explicit false value', () => {
    expect(devReviewRequested('http://localhost/?dev-review=1', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?project=demo&dev-review=1', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?dev-review=0', true)).toBe(true);
    expect(devReviewRequested('http://localhost/', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?dev-review=false', true)).toBe(false);
    expect(devReviewRequested('https://example.test/?dev-review=1', false)).toBe(false);
  });
});
