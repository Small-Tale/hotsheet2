import { deviceClass } from '@kerfjs/ui/device-class';
import { Select } from '@kerfjs/ui/select';
import { wireNavStack } from '@kerfjs/ui/wire-nav-stack';
import { computed, delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CodeReview, RepositoryStatus } from '../api';
import {
  ChangeEvidenceDialog,
  type ChangeEvidenceView,
  type RepositoryFileMenu,
  RepositoryStatusPopover,
  type RepositoryStatusView,
} from '../components/repository-status-popover';
import type { CodeReviewComparison } from '../components/ticket-code-review';
import { revealContextPopupMenu } from '../context-menu-position';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

export type RepositoryDemoScenario = 'clean' | 'dirty' | 'ahead' | 'behind' | 'diverged' | 'conflicted' | 'error';

export const repositoryDemoView = signal<RepositoryStatusView>('unstaged');
export const repositoryDemoEvent = signal('Choose a view or open a file or commit.');
export const repositoryDemoComparison = signal<CodeReviewComparison>({ active: false, side: 'a' });
export const repositoryDemoExpandedCommits = signal<string[]>([]);
export const repositoryDemoScenario = signal<RepositoryDemoScenario>('conflicted');
export const changeEvidenceDemoView = signal<ChangeEvidenceView>('tests');
export const repositoryDemoFileMenu = signal<RepositoryFileMenu | undefined>(undefined);
/** Compact devices drill from the repository list into a view's detail, as in production (HS2-3B8345). */
export const repositoryDemoDetailActive = signal(false);
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
  if (root) syncSettingsControls(root, 'repository-status-popover', { values: { scenario: 'conflicted' } });
}

export function RepositoryStatusPopoverDemo() {
  const scenario = repositoryDemoScenario.value;
  return (
    <section class="repository-status-demo" aria-label="RepositoryStatusPopover demo">
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
      />
      <p class="component-stage__event" aria-live="polite">
        {repositoryDemoEvent.value}
      </p>
    </section>
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
    <section aria-label="ChangeEvidenceDialog demo">
      <ChangeEvidenceDialog
        embedded
        review={evidence}
        view={changeEvidenceDemoView.value}
        fileMenu={repositoryDemoFileMenu.value}
        platform="macos"
      />
      <p class="component-stage__event" aria-live="polite">
        {repositoryDemoEvent.value}
      </p>
    </section>
  );
}

