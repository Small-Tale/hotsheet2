import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { LiveStoreState, LiveWord } from '../api';
import { StandbyScreen } from '../components/standby-screen';
import { StandbySettings } from '../components/standby-settings';
import { formatStandbyClock, standbyFreshnessLabel, standbyModel, type StandbyScope } from '../standby';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** A fixed demo instant (2:47 at night) so the clock and ages are deterministic. */
const NOW = new Date(2026, 10, 6, 2, 47, 12).getTime();
const MINUTE = 60_000;

type Scenario = 'all-clear' | 'attention' | 'project' | 'connection-lost' | 'empty';
type Orientation = 'landscape' | 'portrait';

const SCENARIO_CHOICES: SelectChoice[] = [
  { value: 'all-clear', label: 'All Clear (m4)' },
  { value: 'attention', label: 'A Project Stalled (m5)' },
  { value: 'project', label: 'One Project (m6)' },
  { value: 'connection-lost', label: 'Connection Lost' },
  { value: 'empty', label: 'No Live State Yet' },
];

/** Synthetic live state in the server's exact shape; no customer content. */
function store(
  id: string,
  name: string,
  word: LiveWord,
  extra: { slug?: string; waiting?: number; workers?: number; progressMinutes?: number } = {},
): LiveStoreState {
  const minute = Math.floor(NOW / MINUTE) * MINUTE;
  return {
    store: id,
    root: `/work/${id}.hs2`,
    name,
    checkouts: [`${id}-checkout`],
    epoch_ms: 1,
    revision: 1,
    state: word,
    activity: {
      word,
      active_since_ms: NOW - 40 * MINUTE,
      stopped_at_ms: word === 'stopped' ? NOW - 90 * MINUTE : null,
      last_progress_ms: NOW - (extra.progressMinutes ?? 2) * MINUTE,
      idle_deadline_ms: null,
      stalled: word === 'stalled' ? [{ slug: extra.slug ?? 'STW-4K2P', worker: 'worker-2' }] : [],
      halted: [],
      causes: [],
    },
    claims:
      word === 'active' && extra.slug
        ? [
            {
              ticket: `${id}-ticket`,
              slug: extra.slug,
              title: 'Synthetic work',
              worker: 'worker-1',
              phase: 'working',
              claimed_at_ms: NOW - 20 * MINUTE,
              lease_expires_at_ms: NOW + 10 * MINUTE,
              eta_at_ms: null,
            },
          ]
        : [],
    workers: { live: extra.workers ?? (word === 'active' ? 1 : 0), expected: 1 },
    waiting: {
      count: extra.waiting ?? 0,
      tickets: Array.from({ length: extra.waiting ?? 0 }, (_, index) => ({
        ticket: `${id}-wait-${index}`,
        slug: `${id.toUpperCase()}-${index + 1}`,
        title: 'Synthetic question',
        reason: 'feedback' as const,
      })),
    },
    runway: { ready: 0, estimate_ms: null },
    rates: {
      tokens: {
        window_minutes: 5,
        minutes:
          word === 'active'
            ? [
                { start_ms: minute - 4 * MINUTE, tokens: 2_400 },
                { start_ms: minute - 2 * MINUTE, tokens: 2_000 },
                { start_ms: minute, tokens: 1_600 },
              ]
            : [],
        peak_per_minute: 4_900,
      },
      tickets_updated: {
        window_minutes: 60,
        at_ms: word === 'active' ? Array.from({ length: 7 }, (_, index) => NOW - (index * 8 + 1) * MINUTE) : [],
        peak_per_hour: 20,
      },
    },
  };
}

function fixtures(scenario: Scenario): { states: LiveStoreState[]; scope: StandbyScope } {
  const all: StandbyScope = { kind: 'all' };
  const hotSheet = store('hs2', 'Hot Sheet 2', 'active', { slug: 'HS2-R76MMW', workers: 2 });
  const site = (word: LiveWord) => store('stw', 'Small Tale Website', word, { slug: 'STW-4K2P', progressMinutes: 12 });
  // All clear means no problem anywhere: Waiting on you is a problem and brightens (§34.5).
  const review = store('ux', 'UX Review', 'active', { slug: 'UX-12' });
  const kerf = store('kerf', 'Kerf', 'stopped');
  switch (scenario) {
    case 'all-clear':
    case 'connection-lost':
      return { states: [hotSheet, site('active'), review, kerf], scope: all };
    case 'attention':
      return { states: [hotSheet, site('stalled'), review, kerf], scope: all };
    case 'project':
      return {
        states: [store('hs2', 'Hot Sheet 2', 'active', { slug: 'HS2-R76MMW', workers: 2, waiting: 2 })],
        scope: { kind: 'project', projectId: 'hs2-checkout', name: 'Hot Sheet 2' },
      };
    case 'empty':
      return { states: [], scope: all };
    default:
      return scenario satisfies never;
  }
}

