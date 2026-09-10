import './drive-control.css';

import { Triangle } from 'lucide';

import { LucideIcon } from './lucide-icon';

export interface DriveControlProps { running: boolean; tool?: string; disabled?: boolean; disabledReason?: string; optionsOpen?: boolean; optionsDisabled?:boolean }
export function DriveControl({ running, tool = 'AI tool', disabled = false, disabledReason, optionsOpen = false,optionsDisabled=false }: DriveControlProps) {
  const actionLabel = running ? `${tool} workflow is running` : `Drive with ${tool}`;
  return <div class="drive-control" data-component="drive-control" data-running={String(running)}>
    <button type="button" class="drive-control__primary" data-action="toggle-drive" aria-label={actionLabel} disabled={disabled || undefined} title={disabledReason ?? actionLabel}>
      <span>{running ? `${tool} running` : `Drive with ${tool}`}</span>
    </button>
    <button type="button" class="drive-control__options" data-action="toggle-drive-options" aria-label="Choose Drive provider, model, and effort" aria-expanded={String(optionsOpen)} disabled={optionsDisabled||undefined} title="Choose Drive provider, model, and effort"><LucideIcon icon={Triangle} name="triangle" /></button>
  </div>;
}
