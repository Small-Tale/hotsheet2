import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { COMMAND_SCHEDULE_DIALOG_ID } from './command-schedule-dialog';

export interface CommandScheduleDialogBindings {
  ownsCommand: (id: string) => boolean;
  open: (id: string) => void;
  change: (name: string, value: string | string[]) => void;
  save: (fields: FormData) => void | Promise<void>;
  close: () => void;
  remove: () => void | Promise<void>;
  togglePaused: () => void | Promise<void>;
}

/** Shared production and catalog wiring for the controlled schedule form (HS2-17663F). */
export function wireCommandScheduleDialog(root: HTMLElement, bindings: CommandScheduleDialogBindings): () => void {
  const scope = createScope();
  scope.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.openCommandSchedule.selector, (_event, target) => {
      const id = target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;
      if (id && bindings.ownsCommand(id)) bindings.open(id);
    }),
  );
  scope.add(
    delegate(root, 'change', `#${COMMAND_SCHEDULE_DIALOG_ID} [name]`, (_event, target) => {
      const field = target as HTMLInputElement & { value: string | string[] };
      if (field.name) bindings.change(field.name, field.value);
    }),
  );
  scope.add(
    delegate(root, 'submit', COMMANDS_AND_AI_ACTIONS.saveCommandSchedule.selector, (event, target) => {
      event.preventDefault();
      void bindings.save(new FormData(target as HTMLFormElement));
    }),
  );
  scope.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.cancelCommandSchedule.selector, () => {
      bindings.close();
    }),
  );
  scope.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.deleteCommandSchedule.selector, () => {
      void bindings.remove();
    }),
  );
  scope.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.toggleCommandSchedule.selector, () => {
      void bindings.togglePaused();
    }),
  );
  return () => {
    scope.dispose();
  };
}
