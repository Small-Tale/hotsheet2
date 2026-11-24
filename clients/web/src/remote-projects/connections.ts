/** A remote server reports its own paths; the browser knows only the tab's composite id. */
export interface RemoteConnectionScope {
  id: string;
  root: string;
  stores: readonly string[];
}

export function remoteConnectionCreate(value: unknown, projectId: string, checkoutId: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value))
    throw new Error('AI connection creation must be a JSON object.');
  const request = value as Record<string, unknown>;
  if (request.checkout !== projectId) throw new Error('This AI connection belongs to a different remote project.');
  return { ...request, checkout: checkoutId };
}

function belongsToTab(project: unknown, projectId: string, scopes: readonly RemoteConnectionScope[]): boolean {
  if (typeof project !== 'string' || !project) return false;
  // Connections name the exact checkout or ticket-store root, unlike terminals whose cwd may
  // be anywhere under a checkout. A shared store belongs to every tab that links it.
  return scopes.some((scope) => scope.id === projectId && (scope.root === project || scope.stores.includes(project)));
}

export function remoteConnectionList(
  list: unknown,
  projectId: string,
  scopes: readonly RemoteConnectionScope[],
): unknown {
  if (!Array.isArray(list)) return list;
  return list
    .filter(
      (item): item is Record<string, unknown> =>
        typeof item === 'object' &&
        item !== null &&
        !Array.isArray(item) &&
        belongsToTab((item as Record<string, unknown>).project, projectId, scopes),
    )
    .map((item) => ({ ...item, project: projectId }));
}

export function remoteConnectionItem(
  item: unknown,
  projectId: string,
  scopes: readonly RemoteConnectionScope[],
): unknown {
  if (typeof item !== 'object' || item === null || Array.isArray(item)) return undefined;
  const record = item as Record<string, unknown>;
  return belongsToTab(record.project, projectId, scopes) ? { ...record, project: projectId } : undefined;
}
