export type TicketEditorKind = 'details' | 'blocked-reason' | 'note';
export type TicketEditorPresentation = 'sidebar' | 'reader';

const PREFIX = 'hotsheet.ticket-editor-height';

export function ticketEditorSizeVariable(kind: TicketEditorKind, presentation: TicketEditorPresentation): string {
  return `--hs-${kind}-${presentation}-height`;
}

export function ticketEditorSizeStorageKey(kind: TicketEditorKind, presentation: TicketEditorPresentation): string {
  return `${PREFIX}.${kind}.${presentation}`;
}

export function loadTicketEditorSizes(storage: Pick<Storage, 'getItem'>, style: Pick<CSSStyleDeclaration, 'setProperty'>): void {
  for (const kind of ['details', 'blocked-reason', 'note'] as const) {
    for (const presentation of ['sidebar', 'reader'] as const) {
      const value = Number(storage.getItem(ticketEditorSizeStorageKey(kind, presentation)));
      if (Number.isFinite(value) && value > 0) style.setProperty(ticketEditorSizeVariable(kind, presentation), `${Math.round(value)}px`);
    }
  }
}

export function saveTicketEditorSize(storage: Pick<Storage, 'setItem'>, style: Pick<CSSStyleDeclaration, 'setProperty'>, kind: TicketEditorKind, presentation: TicketEditorPresentation, height: number): void {
  if (!Number.isFinite(height) || height <= 0) return;
  const value = Math.round(height);
  storage.setItem(ticketEditorSizeStorageKey(kind, presentation), String(value));
  style.setProperty(ticketEditorSizeVariable(kind, presentation), `${value}px`);
}

export function ticketEditorKind(name: string): TicketEditorKind | undefined {
  if (name === 'markdown-source') return 'details';
  if (name === 'blocked-reason') return 'blocked-reason';
  if (name === 'note-body') return 'note';
  return undefined;
}
