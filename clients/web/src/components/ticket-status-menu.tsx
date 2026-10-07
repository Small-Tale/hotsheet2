import './ticket-status-menu.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';

import type { StartedPhase } from '../api';
import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { STARTED_PHASE_LABELS, StatusBadge, statusPresentation, type TicketStatus } from './status-badge';

export const TICKET_STATUS_CHOICES = (
  ['not_started', 'started', 'completed', 'verified', 'backlog', 'archive'] as const
).map((value) => ({ value, ...statusPresentation(value), separatorBefore: value === 'backlog' }));
/** Leave room above the phone status menu for its seven-row Started submenu. */
export function statusMenuAnchorY(triggerBottom: number, viewportWidth: number, viewportHeight: number): number {
  return viewportWidth <= 480 ? Math.max(triggerBottom, viewportHeight * 0.42) : triggerBottom;
}
export function TicketStatusMenu({
  value,
  startedPhase,
  disabled = false,
  canEditStartedPhase = false,
}: {
  value: TicketStatus;
  startedPhase?: StartedPhase;
  disabled?: boolean;
  canEditStartedPhase?: boolean;
}) {
  const selected = statusPresentation(value);
  const entries: PopupMenuEntry[] = TICKET_STATUS_CHOICES.flatMap((choice) => {
    const item = {
      label: choice.label,
      icon: <LucideIcon icon={choice.icon} name={choice.iconName} />,
      checked: choice.value === value,
      ...(choice.value === 'started' && canEditStartedPhase
        ? {
            attributes: { 'data-ticket-status': 'started' },
            submenu: [
              {
                label: 'No phase',
                checked: value === 'started' && !startedPhase,
                action: INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStartedPhase.value,
                attributes: { 'data-started-phase': '' },
              },
              ...Object.entries(STARTED_PHASE_LABELS).map(([phase, label]) => ({
                label,
                checked: value === 'started' && startedPhase === phase,
                action: INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStartedPhase.value,
                attributes: { 'data-started-phase': phase },
              })),
            ],
          }
        : {
            action: INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStatus.value,
            attributes: { 'data-ticket-status': choice.value },
          }),
    };
    return choice.separatorBefore ? [{ kind: 'divider' as const }, item] : [item];
  });
  return (
    <span class="ticket-status-menu">
      <StatusBadge
        status={value}
        startedPhase={startedPhase}
        weight="semibold"
        interactive={!disabled}
        action={!disabled ? INSPECTOR_AND_EDITOR_ACTIONS.openInspectorStatusMenu.value : undefined}
        hasPopup={!disabled ? 'menu' : undefined}
        actionLabel={`${disabled ? 'Status' : 'Change status'}, ${startedPhase && value === 'started' ? STARTED_PHASE_LABELS[startedPhase] : selected.label}`}
      />
      {!disabled && (
        <span class="ticket-status-menu__popup">
          <PopupMenu
            context
            label="Change status"
            rootAttributes={{ 'data-inspector-status-menu': 'true' }}
            items={entries}
          />
        </span>
      )}
    </span>
  );
}
