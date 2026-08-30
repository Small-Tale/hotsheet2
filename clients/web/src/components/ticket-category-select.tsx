import { DEFAULT_TICKET_CATEGORIES, type TicketCategoryPresentation } from './category-presentation';
import { Select } from './select';

export type TicketCategoryChoice = TicketCategoryPresentation;
export { DEFAULT_TICKET_CATEGORIES } from './category-presentation';

export interface TicketCategorySelectProps { name: string; value: string; label?: string; choices?: readonly TicketCategoryChoice[] }
export function TicketCategorySelect({ name, value, label = 'Category', choices = DEFAULT_TICKET_CATEGORIES }: TicketCategorySelectProps) {
  return <Select className="ticket-category-select" name={name} value={value} label={label} choices={choices} />;
}
