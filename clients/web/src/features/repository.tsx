import type { Signal } from 'kerfjs';
import { signal } from 'kerfjs';

import { Api, type CodeReview, type FullTicket, type RepositoryFile, type RepositoryStatus } from '../api';
import { ChangeEvidenceSurface, RepositoryStatusSurface } from '../components/reader-overlay-surfaces';
import type { RepositorySetupStep } from '../components/repository-setup';
import {
  type ChangeEvidenceView,
  type RepositoryFileMenu,
  type RepositoryStatusView,
} from '../components/repository-status-popover';
import { type CodeReviewComparison } from '../components/ticket-code-review';
import type { Project, RepositoryDetailState } from '../interactions/types';

export interface RepositoryDependencies {
  project: () => Project | undefined;
  selectedTicket: Signal<FullTicket | null>;
  showToast: (message: string) => void;
}

export function createRepositoryController(dependencies: RepositoryDependencies) {
  const { project, selectedTicket, showToast } = dependencies;
  const repository = signal<RepositoryStatus | null>(null),
    repositoryError = signal(''),
    repositoryRefreshing = signal(false);
  const repositoryView = signal<RepositoryStatusView>('unstaged'),
    repositoryFileMenu = signal<RepositoryFileMenu | undefined>(undefined),
    repositorySelectedFiles = signal<string[]>([]);
  const repositorySetupStep = signal<RepositorySetupStep | undefined>(undefined),
    repositorySetupBusy = signal(false),
    repositorySetupError = signal('');
  let repositoryFileSelectionAnchor: string | undefined;
  const changeEvidenceView = signal<ChangeEvidenceView>('docs');
  // Which surface owns the change-evidence popover: a ticket-reader frame id when launched from inside
  // the modal reader (so it renders as a reader descendant and stays interactive, not inert beneath the
  // modal top layer — HS2-6EV2ES / HS2-EZ10RS), or undefined for the non-modal inspector (app root).
  const changeEvidenceReader = signal<string | undefined>(undefined);
  const repositoryComparison = signal<CodeReviewComparison>({ active: false, side: 'a' }),
    expandedCodeReviewCommits = signal<string[]>([]);

  const repositoryDetail = signal<RepositoryDetailState>({
    view: 'unstaged',
    files: [],
    commits: [],
    loading: false,
    loaded: false,
    error: '',
  });
  let repositoryDetailGeneration = 0,
    repositoryPaginationObserver: IntersectionObserver | undefined;
  const codeReview = signal<CodeReview | undefined>(undefined),
    codeReviewLoading = signal(false),
    codeReviewMessage = signal('');

  async function refreshRepositoryStatus() {
    const current = project();
    if (!current || repositoryRefreshing.value) return;
    repositoryRefreshing.value = true;
    try {
      const status = await new Api(current.apiPath).repositoryStatus(current.id);
      if (project()?.id === current.id) {
        repository.value = status;
        repositoryError.value = '';
        repositorySetupError.value = '';
        if (status.initialized === false) repositorySetupStep.value = 'initialize';
        else if (repositorySetupStep.value !== 'remote') repositorySetupStep.value = undefined;
        if (status.initialized !== false && document.querySelector('#repository-status-popover:popover-open'))
          void loadRepositoryDetail(repositoryView.value, true);
      }
    } catch (reason) {
      if (project()?.id === current.id) {
        repository.value = null;
        repositoryError.value = reason instanceof Error ? reason.message : String(reason);
      }
    } finally {
      if (project()?.id === current.id) repositoryRefreshing.value = false;
    }
  }

  async function initializeRepository() {
    const current = project();
    if (!current || repositorySetupBusy.value) return;
    repositorySetupBusy.value = true;
    repositorySetupError.value = '';
    try {
      const status = await new Api(current.apiPath).initializeRepository(current.id);
      if (project()?.id !== current.id) return;
      repository.value = status;
      repositoryError.value = '';
      repositorySetupStep.value = 'remote';
    } catch (reason) {
      if (project()?.id === current.id)
        repositorySetupError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (project()?.id === current.id) repositorySetupBusy.value = false;
    }
  }

  async function connectRepositoryRemote(form: HTMLFormElement) {
    const current = project();
    if (!current || repositorySetupBusy.value) return;
    const remote = (form.elements.namedItem('repository-remote') as HTMLInputElement | null)?.value.trim() ?? '';
    if (!remote) {
      repositorySetupError.value = 'Enter a remote URL.';
      return;
    }
    repositorySetupBusy.value = true;
    repositorySetupError.value = '';
    try {
      const status = await new Api(current.apiPath).configureRepositoryRemote(current.id, remote);
      if (project()?.id !== current.id) return;
      repository.value = status;
      repositoryError.value = '';
      repositorySetupStep.value = undefined;
      showToast('Origin remote added.');
      void loadRepositoryDetail(repositoryView.value, true);
    } catch (reason) {
      if (project()?.id === current.id)
        repositorySetupError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (project()?.id === current.id) repositorySetupBusy.value = false;
    }
  }

  function skipRepositoryRemote() {
    repositorySetupStep.value = undefined;
    repositorySetupError.value = '';
    showToast('Git initialized without a remote.');
    void loadRepositoryDetail(repositoryView.value, true);
  }

  async function loadRepositoryDetail(view: RepositoryStatusView, reset = false) {
    const current = project(),
      previous = repositoryDetail.value;
    if (!current) return;
    if (
      !reset &&
      previous.view === view &&
      (previous.loading || (previous.loaded && previous.nextCursor === undefined))
    )
      return;
    const generation = ++repositoryDetailGeneration,
      cursor = !reset && previous.view === view ? (previous.nextCursor ?? 0) : 0,
      base =
        !reset && previous.view === view
          ? previous
          : { view, files: [], commits: [], loading: false, loaded: false, error: '' };
    repositoryDetail.value = { ...base, view, loading: true, error: '' };
    try {
      const page =
        view === 'commits'
          ? await new Api(current.apiPath).repositoryCommits(current.id, cursor)
          : await new Api(current.apiPath).repositoryFiles(current.id, view, cursor);
      if (generation !== repositoryDetailGeneration || project()?.id !== current.id) return;
      repositoryDetail.value = {
        view,
        files: view === 'commits' ? base.files : [...base.files, ...(page.items as RepositoryFile[])],
        commits: view === 'commits' ? [...base.commits, ...(page.items as CodeReview['commits'])] : base.commits,
        nextCursor: page.next_cursor ?? undefined,
        loading: false,
        loaded: true,
        error: '',
      };
    } catch (reason) {
      if (generation !== repositoryDetailGeneration || project()?.id !== current.id) return;
      repositoryDetail.value = {
        ...base,
        view,
        loading: false,
        loaded: true,
        error: reason instanceof Error ? reason.message : String(reason),
      };
    }
  }

  function syncRepositoryPaginationObserver() {
    repositoryPaginationObserver?.disconnect();
    const root = document.querySelector('.repository-status-popover__detail'),
      sentinel = document.querySelector('[data-repository-pagination-sentinel="true"]');
    repositoryPaginationObserver = undefined;
    if (!root || !sentinel || repositoryDetail.value.loading) return;
    repositoryPaginationObserver = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) void loadRepositoryDetail(repositoryDetail.value.view);
      },
      { root, rootMargin: '0px 0px 120px' },
    );
    repositoryPaginationObserver.observe(sentinel);
  }

  async function refreshCodeReview() {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket) return;
    codeReviewLoading.value = true;
    codeReviewMessage.value = '';
    try {
      const review = await new Api(current.apiPath).codeReview(current.id, ticket.id);
      if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) codeReview.value = review;
    } catch (reason) {
      if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) {
        codeReview.value = undefined;
        codeReviewMessage.value = reason instanceof Error ? reason.message : String(reason);
      }
    } finally {
      if (project()?.id === current.id && selectedTicket.value?.id === ticket.id) codeReviewLoading.value = false;
    }
  }

  function repositoryStatusSurface() {
    const detail = repositoryDetail.value;
    return (
      <RepositoryStatusSurface
        repository={
          project()
            ? {
                status: repository.value,
                error: repositoryError.value,
                initialized: repository.value?.initialized !== false,
                setupStep: repositorySetupStep.value,
                setupBusy: repositorySetupBusy.value,
                setupError: repositorySetupError.value,
                refreshing: repositoryRefreshing.value,
                view: repositoryView.value,
                fileMenu: repositoryFileMenu.value,
                selectedFiles: repositorySelectedFiles.value,
                comparison: repositoryComparison.value,
                expandedCommits: expandedCodeReviewCommits.value,
                detailFiles: detail.view === repositoryView.value ? detail.files : [],
                detailCommits: detail.view === repositoryView.value ? detail.commits : [],
                detailLoading: detail.view === repositoryView.value && detail.loading,
                detailError: detail.view === repositoryView.value ? detail.error : '',
                detailHasMore: detail.view === repositoryView.value && detail.nextCursor !== undefined,
              }
            : undefined
        }
      />
    );
  }

  function changeEvidenceSurfaceProps(readerScope?: string): Parameters<typeof ChangeEvidenceSurface>[0]['evidence'] {
    if (changeEvidenceReader.value !== readerScope) return;
    return {
      review: codeReview.value,
      view: changeEvidenceView.value,
      fileMenu: repositoryFileMenu.value,
      selectedFiles: repositorySelectedFiles.value,
      platform: repository.value?.platform,
    };
  }

  function changeEvidenceSurface(readerScope?: string) {
    return <ChangeEvidenceSurface evidence={changeEvidenceSurfaceProps(readerScope)} />;
  }

  return {
    repository,
    repositoryError,
    repositoryView,
    repositoryFileMenu,
    repositorySelectedFiles,
    repositorySetupStep,
    repositorySetupError,
    changeEvidenceView,
    changeEvidenceReader,
    repositoryComparison,
    expandedCodeReviewCommits,
    repositoryDetail,
    codeReview,
    codeReviewLoading,
    codeReviewMessage,
    refreshRepositoryStatus,
    initializeRepository,
    connectRepositoryRemote,
    skipRepositoryRemote,
    loadRepositoryDetail,
    syncRepositoryPaginationObserver,
    refreshCodeReview,
    repositoryStatusSurface,
    changeEvidenceSurfaceProps,
    changeEvidenceSurface,
    get repositoryFileSelectionAnchor() {
      return repositoryFileSelectionAnchor;
    },
    set repositoryFileSelectionAnchor(value: typeof repositoryFileSelectionAnchor) {
      repositoryFileSelectionAnchor = value;
    },
  };
}
