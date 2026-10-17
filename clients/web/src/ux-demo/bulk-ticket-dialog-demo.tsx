import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { BulkTicketDialog, type BulkTicketDialogState } from '../components/bulk-ticket-dialog';
import { wireBulkTicketDialog } from '../components/bulk-ticket-dialog-interactions';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
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
    <>
      <CatalogExample align="glyph" label="Interactive">
        {!open && <Button action={DEMO_ACTIONS.openBulkTicketDialogDemo.value}>Open Dialog</Button>}
        <BulkTicketDialog state={open ? bulkTicketDialogState(bulkTicketDialogSettings.scenario.value) : undefined} />
      </CatalogExample>
      <DemoEvent>{bulkTicketDialogSettings.event.value}</DemoEvent>
    </>
  );
}

export function BulkTicketDialogSettings() {
  return (
    <DemoSettingsForm settings="bulk-ticket-dialog">
      <Select
        name="bulk-scenario"
        label="Presentation"
        value={bulkTicketDialogSettings.scenario.value}
        choices={BULK_TICKET_DIALOG_SCENARIOS.map(({ value, label }) => ({ value, label }))}
      />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireBulkTicketDialogDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="bulk-ticket-dialog"] [name="bulk-scenario"]', (_event, target) => {
      setBulkTicketDialogScenario((target as FormControl).value as BulkTicketDialogScenario);
    }),
  );
  demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.openBulkTicketDialogDemo.selector, openBulkTicketDialogDemo));
  // The production BulkTicketDialog handlers; each action closes the dialog and reports what
  // production would request (HS2-RWY78R).
  demoListeners.add(
    wireBulkTicketDialog(root, {
      submitTag: (mode, tag) => {
        const adding = mode === 'add';
        closeBulkTicketDialogDemo(
          `${adding ? 'Add' : 'Remove'} tag “${tag.trim()}” ${adding ? 'to' : 'from'} 5 tickets requested.`,
        );
      },
      confirmDelete: () => {
        closeBulkTicketDialogDemo('Delete 5 tickets requested.');
      },
      confirmEmptyTrash: () => {
        closeBulkTicketDialogDemo('Empty Trash requested for 5 tickets.');
      },
      dismiss: (reason) => {
        closeBulkTicketDialogDemo(
          reason === 'cancel' ? 'Cancelled; the selection is unchanged.' : 'Dismissed; the selection is unchanged.',
        );
      },
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}
/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'bulk-ticket-dialog': {
      catalogExamples: true,
      Demo: BulkTicketDialogDemo,
      Settings: BulkTicketDialogSettings,
      reset: resetBulkTicketDialogDemo,
    },
  },
  wire: wireBulkTicketDialogDemo,
};
