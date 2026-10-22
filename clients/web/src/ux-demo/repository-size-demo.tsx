import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CompactionPlan, RepositorySizeEntry, RepositorySizeReport } from '../api';
import { type CompactionPreviewState, RepositorySize, type RepositorySizeState } from '../components/repository-size';
import { RepositorySizeBanner } from '../components/repository-size-banner';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
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

const SAMPLE_PLAN: CompactionPlan = {
  head: 'a'.repeat(40),
  branch: 'main',
  min_age_days: 90,
  oversized_bytes: 10_000_000,
  candidates: [
    {
      ticket_id: 'ticket-one',
      ticket_slug: 'HS2-DONE',
      attachment_id: 'attachment-one',
      filename: 'old-capture.png',
      path: 'attachments/ticket-one/attachment-one/old-capture.png',
      bytes: 18_400_000,
      sha256: 'b'.repeat(64),
      reason: 'oversized',
    },
    {
      ticket_id: 'ticket-two',
      ticket_slug: 'HS2-OLDER',
      attachment_id: 'attachment-two',
      filename: 'duplicate-capture.png',
      path: 'attachments/ticket-two/attachment-two/duplicate-capture.png',
      bytes: 4_200_000,
      sha256: 'c'.repeat(64),
      reason: 'duplicate',
    },
  ],
  reclaimable_payload_bytes: 22_600_000,
  fingerprint: 'd'.repeat(64),
};

function demoCompaction(name: string): CompactionPreviewState {
  const source = STORE.connection_id ?? 'git-store';
  if (name === 'loading') return { status: 'loading', source };
  if (name === 'error')
    return {
      status: 'error',
      source,
      message: 'The ticket store has local changes. Commit or remove them before planning.',
    };
  if (name === 'ready') return { status: 'ready', source, plan: SAMPLE_PLAN };
  return { status: 'idle' };
}

type CompactionDemoState = 'idle' | 'loading' | 'ready' | 'error';
export const repositorySizeSettings = { state: signal<string>('ok'), compaction: signal<CompactionDemoState>('idle') };

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
  repositorySizeSettings.compaction.value = 'idle';
  if (root)
    syncSettingsControls(root, 'repository-size', {
      values: { state: repositorySizeSettings.state.value, compaction: repositorySizeSettings.compaction.value },
    });
}

export function RepositorySizeDemo() {
  return (
    <CatalogExample
      label="Interactive"
      note="Shown in a project's statistics view beside confidence calibration. The server computes each level against GitHub's 5 GB soft and 10 GB hard limits; the component only presents it."
      viewport={{ width: 'full' }}
    >
      {RepositorySize({
        state: repositorySizeDemoState(repositorySizeSettings.state.value),
        compaction: demoCompaction(repositorySizeSettings.compaction.value),
      })}
    </CatalogExample>
  );
}

export function RepositorySizeSettings() {
  return (
    <DemoSettingsForm settings="repository-size">
      <Select name="state" label="State" value={repositorySizeSettings.state.value} choices={STATE_CHOICES} />
      <Select
        name="compaction"
        label="Compaction Preview"
        value={repositorySizeSettings.compaction.value}
        choices={[
          { value: 'idle', label: 'Closed' },
          { value: 'loading', label: 'Loading' },
          { value: 'ready', label: 'Candidates Ready' },
          { value: 'error', label: 'Unavailable' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

function readyReport(name: string): RepositorySizeReport | undefined {
  const state = repositorySizeDemoState(name);
  return state.status === 'ready' ? state.report : undefined;
}

/** The project shell strip shown while a repository is over a GitHub limit (HS2-P1GYWH). */
export function RepositorySizeBannerDemo() {
  return (
    <>
      <CatalogExample
        label="Over Soft Limit"
        note="Shown above the project workspace once the project's repository size has loaded. Dismiss hides it for this level until the project reopens."
        viewport={{ width: 'full' }}
      >
        {RepositorySizeBanner({ report: readyReport('warning'), projectId: 'hotsheet2-1a2b3c' })}
      </CatalogExample>
      <CatalogExample
        label="Over Hard Limit"
        note="A warning dismissed earlier shows again when the repository crosses the hard limit."
        viewport={{ width: 'full' }}
      >
        {RepositorySizeBanner({ report: readyReport('critical'), dismissed: 'warning', projectId: 'hotsheet2-1a2b3c' })}
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireRepositorySizeDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="repository-size"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'state') repositorySizeSettings.state.value = control.value;
      if (
        control.getAttribute('name') === 'compaction' &&
        ['idle', 'loading', 'ready', 'error'].includes(control.value)
      )
        repositorySizeSettings.compaction.value = control.value as CompactionDemoState;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', PROJECT_LIFECYCLE_ACTIONS.previewRepositoryCompaction.selector, () => {
      repositorySizeSettings.compaction.value = 'ready';
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
    'repository-size-banner': { catalogExamples: true, Demo: RepositorySizeBannerDemo },
  },
  wire: wireRepositorySizeDemo,
};
