import { describe, expect, it } from 'vitest';

import { CATEGORY_COLORS, categoryAbbreviation, resolveCategoryColor, resolveCategoryIcon, resolveCategoryIconColor } from './category-presentation';

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

  it('darkens only the neutral swatch for icon contrast', () => {
    expect(resolveCategoryIconColor('#e5e7eb')).toBe('#9ca3af');
    expect(resolveCategoryIconColor('#6b7280')).toBe('#6b7280');
    expect(resolveCategoryIconColor('#3b82f6')).toBe('#3b82f6');
  });

  it('uses HS1 abbreviations and deterministic three-letter custom fallbacks', () => {
    expect(['issue', 'bug', 'feature', 'requirement_change', 'task', 'investigation'].map(category => categoryAbbreviation(category))).toEqual([
      'ISS', 'BUG', 'FEA', 'REQ', 'TSK', 'INV',
    ]);
    expect(categoryAbbreviation('marketing')).toBe('MAR');
    expect(categoryAbbreviation('anything', 'xyz')).toBe('XYZ');
    expect(categoryAbbreviation('x')).toBe('CAT');
  });
});
