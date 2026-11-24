/** HS2-4Q9DVS: hiding a tab never unregisters its project or stops its services. */
const STORAGE_KEY = 'hotsheet.layout.minimized-projects.v1';

export interface MinimizedProjectIdentity {
  id: string;
  name: string;
  root: string;
  remoteHost?: string;
}

export function loadMinimizedProjects(storage: Pick<Storage, 'getItem'>): string[] {
  let stored: unknown;
  try {
    stored = JSON.parse(storage.getItem(STORAGE_KEY) ?? 'null');
  } catch {
    return [];
  }
  if (!Array.isArray(stored)) return [];
  return [...new Set(stored.filter((id): id is string => typeof id === 'string' && id.trim().length > 0))];
}

export function saveMinimizedProjects(storage: Pick<Storage, 'setItem'>, ids: readonly string[]): void {
  storage.setItem(STORAGE_KEY, JSON.stringify(ids));
}

export function visibleProjectIds(order: readonly string[], minimized: readonly string[]): string[] {
  const hidden = new Set(minimized);
  return order.filter((id) => !hidden.has(id));
}

/** Prefer the next visible tab, then the previous one, when the selected tab is minimized. */
export function minimizeProject(
  order: readonly string[],
  minimized: readonly string[],
  selected: string,
  id: string,
): { minimized: string[]; nextSelected?: string } {
  const index = order.indexOf(id);
  if (index < 0 || minimized.includes(id)) return { minimized: [...minimized] };
  const nextMinimized = [...minimized, id];
  if (selected !== id) return { minimized: nextMinimized };
  const hidden = new Set(nextMinimized);
  const nextSelected =
    order.slice(index + 1).find((candidate) => !hidden.has(candidate)) ??
    order
      .slice(0, index)
      .reverse()
      .find((candidate) => !hidden.has(candidate));
  return { minimized: nextMinimized, nextSelected };
}

export function restoreProject(minimized: readonly string[], id: string): string[] {
  return minimized.filter((candidate) => candidate !== id);
}

/** Duplicate names need a host or path so every restore command names one project clearly. */
export function minimizedProjectDescription(
  project: MinimizedProjectIdentity,
  projects: readonly MinimizedProjectIdentity[],
): string | undefined {
  if (projects.filter((candidate) => candidate.name === project.name).length < 2) return undefined;
  if (project.remoteHost) {
    const sameHost = projects.filter(
      (candidate) => candidate.name === project.name && candidate.remoteHost === project.remoteHost,
    );
    if (sameHost.length === 1) return project.remoteHost;
  }
  return project.root;
}
