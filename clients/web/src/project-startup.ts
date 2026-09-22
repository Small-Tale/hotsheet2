import { Api, type ProviderDescriptor } from './api';
import type { Project, UnhealthyServerRecovery } from './interactions/types';

export type ProjectOpenResult =
  | { ok: true; project: Project; providers: ProviderDescriptor[] }
  | { ok: false; error: string; recovery?: UnhealthyServerRecovery };

/** Transport and metadata preparation only: safe to run concurrently without changing UI state. */
export async function openProjectFetch(
  root: string,
  ticketStore?: string,
  request: typeof fetch = fetch,
  providers: (project: Project) => Promise<ProviderDescriptor[]> = (project) => new Api(project.apiPath).providers(),
): Promise<ProjectOpenResult> {
  try {
    const response = await request('/__hotsheet/projects/open', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ root, ticketStore }),
    });
    if (!response.ok) {
      const failure = (await response.json()) as { error?: string; recovery?: UnhealthyServerRecovery };
      return { ok: false, error: failure.error || 'Could not open project.', recovery: failure.recovery };
    }
    const project = (await response.json()) as Project;
    return { ok: true, project, providers: await providers(project).catch(() => []) };
  } catch (reason) {
    return { ok: false, error: reason instanceof Error ? reason.message : String(reason) };
  }
}

interface RestoreProjectsOptions {
  roots: readonly string[];
  activeRoot?: string;
  fetch: (root: string) => Promise<ProjectOpenResult>;
  wire: (root: string, opened: Extract<ProjectOpenResult, { ok: true }>) => Promise<void>;
  retainFailure: (root: string, failure: Extract<ProjectOpenResult, { ok: false }>) => void;
  activate: (project: Project) => Promise<void>;
  selectFailure: (root: string) => void;
  waitForRetry?: () => Promise<void>;
}

/** Fetch each pass together, then serialize registration and restore exactly one active project. */
export async function restoreRememberedProjects(options: RestoreProjectsOptions): Promise<void> {
  const roots = [...new Set(options.roots)];
  const results = new Map<string, ProjectOpenResult>();
  const fetchPass = async (requested: readonly string[]) => {
    const opened = await Promise.all(requested.map((root) => options.fetch(root)));
    requested.forEach((root, index) => {
      const result = opened[index];
      results.set(root, result);
      if (!result.ok) options.retainFailure(root, result);
    });
  };
  await fetchPass(roots);
  const failed = roots.filter((root) => results.get(root)?.ok === false);
  if (failed.length) {
    await (options.waitForRetry ?? (() => new Promise<void>((resolve) => setTimeout(resolve, 500))))();
    await fetchPass(failed);
  }
  const registered = new Map<string, Project>();
  const openedByRoot = new Map<string, Project>();
  for (const root of roots) {
    const result = results.get(root);
    if (!result?.ok) continue;
    await options.wire(root, result);
    registered.set(result.project.id, result.project);
    openedByRoot.set(root, registered.get(result.project.id)!);
  }
  const requestedActive = options.activeRoot ? openedByRoot.get(options.activeRoot) : undefined;
  const active = requestedActive
    ? registered.get(requestedActive.id)
    : [...registered.values()].find((project) => project.root === options.activeRoot);
  if (active) await options.activate(active);
  else if (options.activeRoot && results.get(options.activeRoot)?.ok === false)
    options.selectFailure(options.activeRoot);
  else {
    // Preserve the historical fallback when there is no remembered active tab.
    const fallback = [...registered.values()].at(-1);
    if (fallback) await options.activate(fallback);
    else if (roots.length) options.selectFailure(roots[0]);
  }
}
