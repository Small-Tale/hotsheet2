import { describe, expect, it } from 'vitest';

import { positiveMarginBands } from './catalog-geometry';

describe('catalog geometry inspection', () => {
  it('draws only positive margin area outside the border box', () => {
    expect(positiveMarginBands({ left: 10, top: 20, right: 40, bottom: 50 }, [4, 6, 8, 2])).toEqual([
      { left: 8, top: 16, right: 46, bottom: 20 },
      { left: 8, top: 50, right: 46, bottom: 58 },
      { left: 8, top: 20, right: 10, bottom: 50 },
      { left: 40, top: 20, right: 46, bottom: 50 },
    ]);
    expect(positiveMarginBands({ left: 10, top: 20, right: 40, bottom: 50 }, [-4, 0, 0, 0])).toEqual([]);
  });
});
