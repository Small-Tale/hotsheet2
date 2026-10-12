import { foregroundColorVar } from '@kerfjs/ui/css-values';
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

/** Built-in categories have a `--hs-category-*` token; read the set from the shared presentation (HS2-86A5CR). */
const BUILT_IN_CATEGORY_TOKENS = new Set(
  DEFAULT_TICKET_CATEGORIES.map((category) => category.value.replaceAll('_', '-')),
);

/** The theme token that colors a category's choice icon; a custom category uses the fallback token. */
export function categoryColorToken(category: string): `--hs-category-${string}` {
  const token = category.replaceAll('_', '-');
  return `--hs-category-${BUILT_IN_CATEGORY_TOKENS.has(token) ? token : 'fallback'}`;
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
    color: foregroundColorVar(categoryColorToken(choice.value)),
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
      triggerWidth="fill"
    />
  );
}
