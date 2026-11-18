import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { action } from 'kerfjs/actions';
import { createScope } from 'kerfjs/scope';

import { TicketGroupMergeDialog, type TicketGroupMergeDialogState } from '../components/ticket-group-merge-dialog';
import { TicketGroupOperationBanner } from '../components/ticket-group-operation-banner';
import { TicketGroupSettings } from '../components/ticket-group-settings';
import { wireTicketGroupSettings } from '../components/ticket-group-settings-interactions';
import type { CheckoutGroupSummary } from '../server-response-parsers';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** Synthetic groups for Project Settings › Ticket groups (HS2-GXKX1Q, Design v6). */
const group = (id: string, name: string, count: number, duplicate = false): CheckoutGroupSummary => ({
  id,
  name,
  count,
  named: true,
  legacy: false,
  duplicate,
});
const LIST = [
  group('01M7P6A000', 'Launch readiness', 18),
  group('01M7P6B000', 'Design system', 7),
  group('01M7P6C000', 'Research', 3),
  group('01M7P6D000', 'Future work', 0),
];
const STATES = {
  list: { label: 'Groups', groups: LIST },
  duplicates: {
    label: 'Duplicate Names',
    groups: [
      group('01M7P6A000', 'Launch readiness', 18, true),
      group('01M7P6E000', 'Launch readiness', 3, true),
      group('01M7P6D000', 'Future work', 0),
    ],
  },
  busy: { label: 'Merge Running', groups: LIST, busyGroup: '01M7P6A000' },
  error: { label: 'Read Failed', groups: LIST, error: 'Could not read the ticket groups: the server is unreachable.' },
  empty: { label: 'No Groups', groups: [] },
  loading: { label: 'Loading', groups: undefined },
} as const satisfies Record<
  string,
  { label: string; groups?: readonly CheckoutGroupSummary[]; busyGroup?: string; error?: string }
>;
type DemoState = keyof typeof STATES;

const openMerge = action('demo-open-group-merge');

export const ticketGroupSettingsDemo = {
  state: signal<DemoState>('list'),
  merge: signal<TicketGroupMergeDialogState | undefined>(undefined),
  event: signal(''),
};

export function resetTicketGroupSettingsDemo(root?: ParentNode): void {
  ticketGroupSettingsDemo.state.value = 'list';
  ticketGroupSettingsDemo.merge.value = undefined;
  ticketGroupSettingsDemo.event.value = '';
  if (root) syncSettingsControls(root, 'ticket-group-settings', { values: { state: 'list' } });
}

export function TicketGroupSettingsSettings() {
  return (
    <DemoSettingsForm settings="ticket-group-settings">
      <Select
        name="state"
        label="State"
        value={ticketGroupSettingsDemo.state.value}
        choices={Object.entries(STATES).map(([value, { label }]) => ({ value, label }))}
      />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** The settings page over fixtures; its commands report what production would do. */
export function TicketGroupSettingsDemo() {
  const state: { groups?: readonly CheckoutGroupSummary[]; busyGroup?: string; error?: string } =
    STATES[ticketGroupSettingsDemo.state.value];
  return (
    <>
      <CatalogExample label="Ticket Groups" viewport={{ width: 'full' }}>
        <TicketGroupSettings groups={state.groups} busyGroup={state.busyGroup} error={state.error} />
      </CatalogExample>
      <DemoEvent>{ticketGroupSettingsDemo.event.value}</DemoEvent>
    </>
  );
}

const mergeState = (): TicketGroupMergeDialogState => ({
  group: { id: LIST[0].id, name: LIST[0].name, count: LIST[0].count },
  destinations: LIST.slice(1).map(({ id, name }) => ({ id, name })),
  into: LIST[1].id,
  session: Date.now(),
});

/** Merge into…, opened from a visible trigger and dismissed by Cancel, Escape, or Merge groups. */
export function TicketGroupMergeDialogDemo() {
  return (
    <>
      <CatalogExample label="Merge Into…">
        <Button action={openMerge.value}>Merge Group…</Button>
        <TicketGroupMergeDialog state={ticketGroupSettingsDemo.merge.value} />
      </CatalogExample>
      <DemoEvent>{ticketGroupSettingsDemo.event.value}</DemoEvent>
    </>
  );
}

export function TicketGroupOperationBannerDemo() {
  return (
    <>
      <CatalogExample label="Merge Running" viewport={{ width: 'full' }}>
        {TicketGroupOperationBanner({
          operation: { projectId: 'demo', title: 'Merging Launch readiness into Design system' },
        })}
      </CatalogExample>
      <CatalogExample label="Delete Running" viewport={{ width: 'full' }}>
        {TicketGroupOperationBanner({ operation: { projectId: 'demo', title: 'Deleting Research' } })}
      </CatalogExample>
    </>
  );
}

const name = (id: string) => LIST.find((item) => item.id === id)?.name ?? id;

function wireTicketGroupSettingsDemo(root: HTMLElement): () => void {
  const scope = createScope();
  const report = (message: string) => {
    ticketGroupSettingsDemo.event.value = message;
  };
  scope.add(
    wireTicketGroupSettings(root, {
      newGroup: () => {
        report('New group: opens the New Group dialog.');
      },
      viewTickets: (id) => {
        report(`View tickets: shows the tickets in “${name(id)}”.`);
      },
      rename: (id) => {
        report(`Rename: opens Rename Group for “${name(id)}”.`);
      },
      merge: (id) => {
        ticketGroupSettingsDemo.merge.value = { ...mergeState(), group: { ...mergeState().group, id } };
      },
      remove: (id) => {
        report(`Delete group: asks to confirm deleting “${name(id)}”.`);
      },
      chooseMergeDestination: (id) => {
        const dialog = ticketGroupSettingsDemo.merge.value;
        if (dialog) ticketGroupSettingsDemo.merge.value = { ...dialog, into: id };
      },
      submitMerge: () => {
        const dialog = ticketGroupSettingsDemo.merge.value;
        ticketGroupSettingsDemo.merge.value = undefined;
        if (dialog) report(`Merge: moves “${dialog.group.name}” into “${name(dialog.into)}” and removes it.`);
      },
      cancelMerge: () => {
        ticketGroupSettingsDemo.merge.value = undefined;
        report('Merge cancelled.');
      },
    }),
  );
  scope.add(
    delegate(root, 'click', openMerge.selector, () => {
      ticketGroupSettingsDemo.merge.value = mergeState();
    }),
  );
  scope.add(
    delegate(root, 'change', '[data-settings="ticket-group-settings"] [name="state"]', (_event, target) => {
      const value = (target as FormControl).value;
      if (Object.hasOwn(STATES, value)) ticketGroupSettingsDemo.state.value = value as DemoState;
    }),
  );
  return () => {
    scope.dispose();
  };
}

/** Project Settings › Ticket groups, its Merge into… dialog, and its running-operation banner (HS2-GXKX1Q). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-group-settings': {
      catalogExamples: true,
      Demo: TicketGroupSettingsDemo,
      Settings: TicketGroupSettingsSettings,
      reset: resetTicketGroupSettingsDemo,
    },
    'ticket-group-merge-dialog': {
      catalogExamples: true,
      Demo: TicketGroupMergeDialogDemo,
      reset: () => {
        ticketGroupSettingsDemo.merge.value = undefined;
      },
    },
    'ticket-group-operation-banner': { catalogExamples: true, Demo: TicketGroupOperationBannerDemo },
  },
  wire: wireTicketGroupSettingsDemo,
};
