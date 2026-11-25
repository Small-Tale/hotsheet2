export interface TicketSelectionState {
  anchor?: string;
  selected: ReadonlySet<string>;
}

export interface TicketSelectionIntent {
  range?: boolean;
  toggle?: boolean;
}

/** A plain activation of the one fully loaded ticket already selected changes nothing. */
export function isPlainTicketReselection(
  selectedSlugs: readonly string[],
  loadedSlug: string | undefined,
  slug: string,
  intent: TicketSelectionIntent = {},
): boolean {
  return (
    !intent.range && !intent.toggle && loadedSlug === slug && selectedSlugs.length === 1 && selectedSlugs[0] === slug
  );
}

export function updateTicketSelection(
  orderedSlugs: readonly string[],
  state: TicketSelectionState,
  slug: string,
  intent: TicketSelectionIntent = {},
): TicketSelectionState {
  if (!orderedSlugs.includes(slug)) return state;
  if (intent.range && state.anchor && orderedSlugs.includes(state.anchor)) {
    const start = orderedSlugs.indexOf(state.anchor);
    const end = orderedSlugs.indexOf(slug);
    const range = orderedSlugs.slice(Math.min(start, end), Math.max(start, end) + 1);
    return { anchor: state.anchor, selected: new Set(intent.toggle ? [...state.selected, ...range] : range) };
  }
  if (intent.toggle) {
    const selected = new Set(state.selected);
    if (selected.has(slug)) selected.delete(slug);
    else selected.add(slug);
    return { anchor: slug, selected };
  }
  return { anchor: slug, selected: new Set([slug]) };
}

/**
 * Select every ticket in `orderedSlugs`. The anchor is the first one a person can reach
 * (`reachableSlugs`, the rows drawn), so a later range selection starts from a visible row even
 * when the collection opens with a collapsed group (HS2-3SJ5G8).
 */
export function selectAllTickets(
  orderedSlugs: readonly string[],
  reachableSlugs: readonly string[] = orderedSlugs,
): TicketSelectionState {
  const selected = new Set(orderedSlugs);
  return { anchor: reachableSlugs.find((slug) => selected.has(slug)) ?? orderedSlugs[0], selected };
}

/** One entry of a collection in drawn order: a drawn ticket row, or the members a collapsed group hides. */
export type DrawnTicketEntry = { readonly slug: string } | { readonly hiddenSlugs: readonly string[] };

/**
 * Every ticket a collection holds, in drawn order and without duplicates: drawn rows, plus the
 * members of each collapsed group where that group sits (HS2-3SJ5G8). A column header's Select
 * All and ⌘A select these, so a collapsed group's tickets are selected with the rest.
 */
export function collectionTicketSlugs(entries: readonly DrawnTicketEntry[]): string[] {
  const slugs = new Set<string>();
  for (const entry of entries) {
    if ('slug' in entry) slugs.add(entry.slug);
    else for (const slug of entry.hiddenSlugs) slugs.add(slug);
  }
  return [...slugs];
}

/** The attribute a collapsed group carries its hidden members' slugs in (HS2-3SJ5G8). */
export const HIDDEN_TICKET_SLUGS_ATTRIBUTE = 'data-ticket-group-hidden-slugs';

/** The attribute value a collapsed group carries for its hidden members' slugs (HS2-3SJ5G8). */
export function encodeHiddenTicketSlugs(slugs: readonly string[]): string {
  return JSON.stringify(slugs);
}

/** Read a collapsed group's hidden member slugs; a missing or malformed value holds none. */
export function decodeHiddenTicketSlugs(value: string | undefined): string[] {
  if (!value) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) ? parsed.filter((slug): slug is string => typeof slug === 'string' && !!slug) : [];
  } catch {
    return [];
  }
}

export function adjacentTicketSlug(orderedSlugs: readonly string[], slug: string, delta: -1 | 1): string | undefined {
  const index = orderedSlugs.indexOf(slug);
  if (index < 0) return orderedSlugs[0];
  return orderedSlugs[Math.max(0, Math.min(orderedSlugs.length - 1, index + delta))];
}
