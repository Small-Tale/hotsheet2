import { delegate, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';
import { updateRepositoryFileSelection } from '../repository-file-selection';
import type { ChangeEvidenceView, RepositoryFileMenu, RepositoryStatusView } from './repository-status-popover';
import type { CodeReviewComparison } from './ticket-code-review';

/** Where a repository file menu was opened: the repository browser or a ticket's change evidence. */
export type RepositoryFileSource = 'repository' | 'ticket';
/** The diff a file menu's Show Diff action opens. */
export type RepositoryFileDiff = NonNullable<RepositoryFileMenu['diff']>;

/**
 * State and side effects for the shared repository browser and change-evidence interactions
 * (HS2-RWY78R). Production passes live signals and API calls; the UX demo passes fixture signals
 * and callbacks that report what would happen, so both run the same handlers.
 */
export interface RepositoryBrowserBindings {
  readonly view: Signal<RepositoryStatusView>;
  readonly detailActive: Signal<boolean>;
  readonly fileMenu: Signal<RepositoryFileMenu | undefined>;
  readonly selectedFiles: Signal<string[]>;
  readonly selectionAnchor: { get(): string | undefined; set(path: string | undefined): void };
  readonly comparison: Signal<CodeReviewComparison>;
  readonly expandedCommits: Signal<string[]>;
  readonly changeEvidenceView: Signal<ChangeEvidenceView>;
  /** A repository view was chosen and its detail should load. */
  readonly onViewSelected: (view: RepositoryStatusView) => void;
  /** Comparison mode started on the commits view. */
  readonly onComparisonStarted: () => void;
  /** Whether a diff tool is available for files from this source. */
  readonly canDiff: (source: RepositoryFileSource) => boolean;
  /** The absolute path for a repository-relative path, when the root is known. */
  readonly absolutePath: (path: string) => string | undefined;
  readonly onFileAction: (path: string, action: 'open' | 'reveal') => void;
  readonly onFileDiff: (paths: string[], diff: RepositoryFileDiff) => void;
  readonly onCopyPaths: (values: string[], action: 'copy-path' | 'copy-absolute-path') => void;
}

const fileSelector = '[data-action="select-repository-file"]';
const triggerSelector = REPOSITORY_ACTIONS.openRepositoryFileMenuTrigger.selector;
const datasetOf = (element: Element) => (element as HTMLElement).dataset;

/**
 * Wire repository view navigation, commit comparison/expansion, file selection, the file context
 * menu and its actions, and change-evidence view selection under `root`.
 */
export function wireRepositoryBrowser(root: HTMLElement, bindings: RepositoryBrowserBindings): () => void {
  const lifetime = createScope();
  const {
    view,
    detailActive,
    fileMenu,
    selectedFiles,
    selectionAnchor,
    comparison,
    expandedCommits,
    changeEvidenceView,
  } = bindings;
  lifetime.add(
    delegate(root, 'click', REPOSITORY_ACTIONS.selectRepositoryView.selector, (_event, target) => {
      const next = datasetOf(target).itemId as RepositoryStatusView;
      view.value = next;
      detailActive.value = true;
      fileMenu.value = undefined;
      selectedFiles.value = [];
      selectionAnchor.set(undefined);
      bindings.onViewSelected(next);
    }),
  );
  function selectComparisonCommit(sha: string) {
    const current = comparison.value;
    comparison.value = current.side === 'a' ? { ...current, a: sha, side: 'b' } : { ...current, b: sha };
  }
  function toggleExpandedCommit(sha: string) {
    expandedCommits.value = expandedCommits.value.includes(sha)
      ? expandedCommits.value.filter((item) => item !== sha)
      : [...expandedCommits.value, sha];
  }
  lifetime.add(
    delegate(root, 'click', REPOSITORY_ACTIONS.toggleRepositoryComparison.selector, () => {
      if (comparison.value.active) {
        comparison.value = { active: false, side: 'a' };
        return;
      }
      view.value = 'commits';
      detailActive.value = true;
      comparison.value = { active: true, side: 'a' };
      bindings.onComparisonStarted();
    }),
  );
  lifetime.add(
    delegate(root, 'click', REPOSITORY_ACTIONS.setRepositoryComparisonSide.selector, (_event, target) => {
      comparison.value = { ...comparison.value, side: datasetOf(target).comparisonSide as 'a' | 'b' };
    }),
  );
  lifetime.add(
    delegate(root, 'click', REPOSITORY_ACTIONS.selectRepositoryComparisonCommit.selector, (_event, target) => {
      selectComparisonCommit(datasetOf(target).commitSha!);
    }),
  );
  lifetime.add(
    delegate(root, 'click', REPOSITORY_ACTIONS.toggleCodeReviewCommit.selector, (_event, target) => {
      toggleExpandedCommit(datasetOf(target).commitSha!);
    }),
  );
  lifetime.add(
    delegate(
      root,
      'keydown',
      '[data-action="select-repository-comparison-commit"],[data-action="toggle-code-review-commit"]',
      (event, target) => {
        const key = (event as KeyboardEvent).key;
        if (key !== 'Enter' && key !== ' ') return;
        event.preventDefault();
        const { action, commitSha } = datasetOf(target);
        if (action === 'select-repository-comparison-commit') selectComparisonCommit(commitSha!);
        else toggleExpandedCommit(commitSha!);
      },
    ),
  );
  function visibleFilePaths(target: Element) {
    return [...(target.closest('[data-repository-file-list]')?.querySelectorAll<HTMLElement>(fileSelector) ?? [])]
      .map((item) => item.dataset.itemId!)
      .filter(Boolean);
  }
  function selectFile(target: Element, event: MouseEvent | KeyboardEvent) {
    const path = datasetOf(target).itemId;
    if (!path) return;
    selectedFiles.value = updateRepositoryFileSelection(selectedFiles.value, visibleFilePaths(target), path, {
      additive: event.metaKey || event.ctrlKey,
      range: event.shiftKey,
      anchor: selectionAnchor.get(),
    });
    if (!event.shiftKey) selectionAnchor.set(path);
    fileMenu.value = undefined;
  }
  function openFileMenu(target: Element, x: number, y: number) {
    const row = target.closest<HTMLElement>(fileSelector) ?? (target as HTMLElement),
      path = row.dataset.itemId;
    if (!path) return;
    const source: RepositoryFileSource = target.closest('[data-component="change-evidence-dialog"]')
        ? 'ticket'
        : 'repository',
      selected = selectedFiles.value.includes(path) ? selectedFiles.value : [path];
    selectedFiles.value = selected;
    selectionAnchor.set(path);
    const current = view.value,
      diff: RepositoryFileDiff | undefined = bindings.canDiff(source)
        ? source === 'ticket'
          ? 'ticket'
          : current === 'staged' || current === 'unstaged'
            ? current
            : current === 'conflicted'
              ? 'unstaged'
              : undefined
        : undefined,
      width = 232,
      height = 226,
      viewportPosition = viewportSafeContextMenuPosition(x, y, window.innerWidth, window.innerHeight, {
        width,
        height,
      }),
      bounds = row.closest('.dialog-surface')?.getBoundingClientRect(),
      position = bounds
        ? {
            x: Math.max(bounds.left + 8, Math.min(viewportPosition.x, bounds.right - width - 8)),
            y: Math.max(bounds.top + 8, Math.min(viewportPosition.y, bounds.bottom - height - 8)),
          }
        : viewportPosition,
      absolutePaths = selected.flatMap((item) => {
        const absolute = bindings.absolutePath(item);
        return absolute ? [absolute] : [];
      });
    fileMenu.value = {
      path,
      paths: selected,
      absolutePath: absolutePaths[0],
      absolutePaths,
      diff,
      source,
      ...position,
    };
  }
  lifetime.add(
    delegate(root, 'click', triggerSelector, (event, target) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      const box = target.getBoundingClientRect();
      openFileMenu(target, box.right, box.bottom);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', triggerSelector, (event, target) => {
      const key = (event as KeyboardEvent).key;
      if (key !== 'Enter' && key !== ' ') return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const box = target.getBoundingClientRect();
      openFileMenu(target, box.right, box.bottom);
    }),
  );
  lifetime.add(
    delegate(root, 'click', fileSelector, (event, target) => {
      selectFile(target, event as MouseEvent);
    }),
  );
  lifetime.add(
    delegate(root, 'dblclick', fileSelector, (event, target) => {
      if ((event.target as Element).closest(triggerSelector)) return;
      fileMenu.value = undefined;
      bindings.onFileAction(datasetOf(target).itemId!, 'open');
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', fileSelector, (event, target) => {
      const key = (event as KeyboardEvent).key;
      if (key !== 'Enter' && key !== ' ') return;
      event.preventDefault();
      selectFile(target, event as KeyboardEvent);
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', fileSelector, (event, target) => {
      event.preventDefault();
      const pointer = event as MouseEvent;
      openFileMenu(target, pointer.clientX, pointer.clientY);
    }),
  );
  lifetime.add(
    delegate(root, 'click', '[data-repository-file-action]', (event, target) => {
      event.stopPropagation();
      const menu = fileMenu.value,
        action = datasetOf(target).repositoryFileAction;
      if (!menu) return;
      const paths = menu.paths ?? [menu.path];
      fileMenu.value = undefined;
      if (action === 'show-diff') {
        if (menu.diff) bindings.onFileDiff(paths, menu.diff);
        return;
      }
      if (action === 'copy-path' || action === 'copy-absolute-path') {
        bindings.onCopyPaths(
          action === 'copy-path' ? paths : (menu.absolutePaths ?? (menu.absolutePath ? [menu.absolutePath] : [])),
          action,
        );
        return;
      }
      if ((action === 'open' || action === 'reveal') && paths.length === 1) bindings.onFileAction(paths[0], action);
    }),
  );
  lifetime.add(
    delegate(root, 'click', REPOSITORY_ACTIONS.selectChangeEvidenceView.selector, (_event, target) => {
      changeEvidenceView.value = datasetOf(target).itemId as ChangeEvidenceView;
      selectedFiles.value = [];
      selectionAnchor.set(undefined);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
