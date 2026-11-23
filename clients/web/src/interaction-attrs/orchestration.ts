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
  /** HS2-190RDX: re-plan while the current plan keeps running, and cancel that update. */
  updateOrchestrationPlan: action('update-orchestration-plan'),
  cancelOrchestrationPlanUpdate: action('cancel-orchestration-plan-update'),
  /** HS2-N5BDE4: open the goal ticket where the planner asked the person a question. */
  answerOrchestrationQuestion: action('answer-orchestration-question'),
  chooseOrchestrationProjects: action('choose-orchestration-projects'),
  orchestrationRunControl: action('orchestration-run-control'),
  showOrchestrationRequest: action('show-orchestration-request'),
  openOrchestrationGoal: action('open-orchestration-goal'),
  /** HS2-9XJEZC: run every validation step of the selected card that has not passed. */
  runOrchestrationValidation: action('run-orchestration-validation'),
  /** HS2-0D60T8: reopen the selected card's built-ahead work with the item's `data-reopen-reason`. */
  reopenOrchestrationWork: action('reopen-orchestration-work'),
  /** HS2-77SRHC: the overview's Show All Decisions…, and the dialog's Load Earlier. */
  showOrchestrationDecisions: action('show-orchestration-decisions'),
  loadEarlierOrchestrationDecisions: action('load-earlier-orchestration-decisions'),
  /** HS2-F1BKQ0: a person drops this machine's conflicting pre-sharing plan definitions. */
  keepCheckedInOrchestrationPlan: action('keep-checked-in-orchestration-plan'),
} as const;

/** Other delegated targets of the Orchestration view. */
export const ORCHESTRATION_TARGETS = {
  /** The view's root: ticket-collection and group handlers leave what is inside it to this view. */
  view: attr('data-orchestration-view', 'true'),
  /** HS2-77SRHC: the Show All Decisions… dialog, whose own close request dismisses it. */
  decisionsDialog: attr('data-dialog-component', 'orchestration-decisions-dialog'),
} as const;
