import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import { Bug, CircleAlert, ListChecks, RefreshCw, Search, Sparkles, type IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './ticket-category-select.css';

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
  const selected = choices.find(choice => choice.value === value);
  return <wa-select class="ticket-category-select" name={name} label={label} value={value}>
    {selected && <span slot="start" class="ticket-category-select__icon ticket-category-select__icon--selected" style={`color:${selected.color}`}><LucideIcon icon={selected.icon} name={selected.iconName} /></span>}
    {choices.map(choice => <wa-option value={choice.value}><span slot="start" class="ticket-category-select__icon" style={`color:${choice.color}`}><LucideIcon icon={choice.icon} name={choice.iconName} /></span>{choice.label}</wa-option>)}
  </wa-select>;
}
