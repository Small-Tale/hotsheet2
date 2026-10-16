import { SplitButton } from '@kerfjs/ui/split-button';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';

export interface DriveControlProps {
  running: boolean;
  tool?: string;
  disabled?: boolean;
  disabledReason?: string;
  optionsOpen?: boolean;
  optionsDisabled?: boolean;
}

/**
 * The project sidebar's Drive action with its provider/model/effort options trigger: a Kerf
 * SplitButton on the success fill that switches to the neutral fill while the workflow runs
 * (KF-KFC7C2, HS2-TQDZET). The unstyled wrapper keeps the `drive-control` identity and running
 * state that the sidebar and tests read.
 */
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
    <div data-component="drive-control" data-running={String(running)}>
      <SplitButton
        label={running ? `${tool} running` : `Drive with ${tool}`}
        accessibleLabel={actionLabel}
        action={COMMANDS_AND_AI_ACTIONS.toggleDrive.value}
        optionsLabel="Choose Drive Provider, Model, and Effort"
        optionsAction={COMMANDS_AND_AI_ACTIONS.toggleDriveOptions.value}
        optionsExpanded={optionsOpen}
        tone={running ? 'neutral' : 'success'}
        fill
        disabled={disabled}
        optionsDisabled={optionsDisabled}
        title={disabledReason ?? actionLabel}
      />
    </div>
  );
}
