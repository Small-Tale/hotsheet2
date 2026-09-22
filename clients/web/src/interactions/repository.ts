import { delegate, type Signal } from 'kerfjs';

import { Api, type CodeReview, type FullTicket, type RepositoryStatus } from '../api';
import { type RepositorySetupStep } from '../components/repository-setup';
import {
  type ChangeEvidenceView,
  repositoryAbsolutePath,
  type RepositoryFileMenu,
  type RepositoryStatusView,
} from '../components/repository-status-popover';
import { type CodeReviewComparison, codeReviewTarget } from '../components/ticket-code-review';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { updateRepositoryFileSelection } from '../repository-file-selection';
import { data } from './dom';
import { type Control, type Project, type RepositoryDetailState } from './types';

/** Live application bindings used by this handler group. */
export interface RepositoryInteractionsDependencies {
  readonly repository: Signal<RepositoryStatus | null>;
  readonly repositoryView: Signal<RepositoryStatusView>;
  readonly repositorySetupStep: Signal<RepositorySetupStep | undefined>;
  readonly repositorySetupError: Signal<string>;
  readonly repositoryFileMenu: Signal<RepositoryFileMenu | undefined>;
  readonly repositorySelectedFiles: Signal<string[]>;
  repositoryFileSelectionAnchor: string | undefined;
  readonly repositoryComparison: Signal<CodeReviewComparison>;
  readonly expandedCodeReviewCommits: Signal<string[]>;
  readonly loadRepositoryDetail: (view: RepositoryStatusView, reset?: boolean) => Promise<void>;
  readonly refreshRepositoryStatus: () => Promise<void>;
  readonly initializeRepository: () => Promise<void>;
  readonly connectRepositoryRemote: (form: HTMLFormElement) => Promise<void>;
  readonly skipRepositoryRemote: () => void;
  readonly repositoryDetail: Signal<RepositoryDetailState>;
  readonly project: () => Project | undefined;
  readonly showToast: (message: string) => void;
  readonly error: Signal<string>;
  readonly codeReview: Signal<CodeReview | undefined>;
  readonly changeEvidenceView: Signal<ChangeEvidenceView>;
  readonly changeEvidenceReader: Signal<string | undefined>;
  readonly selectedTicket: Signal<FullTicket | null>;
  readonly codeReviewMessage: Signal<string>;
  readonly openProject: (
    root: string,
    ticketStore?: string,
    remember?: boolean,
    reportError?: boolean,
    retainFailure?: boolean,
  ) => Promise<boolean>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireRepositoryInteractions(dependencies: RepositoryInteractionsDependencies) {
  const {
    repository,
    repositoryView,
    repositorySetupStep,
    repositorySetupError,
    repositoryFileMenu,
    repositorySelectedFiles,
    repositoryComparison,
    expandedCodeReviewCommits,
    loadRepositoryDetail,
    refreshRepositoryStatus,
    initializeRepository,
    connectRepositoryRemote,
    skipRepositoryRemote,
    repositoryDetail,
    project,
    showToast,
    error,
    codeReview,
    changeEvidenceView,
    changeEvidenceReader,
    selectedTicket,
    codeReviewMessage,
    openProject,
  } = dependencies;
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  delegate(document.body,'click','[data-action="open-repository-status"]',()=>{const status=repository.value,view=status?.conflicted?'conflicted':status?.unstaged?'unstaged':status?.staged?'staged':status?.untracked?'untracked':'commits';repositoryView.value=view;repositorySetupStep.value=status?.initialized===false?'initialize':undefined;repositorySetupError.value='';repositoryFileMenu.value=undefined;repositorySelectedFiles.value=[];dependencies.repositoryFileSelectionAnchor=undefined;repositoryComparison.value={active:false,side:'a'};expandedCodeReviewCommits.value=[];(document.querySelector('#repository-status-popover') as Control).showPopover?.();if(status?.initialized!==false)void loadRepositoryDetail(view,true)});
  delegate(document.body, 'click', '[data-action="refresh-repository-status"]', () => {
    void refreshRepositoryStatus();
  });
  delegate(document.body, 'click', '[data-action="initialize-repository"]', () => {
    void initializeRepository();
  });
  delegate(document.body, 'submit', '[data-action="connect-repository-remote"]', (event, target) => {
    event.preventDefault();
    void connectRepositoryRemote(target as HTMLFormElement);
  });
  delegate(document.body, 'click', '[data-action="skip-repository-remote"]', () => {
    skipRepositoryRemote();
  });
  delegate(document.body, 'click', '[data-action="select-repository-view"]', (_event, target) => {
    const view = data(target).itemId as RepositoryStatusView;
    repositoryView.value = view;
    repositoryFileMenu.value = undefined;
    repositorySelectedFiles.value = [];
    dependencies.repositoryFileSelectionAnchor = undefined;
    void loadRepositoryDetail(view, true);
  });
  function selectRepositoryComparisonCommit(sha: string) {
    const current = repositoryComparison.value;
    repositoryComparison.value = current.side === 'a' ? { ...current, a: sha, side: 'b' } : { ...current, b: sha };
  }
  function toggleExpandedCodeReviewCommit(sha: string) {
    expandedCodeReviewCommits.value = expandedCodeReviewCommits.value.includes(sha)
      ? expandedCodeReviewCommits.value.filter((item) => item !== sha)
      : [...expandedCodeReviewCommits.value, sha];
  }
  delegate(document.body, 'click', '[data-action="toggle-repository-comparison"]', () => {
    if (repositoryComparison.value.active) {
      repositoryComparison.value = { active: false, side: 'a' };
      return;
    }
    repositoryView.value = 'commits';
    repositoryComparison.value = { active: true, side: 'a' };
    if (repositoryDetail.value.view !== 'commits') void loadRepositoryDetail('commits', true);
  });
  delegate(document.body, 'click', '[data-action="set-repository-comparison-side"]', (_event, target) => {
    repositoryComparison.value = { ...repositoryComparison.value, side: data(target).comparisonSide as 'a' | 'b' };
  });
  delegate(document.body, 'click', '[data-action="select-repository-comparison-commit"]', (_event, target) => {
    selectRepositoryComparisonCommit(data(target).commitSha!);
  });
  delegate(document.body, 'click', '[data-action="toggle-code-review-commit"]', (_event, target) => {
    toggleExpandedCodeReviewCommit(data(target).commitSha!);
  });
  delegate(
    document.body,
    'keydown',
    '[data-action="select-repository-comparison-commit"],[data-action="toggle-code-review-commit"]',
    (event, target) => {
      const key = (event as KeyboardEvent).key;
      if (key !== 'Enter' && key !== ' ') return;
      event.preventDefault();
      const action = data(target).action,
        sha = data(target).commitSha!;
      if (action === 'select-repository-comparison-commit') selectRepositoryComparisonCommit(sha);
      else toggleExpandedCodeReviewCommit(sha);
    },
  );
  async function performRepositoryFileAction(path: string, action: 'open' | 'reveal') {
    const current = project();
    if (!current) return;
    repositoryFileMenu.value = undefined;
    try {
      await new Api(current.apiPath).repositoryFileAction(current.id, path, action);
      showToast(action === 'open' ? 'Opened file.' : 'Opened file location.');
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  const repositoryFileSelector = '[data-action="select-repository-file"]';
  function visibleRepositoryFilePaths(target: Element) {
    return [
      ...(target.closest('.repository-status-popover__files')?.querySelectorAll<HTMLElement>(repositoryFileSelector) ??
        []),
    ]
      .map((item) => data(item).itemId!)
      .filter(Boolean);
  }
  function selectRepositoryFile(target: Element, event: MouseEvent | KeyboardEvent) {
    const path = data(target).itemId;
    if (!path) return;
    repositorySelectedFiles.value = updateRepositoryFileSelection(
      repositorySelectedFiles.value,
      visibleRepositoryFilePaths(target),
      path,
      {
        additive: event.metaKey || event.ctrlKey,
        range: event.shiftKey,
        anchor: dependencies.repositoryFileSelectionAnchor,
      },
    );
    if (!event.shiftKey) dependencies.repositoryFileSelectionAnchor = path;
    repositoryFileMenu.value = undefined;
  }
  async function openRepositoryFileDiff(paths: string[], area: 'staged' | 'unstaged') {
    const current = project();
    if (!current) return;
    try {
      await Promise.all(
        paths.map((path) =>
          new Api(current.apiPath).openRepositoryReview(current.id, { mode: 'worktree_file', path, area }),
        ),
      );
      showToast(
        `Opened ${paths.length === 1 ? `${area} file diff` : `${paths.length} ${area} file diffs`} in ${repository.value?.difftool ?? 'the configured diff tool'}.`,
      );
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  function openRepositoryFileMenu(target: Element, x: number, y: number) {
    const row = target.closest<HTMLElement>(repositoryFileSelector) ?? (target as HTMLElement),
      path = data(row).itemId;
    if (!path) return;
    const source = target.closest('[data-component="change-evidence-dialog"]') ? 'ticket' : 'repository',
      selected = repositorySelectedFiles.value.includes(path) ? repositorySelectedFiles.value : [path];
    repositorySelectedFiles.value = selected;
    dependencies.repositoryFileSelectionAnchor = path;
    const canDiff = source === 'ticket' ? Boolean(codeReview.value?.difftool) : Boolean(repository.value?.difftool),
      diff = canDiff
        ? source === 'ticket'
          ? 'ticket'
          : repositoryView.value === 'staged' || repositoryView.value === 'unstaged'
            ? repositoryView.value
            : repositoryView.value === 'conflicted'
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
        const absolute = repositoryAbsolutePath(repository.value?.root, item, repository.value?.platform);
        return absolute ? [absolute] : [];
      });
    repositoryFileMenu.value = {
      path,
      paths: selected,
      absolutePath: absolutePaths[0],
      absolutePaths,
      diff,
      ...position,
    };
  }
  delegate(document.body, 'click', '[data-action="open-repository-file-menu-trigger"]', (event, target) => {
    event.preventDefault();
    event.stopImmediatePropagation();
    const box = target.getBoundingClientRect();
    openRepositoryFileMenu(target, box.right, box.bottom);
  });
  delegate(document.body, 'keydown', '[data-action="open-repository-file-menu-trigger"]', (event, target) => {
    const key = (event as KeyboardEvent).key;
    if (key !== 'Enter' && key !== ' ') return;
    event.preventDefault();
    event.stopImmediatePropagation();
    const box = target.getBoundingClientRect();
    openRepositoryFileMenu(target, box.right, box.bottom);
  });
  delegate(document.body, 'click', repositoryFileSelector, (event, target) => {
    selectRepositoryFile(target, event as MouseEvent);
  });
  delegate(document.body, 'dblclick', repositoryFileSelector, (event, target) => {
    if ((event.target as Element).closest('[data-action="open-repository-file-menu-trigger"]')) return;
    void performRepositoryFileAction(data(target).itemId!, 'open');
  });
  delegate(document.body, 'keydown', repositoryFileSelector, (event, target) => {
    const key = (event as KeyboardEvent).key;
    if (key !== 'Enter' && key !== ' ') return;
    event.preventDefault();
    selectRepositoryFile(target, event as KeyboardEvent);
  });
  delegate(document.body, 'contextmenu', repositoryFileSelector, (event, target) => {
    event.preventDefault();
    const pointer = event as MouseEvent;
    openRepositoryFileMenu(target, pointer.clientX, pointer.clientY);
  });
  delegate(document.body, 'click', '[data-repository-file-action]', (event, target) => {
    event.stopPropagation();
    const menu = repositoryFileMenu.value,
      action = data(target).repositoryFileAction;
    if (!menu) return;
    const paths = menu.paths ?? [menu.path];
    repositoryFileMenu.value = undefined;
    if (action === 'show-diff') {
      if (menu.diff === 'ticket') openTicketFileDiff(paths);
      else if (menu.diff === 'staged' || menu.diff === 'unstaged') void openRepositoryFileDiff(paths, menu.diff);
      return;
    }
    if (action === 'copy-path' || action === 'copy-absolute-path') {
      const values =
        action === 'copy-path' ? paths : (menu.absolutePaths ?? (menu.absolutePath ? [menu.absolutePath] : []));
      void navigator.clipboard
        .writeText(values.join('\n'))
        .then(() => {
          showToast(`${values.length === 1 ? 'Path' : `${values.length} paths`} copied.`);
        })
        .catch((reason: unknown) => {
          error.value = `Copy failed: ${reason instanceof Error ? reason.message : String(reason)}`;
        });
      return;
    }
    if ((action === 'open' || action === 'reveal') && paths.length === 1)
      void performRepositoryFileAction(paths[0], action);
  });
  delegate(document.body, 'click', '[data-action="open-repository-review"]', (_event, target) => {
    const current = project(),
      reviewTarget = codeReviewTarget(data(target));
    if (!current || !reviewTarget) return;
    void new Api(current.apiPath)
      .openRepositoryReview(current.id, reviewTarget)
      .then(() => {
        showToast(`Opened in ${repository.value?.difftool ?? 'the configured diff tool'}.`);
      })
      .catch((reason: unknown) => {
        error.value = reason instanceof Error ? reason.message : String(reason);
      });
  });
  // prettier-ignore
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
  delegate(document.body,'click','[data-action="open-change-evidence"]',(_event,target)=>{const review=codeReview.value;if(!review)return;changeEvidenceView.value=(['docs','tests','source','other'] as const).find(category=>review.files?.some(file=>file.category===category))??'docs';repositorySelectedFiles.value=[];dependencies.repositoryFileSelectionAnchor=undefined;changeEvidenceReader.value=target.closest<HTMLElement>('[data-component="ticket-reader"]')?.dataset.readerFrameId;requestAnimationFrame(()=>requestAnimationFrame(()=>document.querySelector<Control>('#change-evidence-dialog')?.showPopover?.()))});
  delegate(document.body, 'click', '[data-action="select-change-evidence-view"]', (_event, target) => {
    changeEvidenceView.value = data(target).itemId as ChangeEvidenceView;
    repositorySelectedFiles.value = [];
    dependencies.repositoryFileSelectionAnchor = undefined;
  });
  function openTicketFileDiff(paths: string[]) {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket) return;
    codeReviewMessage.value = `Opening ${paths.length === 1 ? 'file diff' : `${paths.length} file diffs`}…`;
    void Promise.all(
      paths.map((path) =>
        new Api(current.apiPath).openCodeReview(current.id, ticket.id, { mode: 'ticket_file', path }),
      ),
    )
      .then(() => {
        if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) {
          codeReviewMessage.value = '';
          showToast(
            `Opened ${paths.length === 1 ? 'file diff' : `${paths.length} file diffs`} in ${codeReview.value?.difftool ?? 'the configured diff tool'}.`,
          );
        }
      })
      .catch((reason: unknown) => {
        if (project()?.id === current.id && selectedTicket.value?.id === ticket.id)
          codeReviewMessage.value = reason instanceof Error ? reason.message : String(reason);
      });
  }
  delegate(document.body, 'submit', '[data-action="open-project-form"]', (event, target) => {
    event.preventDefault();
    const root = (target.querySelector('[name="project-root"]') as Control).value,
      store = (target.querySelector('[name="ticket-store"]') as Control).value;
    void openProject(root, store || undefined);
  });
}
