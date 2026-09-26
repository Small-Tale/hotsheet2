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
  /** Register one opened project; `index` is its root's position in `roots` (for stable tab order). */
  wire: (root: string, opened: Extract<ProjectOpenResult, { ok: true }>, index: number) => Promise<void>;
  retainFailure: (root: string, failure: Extract<ProjectOpenResult, { ok: false }>) => void;
  activate: (project: Project) => Promise<void>;
  selectFailure: (root: string) => void;
  /** Called once the active project (or its failure) is presented; the rest may still be restoring. */
  activeReady?: () => void;
  waitForRetry?: () => Promise<void>;
}

/**
 * Fetch every remembered project together. When the remembered active root opens on its first
 * attempt, register and activate it immediately so the app is usable without waiting for every other
 * project and its retries; the rest are then registered in remembered order (HS2-X74D4B). Otherwise
 * (no remembered active root, a checkout alias, or a failed first attempt) wait for the full pass and
 * its one bounded retry, then restore exactly one active project as before.
 */
export async function restoreRememberedProjects(options: RestoreProjectsOptions): Promise<void> {
  const roots = [...new Set(options.roots)];
  const results = new Map<string, ProjectOpenResult>();
  const pending = new Map(roots.map((root) => [root, options.fetch(root)]));
  const record = (root: string, result: ProjectOpenResult) => {
    results.set(root, result);
    if (!result.ok) options.retainFailure(root, result);
  };
  let announced = false;
  const announce = () => {
    if (announced) return;
    announced = true;
    options.activeReady?.();
  };
  const registered = new Map<string, Project>();
  const openedByRoot = new Map<string, Project>();
  const wired = new Set<string>();
  const wire = async (root: string) => {
    const result = results.get(root);
    if (!result?.ok || wired.has(root)) return;
    wired.add(root);
    await options.wire(root, result, roots.indexOf(root));
    registered.set(result.project.id, result.project);
    openedByRoot.set(root, registered.get(result.project.id)!);
  };

  let activated: Project | undefined;
  const early = options.activeRoot && pending.has(options.activeRoot) ? options.activeRoot : undefined;
  if (early) {
    const result = await pending.get(early)!;
    record(early, result);
    if (result.ok) {
      await wire(early);
      activated = registered.get(result.project.id);
      await options.activate(activated!);
      announce();
    }
  }
  const firstPass = await Promise.all(roots.map((root) => pending.get(root)!));
  roots.forEach((root, index) => {
    if (!results.has(root)) record(root, firstPass[index]);
  });
  const failed = roots.filter((root) => results.get(root)?.ok === false);
  if (failed.length) {
    await (options.waitForRetry ?? (() => new Promise<void>((resolve) => setTimeout(resolve, 500))))();
    const retried = await Promise.all(failed.map((root) => options.fetch(root)));
    failed.forEach((root, index) => {
      record(root, retried[index]);
    });
  }
  for (const root of roots) await wire(root);
  if (activated) return;
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
  announce();
}
