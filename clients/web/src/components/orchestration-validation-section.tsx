import { Button } from '@kerfjs/ui/button';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { Play, ShieldQuestion } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import {
  type PendingValidation,
  stepsToRun,
  validationStepLabel,
  validationStepValue,
  type ValidationStepView,
} from '../orchestration-validation-steps';
import { OrchestrationInspectorSection } from './orchestration-inspector-section';

export interface OrchestrationValidationSectionProps {
  steps: readonly ValidationStepView[];
  /** Requests waiting on a person's answer, by step. */
  pending?: readonly PendingValidation[];
  busy?: boolean;
  clock: (ms: number) => string;
}

/**
 * HS2-9XJEZC (design HS2-2XJSJK, answer A): each validation step with its latest result, time, exit
 * code and evidence digest, never its output, and one Run Validation that runs every step not passed.
 * A step the orchestrator may not run without a person (a command step always) asks through the
 * ordinary permission prompt; its row then waits for permission and Show Request opens that request.
 */
export function OrchestrationValidationSection({
  steps,
  pending = [],
  busy = false,
  clock,
}: OrchestrationValidationSectionProps) {
  const waiting = (step: ValidationStepView) => pending.find((entry) => entry.step_id === step.step_id),
    runnable = stepsToRun(steps).length > 0;
  return (
    <OrchestrationInspectorSection
      label="Validation"
      note="A command step asks first, through the permission prompt. Output is never shown here."
    >
      <ValueTable label="Validation" density="compact" stackBelow="narrow">
        {steps.map((step) => (
          <ValueTableRow label={validationStepLabel(step)} value={validationStepValue(step, clock, waiting(step))} />
        ))}
      </ValueTable>
      <Row gap="xs" wrap controlInsets="rl">
        <Button
          size="compact"
          action={ORCHESTRATION_ACTIONS.runOrchestrationValidation.value}
          disabled={busy || !runnable}
          title={runnable ? undefined : 'Every step passed or is running'}
          icon={<LucideIcon size="s" icon={Play} name="play" />}
        >
          Run Validation
        </Button>
        {pending.length > 0 ? (
          <Button
            size="compact"
            action={ORCHESTRATION_ACTIONS.showOrchestrationRequest.value}
            icon={<LucideIcon size="s" icon={ShieldQuestion} name="shield-question" />}
            rootAttributes={{ 'data-request-id': pending[0].request_id }}
          >
            Show Request
          </Button>
        ) : undefined}
      </Row>
    </OrchestrationInspectorSection>
  );
}
