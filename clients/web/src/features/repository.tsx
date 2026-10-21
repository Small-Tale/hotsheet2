import { deviceClass } from '@kerfjs/ui/device-class';
import type { Signal } from 'kerfjs';
import { computed, signal } from 'kerfjs';

import { Api, type CodeReview, type FullTicket } from '../api';
import { ChangeEvidenceSurface, RepositoryStatusSurface } from '../components/reader-overlay-surfaces';
import {
  type ChangeEvidenceView,
  DETAIL_SCROLL_OWNER,
  type RepositoryFileMenu,
  type RepositoryStatusView,
} from '../components/repository-status-popover';
import { type CodeReviewComparison } from '../components/ticket-code-review';
import type { Project } from '../interactions/types';
import { createRepositoryWorkflow } from './repository-workflow';

export interface RepositoryDependencies {
  project: () => Project | undefined;
  selectedTicket: Signal<FullTicket | null>;
  showToast: (message: string) => void;
}

export function createRepositoryController(dependencies: RepositoryDependencies) {
  const { project, selectedTicket, showToast } = dependencies;
  const workflow = createRepositoryWorkflow({ project });
  const {
    repository,
    repositoryError,
    repositoryRefreshing,
    repositorySetupStep,
    repositorySetupBusy,
    repositorySetupError,
    repositoryDetail,
    initializeRepository,
    loadRepositoryDetail,
  } = workflow;
  // Compact devices drill from the repository list into the selected view's detail (HS2-3B8345).
  // Read only the compact flag: deviceClass() emits on every viewport resize, and an unchanged
  // class must not re-render the shell (startup stays atomic).
  const repositoryDetailActive = signal(false),
    device = deviceClass(),
    compactDevice = computed(() => device.value.compact);
  const repositoryView = signal<RepositoryStatusView>('unstaged'),
    repositoryFileMenu = signal<RepositoryFileMenu | undefined>(undefined),
    repositorySelectedFiles = signal<string[]>([]);
  let repositoryFileSelectionAnchor: string | undefined;
  const changeEvidenceView = signal<ChangeEvidenceView>('docs');
  // Which surface owns the change-evidence popover: a ticket-reader frame id when launched from inside
  // the modal reader (so it renders as a reader descendant and stays interactive, not inert beneath the
  // modal top layer — HS2-6EV2ES / HS2-EZ10RS), or undefined for the non-modal inspector (app root).
  const changeEvidenceReader = signal<string | undefined>(undefined);
  const repositoryComparison = signal<CodeReviewComparison>({ active: false, side: 'a' }),
    expandedCodeReviewCommits = signal<string[]>([]);

  let repositoryPaginationObserver: IntersectionObserver | undefined;
  const codeReview = signal<CodeReview | undefined>(undefined),
    codeReviewLoading = signal(false),
    codeReviewMessage = signal('');

  async function refreshRepositoryStatus() {
    if ((await workflow.refreshRepositoryStatus()) && document.querySelector('#repository-status-popover:popover-open'))
      void loadRepositoryDetail(repositoryView.value, true);
  }

  async function connectRepositoryRemote(form: HTMLFormElement) {
    const remote = (form.elements.namedItem('repository-remote') as HTMLInputElement | null)?.value.trim() ?? '';
    if (await workflow.connectRepositoryRemote(remote)) {
      showToast('Origin remote added.');
      void loadRepositoryDetail(repositoryView.value, true);
    }
  }

  function skipRepositoryRemote() {
    workflow.skipRepositoryRemote();
    showToast('Git initialized without a remote.');
    void loadRepositoryDetail(repositoryView.value, true);
  }

  function syncRepositoryPaginationObserver() {
    repositoryPaginationObserver?.disconnect();
    const root = document.querySelector(`.${DETAIL_SCROLL_OWNER}`),
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
      const review = await new Api(current.apiPath).codeReview(current.id, ticket.qualified_id);
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
                compact: compactDevice.value,
                detailActive: repositoryDetailActive.value,
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
      compact: compactDevice.value,
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
    repositoryDetailActive,
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
