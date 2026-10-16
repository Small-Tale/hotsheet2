import { CatalogExample } from '@kerfjs/ui/catalog';
import { deviceClass } from '@kerfjs/ui/device-class';
import { Select } from '@kerfjs/ui/select';
import { wireNavStack } from '@kerfjs/ui/wire-nav-stack';
import { computed, delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CodeReview, RepositoryStatus } from '../api';
import { wireRepositoryBrowser } from '../components/repository-status-interactions';
import {
  ChangeEvidenceDialog,
  type ChangeEvidenceView,
  repositoryAbsolutePath,
  type RepositoryFileMenu,
  RepositoryStatusPopover,
  type RepositoryStatusView,
} from '../components/repository-status-popover';
import type { CodeReviewComparison } from '../components/ticket-code-review';
import { DemoEvent } from './components/demo-event';
import { type DemoDevice, DeviceSetting } from './components/device-setting';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

export type RepositoryDemoScenario = 'clean' | 'dirty' | 'ahead' | 'behind' | 'diverged' | 'conflicted' | 'error';

export const repositoryDemoView = signal<RepositoryStatusView>('unstaged');
export const repositoryDemoEvent = signal('Choose a view or open a file or commit.');
export const repositoryDemoComparison = signal<CodeReviewComparison>({ active: false, side: 'a' });
export const repositoryDemoExpandedCommits = signal<string[]>([]);
export const repositoryDemoScenario = signal<RepositoryDemoScenario>('conflicted');
export const changeEvidenceDemoView = signal<ChangeEvidenceView>('tests');
export const repositoryDemoFileMenu = signal<RepositoryFileMenu | undefined>(undefined);
export const repositoryDemoSelectedFiles = signal<string[]>([]);
/** Compact devices drill from the repository list into a view's detail, as in production (HS2-3B8345). */
export const repositoryDemoDetailActive = signal(false);
/** Remote clients disable the host-only diff-tool and file actions (HS2-NWWF1Z). */
export const repositoryDemoClient = signal<DemoDevice>('host');
const repositoryDemoDevice = deviceClass(),
  repositoryDemoCompact = computed(() => repositoryDemoDevice.value.compact);

const status: RepositoryStatus = {
  branch: 'feature/repository-dialog',
  upstream: 'origin/main',
  ahead: 2,
  behind: 1,
  staged: 2,
  unstaged: 2,
  untracked: 1,
  conflicted: 1,
  clean: false,
  root: '/work/hotsheet2',
  platform: 'macos',
  commit_count: 24,
  difftool: 'Glassbox',
  truncated: false,
  files: [
    {
      path: 'clients/web/src/components/repository-status-popover.tsx',
      staged: 'modified',
      untracked: false,
      conflicted: false,
    },
    {
      path: 'clients/web/src/ux-demo/repository-status-demo.tsx',
      staged: 'added',
      untracked: false,
      conflicted: false,
    },
    { path: 'clients/web/src/main.tsx', unstaged: 'modified', untracked: false, conflicted: false },
    { path: 'docs/06-clients.md', unstaged: 'modified', untracked: false, conflicted: false },
    { path: 'notes/repository-review.md', untracked: true, conflicted: false },
    {
      path: 'clients/web/src/hot-sheet-tokens.css',
      staged: 'unmerged',
      unstaged: 'unmerged',
      untracked: false,
      conflicted: true,
    },
  ],
  ranges: [{ from: 'aaa1111', to: 'bbb2222', count: 2 }],
  commits: [
    {
      sha: 'bbb2222',
      short_sha: 'bbb2222',
      subject: 'Refine repository status composition',
      body: 'Use the shared menu primitives.\n\n- Align status icons\n- Preserve native file actions',
      committed_at: '2026-09-04T10:00:00Z',
    },
    {
      sha: 'aaa1111',
      short_sha: 'aaa1111',
      subject: 'Build repository status browser',
      body: 'Add the initial master-detail repository browser.\nIncludes **Markdown** commit details.',
      committed_at: '2026-09-04T09:00:00Z',
    },
  ],
};

