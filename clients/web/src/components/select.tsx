import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './select.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';
import type { IconNode } from 'lucide';

import { LucideIcon } from './lucide-icon';

export interface SelectChoice<Value extends string = string> { value: Value; label: string; icon?: IconNode; iconName?: string; color?: string; group?: string; separatorBefore?: boolean }
export interface SelectProps<Value extends string = string> { name: string; value: Value; label?: string; ariaLabel?: string; choices: readonly SelectChoice<Value>[]; className?: string; placeholder?: string; disabled?: boolean; fitMenu?:boolean; renderSelected?: (choice: SelectChoice<Value>) => SafeHtml }

export function Select<Value extends string>({ name, value, label, ariaLabel, choices, className = '', placeholder, disabled = false,fitMenu=false, renderSelected }: SelectProps<Value>) {
  const selected = choices.find(choice => choice.value === value);
  const icon = (choice: SelectChoice<Value>, selectedIcon = false) => <span slot="start" class={`select__icon${selectedIcon ? ' select__icon--selected' : ''}`} style={choice.color ? `color:${choice.color}` : undefined}>{choice.icon&&choice.iconName?<LucideIcon icon={choice.icon} name={choice.iconName} />:null}</span>;
  const option = (choice: SelectChoice<Value>) => <>{choice.separatorBefore && <wa-divider></wa-divider>}<wa-option value={choice.value}>{icon(choice)}{choice.label}</wa-option></>;
  const groups = [...new Set(choices.map(choice => choice.group).filter((group): group is string => Boolean(group)))];
  return <wa-select class={`select${renderSelected?' select--custom-selected':''}${fitMenu?' select--fit-menu':''} ${className}`.trim()} data-component="select" name={name} label={label} aria-label={ariaLabel} value={value} placeholder={placeholder} disabled={disabled}>
    {selected && (renderSelected ? <span slot="start" class="select__custom-selected">{renderSelected(selected)}</span> : selected.icon ? icon(selected, true) : null)}
    {groups.length === 0 ? choices.map(option) : groups.map((group, index) => <div class={`select__group${index > 0 ? ' select__group--separated' : ''}`} role="group" aria-label={group}><span class="select__group-title">{group}</span>{choices.filter(choice => choice.group === group).map(option)}</div>)}
  </wa-select>;
}
