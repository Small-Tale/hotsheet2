import '@kerfjs/ui/menu-header.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import type { IconNode } from 'lucide';

export interface MenuHeaderProps { label: string; action?: string; actionLabel?: string; actionIcon?: IconNode; actionIconName?: string; actionDisabled?: boolean; actionPopoverTarget?: string; disabledReason?: string; expanded?: boolean; toggle?: boolean }

export function MenuHeader({ label, action, actionLabel, actionIcon, actionIconName, actionDisabled = false, actionPopoverTarget, disabledReason, expanded, toggle = false }: MenuHeaderProps) {
  if (toggle) return <button type="button" class="kui-menu-header kui-menu-header--toggle" data-component="menu-header" data-action={action} aria-expanded={String(Boolean(expanded))}><span>{label}</span>{actionIcon && <LucideIcon icon={actionIcon} name={actionIconName!} />}</button>;
  return <header class="kui-menu-header" data-component="menu-header"><h2>{label}</h2>{action && actionIcon && <button type="button" data-action={action} popoverTarget={actionPopoverTarget} aria-haspopup={actionPopoverTarget?'dialog':undefined} aria-controls={actionPopoverTarget} aria-label={actionLabel} title={actionDisabled ? disabledReason : actionLabel} disabled={actionDisabled || undefined}><LucideIcon icon={actionIcon} name={actionIconName!} /></button>}</header>;
}
