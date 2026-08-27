/** Exact HS1 custom-command palette, shared by configurable client accents. */
export const CUSTOMIZATION_COLORS = [
  { value: '#e5e7eb', label: 'Neutral' },
  { value: '#3b82f6', label: 'Blue' },
  { value: '#22c55e', label: 'Green' },
  { value: '#f97316', label: 'Orange' },
  { value: '#ef4444', label: 'Red' },
  { value: '#8b5cf6', label: 'Purple' },
  { value: '#ec4899', label: 'Pink' },
  { value: '#14b8a6', label: 'Teal' },
  { value: '#6b7280', label: 'Gray' },
] as const;

export function resolveCustomizationColor(color?: string): string {
  return CUSTOMIZATION_COLORS.some(option => option.value === color) ? color! : CUSTOMIZATION_COLORS[0].value;
}

export function customizationContrastColor(color: string): '#1a1a1a' | '#ffffff' {
  const resolved = resolveCustomizationColor(color);
  const red = Number.parseInt(resolved.slice(1, 3), 16);
  const green = Number.parseInt(resolved.slice(3, 5), 16);
  const blue = Number.parseInt(resolved.slice(5, 7), 16);
  return (0.299 * red + 0.587 * green + 0.114 * blue) / 255 > 0.6 ? '#1a1a1a' : '#ffffff';
}
