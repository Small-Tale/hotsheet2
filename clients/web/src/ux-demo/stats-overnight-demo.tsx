import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { LiveHistorySegment, LiveHistoryStore } from '../api';
import { StatsOvernight, type StatsOvernightProps } from '../components/stats-overnight';
import { hourTicks, timelineLane } from '../overnight-timeline';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** A fixed night, 9 PM to 3 AM, so every time is deterministic. */
const FROM = new Date(2026, 10, 5, 21, 0).getTime();
const TO = FROM + 6 * 3_600_000;
const at = (hours: number) => FROM + hours * 3_600_000;

type Scenario = 'overnight' | 'empty' | 'loading' | 'error';

const seg = (state: LiveHistorySegment['state'], from: number, to: number): LiveHistorySegment => ({
  state,
  start_ms: at(from),
  end_ms: at(to),
});
const ticks = (...hours: number[]) =>
  hours.map((hour, index) => ({ slug: `HS2-${String(index + 1)}`, at_ms: at(hour) }));

/** The mock's four projects in the server's exact shape; synthetic content only. */
const STORES: readonly LiveHistoryStore[] = [
  {
    store: 'hs2',
    name: 'Hot Sheet 2',
    checkouts: ['hs2-checkout'],
    segments: [seg('active', -1, 2.4), seg('waiting_on_ci', 2.4, 2.8), seg('active', 2.8, 6)],
    completions: ticks(0.5, 1, 1.8, 2.3, 3.3, 3.9, 4.5, 5.4, 5.9),
  },
  {
    store: 'stw',
    name: 'Small Tale Website',
    checkouts: ['stw-checkout'],
    segments: [seg('active', -2, 3.9), seg('may_be_stuck', 3.9, 6)],
    completions: ticks(0.8, 2, 3.2),
  },
  {
    store: 'ux',
    name: 'UX Review',
    checkouts: ['ux-checkout'],
    segments: [seg('active', -2, 2.5), seg('waiting_on_you', 2.5, 6)],
    completions: ticks(1.2, 2.1),
  },
  {
    store: 'kerf',
    name: 'Kerf',
    checkouts: ['kerf-checkout'],
    segments: [seg('active', 0, 3.5), seg('stopped', 3.5, 6)],
    completions: ticks(0.7, 1.4, 2, 2.9, 3.4),
  },
];

function props(scenario: Scenario): StatsOvernightProps {
  return {
    window: 'standby',
    standbyAvailable: true,
    fromMs: FROM,
    toMs: TO,
    lanes: scenario === 'overnight' ? STORES.map((store) => timelineLane(store, store.segments, FROM, TO)) : [],
    axis: hourTicks(FROM, TO),
    status: scenario === 'loading' ? 'loading' : scenario === 'error' ? 'error' : 'ready',
    ...(scenario === 'error' ? { message: 'The server did not answer.' } : {}),
    locale: 'en-US',
  };
}

const DEFAULTS = { scenario: 'overnight' as Scenario };

export const statsOvernightSettings = { scenario: signal<Scenario>(DEFAULTS.scenario) };

export function resetStatsOvernightDemo(root?: ParentNode): void {
  statsOvernightSettings.scenario.value = DEFAULTS.scenario;
  if (root) syncSettingsControls(root, 'stats-overnight', { values: { ...DEFAULTS } });
}

/** The Overnight swimlanes over a fixed night (HS2-9AN4DN, mock `m3-stats-right-now`). */
export function StatsOvernightDemo() {
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Overnight"
        note="One lane per project, coloured by state, with a tick per completed ticket. Hover a segment for its state, times, and duration."
        viewport={{ width: 'full' }}
      >
        {StatsOvernight(props(statsOvernightSettings.scenario.value))}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export function StatsOvernightSettings() {
  return (
    <DemoSettingsForm settings="stats-overnight">
      <Select
        name="scenario"
        label="Scenario"
        value={statsOvernightSettings.scenario.value}
        choices={[
          { value: 'overnight', label: 'Overnight (m3)' },
          { value: 'empty', label: 'No History Yet' },
          { value: 'loading', label: 'Loading' },
          { value: 'error', label: 'Read Failed' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

function wireStatsOvernightDemo(root: HTMLElement): () => void {
  const listeners = createScope();
  listeners.add(
    delegate(root, 'change', '[data-settings="stats-overnight"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'scenario')
        statsOvernightSettings.scenario.value = control.value as Scenario;
    }),
  );
  return () => {
    listeners.dispose();
  };
}

export const demoModule: DemoModule = {
  entries: {
    'stats-overnight': {
      catalogExamples: true,
      stackWidth: 'full',
      Demo: StatsOvernightDemo,
      Settings: StatsOvernightSettings,
      reset: resetStatsOvernightDemo,
    },
  },
  wire: wireStatsOvernightDemo,
};
