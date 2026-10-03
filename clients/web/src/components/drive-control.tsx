import './drive-control.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Triangle } from 'lucide';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

export interface DriveControlProps {
  running: boolean;
  tool?: string;
  disabled?: boolean;
  disabledReason?: string;
  optionsOpen?: boolean;
  optionsDisabled?: boolean;
}
export function DriveControl({
  running,
  tool = 'AI tool',
  disabled = false,
  disabledReason,
  optionsOpen = false,
  optionsDisabled = false,
}: DriveControlProps) {
  const actionLabel = running ? `${tool} workflow is running` : `Drive with ${tool}`;
  return (
    <div class="drive-control" data-component="drive-control" data-running={String(running)}>
      <button
        type="button"
        class="drive-control__primary"
        {...COMMANDS_AND_AI_ACTIONS.toggleDrive.attrs}
        aria-label={actionLabel}
        disabled={disabled || undefined}
        title={disabledReason ?? actionLabel}
      >
        <span>{running ? `${tool} running` : `Drive with ${tool}`}</span>
      </button>
      <button
        type="button"
        class="drive-control__options"
        {...COMMANDS_AND_AI_ACTIONS.toggleDriveOptions.attrs}
        aria-label="Choose Drive provider, model, and effort"
        aria-expanded={String(optionsOpen)}
        disabled={optionsDisabled || undefined}
        title="Choose Drive provider, model, and effort"
      >
        <LucideIcon size={11.2} icon={Triangle} name="triangle" appearance="solid" />
      </button>
    </div>
  );
}
