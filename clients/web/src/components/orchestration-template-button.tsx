import { Button } from '@kerfjs/ui/button';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { LayoutTemplate } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';

/**
 * HS2-A4RVYA: opens Use Template…, where a plan template's typed inputs are filled and the template is applied:
 * a new goal ticket handed to the planner, whose proposed plan is reviewed like any plan.
 */
export function OrchestrationTemplateButton({
  emphasis,
  busy = false,
}: {
  emphasis: 'primary' | 'secondary';
  busy?: boolean;
}) {
  return (
    <Button
      emphasis={emphasis}
      shape="pill"
      action={ORCHESTRATION_ACTIONS.useOrchestrationTemplate.value}
      disabled={busy}
      title="Apply a saved plan template: the planner adapts it into a plan for you to review"
      icon={<LucideIcon size="s" icon={LayoutTemplate} name="layout-template" />}
    >
      Use Template…
    </Button>
  );
}
