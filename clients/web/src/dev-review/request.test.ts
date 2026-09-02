import { describe, expect, it } from 'vitest';

import { devReviewRequested } from './request';

describe('devReviewRequested', () => {
  it('requires both a development build and the explicit query value', () => {
    expect(devReviewRequested('http://localhost/?dev-review=1', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?project=demo&dev-review=1', true)).toBe(true);
    expect(devReviewRequested('http://localhost/?dev-review=0', true)).toBe(false);
    expect(devReviewRequested('http://localhost/', true)).toBe(false);
    expect(devReviewRequested('https://example.test/?dev-review=1', false)).toBe(false);
  });
});
