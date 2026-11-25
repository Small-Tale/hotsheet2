import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { closeDialog, openDialog } from '@kerfjs/ui/surface-scaffold';
import { attr, delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  newScheduleDraft,
  type ScheduleFormDraft,
  scheduleInputFromDraft,
  withRepeats,
} from '../command-schedule-form';
import type { ScheduleView } from '../command-schedules';
import { COMMAND_SCHEDULE_DIALOG_ID, CommandScheduleDialog } from '../components/command-schedule-dialog';
import { wireCommandScheduleDialog } from '../components/command-schedule-dialog-interactions';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import type { DemoModule } from './demo-module';

const draft = signal(newScheduleDraft());
const saved = signal<ScheduleView | undefined>(undefined);
const samplePreview = ['2027-02-15T01:00:00Z', '2028-02-21T01:00:00Z', '2029-02-19T01:00:00Z'];
const preview = signal(samplePreview);
const showSkippedRun = attr('data-action', 'demo-command-schedule-skipped');
const showHourly = attr('data-action', 'demo-command-schedule-hourly');
const hourlyPreview = ['2027-02-15T01:00:00Z', '2027-02-15T02:00:00Z', '2027-02-15T03:00:00Z'];

/** Deterministic synthetic schedule; the dialog and its controls are production components. */
export function CommandScheduleDialogDemo() {
  return (
    <CatalogExample
      label="Schedule AI Command"
      note="Open the dialog, choose a recurrence, then save, pause, resume, or delete it."
    >
      <div data-command-id="review-architecture">
        <Button action={COMMANDS_AND_AI_ACTIONS.openCommandSchedule.value} ariaHasPopup="dialog">
          {saved.value ? 'Manage Schedule…' : 'Schedule…'}
        </Button>
        {saved.value ? <Button action={showSkippedRun.value}>Show Skipped Run</Button> : null}
        <Button action={showHourly.value}>Show Hourly</Button>
      </div>
      <CommandScheduleDialog
        commandTitle="Review Architecture"
        draft={draft.value}
        saved={saved.value}
        preview={preview.value}
      />
    </CatalogExample>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'command-schedule-dialog': {
      catalogExamples: true,
      Demo: CommandScheduleDialogDemo,
      reset: () => {
        draft.value = newScheduleDraft();
        saved.value = undefined;
        preview.value = samplePreview;
      },
    },
  },
  wire(root) {
    const scope = createScope();
    scope.add(
      wireCommandScheduleDialog(root, {
        ownsCommand: (id) => id === 'review-architecture',
        open: () => {
          const dialog = root.querySelector<HTMLDialogElement>(`#${COMMAND_SCHEDULE_DIALOG_ID}`);
          if (dialog) openDialog(dialog);
        },
        change: (name, value) => {
          if (name === 'weekdays') draft.value = { ...draft.value, weekdays: (value as string[]).map(Number) };
          else if (name === 'repeats') draft.value = withRepeats(draft.value, value as ScheduleFormDraft['repeats']);
          else draft.value = { ...draft.value, [name]: value };
          preview.value = [];
        },
        save: () => {
          saved.value = {
            schedule: { command_id: 'review-architecture', ...scheduleInputFromDraft(draft.value) },
            next_runs: preview.value,
            history: [],
          };
          const dialog = root.querySelector<HTMLDialogElement>(`#${COMMAND_SCHEDULE_DIALOG_ID}`);
          if (dialog) closeDialog(dialog);
        },
        close: () => {
          const dialog = root.querySelector<HTMLDialogElement>(`#${COMMAND_SCHEDULE_DIALOG_ID}`);
          if (dialog) closeDialog(dialog);
        },
        remove: () => {
          saved.value = undefined;
          draft.value = newScheduleDraft();
          const dialog = root.querySelector<HTMLDialogElement>(`#${COMMAND_SCHEDULE_DIALOG_ID}`);
          if (dialog) closeDialog(dialog);
        },
        togglePaused: () => {
          draft.value = { ...draft.value, paused: !draft.value.paused };
        },
      }),
    );
    scope.add(
      delegate(root, 'click', showHourly.selector, () => {
        // HS2-29V4ZG: a sub-day draft shows the minute floor and the hourly preview.
        draft.value = withRepeats({ ...draft.value, scheduleType: 'repeating' }, 'hourly');
        preview.value = hourlyPreview;
      }),
    );
    scope.add(
      delegate(root, 'click', showSkippedRun.selector, () => {
        if (!saved.value) return;
        saved.value = {
          ...saved.value,
          history: [{ occurrence_date: '2027-02-15', outcome: 'skipped_open_ticket', ticket_slug: 'HS2-DEMO01' }],
        };
      }),
    );
    return () => {
      scope.dispose();
    };
  },
};
