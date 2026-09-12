import './menu-item.css';
import '@kerfjs/ui/menu-item.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';

export interface MenuItemProps {
  label: string | SafeHtml;
  icon: SafeHtml;
  trailing?: SafeHtml;
  selected?: boolean;
  action: string;
  itemId?: string;
  className?: string;
  style?: string;
  pressed?: boolean;
  accessibleLabel?: string;
  commandColor?: string;
  dropStatus?: string;
  title?: string;
  multiline?: boolean;
  state?: string;
  role?: 'menuitem';
  disabled?: boolean;
}

export function MenuItem({ label, icon, trailing, selected = false, action, itemId, className = '', style, pressed, accessibleLabel, commandColor, dropStatus, title, multiline = false, state,role,disabled=false }: MenuItemProps) {
  return <button type="button" role={role} class={`kui-menu-item ${className}`.trim()} style={style} title={title} disabled={disabled} data-component="menu-item" data-action={action} data-item-id={itemId} data-command-color={commandColor} data-ticket-drop-status={dropStatus} data-multiline={multiline ? 'true' : undefined} data-state={state} aria-label={accessibleLabel} aria-current={selected ? 'page' : undefined} aria-pressed={pressed === undefined ? undefined : String(pressed)}>
    <span class="kui-menu-item__icon">{icon}</span>
    <span class="kui-menu-item__label">{label}</span>
    {trailing && <span class="kui-menu-item__trailing">{trailing}</span>}
  </button>;
}
