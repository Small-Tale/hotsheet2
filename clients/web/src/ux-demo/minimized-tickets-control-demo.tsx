import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { Bug, ListChecks, Plus, Search, Sparkles } from 'lucide';

import {
  MinimizedTicketsControl,
  type MinimizedTicketSummary,
  wireMinimizedTicketsControl,
} from '../components/minimized-tickets-control';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** Synthetic minimized tickets, including a long title and each draft indicator. */
const FIXTURES: readonly MinimizedTicketSummary[] = [
  {
    id: 'demo-1',
    kind: 'ticket-reader',
    label: 'HS2-DEMO01 Keep reader drafts when switching projects',
    shortLabel: 'HS2-DEMO01',
    icon: { icon: Sparkles, name: 'sparkles' },
    draft: 'Unsent note',
  },
  {
    id: 'demo-2',
    kind: 'ticket-reader',
    label: 'HS2-START02 Retry a failed attachment upload',
    shortLabel: 'HS2-START02',
    icon: { icon: Bug, name: 'bug' },
    draft: 'Reply in progress',
  },
  {
    id: 'demo-3',
    kind: 'ticket-reader',
    label: 'HS2-NEXT01 Show CI run duration in the ticket timeline',
    shortLabel: 'HS2-NEXT01',
    icon: { icon: ListChecks, name: 'list-checks' },
  },
  {
    id: 'demo-4',
    kind: 'ticket-reader',
    label: 'HS2-EDIT04 Summarize a long investigation',
    shortLabel: 'HS2-EDIT04',
    icon: { icon: Search, name: 'search' },
    draft: 'Unsaved edit',
  },
];

/** A minimized new-ticket composer (HS2-3JR0ZH), listed after the minimized readers. */
const COMPOSER_FIXTURE: MinimizedTicketSummary = {
  id: 'new-ticket:demo',
  kind: 'new-ticket',
  label: 'New ticket: Draft the release checklist',
  shortLabel: 'new ticket',
  icon: { icon: Plus, name: 'plus' },
  draft: 'Unsaved ticket',
};

type DemoCount = '0' | '1' | '3' | '4';

/** Every public state: none, one, or several minimized tickets, with or without drafts. */
export const minimizedTicketsControlSettings = {
  count: signal<DemoCount>('3'),
  drafts: signal<'with' | 'without'>('with'),
  /** Whether a minimized new-ticket composer joins the list (HS2-3JR0ZH). */
  composer: signal<'none' | 'minimized'>('none'),
};

function demoTickets(): MinimizedTicketSummary[] {
  const count = Number(minimizedTicketsControlSettings.count.value),
    drafts = minimizedTicketsControlSettings.drafts.value === 'with';
  const listed = [
    ...FIXTURES.slice(0, count),
    ...(minimizedTicketsControlSettings.composer.value === 'minimized' ? [COMPOSER_FIXTURE] : []),
  ];
  return listed.map((ticket) => (drafts ? ticket : { ...ticket, draft: undefined }));
}

export function resetMinimizedTicketsControlDemo(root?: ParentNode): void {
  minimizedTicketsControlSettings.count.value = '3';
  minimizedTicketsControlSettings.drafts.value = 'with';
  minimizedTicketsControlSettings.composer.value = 'none';
  if (root)
    syncSettingsControls(root, 'minimized-tickets-control', {
      values: {
        count: minimizedTicketsControlSettings.count.value,
        drafts: minimizedTicketsControlSettings.drafts.value,
        composer: minimizedTicketsControlSettings.composer.value,
      },
    });
}

export function MinimizedTicketsControlDemo() {
  const tickets = demoTickets();
  return (
    <CatalogExample
      label="In the Floating Control Cluster"
      note="One minimized ticket restores on click. Several show a count and open a list with each ticket's draft indicator, Restore All, and Close All…, which asks first only when a draft would be lost. A minimized New Ticket composer joins the same list. Restoring removes the ticket from this fixture."
      viewport={{ width: 'full', minHeight: 'short', surface: 'lowered' }}
    >
      <FloatingToolbar label="Minimized Tickets" position="bottom-end">
        <MinimizedTicketsControl tickets={tickets} />
      </FloatingToolbar>
    </CatalogExample>
  );
}

export function MinimizedTicketsControlSettings() {
  return (
    <DemoSettingsForm settings="minimized-tickets-control">
      <Select
        name="count"
        label="Minimized Tickets"
        value={minimizedTicketsControlSettings.count.value}
        choices={[
          { value: '0', label: 'None' },
          { value: '1', label: 'One' },
          { value: '3', label: 'Three' },
          { value: '4', label: 'Four' },
        ]}
      />
      <Select
        name="drafts"
        label="Drafts"
        value={minimizedTicketsControlSettings.drafts.value}
        choices={[
          { value: 'with', label: 'With Drafts' },
          { value: 'without', label: 'Without Drafts' },
        ]}
      />
      <Select
        name="composer"
        label="New Ticket Composer"
        value={minimizedTicketsControlSettings.composer.value}
        choices={[
          { value: 'none', label: 'Not Minimized' },
          { value: 'minimized', label: 'Minimized' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Restoring or closing in the demo removes fixtures, as the real control's owner does. */
function wireMinimizedTicketsControlDemo(root: HTMLElement): () => void {
  const listeners = createScope();
  const setCount = (count: number) => {
    minimizedTicketsControlSettings.count.value = String(Math.max(0, count)) as DemoCount;
    syncSettingsControls(root, 'minimized-tickets-control', {
      values: { count: minimizedTicketsControlSettings.count.value },
    });
  };
  const setComposer = (composer: 'none' | 'minimized') => {
    minimizedTicketsControlSettings.composer.value = composer;
    syncSettingsControls(root, 'minimized-tickets-control', { values: { composer } });
  };
  listeners.add(
    wireMinimizedTicketsControl(root, {
      restore: (id) => {
        if (id === COMPOSER_FIXTURE.id) setComposer('none');
        else setCount(Number(minimizedTicketsControlSettings.count.value) - 1);
      },
      restoreAll: () => {
        setCount(0);
        setComposer('none');
      },
      closeAll: () => {
        setCount(0);
        setComposer('none');
      },
    }),
  );
  listeners.add(
    delegate(root, 'change', '[data-settings="minimized-tickets-control"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'count')
        minimizedTicketsControlSettings.count.value = control.value as DemoCount;
      if (control.getAttribute('name') === 'drafts')
        minimizedTicketsControlSettings.drafts.value = control.value === 'without' ? 'without' : 'with';
      if (control.getAttribute('name') === 'composer')
        minimizedTicketsControlSettings.composer.value = control.value === 'minimized' ? 'minimized' : 'none';
    }),
  );
  return () => {
    listeners.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'minimized-tickets-control': {
      catalogExamples: true,
      Demo: MinimizedTicketsControlDemo,
      Settings: MinimizedTicketsControlSettings,
      reset: resetMinimizedTicketsControlDemo,
    },
  },
  wire: wireMinimizedTicketsControlDemo,
};
