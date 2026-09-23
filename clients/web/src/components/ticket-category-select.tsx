import { colorVar } from '@kerfjs/ui/css-values';
import { Select } from '@kerfjs/ui/select';

import { DEFAULT_TICKET_CATEGORIES, type TicketCategoryPresentation } from './category-presentation';

export type TicketCategoryChoice = TicketCategoryPresentation;
export { DEFAULT_TICKET_CATEGORIES } from './category-presentation';

export interface TicketCategorySelectProps {
  name: string;
  value: string;
  label?: string;
  ariaLabel?: string;
  choices?: readonly TicketCategoryChoice[];
  disabled?: boolean;
  placeholder?: boolean;
}

function categoryColor(category: string) {
  const token = new Set(['task', 'feature', 'bug', 'investigation', 'requirement-change', 'issue']).has(
    category.replaceAll('_', '-'),
  )
    ? category.replaceAll('_', '-')
    : 'fallback';
  return colorVar(`--hs-category-${token}`);
}

export function TicketCategorySelect({
  name,
  value,
  label = 'Category',
  ariaLabel,
  choices = DEFAULT_TICKET_CATEGORIES,
  disabled = false,
  placeholder = false,
}: TicketCategorySelectProps) {
  const accessibleName = ariaLabel ? { ariaLabel } : { label };
  const typedChoices = choices.map((choice) => ({
    ...choice,
    color: categoryColor(choice.value),
  }));
  return (
    <Select
      className="ticket-category-select"
      name={name}
      value={value}
      {...accessibleName}
      choices={typedChoices}
      disabled={disabled}
      placeholder={placeholder}
    />
  );
}
