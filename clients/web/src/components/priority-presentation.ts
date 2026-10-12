import { ChevronDown, ChevronsUp, ChevronUp, type IconNode, Minus } from 'lucide';

export type TicketPriority = 'low' | 'default' | 'high' | 'urgent';

export interface TicketPriorityPresentation {
  value: TicketPriority;
  label: string;
  /** The app color token (`--hs-priority-*`) that tints the priority icon. */
  colorToken: `--hs-priority-${TicketPriority}`;
  icon: IconNode;
  iconName: string;
}

/**
 * The one priority presentation (label, Lucide icon, color token) that TicketRow's indicator and
 * TicketPrioritySelect's choices both read, most to least urgent (HS2-8MT5KG).
 */
export const TICKET_PRIORITIES: readonly TicketPriorityPresentation[] = [
  { value: 'urgent', label: 'Urgent', colorToken: '--hs-priority-urgent', icon: ChevronsUp, iconName: 'chevrons-up' },
  { value: 'high', label: 'High', colorToken: '--hs-priority-high', icon: ChevronUp, iconName: 'chevron-up' },
  { value: 'default', label: 'Default', colorToken: '--hs-priority-default', icon: Minus, iconName: 'minus' },
  { value: 'low', label: 'Low', colorToken: '--hs-priority-low', icon: ChevronDown, iconName: 'chevron-down' },
];

const BY_VALUE = new Map(TICKET_PRIORITIES.map((priority) => [priority.value, priority]));

/** The presentation for one priority. */
export function priorityPresentation(priority: TicketPriority): TicketPriorityPresentation {
  return BY_VALUE.get(priority) ?? BY_VALUE.get('default')!;
}
