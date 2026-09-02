import './menu-item.css';

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
}

export function MenuItem({ label, icon, trailing, selected = false, action, itemId, className = '', style, pressed, accessibleLabel, commandColor, dropStatus, title }: MenuItemProps) {
  return <button type="button" class={`menu-item ${className}`.trim()} style={style} title={title} data-component="menu-item" data-action={action} data-item-id={itemId} data-command-color={commandColor} data-ticket-drop-status={dropStatus} aria-label={accessibleLabel} aria-current={selected ? 'page' : undefined} aria-pressed={pressed === undefined ? undefined : String(pressed)}>
    <span class="menu-item__icon">{icon}</span>
    <span class="menu-item__label">{label}</span>
    {trailing && <span class="menu-item__trailing">{trailing}</span>}
  </button>;
}
