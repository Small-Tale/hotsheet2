import { DEFAULT_TICKET_CATEGORIES } from './components/category-presentation';

const STORAGE_KEY = 'hotsheet.ticket-composer.last-category.v1';
const FALLBACK_CATEGORY = 'task';
const categoryValues = new Set(DEFAULT_TICKET_CATEGORIES.map(category => category.value));

export function loadLastTicketCategory(storage: Pick<Storage, 'getItem'>): string {
  const stored = storage.getItem(STORAGE_KEY);
  return stored && categoryValues.has(stored) ? stored : FALLBACK_CATEGORY;
}

export function saveLastTicketCategory(storage: Pick<Storage, 'setItem'>, category: string): void {
  if (categoryValues.has(category)) storage.setItem(STORAGE_KEY, category);
}
