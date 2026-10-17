type Presentation = 'sidebar' | 'reader';

// A disclosure can survive the inspector and reader being rebuilt as ticket data changes.
// Keep only expanded titles; collapsed is the default for every ticket and presentation.
const expandedTitles = new Set<string>();

function key(slug: string, presentation: Presentation): string {
  return `${presentation}:${slug}`;
}

export function isTicketTitleExpanded(slug: string, presentation: Presentation): boolean {
  return expandedTitles.has(key(slug, presentation));
}

export function toggleTicketTitleExpanded(slug: string, presentation: Presentation): boolean {
  const id = key(slug, presentation);
  if (expandedTitles.has(id)) {
    expandedTitles.delete(id);
    return false;
  }
  expandedTitles.add(id);
  return true;
}
