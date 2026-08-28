import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import type { IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './select.css';

export interface SelectChoice<Value extends string = string> { value: Value; label: string; icon: IconNode; iconName: string; color?: string }
export interface SelectProps<Value extends string = string> { name: string; value: Value; label: string; choices: readonly SelectChoice<Value>[]; className?: string }

export function Select<Value extends string>({ name, value, label, choices, className = '' }: SelectProps<Value>) {
  const selected = choices.find(choice => choice.value === value);
  const icon = (choice: SelectChoice<Value>, selectedIcon = false) => <span slot="start" class={`select__icon${selectedIcon ? ' select__icon--selected' : ''}`} style={choice.color ? `color:${choice.color}` : undefined}><LucideIcon icon={choice.icon} name={choice.iconName} /></span>;
  return <wa-select class={`select ${className}`.trim()} data-component="select" name={name} label={label} value={value}>
    {selected && icon(selected, true)}
    {choices.map(choice => <wa-option value={choice.value}>{icon(choice)}{choice.label}</wa-option>)}
  </wa-select>;
}
