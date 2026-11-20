import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

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
    slug: 'HS2-DEMO01',
    title: 'Keep reader drafts when switching projects',
    category: 'feature',
    draft: 'Unsent note',
  },
  {
    id: 'demo-2',
    slug: 'HS2-START02',
    title: 'Retry a failed attachment upload',
    category: 'bug',
    draft: 'Reply in progress',
  },
  { id: 'demo-3', slug: 'HS2-NEXT01', title: 'Show CI run duration in the ticket timeline', category: 'task' },
  {
    id: 'demo-4',
    slug: 'HS2-EDIT04',
    title: 'Summarize a long investigation',
    category: 'investigation',
    draft: 'Unsaved edit',
  },
];

type DemoCount = '0' | '1' | '3' | '4';

/** Every public state: none, one, or several minimized tickets, with or without drafts. */
export const minimizedTicketsControlSettings = {
  count: signal<DemoCount>('3'),
  drafts: signal<'with' | 'without'>('with'),
};

function demoTickets(): MinimizedTicketSummary[] {
  const count = Number(minimizedTicketsControlSettings.count.value),
    drafts = minimizedTicketsControlSettings.drafts.value === 'with';
  return FIXTURES.slice(0, count).map((ticket) => (drafts ? ticket : { ...ticket, draft: undefined }));
}

export function resetMinimizedTicketsControlDemo(root?: ParentNode): void {
  minimizedTicketsControlSettings.count.value = '3';
  minimizedTicketsControlSettings.drafts.value = 'with';
  if (root)
    syncSettingsControls(root, 'minimized-tickets-control', {
      values: {
        count: minimizedTicketsControlSettings.count.value,
        drafts: minimizedTicketsControlSettings.drafts.value,
      },
    });
}

export function MinimizedTicketsControlDemo() {
  const tickets = demoTickets();
  return (
    <CatalogExample
      label="In the Floating Control Cluster"
      note="One minimized ticket restores on click. Several show a count and open a list with each ticket's draft indicator, Restore All, and Close All…, which asks first only when a draft would be lost. Restoring removes the ticket from this fixture."
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
  listeners.add(
    wireMinimizedTicketsControl(root, {
      restore: () => {
        setCount(Number(minimizedTicketsControlSettings.count.value) - 1);
      },
      restoreAll: () => {
        setCount(0);
      },
      closeAll: () => {
        setCount(0);
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