export function repositoryStatusForScenario(scenario: RepositoryDemoScenario): RepositoryStatus | null {
  if (scenario === 'error') return null;
  if (scenario === 'conflicted') return status;
  const withoutChanges = { ...status, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, files: [], clean: true };
  if (scenario === 'dirty')
    return {
      ...status,
      ahead: 0,
      behind: 0,
      conflicted: 0,
      clean: false,
      files: status.files?.filter((file) => !file.conflicted),
    };
  if (scenario === 'ahead') return { ...withoutChanges, ahead: 2, behind: 0 };
  if (scenario === 'behind') return { ...withoutChanges, ahead: 0, behind: 2 };
  if (scenario === 'diverged') return { ...withoutChanges, ahead: 2, behind: 1 };
  return { ...withoutChanges, ahead: 0, behind: 0 };
}

export function resetRepositoryStatusDemo(root?: ParentNode): void {
  repositoryDemoScenario.value = 'conflicted';
  repositoryDemoDetailActive.value = false;
  repositoryDemoSelectedFiles.value = [];
  repositoryDemoFileMenu.value = undefined;
  repositoryDemoClient.value = 'host';
  if (root)
    syncSettingsControls(root, 'repository-status-popover', { values: { scenario: 'conflicted', device: 'host' } });
}

export function RepositoryStatusPopoverDemo() {
  const scenario = repositoryDemoScenario.value;
  return (
    <CatalogExample
      label="Interactive"
      viewport={{ width: 'full' }}
      rootAttributes={{ 'data-repository-status-demo': '' }}
    >
      <RepositoryStatusPopover
        embedded
        status={repositoryStatusForScenario(scenario)}
        error={scenario === 'error' ? 'git status failed: repository is unavailable' : ''}
        view={repositoryDemoView.value}
        compact={repositoryDemoCompact.value}
        detailActive={repositoryDemoDetailActive.value}
        comparison={repositoryDemoComparison.value}
        expandedCommits={repositoryDemoExpandedCommits.value}
        fileMenu={repositoryDemoFileMenu.value}
        selectedFiles={repositoryDemoSelectedFiles.value}
        hostActionsAvailable={repositoryDemoClient.value === 'host'}
      />
      <DemoEvent>{repositoryDemoEvent.value}</DemoEvent>
    </CatalogExample>
  );
}

const evidence: CodeReview = {
  difftool: 'Glassbox',
  truncated: false,
  ranges: [],
  commits: [],
  summary: { files: { total: 4, docs: 1, tests: 1, source: 1, other: 1 }, tests_added: 1, tests_modified: 0 },
  files: [
    { path: 'docs/06-clients.md', change: 'modified', category: 'docs' },
    { path: 'clients/web/src/change-evidence.test.ts', change: 'added', category: 'tests' },
    { path: 'clients/web/src/components/change-evidence.tsx', change: 'modified', category: 'source' },
    { path: 'Cargo.toml', change: 'modified', category: 'other' },
  ],
};
export function ChangeEvidenceDialogDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'full' }}>
      <ChangeEvidenceDialog
        embedded
        review={evidence}
        view={changeEvidenceDemoView.value}
        fileMenu={repositoryDemoFileMenu.value}
        selectedFiles={repositoryDemoSelectedFiles.value}
        platform="macos"
        hostActionsAvailable={repositoryDemoClient.value === 'host'}
      />
      <DemoEvent>{repositoryDemoEvent.value}</DemoEvent>
    </CatalogExample>
  );
}

export function RepositoryStatusPopoverSettings() {
  return (
    <DemoSettingsForm settings="repository-status-popover">
      <Select
        name="scenario"
        label="Repository scenario"
        value={repositoryDemoScenario.value}
        choices={(['clean', 'dirty', 'ahead', 'behind', 'diverged', 'conflicted', 'error'] as const).map((value) => ({
          value,
          label: value[0].toUpperCase() + value.slice(1),
        }))}
      />
      <DeviceSetting value={repositoryDemoClient.value} />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </DemoSettingsForm>
  );
}

