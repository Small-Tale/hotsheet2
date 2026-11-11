import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { action } from 'kerfjs/actions';
import { createScope } from 'kerfjs/scope';

import type { AwayDigest } from '../away-digest';
import { AwayDigestDialog } from '../components/away-digest-dialog';
import { syncControlledOpen } from '../controlled-open';
import { SHELL_AND_GLOBAL_ACTIONS } from '../interaction-attrs/shell-and-global';
import { DemoEvent } from './components/demo-event';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** A fixed night (11:42 PM to 7:08 AM) so every time is deterministic. */
const ENTERED = new Date(2026, 10, 5, 23, 42).getTime();
const at = (hour: number, minute: number) => new Date(2026, 10, hour < 12 ? 6 : 5, hour, minute).getTime();
const EXITED = at(7, 8);

type Scenario = 'overnight' | 'nothing-needs-you' | 'completions-only';

const SCENARIO_CHOICES: SelectChoice[] = [
  { value: 'overnight', label: 'Overnight (m7)' },
  { value: 'nothing-needs-you', label: 'Nothing Needs You' },
  { value: 'completions-only', label: 'Completions Only' },
];

/** Synthetic digests in the controller's exact shape; no customer content. */
function digest(scenario: Scenario): AwayDigest {
  const base = { enteredAtMs: ENTERED, exitedAtMs: EXITED };
  switch (scenario) {
    case 'overnight':
      return {
        ...base,
        completed: 14,
        needYou: 3,
        tokens: 412_000,
        // Oldest first, as the controller orders them.
        events: [
          {
            kind: 'waiting',
            store: 'ux',
            name: 'UX Review',
            projectId: 'ux-checkout',
            slug: 'UX-12',
            title: 'Pick a header layout',
            fromMs: at(0, 10),
          },
          { kind: 'queue_finished', store: 'kerf', name: 'Kerf', atMs: at(1, 2), tickets: 5 },
          {
            kind: 'usage_limit',
            store: 'hs2',
            name: 'Hot Sheet 2',
            worker: 'worker-1',
            sinceMs: at(2, 30),
            resetsAtMs: at(3, 10),
            resumedAtMs: at(3, 10),
          },
          {
            kind: 'stalled',
            store: 'stw',
            name: 'Small Tale Website',
            projectId: 'stw-checkout',
            slug: 'STW-4K2P',
            atMs: at(2, 35),
          },
          { kind: 'host', signal: 'disk', level: 'warning', atMs: at(4, 10), detail: '12 GB free' },
        ],
      };
    case 'nothing-needs-you':
      return {
        ...base,
        completed: 6,
        needYou: 0,
        tokens: 98_000,
        events: [{ kind: 'queue_finished', store: 'kerf', name: 'Kerf', atMs: at(1, 2), tickets: 5 }],
      };
    case 'completions-only':
      return { ...base, completed: 2, needYou: 0, tokens: 0, events: [] };
    default:
      return scenario satisfies never;
  }
}

const open = action('demo-open-away-digest');

export const awayDigestDemoSettings = {
  scenario: signal<Scenario>('overnight'),
  shown: signal(false),
  event: signal(''),
};

export function resetAwayDigestDemo(root?: ParentNode): void {
  awayDigestDemoSettings.scenario.value = 'overnight';
  awayDigestDemoSettings.shown.value = false;
  awayDigestDemoSettings.event.value = '';
  if (root) syncSettingsControls(root, 'away-digest-dialog', { values: { scenario: 'overnight' } });
}

/** The production dialog over each scenario, opened from a visible trigger like leaving standby. */
export function AwayDigestDemo() {
  return (
    <>
      <CatalogExample
        label="Interactive"
        note="In the app it opens when you leave standby, only if something happened."
      >
        <Button action={open.value}>Leave Standby…</Button>
        <AwayDigestDialog
          digest={awayDigestDemoSettings.shown.value ? digest(awayDigestDemoSettings.scenario.value) : undefined}
        />
      </CatalogExample>
      <DemoEvent>{awayDigestDemoSettings.event.value}</DemoEvent>
    </>
  );
}

export function AwayDigestSettings() {
  return (
    <DemoSettingsForm settings="away-digest-dialog">
      <Select
        name="scenario"
        label="Scenario"
        value={awayDigestDemoSettings.scenario.value}
        choices={SCENARIO_CHOICES}
      />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireAwayDigestDemo(root: HTMLElement): () => void {
  const scope = createScope();
  const close = (label: string) => {
    awayDigestDemoSettings.shown.value = false;
    awayDigestDemoSettings.event.value = label;
    queueMicrotask(() => {
      syncControlledOpen(root);
    });
  };
  scope.add(
    delegate(root, 'click', open.selector, () => {
      awayDigestDemoSettings.shown.value = true;
      queueMicrotask(() => {
        syncControlledOpen(root);
      });
    }),
  );
  scope.add(
    delegate(root, 'click', SHELL_AND_GLOBAL_ACTIONS.dismissAwayDigest.selector, () => {
      close('Dismissed');
    }),
  );
  scope.add(
    delegate(root, 'kui-request-close', '[data-away-digest]', () => {
      close('Closed');
    }),
  );
  scope.add(
    delegate(root, 'click', SHELL_AND_GLOBAL_ACTIONS.showWhatNeedsMe.selector, () => {
      close('Show What Needs Me: opens Stats for all projects');
    }),
  );
  scope.add(
    delegate(root, 'click', SHELL_AND_GLOBAL_ACTIONS.openAwayEvent.selector, (_event, target) => {
      const element = target as HTMLElement;
      close(`Opened ${element.dataset.ticketSlug ?? ''} in ${element.dataset.projectId ?? ''}`);
    }),
  );
  scope.add(
    delegate(root, 'change', '[data-settings="away-digest-dialog"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'scenario')
        awayDigestDemoSettings.scenario.value = control.value as Scenario;
    }),
  );
  return () => {
    scope.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'away-digest-dialog': {
      catalogExamples: true,
      Demo: AwayDigestDemo,
      Settings: AwayDigestSettings,
      reset: resetAwayDigestDemo,
    },
  },
  wire: wireAwayDigestDemo,
};
