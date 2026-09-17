import { Select } from '@kerfjs/ui/select';

import { DEFAULT_TICKET_CATEGORIES, type TicketCategoryPresentation } from './category-presentation';

export type TicketCategoryChoice = TicketCategoryPresentation;
export { DEFAULT_TICKET_CATEGORIES } from './category-presentation';

export interface TicketCategorySelectProps { name: string; value: string; label?: string; ariaLabel?: string; choices?: readonly TicketCategoryChoice[]; disabled?: boolean; placeholder?: boolean }
export function TicketCategorySelect({ name, value, label = 'Category', ariaLabel, choices = DEFAULT_TICKET_CATEGORIES, disabled = false, placeholder = false }: TicketCategorySelectProps) {
  // When an ariaLabel is supplied the visible label is rendered separately (e.g. a ListHeader), so the
  // control itself carries only the accessible name (HS2-R64ETQ).
  return <Select className="ticket-category-select" name={name} value={value} label={ariaLabel ? undefined : label} ariaLabel={ariaLabel} choices={choices} disabled={disabled} placeholder={placeholder} />;
}
