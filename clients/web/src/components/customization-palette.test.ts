import { describe, expect, it } from 'vitest';
import { CUSTOMIZATION_COLORS, customizationContrastColor, resolveCustomizationColor } from './customization-palette';

describe('shared HS1 customization palette', () => {
  it('retains the exact ordered custom-command colors', () => {
    expect(CUSTOMIZATION_COLORS.map(option => option.value)).toEqual(['#e5e7eb', '#3b82f6', '#22c55e', '#f97316', '#ef4444', '#8b5cf6', '#ec4899', '#14b8a6', '#6b7280']);
  });

  it('rejects arbitrary colors and preserves readable neutral contrast', () => {
    expect(resolveCustomizationColor('#123456')).toBe('#e5e7eb');
    expect(customizationContrastColor('#e5e7eb')).toBe('#1a1a1a');
    expect(customizationContrastColor('#3b82f6')).toBe('#ffffff');
  });
});
