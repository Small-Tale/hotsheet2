import { Bug, CircleAlert, ListChecks, RefreshCw, Search, Sparkles, type IconNode } from 'lucide';

export const CATEGORY_COLORS = [
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

export const CATEGORY_ICONS: ReadonlyArray<{ value: string; label: string; icon?: IconNode }> = [
  { value: '', label: 'No icon' },
  { value: 'sparkles', label: 'Sparkles', icon: Sparkles },
  { value: 'bug', label: 'Bug', icon: Bug },
  { value: 'list-checks', label: 'List checks', icon: ListChecks },
  { value: 'search', label: 'Search', icon: Search },
  { value: 'refresh-cw', label: 'Refresh', icon: RefreshCw },
  { value: 'circle-alert', label: 'Alert', icon: CircleAlert },
];

export function resolveCategoryIcon(name?: string): IconNode | undefined {
  return CATEGORY_ICONS.find(option => option.value === name)?.icon;
}

export function resolveCategoryColor(color?: string): string {
  return CATEGORY_COLORS.some(option => option.value === color) ? color! : '#6b7280';
}

/** Neutral remains pale for filled controls but needs darker strokes on white. */
export function resolveCategoryIconColor(color?: string): string {
  const resolved = resolveCategoryColor(color);
  return resolved === '#e5e7eb' ? '#9ca3af' : resolved;
}

const DEFAULT_CATEGORY_ABBREVIATIONS: Record<string, string> = {
  issue: 'ISS', bug: 'BUG', feature: 'FEA', requirement_change: 'REQ', task: 'TSK', investigation: 'INV',
};

export function categoryAbbreviation(category: string, configured?: string): string {
  const normalizedCategory = category.trim().toLowerCase();
  const candidate = configured?.trim() || DEFAULT_CATEGORY_ABBREVIATIONS[normalizedCategory] || category;
  const abbreviation = candidate.replace(/[^a-z0-9]/gi, '').toUpperCase().slice(0, 3);
  return abbreviation.length === 3 ? abbreviation : 'CAT';
}
