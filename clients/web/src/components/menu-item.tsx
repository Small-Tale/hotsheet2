import type { SafeHtml } from 'kerfjs/jsx-runtime';
import './menu-item.css';

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
}

export function MenuItem({ label, icon, trailing, selected = false, action, itemId, className = '', style, pressed, accessibleLabel, commandColor }: MenuItemProps) {
  return <button type="button" class={`menu-item ${className}`.trim()} style={style} data-component="menu-item" data-action={action} data-item-id={itemId} data-command-color={commandColor} aria-label={accessibleLabel} aria-current={selected ? 'page' : undefined} aria-pressed={pressed === undefined ? undefined : String(pressed)}>
    <span class="menu-item__icon">{icon}</span>
    <span class="menu-item__label">{label}</span>
    {trailing && <span class="menu-item__trailing">{trailing}</span>}
  </button>;
}
