import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { MessageCircleQuestionMark, Pause, Play, RefreshCw, Settings, Sparkles, X } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import {
  type ClockFormat,
  clockTime,
  dryRunPlan,
  plannerWorkingLabel,
  startBlockedInSettings,
  startBlockedReason,
  startExplanation,
  timelineStatus,
} from '../orchestration-timeline-board';
import type { OrchestrationProposeAvailability } from './orchestration-timeline-view';

export interface OrchestrationToolbarProps {
  timeline: OrchestrationTimeline;
  propose?: OrchestrationProposeAvailability;
  /** HS2-N5BDE4: the slug of the goal ticket holding the planner's question, while it waits. */
  question?: string;
  /**
   * HS2-YSRT6Y: no explicit goal ticket is open, so planning works through all open tickets of the
   * orchestrated projects (the default goal).
   */
  defaultGoal?: boolean;
  /** HS2-190RDX: whether Update Plan can be pressed, and why not. */
  update?: OrchestrationProposeAvailability;
  /** A kick-off change is on its way to the server. */
  busy?: boolean;
  /**
   * HS2-HCW7MD: `planning` while the planning session works on the plan; HS2-5RYXNN: `waiting` while it
   * waits on a first-run question in its terminal tab; HS2-FDS95D: `replannable` once it finished its turn;
   * HS2-190RDX: `updating` while a plan update drafts beside the running plan.
   */
  planning?: string;
  clock?: ClockFormat;
}

/** What the header adds for a planning session. */
const PLANNING_STATUS: Record<string, string> = {
  planning: ' · Planning',
  // HS2-FDS95D: it finished its turn; Propose Plan gives it a new proposal.
  replannable: ' · Planning',
  waiting: ' · Planner waiting in its terminal',
  // HS2-190RDX: the current plan keeps running while the new one is drafted.
  updating: ' · Updating Plan',
  // HS2-N5BDE4: the planner asked instead of stalling; the person answers on the goal ticket.
  asked: ' · Planner asked a question',
};

/**
 * HS2-J89JC7: the Orchestration view's page header. It says where orchestration stands and holds the
 * one action that changes it: Propose Plan with no plan, Start Orchestration for a dry-run plan (it
 * turns automatic placement on through the policy), and Pause while running. A proposed plan is
 * labeled as the dry run it is. HS2-R2N33M: placement left Auto runs once a plan exists, under a banner that says
 * sessions start automatically, with Pause beside it. HS2-190RDX: once there is a plan, Update Plan re-plans beside it while its
 * work keeps running (no pause), and Cancel Plan Update replaces it while an update drafts.
 */
