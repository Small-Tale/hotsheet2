import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { RepositorySizeEntry, RepositorySizeReport } from '../api';
import { RepositorySize, type RepositorySizeState } from '../components/repository-size';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

const THRESHOLDS = { warning_bytes: 5_000_000_000, critical_bytes: 10_000_000_000 };

function git(total: number, objects: number) {
  return {
    loose_objects: 12,
    loose_bytes: 48_000,
    packed_objects: objects,
    packs: 2,
    packed_bytes: total - 48_000,
    total_bytes: total,
  };
}

const CODE: RepositorySizeEntry = {
  kind: 'code',
  label: 'hotsheet2',
  locator: '~/Documents/hotsheet2',
  status: 'measured',
  git: git(63_500_000, 48_384),
  shared_with_code: false,
  level: 'ok',
};
const STORE: RepositorySizeEntry = {
  kind: 'ticket_store',
  connection_id: '6f1c2a9e4b7d3c01',
  provider: 'git',
  label: 'hotsheet2.hs2',
  locator: '~/Documents/hotsheet2.hs2',
  status: 'measured',
  git: git(1_240_000_000, 91_207),
  attachments: { bytes: 1_080_000_000, files: 3_412, truncated: false },
  shared_with_code: false,
  level: 'ok',
};
const GITHUB: RepositorySizeEntry = {
  kind: 'ticket_store',
  connection_id: 'github-main',
  provider: 'github',
  label: 'github-main',
  locator: 'Small-Tale/hotsheet2',
  status: 'unsupported',
  shared_with_code: false,
  level: 'ok',
  detail: 'This github source is hosted by its provider, so there is no local repository to measure.',
};
const UNMEASURED: RepositorySizeEntry = {
  kind: 'ticket_store',
  connection_id: '0a9b8c7d6e5f4a3b',
  provider: 'git',
  label: 'archive-tickets',
  locator: '~/Documents/archive-tickets',
  status: 'not_repository',
  attachments: { bytes: 2_400_000, files: 18, truncated: false },
  shared_with_code: false,
  level: 'ok',
  detail: 'The directory is not inside a Git repository.',
};

function report(entries: RepositorySizeEntry[]): RepositorySizeReport {
  const order = ['ok', 'warning', 'critical'] as const;
  const level = entries.reduce<RepositorySizeReport['level']>(
    (highest, entry) => (order.indexOf(entry.level) > order.indexOf(highest) ? entry.level : highest),
    'ok',
  );
  return { checkout_id: 'hotsheet2-1a2b3c', thresholds: THRESHOLDS, level, entries };
}

const STATE_CHOICES: SelectChoice[] = [
  { value: 'ok', label: 'Within Limits' },
  { value: 'warning', label: 'Over Soft Limit' },
  { value: 'critical', label: 'Over Hard Limit' },
  { value: 'unsupported', label: 'Unsupported and Unmeasured Sources' },
  { value: 'loading', label: 'Loading' },
  { value: 'error', label: 'Error' },
];

export const repositorySizeSettings = { state: signal<string>('ok') };

/** Deterministic fixtures for every state the stats page shows (HS2-QF44T7). */
export function repositorySizeDemoState(name: string): RepositorySizeState {
  if (name === 'loading') return { status: 'loading' };
  if (name === 'error') return { status: 'error', message: 'The Hot Sheet server could not be reached.' };
  if (name === 'warning')
    return {
      status: 'ready',
      report: report([
        CODE,
        {
          ...STORE,
          git: git(6_240_000_000, 91_207),
          attachments: { bytes: 5_910_000_000, files: 3_412, truncated: false },
          level: 'warning',
        },
      ]),
    };
  if (name === 'critical')
    return {
      status: 'ready',
      report: report([
        CODE,
        {
          ...STORE,
          git: git(10_700_000_000, 91_207),
          attachments: { bytes: 10_100_000_000, files: 3_412, truncated: false },
          level: 'critical',
        },
      ]),
    };
  if (name === 'unsupported')
    return {
      status: 'ready',
      report: report([
        CODE,
        // An embedded store reports its enclosing repository: the code repository's own objects.
        {
          ...STORE,
          label: '.hotsheet',
          locator: '~/Documents/hotsheet2/.hotsheet',
          git: CODE.git,
          attachments: { bytes: 41_300_000, files: 212, truncated: false },
          shared_with_code: true,
        },
        UNMEASURED,
        GITHUB,
      ]),
    };
  return { status: 'ready', report: report([CODE, STORE]) };
}

export function resetRepositorySizeDemo(root?: ParentNode): void {
  repositorySizeSettings.state.value = 'ok';
  if (root)
    syncSettingsControls(root, 'repository-size', {
      values: { state: repositorySizeSettings.state.value },
    });
}

export function RepositorySizeDemo() {
  return (
    <CatalogExample
      label="Interactive"
      note="Shown in a project's statistics view beside confidence calibration. The server computes each level against GitHub's 5 GB soft and 10 GB hard limits; the component only presents it."
      viewport={{ width: 'full' }}
    >
      {RepositorySize({ state: repositorySizeDemoState(repositorySizeSettings.state.value) })}
    </CatalogExample>
  );
}

export function RepositorySizeSettings() {
  return (
    <form class="settings-form" data-settings="repository-size">
      <Select name="state" label="State" value={repositorySizeSettings.state.value} choices={STATE_CHOICES} />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireRepositorySizeDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="repository-size"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'state') repositorySizeSettings.state.value = control.value;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'repository-size': {
      catalogExamples: true,
      Demo: RepositorySizeDemo,
      Settings: RepositorySizeSettings,
      reset: resetRepositorySizeDemo,
    },
  },
  wire: wireRepositorySizeDemo,
};
