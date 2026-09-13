import '@kerfjs/ui/menu-header.css';
import './menu-header.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import type { IconNode } from 'lucide';

export interface MenuHeaderProps { label: string; count?:number; countLabel?:string; action?: string; actionLabel?: string; actionIcon?: IconNode; actionIconName?: string; actionDisabled?: boolean; actionPopoverTarget?: string; disabledReason?: string; expanded?: boolean; toggle?: boolean }

export function MenuHeader({ label,count,countLabel, action, actionLabel, actionIcon, actionIconName, actionDisabled = false, actionPopoverTarget, disabledReason, expanded, toggle = false }: MenuHeaderProps) {
  const content=<>{label}{count!==undefined&&<span class="kui-menu-header__count" aria-label={countLabel}>{count}</span>}</>;
  if (toggle) return <button type="button" class="kui-menu-header kui-menu-header--toggle" data-component="menu-header" data-action={action} aria-expanded={String(Boolean(expanded))}><span>{content}</span>{actionIcon && <span class="kui-menu-header__action-layer"><LucideIcon icon={actionIcon} name={actionIconName!} /></span>}</button>;
  return <header class="kui-menu-header" data-component="menu-header"><h2>{content}</h2>{action && actionIcon && <button type="button" data-action={action} popoverTarget={actionPopoverTarget} aria-haspopup={actionPopoverTarget?'dialog':undefined} aria-controls={actionPopoverTarget} aria-label={actionLabel} title={actionDisabled ? disabledReason : actionLabel} disabled={actionDisabled || undefined}><LucideIcon icon={actionIcon} name={actionIconName!} /></button>}</header>;
}
