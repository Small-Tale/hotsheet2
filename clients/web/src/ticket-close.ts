export type TicketCloseReason = 'completed' | 'not_planned' | 'duplicate' | 'obsolete';

export interface DuplicateTarget {
  id: string;
  slug: string;
  title: string;
}

export const TICKET_CLOSE_REASON_CHOICES: ReadonlyArray<{ value: TicketCloseReason; label: string }> = [
  { value: 'completed', label: 'Completed' },
  { value: 'not_planned', label: 'Not planned' },
  { value: 'duplicate', label: 'Duplicate' },
  { value: 'obsolete', label: 'Obsolete' },
];

export function validateTicketClose(reason: TicketCloseReason, sourceId: string, target?: DuplicateTarget): string {
  if (reason !== 'duplicate') return '';
  if (!target) return 'Select the existing ticket that this duplicates.';
  if (target.id === sourceId) return 'A ticket cannot be a duplicate of itself.';
  return '';
}

export function ticketCloseReasonLabel(reason?: TicketCloseReason): string | undefined {
  return TICKET_CLOSE_REASON_CHOICES.find(choice => choice.value === reason)?.label;
}