const DEFAULTS = { scenario: 'all-clear' as Scenario, orientation: 'landscape' as Orientation };

export const standbyDemoSettings = {
  scenario: signal<Scenario>(DEFAULTS.scenario),
  orientation: signal<Orientation>(DEFAULTS.orientation),
};

export function resetStandbyDemo(root?: ParentNode): void {
  standbyDemoSettings.scenario.value = DEFAULTS.scenario;
  standbyDemoSettings.orientation.value = DEFAULTS.orientation;
  if (root) syncSettingsControls(root, 'standby-screen', { values: { ...DEFAULTS } });
}

function standbyFrame(scenario: Scenario, orientation: Orientation) {
  const { states, scope } = fixtures(scenario);
  const live = scenario !== 'connection-lost';
  return (
    <div class="demo-standby-frame" data-orientation={orientation}>
      <StandbyScreen
        presentation="inline"
        clock={formatStandbyClock(NOW, 'en-US')}
        model={standbyModel(states, scope, NOW)}
        freshness={{
          live,
          label: standbyFreshnessLabel({ live, updatedAtMs: NOW - (live ? 0 : 6 * MINUTE) }, 'en-US'),
        }}
        touch={orientation === 'portrait'}
      />
    </div>
  );
}

/** The interactive night display plus the approved mock states. */
export function StandbyScreenDemo() {
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Interactive"
        note="In the app it covers the viewport in the top layer and any click, tap, or key exits. Pick a scenario and orientation."
      >
        {standbyFrame(standbyDemoSettings.scenario.value, standbyDemoSettings.orientation.value)}
      </CatalogExample>
      <CatalogExample label="A Project Stalled" note="Only the problem brightens: an amber callout and its row (m5).">
        {standbyFrame('attention', 'landscape')}
      </CatalogExample>
      <CatalogExample label="Phone, One Project" note="Entered from a project tab, so it shows that project (m6).">
        {standbyFrame('project', 'portrait')}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export function StandbyScreenSettings() {
  return (
    <DemoSettingsForm settings="standby-screen">
      <Select name="scenario" label="Scenario" value={standbyDemoSettings.scenario.value} choices={SCENARIO_CHOICES} />
      <Select
        name="orientation"
        label="Orientation"
        value={standbyDemoSettings.orientation.value}
        choices={[
          { value: 'landscape', label: 'Landscape (Desktop)' },
          { value: 'portrait', label: 'Portrait (Phone)' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

const SETTINGS = {
  defaults: { enabled: true, idle_minutes: 15 },
  configured: { enabled: null, idle_minutes: null },
  effective: { enabled: true, idle_minutes: 15 },
};

/** App Settings → General → Standby in its states. */
export function StandbySettingsDemo() {
  return (
    <>
      <CatalogExample label="Defaults">
        <StandbySettings settings={SETTINGS} />
      </CatalogExample>
      <CatalogExample label="Off">
        <StandbySettings
          settings={{
            ...SETTINGS,
            configured: { enabled: false, idle_minutes: null },
            effective: { enabled: false, idle_minutes: 15 },
          }}
        />
      </CatalogExample>
      <CatalogExample label="Set From the CLI" note="A threshold outside the presets is still listed and selected.">
        <StandbySettings
          settings={{
            ...SETTINGS,
            configured: { enabled: null, idle_minutes: 45 },
            effective: { enabled: true, idle_minutes: 45 },
          }}
        />
      </CatalogExample>
      <CatalogExample label="Loading">
        <StandbySettings />
      </CatalogExample>
      <CatalogExample label="Save Failed">
        <StandbySettings settings={SETTINGS} message="standby.idle_minutes must be a whole number from 1 to 1440" />
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireStandbyDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="standby-screen"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'scenario') standbyDemoSettings.scenario.value = control.value as Scenario;
      if (name === 'orientation') standbyDemoSettings.orientation.value = control.value as Orientation;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'standby-screen': {
      catalogExamples: true,
      Demo: StandbyScreenDemo,
      Settings: StandbyScreenSettings,
      reset: resetStandbyDemo,
    },
    'standby-settings': { catalogExamples: true, Demo: StandbySettingsDemo },
  },
  wire: wireStandbyDemo,
};
