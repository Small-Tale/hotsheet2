import { Select } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import { BulkTicketDialog, type BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import { DEMO_ACTIONS } from './demo-actions';
import { syncSettingsControls } from './settings-controls';

/** Every public BulkTicketDialog presentation the production selection flows render (HS2-PS9BQV). */
export type BulkTicketDialogScenario =
  'add-tag' | 'remove-tag' | 'delete' | 'empty-trash' | 'empty-trash-busy' | 'empty-trash-error';

const COUNT = 5;
const TAG_CHOICES = ['bug', 'ui', 'backend', 'docs'];

export const BULK_TICKET_DIALOG_SCENARIOS: readonly { value: BulkTicketDialogScenario; label: string }[] = [
  { value: 'add-tag', label: 'Add tag' },
  { value: 'remove-tag', label: 'Remove tag (with choices)' },
  { value: 'delete', label: 'Delete confirmation' },
  { value: 'empty-trash', label: 'Empty Trash confirmation' },
  { value: 'empty-trash-busy', label: 'Empty Trash (busy)' },
  { value: 'empty-trash-error', label: 'Empty Trash (error)' },
];

export const bulkTicketDialogSettings = {
  scenario: signal<BulkTicketDialogScenario>('add-tag'),
  open: signal(true),
  event: signal('Choose a presentation in the demo settings.'),
};

export function bulkTicketDialogState(scenario: BulkTicketDialogScenario): BulkTicketDialogState {
  switch (scenario) {
    case 'add-tag':
      return { kind: 'tag', mode: 'add', count: COUNT, choices: TAG_CHOICES };
    case 'remove-tag':
      return { kind: 'tag', mode: 'remove', count: COUNT, choices: TAG_CHOICES };
    case 'delete':
      return { kind: 'delete', count: COUNT };
    case 'empty-trash':
      return { kind: 'empty-trash', count: COUNT };
    case 'empty-trash-busy':
      return { kind: 'empty-trash', count: COUNT, busy: true };
    case 'empty-trash-error':
      return {
        kind: 'empty-trash',
        count: COUNT,
        error: 'Could not empty Trash: the store is locked by another process.',
      };
  }
}

/** Show the chosen presentation, reopening the dialog so the change is visible. */
export function setBulkTicketDialogScenario(scenario: BulkTicketDialogScenario): void {
  bulkTicketDialogSettings.scenario.value = scenario;
  bulkTicketDialogSettings.open.value = true;
}

export function resetBulkTicketDialogDemo(root: ParentNode): void {
  setBulkTicketDialogScenario('add-tag');
  bulkTicketDialogSettings.event.value = 'Choose a presentation in the demo settings.';
  syncSettingsControls(root, 'bulk-ticket-dialog', { values: { 'bulk-scenario': 'add-tag' } });
}

export function openBulkTicketDialogDemo(): void {
  bulkTicketDialogSettings.open.value = true;
}

/** Demo stand-ins for the production selection side effects: close the dialog and report the request. */
export function closeBulkTicketDialogDemo(event: string): void {
  bulkTicketDialogSettings.open.value = false;
  bulkTicketDialogSettings.event.value = event;
}

export function BulkTicketDialogDemo() {
  const open = bulkTicketDialogSettings.open.value;
  return (
    <section class="component-stage" aria-label="BulkTicketDialog demo">
      {!open && (
        <button type="button" {...DEMO_ACTIONS.openBulkTicketDialogDemo.attrs}>
          Open dialog
        </button>
      )}
      <BulkTicketDialog state={open ? bulkTicketDialogState(bulkTicketDialogSettings.scenario.value) : undefined} />
      <p class="component-stage__event" aria-live="polite">
        {bulkTicketDialogSettings.event.value}
      </p>
    </section>
  );
}

export function BulkTicketDialogSettings() {
  return (
    <form class="settings-form" data-settings="bulk-ticket-dialog">
      <Select
        name="bulk-scenario"
        label="Presentation"
        value={bulkTicketDialogSettings.scenario.value}
        choices={BULK_TICKET_DIALOG_SCENARIOS.map(({ value, label }) => ({ value, label }))}
      />
      <wa-button type="button" {...DEMO_ACTIONS.resetSettings.attrs}>
        Reset
      </wa-button>
    </form>
  );
}
