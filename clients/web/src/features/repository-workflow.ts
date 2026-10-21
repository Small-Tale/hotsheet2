import { signal } from 'kerfjs';

import { Api, type CodeReview, type RepositoryFile, type RepositoryStatus } from '../api';
import type { Project } from '../interactions/types';
import type { RepositoryDetailState, RepositorySetupStep, RepositoryStatusView } from '../repository-state';

type RepositoryApi = Pick<
  Api,
  'repositoryStatus' | 'initializeRepository' | 'configureRepositoryRemote' | 'repositoryCommits' | 'repositoryFiles'
>;

export interface RepositoryWorkflowDependencies {
  project: () => Project | undefined;
  api?: (path: string) => RepositoryApi;
}

/** API-consumer state only; browser visibility, forms, observers, and rendering stay in the adapter. */
export function createRepositoryWorkflow({ project, api = (path) => new Api(path) }: RepositoryWorkflowDependencies) {
  const repository = signal<RepositoryStatus | null>(null),
    repositoryError = signal(''),
    repositoryRefreshing = signal(false),
    repositorySetupStep = signal<RepositorySetupStep | undefined>(undefined),
    repositorySetupBusy = signal(false),
    repositorySetupError = signal(''),
    repositoryDetail = signal<RepositoryDetailState>({
      view: 'unstaged',
      files: [],
      commits: [],
      loading: false,
      loaded: false,
      error: '',
    });
  let repositoryDetailGeneration = 0,
    detailProjectId: string | undefined,
    refreshingProjectId: string | undefined,
    setupProjectId: string | undefined;

  async function refreshRepositoryStatus(): Promise<boolean> {
    const current = project();
    if (!current || (repositoryRefreshing.value && refreshingProjectId === current.id)) return false;
    refreshingProjectId = current.id;
    repositoryRefreshing.value = true;
    try {
      const status = await api(current.apiPath).repositoryStatus(current.id);
      if (project()?.id !== current.id) return false;
      repository.value = status;
      repositoryError.value = '';
      repositorySetupError.value = '';
      if (status.initialized === false) repositorySetupStep.value = 'initialize';
      else if (repositorySetupStep.value !== 'remote') repositorySetupStep.value = undefined;
      return status.initialized !== false;
    } catch (reason) {
      if (project()?.id === current.id) {
        repository.value = null;
        repositoryError.value = reason instanceof Error ? reason.message : String(reason);
      }
      return false;
    } finally {
      if (refreshingProjectId === current.id) {
        repositoryRefreshing.value = false;
        refreshingProjectId = undefined;
      }
    }
  }

  async function initializeRepository() {
    const current = project();
    if (!current || (repositorySetupBusy.value && setupProjectId === current.id)) return;
    setupProjectId = current.id;
    repositorySetupBusy.value = true;
    repositorySetupError.value = '';
    try {
      const status = await api(current.apiPath).initializeRepository(current.id);
      if (project()?.id !== current.id) return;
      repository.value = status;
      repositoryError.value = '';
      repositorySetupStep.value = 'remote';
    } catch (reason) {
      if (project()?.id === current.id)
        repositorySetupError.value = reason instanceof Error ? reason.message : String(reason);
    } finally {
      if (setupProjectId === current.id) {
        repositorySetupBusy.value = false;
        setupProjectId = undefined;
      }
    }
  }

  async function connectRepositoryRemote(remoteValue: string): Promise<boolean> {
    const current = project();
    if (!current || (repositorySetupBusy.value && setupProjectId === current.id)) return false;
    const remote = remoteValue.trim();
    if (!remote) {
      repositorySetupError.value = 'Enter a remote URL.';
      return false;
    }
    setupProjectId = current.id;
    repositorySetupBusy.value = true;
    repositorySetupError.value = '';
    try {
      const status = await api(current.apiPath).configureRepositoryRemote(current.id, remote);
      if (project()?.id !== current.id) return false;
      repository.value = status;
      repositoryError.value = '';
      repositorySetupStep.value = undefined;
      return true;
    } catch (reason) {
      if (project()?.id === current.id)
        repositorySetupError.value = reason instanceof Error ? reason.message : String(reason);
      return false;
    } finally {
      if (setupProjectId === current.id) {
        repositorySetupBusy.value = false;
        setupProjectId = undefined;
      }
    }
  }

  function skipRepositoryRemote() {
    repositorySetupStep.value = undefined;
    repositorySetupError.value = '';
  }

  async function loadRepositoryDetail(view: RepositoryStatusView, reset = false) {
    const current = project(),
      previous = repositoryDetail.value;
    if (!current) return;
    const sameProject = detailProjectId === current.id;
    if (
      !reset &&
      sameProject &&
      previous.view === view &&
      (previous.loading || (previous.loaded && previous.nextCursor === undefined))
    )
      return;
    const generation = ++repositoryDetailGeneration,
      cursor = !reset && sameProject && previous.view === view ? (previous.nextCursor ?? 0) : 0,
      base =
        !reset && sameProject && previous.view === view
          ? previous
          : { view, files: [], commits: [], loading: false, loaded: false, error: '' };
    detailProjectId = current.id;
    repositoryDetail.value = { ...base, view, loading: true, error: '' };
    try {
      const page =
        view === 'commits'
          ? await api(current.apiPath).repositoryCommits(current.id, cursor)
          : await api(current.apiPath).repositoryFiles(current.id, view, cursor);
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

  return {
    repository,
    repositoryError,
    repositoryRefreshing,
    repositorySetupStep,
    repositorySetupBusy,
    repositorySetupError,
    repositoryDetail,
    refreshRepositoryStatus,
    initializeRepository,
    connectRepositoryRemote,
    skipRepositoryRemote,
    loadRepositoryDetail,
  };
}
