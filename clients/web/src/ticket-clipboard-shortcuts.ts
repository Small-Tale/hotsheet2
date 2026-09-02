export type TicketClipboardAction = 'copy' | 'cut' | 'paste';

export interface TicketClipboardContext {
  key: string;
  command: boolean;
  ticketWorkAreaFocused: boolean;
  editable: boolean;
  textSelected: boolean;
  hasTicketSelection: boolean;
  hasTicketClipboard: boolean;
}

/** Pure policy for ticket clipboard shortcuts; DOM focus/selection are adapted by main. */
export function ticketClipboardAction(context: TicketClipboardContext): TicketClipboardAction | undefined {
  if (!context.command || !context.ticketWorkAreaFocused || context.editable || context.textSelected) return undefined;
  const key = context.key.toLowerCase();
  if ((key === 'c' || key === 'x') && context.hasTicketSelection) return key === 'c' ? 'copy' : 'cut';
  if (key === 'v' && context.hasTicketClipboard) return 'paste';
  return undefined;
}
