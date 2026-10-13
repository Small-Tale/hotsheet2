export interface CopiedTicketPlacement {
  status?: string;
  up_next: boolean;
}

export function copiedTicketPlacement(source: CopiedTicketPlacement) {
  return {
    status: 'not_started' as const,
    up_next: source.up_next,
  };
}

/** The plain text a ticket copy puts on the system pasteboard: id, title, details, and notes. */
export function ticketClipboardText(
  tickets: ReadonlyArray<{
    readonly slug: string;
    readonly title: string;
    readonly details?: string | null;
    readonly notes: ReadonlyArray<{ readonly text: string }>;
  }>,
): string {
  return tickets
    .map((ticket) =>
      [`${ticket.slug}: ${ticket.title}`, ticket.details, ...ticket.notes.map((note) => `- ${note.text}`)]
        .filter(Boolean)
        .join('\n\n'),
    )
    .join('\n\n');
}
