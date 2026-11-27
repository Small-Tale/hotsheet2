import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { ORCHESTRATION_ACTIONS, ORCHESTRATION_TARGETS } from '../interaction-attrs/orchestration';
import { TICKET_GROUP_ACTIONS, TICKET_GROUP_TARGETS } from '../interaction-attrs/ticket-groups';
import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import type { SaveGoalForm } from '../orchestration-templates';
import { stepOfCard } from '../orchestration-timeline-board';

/**
 * Side effects of the Orchestration view (HS2-J89JC7). Its cards are tickets of other projects, so
 * selecting one selects its plan step rather than a ticket of this project, its project groups
 * collapse in the view's own scope, and the ticket context menus do not open on it. Production passes
 * the orchestration controller; the UX demo passes fixture state, so both run the same handlers.
 */
export interface OrchestrationTimelineBindings {
  /**
   * Select a plan step. `moved` is true for an arrow-key move (HS2-T4V1TX), which keeps focus on the
   * board: a phone does not open the inspector sheet over it, as a click, Enter or Space does.
   */
  readonly select: (stepId: string, moved?: boolean) => void;
  readonly toggleGroup: (group: string) => void;
  readonly start: () => void;
  readonly pause: () => void;
  readonly propose: () => void;
  /** HS2-190RDX: Update Plan and Cancel Plan Update. */
  readonly updatePlan: () => void;
  readonly cancelPlanUpdate: () => void;
  /** HS2-N5BDE4: Answer Question opens the goal ticket the planner asked on. */
  readonly answerQuestion: () => void;
  /** HS2-FA8G9J: open the planning session's terminal tab. */
  readonly showPlanner: () => void;
  readonly chooseProjects: () => void;
  /** `pause`, `resume`, `handoff`, `correct` (with the state to record) or `cancel`. */
  readonly runControl: (control: string, state?: string) => void;
  readonly showRequest: (requestId: string) => void;
  /** HS2-A7YCB2: Link Again on the selected run's unknown CI gate. */
  readonly linkGate: (gateId: string) => void;
  readonly openGoal: () => void;
  /** HS2-9XJEZC: Run Validation on the selected card. */
  readonly runValidation: () => void;
  /** HS2-0D60T8: Reopen… with the chosen reason code. */
  readonly reopen: (reason: string) => void;
  /** HS2-77SRHC: open the decision log dialog, read the page before it, and close it. */
  readonly showDecisions: () => void;
  readonly loadEarlierDecisions: () => void;
  readonly closeDecisions: () => void;
  /** HS2-F1BKQ0: Keep the Checked-In Plan… on a plan conflict this machine's copies cause. */
  readonly keepCheckedInPlan: () => void;
  /** HS2-A4RVYA: Use Template… and its dialog. Absent where the surface offers no templates. */
  readonly templates?: OrchestrationTemplateBindings;
}

/** HS2-A4RVYA: the Use Template… dialog's actions. */
export interface OrchestrationTemplateBindings {
  readonly open: () => void;
  readonly select: (templateId: string) => void;
  /** A typed input changed; `commit` is a change event (not each keystroke), which reads the summary again. */
  readonly editInput: (key: string, value: unknown, commit: boolean) => void;
  readonly chooseStep: (step: string, create: boolean) => void;
  readonly apply: () => void;
  readonly openSave: () => void;
  readonly editSave: (change: Partial<SaveGoalForm>) => void;
  readonly cancelSave: () => void;
  readonly submitSave: () => void;
  readonly openGoal: () => void;
  readonly back: () => void;
  readonly close: () => void;
}

const TEMPLATE_INPUT = '[name^="template-input-"]';
const TEMPLATE_STEP = '[name^="template-step-create-"]';
const TEMPLATE_SAVE = '[name^="template-save-"]';
type FieldElement = Element & { value?: unknown; checked?: boolean; type?: string };

/** The value of a template input field: a checkbox's checked state, otherwise its value. */
function fieldValue(target: FieldElement): unknown {
  return target.type === 'checkbox' ? target.checked === true : typeof target.value === 'string' ? target.value : '';
}

