import type { CustomView } from './api';
import { consumeSearchTokens, effectiveSearch, type InlineSearchToken, orderedSearchText } from './inline-search';

function baseViewId(name: string): string {
  const normalized = name
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 54);
  return normalized || 'view';
}

export function uniqueCustomViewId(name: string, existing: readonly CustomView[]): string {
  const base = baseViewId(name),
    ids = new Set(existing.map((view) => view.id.toLowerCase()));
  if (!ids.has(base)) return base;
  for (let suffix = 2; suffix < 10_000; suffix += 1) {
    const id = `${base}-${suffix}`;
    if (!ids.has(id)) return id;
  }
  return `${base}-${Date.now().toString(36)}`;
}

export function customViewNameAvailable(name: string, existing: readonly CustomView[], exceptId?: string): boolean {
  const normalized = name.trim().toLocaleLowerCase();
  return (
    Boolean(normalized) &&
    !existing.some((view) => view.id !== exceptId && view.name.trim().toLocaleLowerCase() === normalized)
  );
}

/**
 * The query text a shared view selects while the search bar holds `barText`/`barTokens` (HS2-50R1YQ): the
 * view query alone, or `(view query) AND (search-bar query)` so a search narrows the view instead of replacing it.
 */
export function customViewQueryText(
  view: CustomView,
  barText: string,
  barTokens: readonly InlineSearchToken[] = [],
): string {
  const bar = orderedSearchText(barText, barTokens, () => true).trim();
  return bar ? `(${view.query}) AND (${bar})` : view.query;
}

/** The effective search (text plus inline tokens) of `customViewQueryText`. */
export function customViewSearch(view: CustomView, barText: string, barTokens: readonly InlineSearchToken[] = []) {
  const parsed = consumeSearchTokens(customViewQueryText(view, barText, barTokens), true);
  return effectiveSearch(parsed.text, parsed.tokens);
}
