import './drive-control.css';

import { Play, Square } from 'lucide';

import { LucideIcon } from './lucide-icon';

export interface DriveControlProps { running: boolean; tool?: string; disabled?: boolean; disabledReason?: string }
export function DriveControl({ running, tool = 'AI tool', disabled = false, disabledReason }: DriveControlProps) {
  const actionLabel = running ? `Stop ${tool}` : `Start ${tool}`;
  return <button type="button" class="drive-control" data-component="drive-control" data-running={String(running)} data-action="toggle-drive" aria-label={actionLabel} disabled={disabled || undefined} title={disabledReason ?? actionLabel}>
    <LucideIcon icon={running ? Square : Play} name={running ? 'square' : 'play'} />
    <span>{running ? `${tool} running` : `Drive with ${tool}`}</span>
  </button>;
}
