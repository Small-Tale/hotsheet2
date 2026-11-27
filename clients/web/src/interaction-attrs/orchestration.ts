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
  /** HS2-FA8G9J: open the drawer on the planning session's terminal tab while it waits there. */
  showOrchestrationPlanner: action('show-orchestration-planner'),
  chooseOrchestrationProjects: action('choose-orchestration-projects'),
  /** HS2-1DC8X1: a blocked Start Orchestration's banner opens the settings that unblock it. */
  openOrchestrationSettings: action('open-orchestration-settings'),
  orchestrationRunControl: action('orchestration-run-control'),
  showOrchestrationRequest: action('show-orchestration-request'),
  /** HS2-A7YCB2: Link Again on an unknown CI gate, with the gate's `data-gate-id`. */
  linkOrchestrationGate: action('link-orchestration-gate'),
  openOrchestrationGoal: action('open-orchestration-goal'),
  /** HS2-36XDDX: accept the template plan a held card waits for, with the apply's `data-operation-id`. */
  acceptOrchestrationTemplatePlan: action('accept-orchestration-template-plan'),
  /** HS2-0YHA2S: the header's Accept Plan, which accepts every template plan held now. */
  acceptOrchestrationTemplatePlans: action('accept-orchestration-template-plans'),
  /** HS2-9XJEZC: run every validation step of the selected card that has not passed. */
  runOrchestrationValidation: action('run-orchestration-validation'),
  /** HS2-0D60T8: reopen the selected card's built-ahead work with the item's `data-reopen-reason`. */
  reopenOrchestrationWork: action('reopen-orchestration-work'),
  /** HS2-77SRHC: the overview's Show All Decisions…, and the dialog's Load Earlier. */
  showOrchestrationDecisions: action('show-orchestration-decisions'),
  loadEarlierOrchestrationDecisions: action('load-earlier-orchestration-decisions'),
  /** HS2-F1BKQ0: a person drops this machine's conflicting pre-sharing plan definitions. */
  keepCheckedInOrchestrationPlan: action('keep-checked-in-orchestration-plan'),
  /**
   * HS2-A4RVYA: Use Template… opens the template dialog; a template row (with `data-template-id`) selects it;
   * Apply Template applies it; Save a Goal as Template… opens the save form, which Save Template submits;
   * the result's Open <goal> opens the goal ticket the apply opened.
   */
  useOrchestrationTemplate: action('use-orchestration-template'),
  selectOrchestrationTemplate: action('select-orchestration-template'),
  applyOrchestrationTemplate: action('apply-orchestration-template'),
  saveOrchestrationGoalTemplate: action('save-orchestration-goal-template'),
  cancelOrchestrationGoalTemplate: action('cancel-orchestration-goal-template'),
  submitOrchestrationGoalTemplate: action('submit-orchestration-goal-template'),
  openOrchestrationTemplateGoal: action('open-orchestration-template-goal'),
  /**
   * HS2-KXKA7X: Triggers… opens the trigger dialog; a trigger row (with `data-trigger-id`) selects it; New
   * Trigger… and Edit… open the setup, which Save submits and Cancel leaves; Turn On… is the reviewed turn on;
   * Pause and Resume change its state; Open <goal> opens the goal of the run a skip waits for.
   */
  openOrchestrationTriggers: action('open-orchestration-triggers'),
  selectOrchestrationTrigger: action('select-orchestration-trigger'),
  newOrchestrationTrigger: action('new-orchestration-trigger'),
  editOrchestrationTrigger: action('edit-orchestration-trigger'),
  cancelOrchestrationTrigger: action('cancel-orchestration-trigger'),
  saveOrchestrationTrigger: action('save-orchestration-trigger'),
  turnOnOrchestrationTrigger: action('turn-on-orchestration-trigger'),
  pauseOrchestrationTrigger: action('pause-orchestration-trigger'),
  resumeOrchestrationTrigger: action('resume-orchestration-trigger'),
  openOrchestrationTriggerGoal: action('open-orchestration-trigger-goal'),
} as const;

/** Other delegated targets of the Orchestration view. */
export const ORCHESTRATION_TARGETS = {
  /** The view's root: ticket-collection and group handlers leave what is inside it to this view. */
  view: attr('data-orchestration-view', 'true'),
  /** HS2-77SRHC: the Show All Decisions… dialog, whose own close request dismisses it. */
  decisionsDialog: attr('data-dialog-component', 'orchestration-decisions-dialog'),
  /** HS2-A4RVYA: the Use Template… dialog, whose own close request dismisses it. */
  templateDialog: attr('data-dialog-component', 'orchestration-template-dialog'),
  /** HS2-KXKA7X: the Triggers… dialog, whose own close request dismisses it. */
  triggerDialog: attr('data-dialog-component', 'orchestration-trigger-dialog'),
} as const;
