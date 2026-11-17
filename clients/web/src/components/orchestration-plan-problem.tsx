import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { GitMerge } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import { type PlanHealth, planProblem, type StepName } from '../orchestration-plan-health';

export interface OrchestrationPlanProblemProps {
  health: PlanHealth;
  name: StepName;
  busy?: boolean;
}

/**
 * HS2-F1BKQ0: in place of "could not be read", what is wrong with the plan: the conflicting step ids, or
 * which plan file is damaged. When dropping this machine's pre-sharing copies resolves a conflict, Keep the
 * Checked-In Plan… asks through the app's confirmation and then runs a person's `readiness migrate` with
 * `discard_local`; anything else is fixed in Git and the view reads the plan again on its own.
 */
export function OrchestrationPlanProblem({ health, name, busy = false }: OrchestrationPlanProblemProps) {
  const problem = planProblem(health, name);
  if (!problem) return <></>;
  return (
    <List controlInsets="trl" rootAttributes={{ 'data-orchestration-plan-problem': health.state }}>
      <StateBanner
        copyLayout="stacked"
        tone={health.state === 'damaged' ? 'danger' : 'warning'}
        title={problem.title}
        detail={problem.detail}
        action={
          problem.canDiscard ? (
            <Button
              size="compact"
              shape="pill"
              disabled={busy}
              action={ORCHESTRATION_ACTIONS.keepCheckedInOrchestrationPlan.value}
              icon={<LucideIcon size="s" icon={GitMerge} name="git-merge" />}
            >
              Keep the Checked-In Plan…
            </Button>
          ) : undefined
        }
      />
    </List>
  );
}
