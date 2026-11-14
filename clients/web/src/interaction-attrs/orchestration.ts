import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/**
 * `data-action` specs of the Orchestration view (HS2-J89JC7): kick-off, the card inspector's run
 * controls and links, and the empty states. Markup spreads `.attrs` or passes `.value` to a Kerf
 * `action` prop; `components/orchestration-timeline-interactions.ts` registers `.selector`.
 */
export const ORCHESTRATION_ACTIONS = {
  startOrchestration: action('start-orchestration'),
  pauseOrchestration: action('pause-orchestration'),
  proposeOrchestrationPlan: action('propose-orchestration-plan'),
  chooseOrchestrationProjects: action('choose-orchestration-projects'),
  orchestrationRunControl: action('orchestration-run-control'),
  showOrchestrationRequest: action('show-orchestration-request'),
  openOrchestrationGoal: action('open-orchestration-goal'),
} as const;

/** Other delegated targets of the Orchestration view. */
export const ORCHESTRATION_TARGETS = {
  /** The view's root: ticket-collection and group handlers leave what is inside it to this view. */
  view: attr('data-orchestration-view', 'true'),
} as const;