export function RepositoryStatusPopoverSettings() {
  return (
    <form class="settings-form" data-settings="repository-status-popover">
      <Select
        name="scenario"
        label="Repository scenario"
        value={repositoryDemoScenario.value}
        choices={(['clean', 'dirty', 'ahead', 'behind', 'diverged', 'conflicted', 'error'] as const).map((value) => ({
          value,
          label: value[0].toUpperCase() + value.slice(1),
        }))}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireRepositoryStatusDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // The compact Repository Status drill-down pops back to its list the same way (HS2-3B8345).
  let demoRepositoryNavStack: { section: Element; dispose: () => void } | undefined;
  function syncDemoRepositoryNavStack() {
    const section = root.querySelector('.repository-status-demo [data-component="nav-stack"]');
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
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectRepositoryView.selector, (_event, target) => {
      repositoryDemoView.value = (target as HTMLElement).dataset.itemId as typeof repositoryDemoView.value;
      repositoryDemoDetailActive.value = true;
      repositoryDemoEvent.value = `${(target as HTMLElement).textContent.trim() || 'Repository view'} selected.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleRepositoryComparison.selector, () => {
      if (repositoryDemoComparison.value.active) {
        repositoryDemoComparison.value = { active: false, side: 'a' };
        return;
      }
      repositoryDemoView.value = 'commits';
      repositoryDemoComparison.value = { active: true, side: 'a' };
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.setRepositoryComparisonSide.selector, (_event, target) => {
      repositoryDemoComparison.value = {
        ...repositoryDemoComparison.value,
        side: (target as HTMLElement).dataset.comparisonSide as 'a' | 'b',
      };
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectRepositoryComparisonCommit.selector, (_event, target) => {
      const sha = (target as HTMLElement).dataset.commitSha!,
        current = repositoryDemoComparison.value;
      repositoryDemoComparison.value =
        current.side === 'a' ? { ...current, a: sha, side: 'b' } : { ...current, b: sha };
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleCodeReviewCommit.selector, (_event, target) => {
      const sha = (target as HTMLElement).dataset.commitSha!;
      repositoryDemoExpandedCommits.value = repositoryDemoExpandedCommits.value.includes(sha)
        ? repositoryDemoExpandedCommits.value.filter((item) => item !== sha)
        : [...repositoryDemoExpandedCommits.value, sha];
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.refreshRepositoryStatus.selector, () => {
      repositoryDemoEvent.value = 'Repository status refreshed.';
    }),
  );
  const repositoryDemoFileSelector = '[data-action="select-repository-file"]';
  const repositoryDemoFileMenuTrigger = '[data-action="open-repository-file-menu-trigger"]';
  function openRepositoryDemoFileMenu(target: Element, x: number, y: number) {
    const element = target.closest<HTMLElement>(repositoryDemoFileSelector) ?? (target as HTMLElement);
    const path = element.dataset.itemId!;
    const ticket =
      (target as HTMLElement).dataset.fileMenuSource === 'ticket' ||
      Boolean(target.closest('[data-component="change-evidence-dialog"]'));
    const view = repositoryDemoView.value;
    const diff = ticket
      ? 'ticket'
      : view === 'staged' || view === 'unstaged'
        ? view
        : view === 'conflicted'
          ? 'unstaged'
          : undefined;
    const width = 232;
    const height = 226;
    const bounds = target.closest('.dialog-surface')?.getBoundingClientRect();
    const minimumX = bounds ? bounds.left + 8 : 8;
    const maximumX = Math.min(
      window.innerWidth - width - 8,
      bounds ? bounds.right - width - 8 : Number.POSITIVE_INFINITY,
    );
    const minimumY = bounds ? bounds.top + 8 : 8;
    const maximumY = Math.min(
      window.innerHeight - height - 8,
      bounds ? bounds.bottom - height - 8 : Number.POSITIVE_INFINITY,
    );
    repositoryDemoFileMenu.value = {
      path,
      absolutePath: `/work/hotsheet2/${path}`,
      diff,
      x: Math.max(minimumX, Math.min(x, maximumX)),
      y: Math.max(minimumY, Math.min(y, maximumY)),
      source: ticket ? 'ticket' : 'repository',
    };
    revealContextPopupMenu('repository-file');
  }
  demoListeners.add(
    delegate(root, 'dblclick', repositoryDemoFileSelector, (event, target) => {
      if ((event.target as Element).closest(repositoryDemoFileMenuTrigger)) return;
      repositoryDemoFileMenu.value = undefined;
      repositoryDemoEvent.value = `Would open ${(target as HTMLElement).dataset.itemId}.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', repositoryDemoFileMenuTrigger, (event, target) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      const box = target.getBoundingClientRect();
      openRepositoryDemoFileMenu(target, box.right, box.bottom);
    }),
  );
  demoListeners.add(
    delegate(root, 'keydown', repositoryDemoFileMenuTrigger, (event, target) => {
      const key = (event as KeyboardEvent).key;
      if (key !== 'Enter' && key !== ' ') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const box = target.getBoundingClientRect();
      openRepositoryDemoFileMenu(target, box.right, box.bottom);
    }),
  );
  demoListeners.add(
    delegate(root, 'contextmenu', repositoryDemoFileSelector, (event, target) => {
      event.preventDefault();
      const pointer = event as MouseEvent;
      openRepositoryDemoFileMenu(target, pointer.clientX, pointer.clientY);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-repository-file-action]', (_event, target) => {
      const action = (target as HTMLElement).dataset.repositoryFileAction;
      const path = (target as HTMLElement).dataset.repositoryFilePath;
      repositoryDemoFileMenu.value = undefined;
      repositoryDemoEvent.value =
        action === 'show-diff' ? `Would review ${path} in Glassbox.` : `Would ${action} ${path}.`;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.selectChangeEvidenceView.selector, (_event, target) => {
      changeEvidenceDemoView.value = (target as HTMLElement).dataset.itemId as typeof changeEvidenceDemoView.value;
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
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'repository-status-popover': {
      Demo: RepositoryStatusPopoverDemo,
      Settings: RepositoryStatusPopoverSettings,
      reset: resetRepositoryStatusDemo,
    },
    'change-evidence-dialog': { Demo: ChangeEvidenceDialogDemo },
  },
  wire: wireRepositoryStatusDemo,
};