function wireTemplateDialog(root: HTMLElement, current: () => OrchestrationTemplateBindings | undefined) {
  const lifetime = createScope();
  const dialog = ORCHESTRATION_TARGETS.templateDialog.selector;
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.useOrchestrationTemplate.selector, () => {
      current()?.open();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.selectOrchestrationTemplate.selector, (_event, target) => {
      const id = target.closest<HTMLElement>('[data-template-id]')?.dataset.templateId;
      if (id) current()?.select(id);
    }),
  );
  for (const commit of [false, true])
    lifetime.add(
      delegate(root, commit ? 'change' : 'input', TEMPLATE_INPUT, (_event, target) => {
        if (!target.closest(dialog)) return;
        const field = target as FieldElement;
        const key = (field.getAttribute('name') ?? '').slice('template-input-'.length);
        if (!key) return;
        // A text field reports each keystroke as input and commits on change; a checkbox or select only changes.
        if (!commit && field.type === 'checkbox') return;
        current()?.editInput(key, fieldValue(field), commit);
      }),
    );
  // Leaving a text field commits it too: WebKit fires no change once a re-render wrote the same value back.
  lifetime.add(
    delegate(root, 'focusout', TEMPLATE_INPUT, (_event, target) => {
      const field = target as FieldElement;
      const key = (field.getAttribute('name') ?? '').slice('template-input-'.length);
      if (key && target.closest(dialog) && (target.localName === 'input' || target.localName === 'textarea'))
        if (field.type !== 'checkbox') current()?.editInput(key, fieldValue(field), true);
    }),
  );
  lifetime.add(
    delegate(root, 'change', TEMPLATE_STEP, (_event, target) => {
      const field = target as FieldElement;
      const step = (field.getAttribute('name') ?? '').slice('template-step-create-'.length);
      if (step && target.closest(dialog)) current()?.chooseStep(step, field.checked === true);
    }),
  );
  for (const type of ['input', 'change'])
    lifetime.add(
      delegate(root, type, TEMPLATE_SAVE, (_event, target) => {
        if (!target.closest(dialog)) return;
        const field = target as FieldElement;
        const value = fieldValue(field);
        switch (field.getAttribute('name')) {
          case 'template-save-goal':
            current()?.editSave({ goal: String(value) });
            break;
          case 'template-save-name':
            current()?.editSave({ name: String(value) });
            break;
          case 'template-save-description':
            current()?.editSave({ description: String(value) });
            break;
          case 'template-save-inputs':
            if (type === 'change') current()?.editSave({ targetsAsInputs: value === true });
            break;
          case null:
          default:
            break;
        }
      }),
    );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.applyOrchestrationTemplate.selector, () => {
      current()?.apply();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.saveOrchestrationGoalTemplate.selector, () => {
      current()?.openSave();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.cancelOrchestrationGoalTemplate.selector, () => {
      current()?.cancelSave();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.submitOrchestrationGoalTemplate.selector, () => {
      current()?.submitSave();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.openOrchestrationTemplateGoal.selector, () => {
      current()?.openGoal();
    }),
  );
  lifetime.add(
    delegate(root, 'click', '[data-nav-back]', (_event, target) => {
      if (target.closest(dialog)) current()?.back();
    }),
  );
  lifetime.add(
    delegate(root, 'kui-request-close', dialog, (event, target) => {
      // Only the dialog's own close request counts (Escape, the close control, Cancel and Close).
      if (event.target === target) current()?.close();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

const inView = (target: Element) => Boolean(target.closest(ORCHESTRATION_TARGETS.view.selector));

export type OrchestrationArrowKey = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight';
const ARROWS: ReadonlySet<string> = new Set(['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight']);

/**
 * HS2-T4V1TX: the card an arrow key moves to on the Orchestration board. `columns` are the drawn
 * columns from left to right, each with its cards' ids from top to bottom (a collapsed project group
 * draws none). Up and Down move within the column and stop at its ends; Left and Right move to the
 * nearest column in that direction that has cards, at the same position or its last card, and stop at
 * the board's edges. An id the board does not draw starts at the first card. `undefined` when the
 * board has no card.
 */
export function adjacentOrchestrationCard(
  columns: ReadonlyArray<readonly string[]>,
  current: string | undefined,
  key: OrchestrationArrowKey,
): string | undefined {
  const column = columns.findIndex((cards) => current !== undefined && cards.includes(current));
  if (column < 0) return columns.find((cards) => cards.length > 0)?.[0];
  const cards = columns[column],
    index = cards.indexOf(current!);
  if (key === 'ArrowUp') return cards[Math.max(0, index - 1)];
  if (key === 'ArrowDown') return cards[Math.min(cards.length - 1, index + 1)];
  const step = key === 'ArrowLeft' ? -1 : 1;
  for (let next = column + step; next >= 0 && next < columns.length; next += step) {
    const target = columns[next];
    if (target.length > 0) return target[Math.min(index, target.length - 1)];
  }
  return current;
}

/** The drawn board's cards per column, left to right, as {@link adjacentOrchestrationCard} reads them. */
function drawnColumns(view: Element): HTMLElement[][] {
  return [...view.querySelectorAll<HTMLElement>('[data-component="ticket-board-column"]')].map((column) => [
    ...column.querySelectorAll<HTMLElement>(TICKET_SELECTION_ACTIONS.selectTicketRow.selector),
  ]);
}

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
      const keyboard = event as KeyboardEvent;
      if (!inView(target)) return;
      if (keyboard.key === 'Enter' || keyboard.key === ' ') {
        event.preventDefault();
        const step = stepOfCard((target as HTMLElement).dataset.ticketQualifiedId);
        if (step) bindings.select(step);
        return;
      }
      // HS2-T4V1TX: arrows move between cards and select the plan step the card shows.
      const view = target.closest(ORCHESTRATION_TARGETS.view.selector);
      if (!view || !ARROWS.has(keyboard.key)) return;
      if (keyboard.altKey || keyboard.metaKey || keyboard.ctrlKey || keyboard.shiftKey) return;
      event.preventDefault();
      const cards = drawnColumns(view),
        id = (card: HTMLElement) => card.dataset.ticketQualifiedId ?? '',
        next = adjacentOrchestrationCard(
          cards.map((column) => column.map(id)),
          id(target as HTMLElement),
          keyboard.key as OrchestrationArrowKey,
        );
      const card = cards.flat().find((candidate) => id(candidate) === next);
      if (!card || card === target) return;
      // Bring the whole column into view first, header included, then the card within it; a
      // focus scroll alone leaves a column cut at the board's edge (WebKit also scrolls the column).
      card.focus({ preventScroll: true });
      card.closest('[data-component="ticket-board-column"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      card.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      const step = stepOfCard(next);
      if (step) bindings.select(step, true);
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
    delegate(root, 'click', ORCHESTRATION_ACTIONS.updateOrchestrationPlan.selector, () => {
      bindings.updatePlan();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.cancelOrchestrationPlanUpdate.selector, () => {
      bindings.cancelPlanUpdate();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.answerOrchestrationQuestion.selector, () => {
      bindings.answerQuestion();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.showOrchestrationPlanner.selector, () => {
      bindings.showPlanner();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.chooseOrchestrationProjects.selector, () => {
      bindings.chooseProjects();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.openOrchestrationSettings.selector, () => {
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
    delegate(root, 'click', ORCHESTRATION_ACTIONS.linkOrchestrationGate.selector, (_event, target) => {
      const gateId = (target as HTMLElement).dataset.gateId;
      if (gateId) bindings.linkGate(gateId);
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
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.runOrchestrationValidation.selector, () => {
      bindings.runValidation();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.reopenOrchestrationWork.selector, (_event, target) => {
      const reason = (target as HTMLElement).dataset.reopenReason;
      if (reason) bindings.reopen(reason);
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.showOrchestrationDecisions.selector, () => {
      bindings.showDecisions();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.loadEarlierOrchestrationDecisions.selector, () => {
      bindings.loadEarlierDecisions();
    }),
  );
  lifetime.add(
    delegate(root, 'click', ORCHESTRATION_ACTIONS.keepCheckedInOrchestrationPlan.selector, () => {
      bindings.keepCheckedInPlan();
    }),
  );
  lifetime.add(
    delegate(root, 'kui-request-close', ORCHESTRATION_TARGETS.decisionsDialog.selector, (event, target) => {
      // Only the dialog's own close request counts (Escape, the backdrop).
      if (event.target === target) bindings.closeDecisions();
    }),
  );
  // HS2-A4RVYA: read lazily, so a surface wired without templates registers handlers that do nothing.
  lifetime.add(wireTemplateDialog(root, () => bindings.templates));
  return () => {
    lifetime.dispose();
  };
}
