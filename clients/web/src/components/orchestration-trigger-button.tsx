import { Button } from '@kerfjs/ui/button';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Zap } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';

/**
 * HS2-KXKA7X: opens Triggers…, this computer's scheduled and event triggers: each applies a template or starts
 * or pauses orchestration when its schedule or event comes, after a review.
 */
export function OrchestrationTriggerButton({ busy = false }: { busy?: boolean }) {
  return (
    <Button
      emphasis="secondary"
      shape="pill"
      action={ORCHESTRATION_ACTIONS.openOrchestrationTriggers.value}
      disabled={busy}
      title="Run a template or start orchestration on a schedule or a project event"
      icon={<LucideIcon size="s" icon={Zap} name="zap" />}
    >
      Triggers…
    </Button>
  );
}
