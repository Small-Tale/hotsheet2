/**
 * HS2-CMXKK2: machine-level reads of a paired remote server, for the Systems screen, without an open
 * project tab. They ride the same private remote connection a remote tab uses (docs/08): the session
 * cookie and TLS material stay in this Node host, the binding is re-checked on every request, and a
 * request needs the same local grant as a remote tab. Only three read-only machine routes pass, with
 * only their own query parameters: host health, the live CPU long poll, and the hosted capacity long
 * poll, read through one checkout the binding's discovery reports as available.
 */
import type { PrivateRemoteConnection, RemoteDiscoveryView } from './host';
import type { RemoteTls } from './transport';

/** The browser-facing prefix; `/__hotsheet/remotes/{binding}/machine` plus one machine route. */
export const REMOTE_MACHINE_SEGMENT = 'machine';

/** The checkout segment the browser sends for the capacity read; the bridge supplies the real one. */
export const REMOTE_MACHINE_CHECKOUT = 'machine';

const QUERY: Readonly<Record<string, readonly string[]>> = {
  '/host-health': [],
  '/host-health/cpu': ['after', 'timeout_ms'],
  // HS2-P2TCQS: Systems asks the machine to re-check AI tool sign-in when it opens.
  '/target-capacity': ['after', 'wait_ms', 'claim_namespaces', 'sign_in_recheck'],
};

export type RemoteMachineRoute =
  | { route: '/host-health' | '/host-health/cpu' | '/target-capacity'; search: string }
  | { error: string; status: number };

/**
 * Which machine route a browser path names, with only that route's query parameters kept. The
 * capacity route is `/checkouts/machine/target-capacity`, so the browser's own capacity call shape
 * works unchanged; any other path or method is refused.
 */
export function remoteMachineRoute(method: string, path: string): RemoteMachineRoute {
  if (method !== 'GET') return { error: 'Only reads reach a remote machine without an open project.', status: 405 };
  const [rawPath = '', query = ''] = path.split('?', 2);
  const pathname = rawPath === `/checkouts/${REMOTE_MACHINE_CHECKOUT}/target-capacity` ? '/target-capacity' : rawPath;
  if (!Object.hasOwn(QUERY, pathname)) return { error: 'This remote machine read is not available.', status: 404 };
  const allowed = QUERY[pathname];
  const incoming = new URLSearchParams(query);
  const kept = new URLSearchParams();
  for (const name of allowed) {
    const value = incoming.get(name);
    if (value !== null) kept.set(name, value);
  }
  const search = kept.toString();
  return { route: pathname as '/host-health' | '/host-health/cpu' | '/target-capacity', search };
}

/** The server path one machine route reaches on the remote, through `checkoutId` for capacity. */
export function remoteMachineServerPath(
  route: '/host-health' | '/host-health/cpu' | '/target-capacity',
  search: string,
  checkoutId: string | undefined,
): string | undefined {
  const query = search ? `?${search}` : '';
  if (route !== '/target-capacity') return `${route}${query}`;
  return checkoutId ? `/checkouts/${encodeURIComponent(checkoutId)}/target-capacity${query}` : undefined;
}

/** What one paired remote's machine reads go through: its private connection and a readable checkout. */
interface MachineTarget {
  connection: PrivateRemoteConnection;
  checkoutId: string | undefined;
}

export interface RemoteMachineBridgeDependencies {
  discover: (bindingId: string) => Promise<RemoteDiscoveryView>;
  connection: (bindingId: string) => Promise<PrivateRemoteConnection>;
  requireCurrent: (connection: PrivateRemoteConnection) => Promise<void>;
  request: (url: URL, init: RequestInit, tls: RemoteTls) => Promise<Response>;
}

const json = (body: { error: string }, status: number) => Response.json(body, { status });

/**
 * The machine reads of every paired remote. A binding's connection and readable checkout are found once
 * (discovery plus the private connection) and kept until a read fails, the binding changes, or the
 * remote refuses the session; the next read finds them again. Nothing here reads on its own.
 */
export function createRemoteMachineBridge(dependencies: RemoteMachineBridgeDependencies) {
  const targets = new Map<string, Promise<MachineTarget>>();

  async function find(bindingId: string): Promise<MachineTarget> {
    const discovery = await dependencies.discover(bindingId);
    if (discovery.remote.revoked) throw new Error('This remote server revoked this computer.');
    const connection = await dependencies.connection(bindingId);
    if (
      connection.binding.revision !== discovery.remote.revision ||
      connection.binding.identity.server_id !== discovery.remote.identity.server_id
    )
      throw new Error('The remote server changed. Review and reconnect it.');
    const checkoutId = discovery.checkouts?.find((checkout) => checkout.state === 'available')?.id;
    return { connection, checkoutId };
  }

  function targetFor(bindingId: string): Promise<MachineTarget> {
    let target = targets.get(bindingId);
    if (!target) {
      target = find(bindingId);
      targets.set(bindingId, target);
      // A failed lookup is not kept: the next read tries again.
      target.catch(() => {
        if (targets.get(bindingId) === target) targets.delete(bindingId);
      });
    }
    return target;
  }

  /** Forget a binding's connection, so the next read finds it again. */
  function forget(bindingId: string) {
    targets.delete(bindingId);
  }

  /** Forward one browser machine read to the paired remote `bindingId`. */
  async function proxy(bindingId: string, method: string, path: string, signal?: AbortSignal): Promise<Response> {
    const route = remoteMachineRoute(method, path);
    if ('error' in route) return json({ error: route.error }, route.status);
    let target: MachineTarget;
    try {
      target = await targetFor(bindingId);
      await dependencies.requireCurrent(target.connection);
    } catch {
      forget(bindingId);
      return json({ error: 'Remote server is unavailable or its approval changed.' }, 503);
    }
    const credentials = target.connection.credentials;
    const token = credentials.bearer_secret.startsWith('session:')
      ? credentials.bearer_secret.slice('session:'.length)
      : undefined;
    if (!token) return json({ error: 'Remote session approval is required.' }, 401);
    const serverPath = remoteMachineServerPath(route.route, route.search, target.checkoutId);
    if (!serverPath)
      return json({ error: 'This remote server has no project this computer can read its capacity through.' }, 409);
    try {
      const response = await dependencies.request(
        new URL(serverPath, target.connection.binding.origin),
        {
          method: 'GET',
          headers: { accept: 'application/json', cookie: `hs2_remote_session=${token}` },
          redirect: 'manual',
          ...(signal ? { signal } : {}),
        },
        credentials,
      );
      // A refused session or a checkout that went away is found again on the next read.
      if ([401, 403, 404].includes(response.status)) forget(bindingId);
      const headers = new Headers();
      const type = response.headers.get('content-type');
      if (type) headers.set('content-type', type);
      return new Response(response.body, { status: response.status, headers });
    } catch (error) {
      if (signal?.aborted) throw error;
      forget(bindingId);
      return json({ error: 'Remote server is unavailable or its approval changed.' }, 503);
    }
  }

  return { proxy, forget };
}

export type RemoteMachineBridge = ReturnType<typeof createRemoteMachineBridge>;