export function ChangeEvidenceDialogSettings() {
  return (
    <DemoSettingsForm settings="change-evidence-dialog">
      <DeviceSetting value={repositoryDemoClient.value} />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </DemoSettingsForm>
  );
}

/** Restore the change evidence dialog's device setting (HS2-NWWF1Z). */
export function resetChangeEvidenceDemo(root?: ParentNode): void {
  repositoryDemoClient.value = 'host';
  if (root) syncSettingsControls(root, 'change-evidence-dialog', { values: { device: 'host' } });
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireRepositoryStatusDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // The compact Repository Status drill-down pops back to its list the same way (HS2-3B8345).
  let demoRepositoryNavStack: { section: Element; dispose: () => void } | undefined;
  function syncDemoRepositoryNavStack() {
    const section = root.querySelector('[data-repository-status-demo] [data-component="nav-stack"]');
    if (section === (demoRepositoryNavStack?.section ?? null)) return;
    demoRepositoryNavStack?.dispose();
    demoRepositoryNavStack = section
      ? {
          section,
          dispose: wireNavStack(section, {
            onBack: () => {
              repositoryDemoDetailActive.value = false;
              repositoryDemoEvent.value = 'Back to repository views.';
            },
          }),
        }
      : undefined;
  }
  new MutationObserver(syncDemoRepositoryNavStack).observe(root, { childList: true, subtree: true });
  queueMicrotask(syncDemoRepositoryNavStack);
  // The production repository browser handlers, with fixture side effects (HS2-RWY78R).
  let selectionAnchor: string | undefined;
  demoListeners.add(
    wireRepositoryBrowser(root, {
      view: repositoryDemoView,
      detailActive: repositoryDemoDetailActive,
      fileMenu: repositoryDemoFileMenu,
      selectedFiles: repositoryDemoSelectedFiles,
      selectionAnchor: {
        get: () => selectionAnchor,
        set: (path) => {
          selectionAnchor = path;
        },
      },
      comparison: repositoryDemoComparison,
      expandedCommits: repositoryDemoExpandedCommits,
      changeEvidenceView: changeEvidenceDemoView,
      onViewSelected: (view) => {
        repositoryDemoEvent.value = `${view[0].toUpperCase()}${view.slice(1)} selected.`;
      },
      onComparisonStarted: () => undefined,
      canDiff: () => true,
      absolutePath: (path) => repositoryAbsolutePath('/work/hotsheet2', path, 'macos'),
      onFileAction: (path, action) => {
        repositoryDemoEvent.value = `Would ${action} ${path}.`;
      },
      onFileDiff: (paths) => {
        repositoryDemoEvent.value = `Would review ${paths.join(', ')} in Glassbox.`;
      },
      onCopyPaths: (values, action) => {
        repositoryDemoEvent.value = `Would ${action} ${values.join(', ')}.`;
      },
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.refreshRepositoryStatus.selector, () => {
      repositoryDemoEvent.value = 'Repository status refreshed.';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openRepositoryReview.selector, (_event, target) => {
      repositoryDemoEvent.value = `Would open ${(target as HTMLElement).dataset.reviewMode} review in Glassbox.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="repository-status-popover"] [name="scenario"]', (_event, target) => {
      repositoryDemoScenario.value = (target as FormControl).value as typeof repositoryDemoScenario.value;
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'change',
      '[data-settings="repository-status-popover"] [name="device"], [data-settings="change-evidence-dialog"] [name="device"]',
      (_event, target) => {
        repositoryDemoClient.value = (target as FormControl).value as DemoDevice;
      },
    ),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'repository-status-popover': {
      catalogExamples: true,
      Demo: RepositoryStatusPopoverDemo,
      Settings: RepositoryStatusPopoverSettings,
      reset: resetRepositoryStatusDemo,
    },
    'change-evidence-dialog': {
      catalogExamples: true,
      Demo: ChangeEvidenceDialogDemo,
      Settings: ChangeEvidenceDialogSettings,
      reset: resetChangeEvidenceDemo,
    },
  },
  wire: wireRepositoryStatusDemo,
};
