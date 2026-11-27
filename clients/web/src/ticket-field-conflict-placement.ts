/**
 * Where the inspector shows a ticket field's conflict card (HS2-3TSEAX). The card appears beneath the
 * field it resolves, so it never pushes the field (or a control above it) down while the user is
 * clicking into it: a double-click on Details used to land its second click on the section tabs. A
 * field without a place in the Info section (title, Up Next, an unknown field), and any field while
 * another section or the loading placeholder is shown, keeps the card in the inspector header so it is
 * never hidden.
 */
export type TicketFieldConflictPlacement = 'header' | 'metadata' | 'blocked' | 'details' | 'tags' | 'note';

const infoPlacements: Readonly<Record<string, TicketFieldConflictPlacement>> = {
  blocked_reason: 'blocked',
  category: 'metadata',
  details: 'details',
  priority: 'metadata',
  status: 'metadata',
  tags: 'tags',
};

export function ticketFieldConflictPlacement(
  key: string,
  { infoShown }: { infoShown: boolean },
): TicketFieldConflictPlacement {
  if (!infoShown) return 'header';
  if (key.startsWith('note:')) return 'note';
  return infoPlacements[key] ?? 'header';
}

/** The note id a `note:<id>` conflict key resolves, or undefined for any other field. */
export function ticketFieldConflictNoteId(key: string): string | undefined {
  return key.startsWith('note:') ? key.slice('note:'.length) : undefined;
}
