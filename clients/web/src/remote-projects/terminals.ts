import { terminalProjectOwner, type TerminalProjectScope } from '../terminal-project-scope';
import { isRemoteProjectId } from './identity';

/** One open remote tab on a reviewed server: its composite browser id and its checkout's root on
 * that server. Only the trusted bridge knows the root; the browser sees the composite id. */
export type RemoteTerminalScope = TerminalProjectScope;

interface TerminalPaths {
  cwd?: unknown;
  launch_cwd?: unknown;
}

function joinRemotePath(root: string, rest: string): string {
  const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/';
  const trimmed = root.replace(/[\\/]+$/, '') || root;
  const relative = rest.replace(/^[\\/]+/, '');
  return relative ? `${trimmed}${separator}${relative.split(/[\\/]+/).join(separator)}` : trimmed;
}

/** The directory a remote host launches a tab's terminal in (HS2-PPHJMZ). The browser addresses its
 * project by composite id, which is no path on the remote host, so the bridge substitutes that
 * checkout's root. Another tab's composite id is refused instead of launching in a wrong place. */
export function remoteTerminalLaunchCwd(cwd: unknown, projectId: string, root: string): string {
  if (cwd === undefined || cwd === null || cwd === '' || cwd === projectId) return root;
  if (typeof cwd !== 'string') throw new Error('Terminal directory must be text.');
  if (cwd.startsWith(`${projectId}/`)) return joinRemotePath(root, cwd.slice(projectId.length));
  if (isRemoteProjectId(cwd)) throw new Error('This terminal directory belongs to a different project.');
  return cwd;
}

/** The open remote tab on the same server that owns a remote terminal (HS2-JJKVJJ). A server lists
 * every terminal it hosts; each belongs only to the most specific open checkout containing its
 * launch directory (else its reported directory), exactly as local terminals do. A terminal an older
 * client launched with the composite id as its directory still belongs to that tab. */
export function remoteTerminalOwner(scopes: readonly RemoteTerminalScope[], info: TerminalPaths): string | undefined {
  const launch = typeof info.launch_cwd === 'string' && info.launch_cwd ? info.launch_cwd : undefined,
    current = typeof info.cwd === 'string' && info.cwd ? info.cwd : undefined;
  for (const path of [launch, current])
    if (path !== undefined && scopes.some((scope) => scope.id === path)) return path;
  // The lone-tab fallback applies only to a terminal reporting no directory at all, so a terminal
  // launched outside every open checkout is never claimed by the only tab on that server.
  if (launch === undefined && current === undefined) return terminalProjectOwner(scopes, undefined);
  return (
    (launch === undefined ? undefined : terminalProjectOwner(scopes, launch)) ??
    (current === undefined ? undefined : terminalProjectOwner(scopes, current))
  );
}

/** Present a remote terminal to its owning tab: the launch directory becomes the tab's composite
 * id, which is the only root the browser can compare. The reported cwd stays the remote path, for
 * display. */
export function remoteTerminalView<T extends object>(info: T, projectId: string): T {
  return { ...info, launch_cwd: projectId };
}

/** The slice of a remote server's terminal list that belongs to one tab, in browser terms. */
export function remoteTerminalList(list: unknown, projectId: string, scopes: readonly RemoteTerminalScope[]): unknown {
  if (!Array.isArray(list)) return list;
  return list
    .filter(
      (info): info is TerminalPaths & object =>
        typeof info === 'object' && info !== null && remoteTerminalOwner(scopes, info as TerminalPaths) === projectId,
    )
    .map((info) => remoteTerminalView(info, projectId));
}

/** One remote terminal (an open or a read answer) in browser terms; an unowned one passes through
 * unchanged so its tab can still address a terminal it already holds. */
export function remoteTerminalItem(item: unknown, projectId: string, scopes: readonly RemoteTerminalScope[]): unknown {
  if (typeof item !== 'object' || item === null || Array.isArray(item)) return item;
  return remoteTerminalOwner(scopes, item) === projectId ? remoteTerminalView(item, projectId) : item;
}
