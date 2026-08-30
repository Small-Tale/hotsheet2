import { Bug, CircleAlert, type IconNode,ListChecks, RefreshCw, Search, Sparkles } from 'lucide';

import { Select } from './select';

export interface TicketCategoryChoice { value: string; label: string; color: string; icon: IconNode; iconName: string }
export const DEFAULT_TICKET_CATEGORIES: readonly TicketCategoryChoice[] = [
  { value: 'task', label: 'Task', color: '#14b8a6', icon: ListChecks, iconName: 'list-checks' },
  { value: 'feature', label: 'Feature', color: '#8b5cf6', icon: Sparkles, iconName: 'sparkles' },
  { value: 'bug', label: 'Bug', color: '#ef4444', icon: Bug, iconName: 'bug' },
  { value: 'investigation', label: 'Investigation', color: '#f97316', icon: Search, iconName: 'search' },
  { value: 'requirement_change', label: 'Requirement change', color: '#3b82f6', icon: RefreshCw, iconName: 'refresh-cw' },
  { value: 'issue', label: 'Issue', color: '#6b7280', icon: CircleAlert, iconName: 'circle-alert' },
];

export interface TicketCategorySelectProps { name: string; value: string; label?: string; choices?: readonly TicketCategoryChoice[] }
export function TicketCategorySelect({ name, value, label = 'Category', choices = DEFAULT_TICKET_CATEGORIES }: TicketCategorySelectProps) {
  return <Select className="ticket-category-select" name={name} value={value} label={label} choices={choices} />;
}
