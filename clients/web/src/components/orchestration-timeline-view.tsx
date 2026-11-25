import './orchestration-timeline-view.css';

import { Button } from '@kerfjs/ui/button';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Sparkles, Workflow } from 'lucide';

import { ORCHESTRATION_ACTIONS, ORCHESTRATION_TARGETS } from '../interaction-attrs/orchestration';
import type { PlanHealth, StepName } from '../orchestration-plan-health';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { type ClockFormat, clockTime, plannerWorkingLabel, timelineColumns } from '../orchestration-timeline-board';
import { OrchestrationPlanProblem } from './orchestration-plan-problem';
import { TicketBoard, type TicketBoardLayout } from './ticket-board';

/** Whether the planner behind Propose Plan (HS2-HCW7MD) can be asked, and why not. */
export interface OrchestrationProposeAvailability {
  available: boolean;
  reason?: string;
}

export interface OrchestrationTimelineViewProps {
  timeline?: OrchestrationTimeline;
  loading?: boolean;
  /** The last read failed; the previous timeline, if any, stays on screen under this banner. */
  error?: string;
  /** HS2-F1BKQ0: the plan cannot be read; its state replaces the generic failure. */
  planHealth?: PlanHealth;
  /** Names a step id by its slug when the last timeline read showed it. */
  stepName?: StepName;
  busy?: boolean;
  layout?: TicketBoardLayout;
  selectedStep?: string;
  collapsedGroups?: readonly string[];
  propose?: OrchestrationProposeAvailability;
  /** HS2-2Z4M6M: where the planner stands, so Propose Plan shows a starting or working planner. */
  planning?: string;
  clock?: ClockFormat;
}

/**
 * HS2-J89JC7: the Orchestration view's work area. The production ticket board read as a timeline of
 * dependency waves, newest on the left and Done on the right, with each column's cards grouped by
 * project through the shared ticket-summary contract. Before any project is added it is the
 * first-run empty state; with projects but no plan it offers Propose Plan.
 */
export function OrchestrationTimelineView({
  timeline,
  loading = false,
  error,
  planHealth,
  stepName = (id: string) => `step ${id.slice(-6)}`,
  busy = false,
  layout = 'grid',
  selectedStep,
  collapsedGroups = [],
  propose = { available: false },
  planning,
  clock = clockTime,
}: OrchestrationTimelineViewProps) {
  const working = plannerWorkingLabel(planning, busy);
  const failure =
    planHealth && planHealth.state !== 'ok' ? (
      <OrchestrationPlanProblem health={planHealth} name={stepName} busy={busy} />
    ) : error ? (
      <List controlInsets="trl">
        <StateBanner
          copyLayout="stacked"
          tone="danger"
          title="The Orchestration Timeline Could Not Be Read"
          detail={`${error} It refreshes when the server answers again.`}
        />
      </List>
    ) : undefined;
  const content = (() => {
    // The plan problem above says it all; no empty state contradicts it.
    if (!timeline && planHealth && planHealth.state !== 'ok') return undefined;
    if (!timeline)
      return (
        <List fill hAlign="center" vAlign="middle">
          <EmptyState
            title={loading ? 'Loading the Orchestration Timeline' : 'No Orchestration Timeline'}
            detail={loading ? 'Reading the plan and its runs…' : 'This project has no orchestration timeline.'}
            busy={loading}
          />
        </List>
      );
    if (timeline.phase === 'no_projects')
      return (
        <List fill hAlign="center" vAlign="middle" rootAttributes={{ 'data-orchestration-empty': 'no-projects' }}>
          <EmptyState
            icon={<LucideIcon icon={Workflow} name="workflow" />}
            title="No Projects to Orchestrate Yet"
            detail="Choose the projects this one plans work for. Then write goal tickets here and in those projects, and start orchestration."
            action={
              <Button emphasis="primary" action={ORCHESTRATION_ACTIONS.chooseOrchestrationProjects.value}>
                Choose Projects…
              </Button>
            }
          />
        </List>
      );
    if (timeline.phase === 'no_plan')
      return (
        <List fill hAlign="center" vAlign="middle" rootAttributes={{ 'data-orchestration-empty': 'no-plan' }}>
          <EmptyState
            icon={<LucideIcon icon={Workflow} name="workflow" />}
            title="No Plan Yet"
            detail="Write goal tickets in this project, then propose a plan. The plan stays a dry run until you start orchestration."
            action={
              working ? (
                <Button
                  emphasis="primary"
                  disabled
                  rootAttributes={{ 'data-planner-state': working }}
                  title={propose.available ? 'Starting the planning session' : propose.reason}
                  icon={<LoadingSpinner size="s" />}
                >
                  {working}
                </Button>
              ) : (
                <Button
                  emphasis="primary"
                  action={ORCHESTRATION_ACTIONS.proposeOrchestrationPlan.value}
                  disabled={!propose.available}
                  title={propose.available ? 'Propose a plan from the goal tickets' : propose.reason}
                  icon={<LucideIcon size="s" icon={Sparkles} name="sparkles" />}
                >
                  Propose Plan
                </Button>
              )
            }
          />
        </List>
      );
    return (
      <TicketBoard
        label={timeline.phase === 'proposed' ? 'Proposed orchestration timeline' : 'Orchestration timeline'}
        layout={layout}
        columns={timelineColumns(timeline, { selectedStep, collapsedGroups, clock })}
      />
    );
  })();
  return (
    <section
      class="orchestration-timeline-view"
      data-component="orchestration-timeline-view"
      data-phase={timeline?.phase ?? (loading ? 'loading' : 'none')}
      data-layout={layout}
      {...ORCHESTRATION_TARGETS.view.attrs}
      aria-label="Orchestration"
    >
      {failure}
      <div class="orchestration-timeline-view__body">{content}</div>
    </section>
  );
}
