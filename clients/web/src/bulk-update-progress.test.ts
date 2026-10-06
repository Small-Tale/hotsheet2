import { describe, expect, it } from 'vitest';

import { type BulkUpdateProgress, createBulkUpdateProgress } from './bulk-update-progress';

describe('bulk update progress', () => {
  it('tracks a sequential update from zero through completion and clears repeatably', () => {
    let visible: BulkUpdateProgress | undefined;
    const progress = createBulkUpdateProgress((value) => {
      visible = value;
    });
    const first = progress.begin(2);
    expect(visible).toEqual({ completed: 0, total: 2 });
    first.advance(1);
    expect(visible).toEqual({ completed: 1, total: 2 });
    first.advance(3);
    expect(visible).toEqual({ completed: 2, total: 2 });
    first.finish();
    expect(visible).toBeUndefined();
    first.advance(1);
    first.finish();
    expect(visible).toBeUndefined();
    const second = progress.begin(1);
    expect(visible).toEqual({ completed: 0, total: 1 });
    second.finish();
    expect(visible).toBeUndefined();
  });

  it('keeps the newest update visible until it finishes, then restores an older active update', () => {
    let visible: BulkUpdateProgress | undefined;
    const progress = createBulkUpdateProgress((value) => {
      visible = value;
    });
    const older = progress.begin(3);
    older.advance(1);
    const newer = progress.begin(2);
    older.advance(2);
    expect(visible).toEqual({ completed: 0, total: 2 });
    newer.advance(1);
    expect(visible).toEqual({ completed: 1, total: 2 });
    newer.finish();
    expect(visible).toEqual({ completed: 2, total: 3 });
    older.finish();
    expect(visible).toBeUndefined();
  });
});
