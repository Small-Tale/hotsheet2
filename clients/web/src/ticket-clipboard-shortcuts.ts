export type TicketClipboardAction = 'copy' | 'cut' | 'paste';

export interface TicketClipboardContext {
  /** The clipboard action whose configured chord matched this event (resolved via the registry), or undefined. */
  action: TicketClipboardAction | undefined;
  ticketWorkAreaFocused: boolean;
  editable: boolean;
  textSelected: boolean;
  hasTicketSelection: boolean;
  hasTicketClipboard: boolean;
}

/**
 * Pure eligibility policy for ticket clipboard shortcuts. The caller resolves which action's
 * (rebindable) chord matched via the keyboard-shortcut registry and passes it as `action`; this
 * gates it on ticket-work-area focus and the absence of an editable field / ordinary text
 * selection so normal text copy/paste is never hijacked. DOM focus/selection are adapted by main.
 */
export function ticketClipboardAction(context: TicketClipboardContext): TicketClipboardAction | undefined {
  if (!context.action || !context.ticketWorkAreaFocused || context.editable || context.textSelected) return undefined;
  if ((context.action === 'copy' || context.action === 'cut') && context.hasTicketSelection) return context.action;
  if (context.action === 'paste' && context.hasTicketClipboard) return context.action;
  return undefined;
}
