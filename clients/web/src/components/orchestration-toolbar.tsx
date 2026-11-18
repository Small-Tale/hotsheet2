import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { Pause, Play, Sparkles } from 'lucide';

import { ORCHESTRATION_ACTIONS } from '../interaction-attrs/orchestration';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { type ClockFormat, clockTime, startBlockedReason, timelineStatus } from '../orchestration-timeline-board';
import type { OrchestrationProposeAvailability } from './orchestration-timeline-view';

export interface OrchestrationToolbarProps {
  timeline: OrchestrationTimeline;
  propose?: OrchestrationProposeAvailability;
  /** A kick-off change is on its way to the server. */
  busy?: boolean;
  /**
   * HS2-HCW7MD: `planning` while the planning session works on the plan; HS2-5RYXNN: `waiting` while it
   * waits on a first-run question in its terminal tab; HS2-FDS95D: `replannable` once it finished its turn.
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
};

/**
 * HS2-J89JC7: the Orchestration view's page header. It says where orchestration stands and holds the
 * one action that changes it: Propose Plan with no plan, Start Orchestration for a dry-run plan (it
 * turns automatic placement on through the policy), and Pause while running. A proposed plan is
 * labeled as the dry run it is.
 */
export function OrchestrationToolbar({
  timeline,
  propose = { available: false },
  busy = false,
  planning,
  clock = clockTime,
}: OrchestrationToolbarProps) {
  const status = `${timelineStatus(timeline, clock)}${PLANNING_STATUS[planning ?? ''] ?? ''}`;
  const blocked = startBlockedReason(timeline);
  const action = (() => {
    switch (timeline.phase) {
      case 'no_projects':
        return undefined;
      case 'no_plan':
        return (
          <Button
            emphasis="primary"
            shape="pill"
            action={ORCHESTRATION_ACTIONS.proposeOrchestrationPlan.value}
            disabled={busy || !propose.available}
            title={propose.available ? 'Propose a plan from the goal tickets' : propose.reason}
            icon={<LucideIcon size="s" icon={Sparkles} name="sparkles" />}
          >
            Propose Plan
          </Button>
        );
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
            title={blocked ?? 'Place sessions automatically within the Orchestration settings'}
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
      {timeline.phase === 'proposed' && (
        <List controlInsets="trl">
          <StateBanner
            copyLayout="stacked"
            tone="info"
            title="This Plan Is a Dry Run"
            detail={`Columns run from the newest work on the left to the first work on the right. Start Orchestration starts sessions within the Orchestration settings.${
              blocked ? ` ${blocked}.` : ''
            }`}
          />
        </List>
      )}
      <Toolbar
        label="Orchestration"
        dividerSides=""
        responsive="wrap"
        leading={<ToolbarText text={status} size="default" headingLevel={2} />}
        trailing={action}
      />
    </List>
  );
}
