import { delegate, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { Api, type CodeReview, type FullTicket, type RepositoryStatus } from '../api';
import { type RepositorySetupStep } from '../components/repository-setup';
import { wireRepositoryBrowser } from '../components/repository-status-interactions';
import {
  type ChangeEvidenceView,
  repositoryAbsolutePath,
  type RepositoryFileMenu,
  type RepositoryStatusView,
} from '../components/repository-status-popover';
import { type CodeReviewComparison, codeReviewTarget } from '../components/ticket-code-review';
import { copyText } from '../copy-text';
import { HostOnlyActionError } from '../host-only-actions';
import { REPOSITORY_ACTIONS } from '../interaction-attrs/repository';
import { data } from './dom';
import { type Control, type Project, type RepositoryDetailState } from './types';

/** Live application bindings used by this handler group. */
export interface RepositoryInteractionsDependencies {
  readonly repository: Signal<RepositoryStatus | null>;
  readonly repositoryView: Signal<RepositoryStatusView>;
  /** Compact list-detail drill-down: the selected view's detail is pushed (HS2-3B8345). */
  readonly repositoryDetailActive: Signal<boolean>;
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
  ) => Promise<boolean | Project>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireRepositoryInteractions(dependencies: RepositoryInteractionsDependencies) {
  const lifetime = createScope();
  const {
    repository,
    repositoryView,
    repositoryDetailActive,
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
  lifetime.add(
    delegate(document.body, 'click', REPOSITORY_ACTIONS.openRepositoryStatus.selector, () => {
      const status = repository.value,
        view = status?.conflicted
          ? 'conflicted'
          : status?.unstaged
            ? 'unstaged'
            : status?.staged
              ? 'staged'
              : status?.untracked
                ? 'untracked'
                : 'commits';
      repositoryView.value = view;
      repositoryDetailActive.value = false;
      repositorySetupStep.value = status?.initialized === false ? 'initialize' : undefined;
      repositorySetupError.value = '';
      repositoryFileMenu.value = undefined;
      repositorySelectedFiles.value = [];
      dependencies.repositoryFileSelectionAnchor = undefined;
      repositoryComparison.value = { active: false, side: 'a' };
      expandedCodeReviewCommits.value = [];
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
      (document.querySelector('#repository-status-popover') as Control).showPopover?.();
      if (status?.initialized !== false) void loadRepositoryDetail(view, true);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', REPOSITORY_ACTIONS.refreshRepositoryStatus.selector, () => {
      void refreshRepositoryStatus();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', REPOSITORY_ACTIONS.initializeRepository.selector, () => {
      void initializeRepository();
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', REPOSITORY_ACTIONS.connectRepositoryRemote.selector, (event, target) => {
      event.preventDefault();
      void connectRepositoryRemote(target as HTMLFormElement);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', REPOSITORY_ACTIONS.skipRepositoryRemote.selector, () => {
      skipRepositoryRemote();
    }),
  );
  lifetime.add(
    wireRepositoryBrowser(document.body, {
      view: repositoryView,
      detailActive: repositoryDetailActive,
      fileMenu: repositoryFileMenu,
      selectedFiles: repositorySelectedFiles,
      selectionAnchor: {
        get: () => dependencies.repositoryFileSelectionAnchor,
        set: (path) => {
          dependencies.repositoryFileSelectionAnchor = path;
        },
      },
      comparison: repositoryComparison,
      expandedCommits: expandedCodeReviewCommits,
      changeEvidenceView,
      onViewSelected: (view) => {
        void loadRepositoryDetail(view, true);
      },
      onComparisonStarted: () => {
        if (repositoryDetail.value.view !== 'commits') void loadRepositoryDetail('commits', true);
      },
      canDiff: (source) =>
        source === 'ticket' ? Boolean(codeReview.value?.difftool) : Boolean(repository.value?.difftool),
      absolutePath: (path) => repositoryAbsolutePath(repository.value?.root, path, repository.value?.platform),
      onFileAction: (path, action) => {
        void performRepositoryFileAction(path, action);
      },
      onFileDiff: (paths, diff) => {
        if (diff === 'ticket') openTicketFileDiff(paths);
        else void openRepositoryFileDiff(paths, diff);
      },
      onCopyPaths: (values) => {
        void copyText(values.join('\n'))
          .then(() => {
            showToast(`${values.length === 1 ? 'Path' : `${values.length} paths`} copied.`);
          })
          .catch((reason: unknown) => {
            error.value = `Copy failed: ${reason instanceof Error ? reason.message : String(reason)}`;
          });
      },
    }),
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
  async function openRepositoryFileDiff(paths: string[], area: 'staged' | 'unstaged') {
    const current = project();
    if (!current) return;
    try {
      // One request, so the configured tool opens once with every selected file (HS2-J7HQ5E).
      await new Api(current.apiPath).openRepositoryReview(current.id, { mode: 'worktree_file', paths, area });
      showToast(
        `Opened ${paths.length === 1 ? `${area} file diff` : `${paths.length} ${area} files in one diff`} in ${repository.value?.difftool ?? 'the configured diff tool'}.`,
      );
    } catch (reason) {
      error.value = reason instanceof Error ? reason.message : String(reason);
    }
  }
  lifetime.add(
    delegate(document.body, 'click', REPOSITORY_ACTIONS.openRepositoryReview.selector, (_event, target) => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', REPOSITORY_ACTIONS.openChangeEvidence.selector, (_event, target) => {
      const review = codeReview.value;
      if (!review) return;
      changeEvidenceView.value =
        (['docs', 'tests', 'source', 'other'] as const).find((category) =>
          review.files?.some((file) => file.category === category),
        ) ?? 'docs';
      repositorySelectedFiles.value = [];
      dependencies.repositoryFileSelectionAnchor = undefined;
      changeEvidenceReader.value = target.closest<HTMLElement>('[data-component="ticket-reader"]')?.dataset
        .readerFrameId;
      requestAnimationFrame(() =>
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type.
        requestAnimationFrame(() => document.querySelector<Control>('#change-evidence-dialog')?.showPopover?.()),
      );
    }),
  );
  function openTicketFileDiff(paths: string[]) {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket) return;
    codeReviewMessage.value = `Opening ${paths.length === 1 ? 'file diff' : `${paths.length} files in one diff`}…`;
    void new Api(current.apiPath)
      .openCodeReview(current.id, ticket.qualified_id, { mode: 'ticket_file', paths })
      .then(() => {
        if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) {
          codeReviewMessage.value = '';
          showToast(
            `Opened ${paths.length === 1 ? 'file diff' : `${paths.length} files in one diff`} in ${codeReview.value?.difftool ?? 'the configured diff tool'}.`,
          );
        }
      })
      .catch((reason: unknown) => {
        if (reason instanceof HostOnlyActionError) {
          // The inline message can sit below the fold of a long commit list (HS2-F78B1S).
          codeReviewMessage.value = '';
          showToast(reason.message);
        } else if (project()?.id === current.id && selectedTicket.value?.id === ticket.id)
          codeReviewMessage.value = reason instanceof Error ? reason.message : String(reason);
      });
  }
  lifetime.add(
    delegate(document.body, 'submit', REPOSITORY_ACTIONS.openProjectForm.selector, (event, target) => {
      event.preventDefault();
      const root = (target.querySelector('[name="project-root"]') as Control).value;
      void openProject(root);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
