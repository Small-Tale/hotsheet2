import type { IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './menu-header.css';

export interface MenuHeaderProps { label: string; action?: string; actionLabel?: string; actionIcon?: IconNode; actionIconName?: string; expanded?: boolean; toggle?: boolean }

export function MenuHeader({ label, action, actionLabel, actionIcon, actionIconName, expanded, toggle = false }: MenuHeaderProps) {
  if (toggle) return <button type="button" class="menu-header menu-header--toggle" data-component="menu-header" data-action={action} aria-expanded={String(Boolean(expanded))}><span>{label}</span>{actionIcon && <LucideIcon icon={actionIcon} name={actionIconName!} />}</button>;
  return <header class="menu-header" data-component="menu-header"><h2>{label}</h2>{action && actionIcon && <button type="button" data-action={action} aria-label={actionLabel} title={actionLabel}><LucideIcon icon={actionIcon} name={actionIconName!} /></button>}</header>;
}
