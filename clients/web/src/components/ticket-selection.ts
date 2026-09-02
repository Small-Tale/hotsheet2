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
  return !intent.range && !intent.toggle
    && loadedSlug === slug
    && selectedSlugs.length === 1
    && selectedSlugs[0] === slug;
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
    if (selected.has(slug)) selected.delete(slug); else selected.add(slug);
    return { anchor: slug, selected };
  }
  return { anchor: slug, selected: new Set([slug]) };
}

export function selectAllTickets(orderedSlugs: readonly string[]): TicketSelectionState {
  return { anchor: orderedSlugs[0], selected: new Set(orderedSlugs) };
}

export function adjacentTicketSlug(orderedSlugs: readonly string[], slug: string, delta: -1 | 1): string | undefined {
  const index = orderedSlugs.indexOf(slug);
  if (index < 0) return orderedSlugs[0];
  return orderedSlugs[Math.max(0, Math.min(orderedSlugs.length - 1, index + delta))];
}
