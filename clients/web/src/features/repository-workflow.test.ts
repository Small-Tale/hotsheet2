import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import type { RepositoryFile, RepositoryStatus } from '../api';
import type { Project } from '../interactions/types';
import { createRepositoryWorkflow } from './repository-workflow';

const project = (id: string): Project => ({
  id,
  name: id,
  root: `/work/${id}`,
  apiPath: `/api/${id}`,
  stores: [],
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
});
const status = (initialized: boolean): RepositoryStatus => ({
  initialized,
  ahead: 0,
  behind: 0,
  staged: 0,
  unstaged: 0,
  untracked: 0,
  conflicted: 0,
});
const file = (path: string): RepositoryFile => ({ path, unstaged: 'modified', untracked: false, conflicted: false });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function owner() {
  const active = signal<Project | undefined>(project('a'));
  const api = {
    repositoryStatus: vi.fn().mockResolvedValue(status(false)),
    initializeRepository: vi.fn().mockResolvedValue(status(true)),
    configureRepositoryRemote: vi.fn().mockResolvedValue(status(true)),
    repositoryFiles: vi.fn().mockResolvedValue({ items: [], next_cursor: null }),
    repositoryCommits: vi.fn().mockResolvedValue({ items: [], next_cursor: null }),
  };
  const workflow = createRepositoryWorkflow({
    project: () => active.value,
    api: () => api,
  });
  return { active, api, workflow };
}

describe('repository API-consumer workflow (HS2-J4WR11)', () => {
  it('walks uninitialized, failed initialization, retry, remote validation, and skip', async () => {
    const { api, workflow } = owner();
    expect(await workflow.refreshRepositoryStatus()).toBe(false);
    expect(workflow.repositorySetupStep.value).toBe('initialize');
    api.initializeRepository.mockRejectedValueOnce(new Error('offline'));
    await workflow.initializeRepository();
    expect(workflow.repositorySetupError.value).toBe('offline');
    expect(workflow.repositorySetupBusy.value).toBe(false);
    await workflow.initializeRepository();
    expect(workflow.repositorySetupStep.value).toBe('remote');
    expect(await workflow.connectRepositoryRemote('  ')).toBe(false);
    expect(workflow.repositorySetupError.value).toBe('Enter a remote URL.');
    api.configureRepositoryRemote.mockRejectedValueOnce(new Error('permission denied'));
    expect(await workflow.connectRepositoryRemote('  git@example.org:repo  ')).toBe(false);
    expect(api.configureRepositoryRemote).toHaveBeenLastCalledWith('a', 'git@example.org:repo');
    expect(workflow.repositorySetupError.value).toBe('permission denied');
    expect(await workflow.connectRepositoryRemote('https://example.org/repo')).toBe(true);
    expect(workflow.repositorySetupStep.value).toBeUndefined();
    workflow.skipRepositoryRemote();
    expect(workflow.repositorySetupError.value).toBe('');
  });

  it('lets a new project refresh while an old response is pending and ignores the old response', async () => {
    const { active, api, workflow } = owner();
    const old = deferred<RepositoryStatus>();
    api.repositoryStatus.mockReturnValueOnce(old.promise).mockResolvedValueOnce(status(true));
    const first = workflow.refreshRepositoryStatus();
    active.value = project('b');
    expect(await workflow.refreshRepositoryStatus()).toBe(true);
    expect(workflow.repository.value?.initialized).toBe(true);
    old.resolve(status(false));
    expect(await first).toBe(false);
    expect(workflow.repository.value?.initialized).toBe(true);
    expect(workflow.repositoryRefreshing.value).toBe(false);
  });

  it('ignores a stale setup response without clearing the new project busy state', async () => {
    const { active, api, workflow } = owner();
    const old = deferred<RepositoryStatus>();
    const next = deferred<RepositoryStatus>();
    api.initializeRepository.mockReturnValueOnce(old.promise).mockReturnValueOnce(next.promise);
    const first = workflow.initializeRepository();
    active.value = project('b');
    const second = workflow.initializeRepository();
    old.resolve(status(true));
    await first;
    expect(workflow.repositorySetupBusy.value).toBe(true);
    next.resolve(status(true));
    await second;
    expect(workflow.repositorySetupBusy.value).toBe(false);
    expect(workflow.repositorySetupStep.value).toBe('remote');
  });

  it('paginates, ignores stale pages, resets on project switch, and refills after empty results', async () => {
    const { active, api, workflow } = owner();
    const old = deferred<{ items: RepositoryFile[]; next_cursor: number | null }>();
    api.repositoryFiles
      .mockReturnValueOnce(old.promise)
      .mockResolvedValueOnce({ items: [file('b.ts')], next_cursor: 1 })
      .mockResolvedValueOnce({ items: [], next_cursor: null })
      .mockResolvedValueOnce({ items: [file('new.ts')], next_cursor: null });
    const first = workflow.loadRepositoryDetail('unstaged');
    active.value = project('b');
    await workflow.loadRepositoryDetail('unstaged');
    old.resolve({ items: [file('stale.ts')], next_cursor: null });
    await first;
    expect(workflow.repositoryDetail.value.files.map((item) => item.path)).toEqual(['b.ts']);
    await workflow.loadRepositoryDetail('unstaged');
    expect(api.repositoryFiles).toHaveBeenLastCalledWith('b', 'unstaged', 1);
    expect(workflow.repositoryDetail.value).toMatchObject({ files: [file('b.ts')], loaded: true, loading: false });
    await workflow.loadRepositoryDetail('unstaged');
    expect(api.repositoryFiles).toHaveBeenCalledTimes(3);
    await workflow.loadRepositoryDetail('unstaged', true);
    expect(workflow.repositoryDetail.value.files.map((item) => item.path)).toEqual(['new.ts']);
  });
});
