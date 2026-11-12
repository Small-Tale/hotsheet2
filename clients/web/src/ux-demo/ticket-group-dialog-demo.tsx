import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { TicketGroupDialog, type TicketGroupDialogState } from '../components/ticket-group-dialog';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** Every TicketGroupDialog presentation the group flows render (HS2-TX17CM, HS2-8CDFKR). */
export type TicketGroupDialogScenario = 'new' | 'duplicate' | 'rename' | 'rename-busy' | 'rename-error';

export const TICKET_GROUP_DIALOG_SCENARIOS: readonly { value: TicketGroupDialogScenario; label: string }[] = [
  { value: 'new', label: 'New group' },
  { value: 'duplicate', label: 'New group (duplicate name)' },
  { value: 'rename', label: 'Rename group' },
  { value: 'rename-busy', label: 'Rename group (saving)' },
  { value: 'rename-error', label: 'Rename group (failed)' },
];

export const TICKET_GROUP_DEMO_GROUPS = ['Board drag & drop', 'Search syntax'];
/** The fixture group's registry id (HS2-9KZSD4). */
export const TICKET_GROUP_DEMO_ID = '7K3M9P2QXA';
const INITIAL_EVENT = 'Choose a presentation in the demo settings.';

export const ticketGroupDialogSettings = {
  scenario: signal<TicketGroupDialogScenario>('new'),
  open: signal(true),
  /** The typed name, so validation runs live as it does in the app. */
  name: signal(''),
  session: signal(0),
  event: signal(INITIAL_EVENT),
};

/** The name each scenario opens with. */
export function ticketGroupDialogScenarioName(scenario: TicketGroupDialogScenario): string {
  if (scenario === 'new') return '';
  return scenario === 'duplicate' ? 'board drag & drop' : 'Board drag & drop';
}

export function ticketGroupDialogState(
  scenario: TicketGroupDialogScenario,
  name: string,
  session: number,
): TicketGroupDialogState {
  const base = { name, groups: TICKET_GROUP_DEMO_GROUPS, session };
  if (scenario === 'new' || scenario === 'duplicate') return { ...base, mode: 'new' };
  return {
    ...base,
    mode: 'rename',
    // A renamed group may keep its own name, so only the other groups' names are taken.
    groups: TICKET_GROUP_DEMO_GROUPS.filter((group) => group !== 'Board drag & drop'),
    group: TICKET_GROUP_DEMO_ID,
    busy: scenario === 'rename-busy',
    error: scenario === 'rename-error' ? 'Could not rename the group: the store is locked.' : undefined,
  };
}

/** Show the chosen presentation, reopening the dialog with that scenario's name. */
export function setTicketGroupDialogScenario(scenario: TicketGroupDialogScenario): void {
  ticketGroupDialogSettings.scenario.value = scenario;
  ticketGroupDialogSettings.name.value = ticketGroupDialogScenarioName(scenario);
  ticketGroupDialogSettings.session.value += 1;
  ticketGroupDialogSettings.open.value = true;
}

export function resetTicketGroupDialogDemo(root: ParentNode): void {
  setTicketGroupDialogScenario('new');
  ticketGroupDialogSettings.event.value = INITIAL_EVENT;
  syncSettingsControls(root, 'ticket-group-dialog', { values: { 'group-dialog-scenario': 'new' } });
}

/** Demo stand-in for the production side effects: close the dialog and report the request. */
export function closeTicketGroupDialogDemo(event: string): void {
  ticketGroupDialogSettings.open.value = false;
  ticketGroupDialogSettings.event.value = event;
}

export function TicketGroupDialogDemo() {
  const { open, scenario, name, session } = ticketGroupDialogSettings;
  return (
    <>
      <CatalogExample align="glyph" label="Interactive">
        {!open.value && <Button action={DEMO_ACTIONS.openTicketGroupDialogDemo.value}>Open Dialog</Button>}
        <TicketGroupDialog
          state={open.value ? ticketGroupDialogState(scenario.value, name.value, session.value) : undefined}
        />
      </CatalogExample>
      <DemoEvent>{ticketGroupDialogSettings.event.value}</DemoEvent>
    </>
  );
}

export function TicketGroupDialogSettings() {
  return (
    <DemoSettingsForm settings="ticket-group-dialog">
      <Select
        name="group-dialog-scenario"
        label="Presentation"
        value={ticketGroupDialogSettings.scenario.value}
        choices={TICKET_GROUP_DIALOG_SCENARIOS.map(({ value, label }) => ({ value, label }))}
      />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/**
 * Install this file's settings interactions on the catalog root (HS2-HMVBZW). The dialog's own
 * production handlers are wired once, with the group header's, in `ticket-group-demo.tsx`.
 */
function wireTicketGroupDialogDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(
      root,
      'change',
      '[data-settings="ticket-group-dialog"] [name="group-dialog-scenario"]',
      (_event, target) => {
        setTicketGroupDialogScenario((target as FormControl).value as TicketGroupDialogScenario);
      },
    ),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openTicketGroupDialogDemo.selector, () => {
      setTicketGroupDialogScenario(ticketGroupDialogSettings.scenario.value);
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}
/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-group-dialog': {
      catalogExamples: true,
      Demo: TicketGroupDialogDemo,
      Settings: TicketGroupDialogSettings,
      reset: resetTicketGroupDialogDemo,
    },
  },
  wire: wireTicketGroupDialogDemo,
};
