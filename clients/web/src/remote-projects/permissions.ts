/** One open remote tab on a reviewed server, in the host's terms: its composite browser id, the
 * host's checkout id, and that checkout's root and linked stores on the host. */
export interface RemotePermissionScope {
  id: string;
  checkoutId: string;
  root: string;
  stores: readonly string[];
}

/** The open tab that owns a host permission request (HS2-HSFVHC). A hook names its checkout as
 * `checkout:<host checkout id>`; older asks and driven connections name a root or store path. */
export function remotePermissionOwner(
  project: unknown,
  scopes: readonly RemotePermissionScope[],
): RemotePermissionScope | undefined {
  if (typeof project !== 'string' || !project) return undefined;
  return scopes.find(
    (scope) => project === `checkout:${scope.checkoutId}` || project === scope.root || scope.stores.includes(project),
  );
}

/** A remote host lists every pending request it holds. Relabel each request an open tab owns with
 * that tab's id, so the browser shows it only in its own project, exactly as a local request is
 * matched by checkout id. A request no open tab owns passes through unchanged, as a local one does. */
export function remotePermissionList(list: unknown, scopes: readonly RemotePermissionScope[]): unknown {
  if (!Array.isArray(list)) return list;
  return list.map((item: unknown) => {
    if (typeof item !== 'object' || item === null || Array.isArray(item)) return item;
    const record = item as Record<string, unknown>,
      owner = remotePermissionOwner(record.project, scopes);
    return owner ? { ...record, project: owner.id } : record;
  });
}
