import { describe, expect, it } from 'vitest';
import { CATEGORY_COLORS, resolveCategoryColor, resolveCategoryIcon } from './category-presentation';

describe('category presentation', () => {
  it('retains the HS1 custom-command palette and resolves registered Lucide icons', () => {
    expect(CATEGORY_COLORS.map(option => option.value)).toEqual([
      '#e5e7eb', '#3b82f6', '#22c55e', '#f97316', '#ef4444', '#8b5cf6', '#ec4899', '#14b8a6', '#6b7280',
    ]);
    expect(resolveCategoryIcon('bug')).toBeDefined();
    expect(resolveCategoryIcon('unknown')).toBeUndefined();
  });

  it('rejects colors outside the supported palette', () => {
    expect(resolveCategoryColor('#3b82f6')).toBe('#3b82f6');
    expect(resolveCategoryColor('red; display:none')).toBe('#6b7280');
  });
});
