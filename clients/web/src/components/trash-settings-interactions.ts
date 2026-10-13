import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';

/**
 * Side effects for TrashSettings (HS2-WFFDAA): production passes the project retention state and
 * server writes; the UX demo passes fixture callbacks, so both resolve the same edit, apply, and
 * leave-warning gestures.
 */
export interface TrashSettingsBindings {
  /** A keystroke in the retention field. */
  readonly input: (value: string, field: HTMLElement) => void;
  /** The form's "Apply now" submit; the helper has already prevented navigation. */
  readonly submit: (form: HTMLElement) => void;
  /** "Apply" in the unapplied-retention leave warning. */
  readonly applyLeaving: (control: HTMLElement) => void;
  /** "Revert" in the unapplied-retention leave warning. */
  readonly revert: (control: HTMLElement) => void;
  /** The leave warning closed without a choice (Escape or its close button): stay with the draft. */
  readonly dismissLeaving: (dialog: HTMLElement) => void;
}

/** Wire every TrashSettings form and its leave warning under `root`. */
export function wireTrashSettings(root: HTMLElement, bindings: TrashSettingsBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'input', COMMANDS_AND_AI_TARGETS.trashRetentionDaysField.selector, (_event, target) => {
      bindings.input((target as HTMLInputElement).value, target as HTMLElement);
    }),
  );
  lifetime.add(
    delegate(root, 'submit', COMMANDS_AND_AI_ACTIONS.saveTrashSettings.selector, (event, target) => {
      event.preventDefault();
      bindings.submit(target as HTMLElement);
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.applyLeavingTrashSettings.selector, (_event, target) => {
      bindings.applyLeaving(target as HTMLElement);
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.revertTrashSettings.selector, (_event, target) => {
      bindings.revert(target as HTMLElement);
    }),
  );
  lifetime.add(
    delegateCapture(root, 'wa-hide', COMMANDS_AND_AI_TARGETS.trashRetentionLeaveDialog.selector, (event, target) => {
      if (event.target !== event.currentTarget && !(event.target as Element).matches('wa-dialog')) return;
      bindings.dismissLeaving(target as HTMLElement);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
