import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { ORCHESTRATION_ACTIONS, ORCHESTRATION_TARGETS } from '../interaction-attrs/orchestration';
import { TICKET_GROUP_ACTIONS, TICKET_GROUP_TARGETS } from '../interaction-attrs/ticket-groups';
import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import { stepOfCard } from '../orchestration-timeline-board';

/**
 * Side effects of the Orchestration view (HS2-J89JC7). Its cards are tickets of other projects, so
 * selecting one selects its plan step rather than a ticket of this project, its project groups
 * collapse in the view's own scope, and the ticket context menus do not open on it. Production passes
 * the orchestration controller; the UX demo passes fixture state, so both run the same handlers.
 */
export interface OrchestrationTimelineBindings {
  readonly select: (stepId: string) => void;
  readonly toggleGroup: (group: string) => void;
  readonly start: () => void;
  readonly pause: () => void;
  readonly propose: () => void;
  readonly chooseProjects: () => void;
  /** `pause`, `resume`, `handoff`, `correct` (with the state to record) or `cancel`. */
  readonly runControl: (control: string, state?: string) => void;
  readonly showRequest: (requestId: string) => void;
  readonly openGoal: () => void;
}

const inView = (target: Element) => Boolean(target.closest(ORCHESTRATION_TARGETS.view.selector));

/** Whether an event target belongs to the Orchestration view, whose cards are not this project's tickets. */
export function insideOrchestrationView(target: EventTarget | null): boolean {
  const closest = (target as Partial<Element> | null)?.closest;
  if (typeof closest !== 'function') return false;
  const view = closest.call(target, ORCHESTRATION_TARGETS.view.selector) as Partial<HTMLElement> | null;
  return view?.dataset?.orchestrationView === 'true';
}

/** Wire the Orchestration view's cards, groups, kick-off, and inspector actions under `root`. */
export function wireOrchestrationTimeline(root: HTMLElement, bindings: OrchestrationTimelineBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', TICKET_SELECTION_ACTIONS.selectTicketRow.selector, (_event, target) => {
      if (!inView(target)) return;
      const step = stepOfCard((target as HTMLElement).dataset.ticketQualifiedId);
      if (step) bindings.select(step);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', TICKET_SELECTION_ACTIONS.selectTicketRow.selector, (event, target) => {
      const key = (event as KeyboardEvent).key;
      if (!inView(target) || (key !== 'Enter' && key !== ' ')) return;
      event.preventDefault();
      const step = stepOfCard((target as HTMLElement).dataset.ticketQualifiedId);
      if (step) bindings.select(step);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.toggleTicketGroup.selector, (_event, target) => {
      if (!inView(target)) return;
      const group =
        (target as HTMLElement).dataset.ticketGroup ??
        target.closest<HTMLElement>(TICKET_GROUP_TARGETS.group.selector)?.dataset.ticketGroup;
      if (group) bindings.toggleGroup(group);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.startOrchestration.selector, () => {
      bindings.start();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.pauseOrchestration.selector, () => {
      bindings.pause();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.proposeOrchestrationPlan.selector, () => {
      bindings.propose();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.chooseOrchestrationProjects.selector, () => {
      bindings.chooseProjects();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.orchestrationRunControl.selector, (_event, target) => {
      const { control, runState } = (target as HTMLElement).dataset;
      if (control) bindings.runControl(control, runState);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.showOrchestrationRequest.selector, (_event, target) => {
      bindings.showRequest((target as HTMLElement).dataset.requestId ?? '');
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.openOrchestrationGoal.selector, () => {
      bindings.openGoal();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
