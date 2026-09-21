import { Select } from '@kerfjs/ui/select';

import { DEFAULT_TICKET_CATEGORIES, type TicketCategoryPresentation } from './category-presentation';

export type TicketCategoryChoice = TicketCategoryPresentation;
export { DEFAULT_TICKET_CATEGORIES } from './category-presentation';

export interface TicketCategorySelectProps { name: string; value: string; label?: string; ariaLabel?: string; choices?: readonly TicketCategoryChoice[]; disabled?: boolean; placeholder?: boolean }
export function TicketCategorySelect({ name, value, label = 'Category', ariaLabel, choices = DEFAULT_TICKET_CATEGORIES, disabled = false, placeholder = false }: TicketCategorySelectProps) {
  const accessibleName=ariaLabel?{ariaLabel}:{label};
  return <Select className="ticket-category-select" name={name} value={value} {...accessibleName} choices={choices} disabled={disabled} placeholder={placeholder} />;
}
