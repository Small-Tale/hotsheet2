/** The bridge path App Settings → Projects reads and removes through (HS2-R806FZ). */
export const REGISTERED_PROJECTS_PATH = '/__hotsheet/registered-projects';

/** One project registered with Hot Sheet on this computer, as App Settings → Projects lists it. */
export interface RegisteredProject {
  id: string;
  alias: string;
  root: string;
  /** Whether the project's folder is a directory right now; false for a deleted or unmounted folder. */
  available: boolean;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

function isRegisteredProject(value: unknown): value is RegisteredProject {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.alias === 'string' &&
    typeof value.root === 'string' &&
    typeof value.available === 'boolean'
  );
}

/** Validate the bridge's registered-project list. */
export function parseRegisteredProjects(value: unknown): RegisteredProject[] {
  if (!Array.isArray(value) || !value.every(isRegisteredProject))
    throw new Error('The Hot Sheet host returned an invalid project list.');
  return value;
}

/** The name a row shows: the alias, else the folder's last path segment, else the whole root. */
export function registeredProjectName(project: Pick<RegisteredProject, 'alias' | 'root'>): string {
  return project.alias || project.root.split(/[\\/]/).filter(Boolean).pop() || project.root;
}

/** Projects ordered for display: by name without regard to case, then by folder. */
export function sortRegisteredProjects(projects: readonly RegisteredProject[]): RegisteredProject[] {
  return [...projects].sort(
    (a, b) =>
      registeredProjectName(a).localeCompare(registeredProjectName(b), undefined, { sensitivity: 'base' }) ||
      a.root.localeCompare(b.root),
  );
}