export function OrchestrationToolbar({
  timeline,
  propose = { available: false },
  update = { available: false },
  question,
  defaultGoal = false,
  busy = false,
  planning,
  clock = clockTime,
}: OrchestrationToolbarProps) {
  const goal = defaultGoal && timeline.phase !== 'no_projects' ? ' · Working through all open tickets' : '';
  const status = `${timelineStatus(timeline, clock)}${goal}${PLANNING_STATUS[planning ?? ''] ?? ''}`;
  const blocked = startBlockedReason(timeline);
  // HS2-1DC8X1: the reason a person can act on is a banner with the way there, never only a disabled title.
  // HS2-D9KP86: with no saved policy the same way leads to the defaults Start Orchestration saves.
  const openSettings =
    startBlockedInSettings(timeline) || timeline.kick_off.default_policy === true ? (
      <Button
        size="compact"
        action={ORCHESTRATION_ACTIONS.openOrchestrationSettings.value}
        icon={<LucideIcon size="s" icon={Settings} name="settings" />}
      >
        Open Orchestration Settings
      </Button>
    ) : undefined;
  const dryRun = dryRunPlan(timeline);
  const updating = planning === 'updating';
  const answer =
    planning === 'asked' && question ? (
      <Button
        emphasis="secondary"
        shape="pill"
        action={ORCHESTRATION_ACTIONS.answerOrchestrationQuestion.value}
        title={`Open ${question}, where the planning session asked you a question`}
        icon={<LucideIcon size="s" icon={MessageCircleQuestionMark} name="message-circle-question-mark" />}
      >
        Answer Question
      </Button>
    ) : undefined;
  const replan =
    timeline.phase === 'proposed' || timeline.phase === 'running' || timeline.phase === 'paused' ? (
      updating ? (
        <Button
          emphasis="secondary"
          shape="pill"
          action={ORCHESTRATION_ACTIONS.cancelOrchestrationPlanUpdate.value}
          disabled={busy}
          title="Stop updating the plan; the current plan stays as it is"
          icon={<LucideIcon size="s" icon={X} name="x" />}
        >
          Cancel Plan Update
        </Button>
      ) : (
        <Button
          emphasis="secondary"
          shape="pill"
          action={ORCHESTRATION_ACTIONS.updateOrchestrationPlan.value}
          disabled={busy || !update.available}
          title={
            update.available
              ? defaultGoal
                ? 'Plan again from all open tickets while the current plan keeps running'
                : 'Plan again from the goal tickets while the current plan keeps running'
              : update.reason
          }
          icon={<LucideIcon size="s" icon={RefreshCw} name="refresh-cw" />}
        >
          Update Plan
        </Button>
      )
    ) : undefined;
  const action = (() => {
    switch (timeline.phase) {
      case 'no_projects':
        return undefined;
      case 'no_plan': {
        // HS2-2Z4M6M: while the press is on its way or the planner starts or works, the button shows
        // that state (busy, with the state as its name) instead of an idle Propose Plan.
        const working = plannerWorkingLabel(planning, busy);
        if (working)
          return (
            <Button
              emphasis="primary"
              shape="pill"
              disabled
              rootAttributes={{ 'data-planner-state': working }}
              title={propose.available ? 'Starting the planning session' : propose.reason}
              icon={<LoadingSpinner size="s" />}
            >
              {working}
            </Button>
          );
        return (
          <Button
            emphasis="primary"
            shape="pill"
            action={ORCHESTRATION_ACTIONS.proposeOrchestrationPlan.value}
            disabled={!propose.available}
            title={
              propose.available
                ? defaultGoal
                  ? 'Propose a plan that works through all open tickets'
                  : 'Propose a plan from the goal tickets'
                : propose.reason
            }
            icon={<LucideIcon size="s" icon={Sparkles} name="sparkles" />}
          >
            Propose Plan
          </Button>
        );
      }
      case 'running':
        return (
          <Button
            emphasis="secondary"
            shape="pill"
            action={ORCHESTRATION_ACTIONS.pauseOrchestration.value}
            disabled={busy || !timeline.kick_off.can_pause}
            title="Stop placing new sessions; running work continues"
            icon={<LucideIcon size="s" icon={Pause} name="pause" />}
          >
            Pause
          </Button>
        );
      case 'proposed':
      case 'paused':
        return (
          <Button
            emphasis="primary"
            shape="pill"
            action={ORCHESTRATION_ACTIONS.startOrchestration.value}
            disabled={busy || !timeline.kick_off.can_start}
            title={
              blocked ??
              (timeline.kick_off.default_policy
                ? 'Save the default policy and place sessions automatically within it'
                : 'Place sessions automatically within the Orchestration settings')
            }
            icon={<LucideIcon size="s" icon={Play} name="play" />}
          >
            Start Orchestration
          </Button>
        );
      default:
        return timeline.phase satisfies never;
    }
  })();
  return (
    <List gap="xs" rootAttributes={{ 'data-orchestration-toolbar': timeline.phase }}>
      {timeline.phase === 'running' && timeline.automatic_by_default === true && (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="info"
            title="Sessions Start Automatically"
            detail="A plan exists, so Hot Sheet starts sessions for ready work within the Orchestration settings. Pause stops placing new sessions; running work continues."
          />
        </List>
      )}
      {dryRun && (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="info"
            title="This Plan Is a Dry Run"
            detail={`Columns run from the newest work on the left to the first work on the right. ${startExplanation(
              timeline,
            )}.${blocked ? ` ${blocked}.` : ''}`}
            action={openSettings}
            actionPlacement="below"
          />
        </List>
      )}
      {timeline.phase === 'paused' && !dryRun && blocked !== undefined && (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="warning"
            title="Start Orchestration Is Unavailable"
            detail={`${blocked}. Work already running continues.`}
            action={openSettings}
            actionPlacement="below"
            rootAttributes={{ 'data-orchestration-start-blocked': timeline.kick_off.start_blocked ?? '' }}
          />
        </List>
      )}
      <Toolbar
        label="Orchestration"
        dividerSides=""
        responsive="wrap"
        leading={<ToolbarText text={status} size="default" headingLevel={2} />}
        trailing={
          <>
            {answer}
            {replan}
            {action}
          </>
        }
      />
    </List>
  );
}
