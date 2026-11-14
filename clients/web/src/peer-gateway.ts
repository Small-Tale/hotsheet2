/**
 * HS2-H4REBS: the peer gateway. A remote-mode browser host is often reachable only through an
 * HTTPS proxy such as Tailscale Serve, which exposes this host and not its machine server. Another
 * Hot Sheet host that paired with this one speaks the machine server's native protocol, so the
 * gateway passes those native routes through unchanged.
 *
 * The gateway adds no authority. It never injects this host's machine secret, and it removes any
 * secret a caller presents (the machine server would otherwise accept it as a loopback bearer).
 * The machine server authenticates the caller's own paired session, Origin and CSRF exactly as on
 * a direct connection, the same as `/__hotsheet/project-api/*` already forwards for a browser.
 */
import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';

import type { MiddlewareHandler } from 'hono';
import { WebSocket, WebSocketServer } from 'ws';

import { bootstrapServerUrl, remoteBrowserAuthRequired } from './project-bridge';

/** The machine server's top-level routes (`crates/hotsheet-server`). The app routes by query
 * string, so none collides with an application route; a policy test pins this list to the router. */
export const PEER_GATEWAY_PREFIXES = [
  'accounts',
  'activity',
  'ai-settings',
  'ai-tools',
  'analytics',
  'announce',
  'batch',
  'checkouts',
  'claim-next',
  'command-groups',
  'command-runs',
  'commands',
  'compatibility',
  'confidence-report',
  'connections',
  'drive',
  'github-auth',
  'groups',
  'health',
  'host-health',
  'lifecycle',
  'live-history',
  'live-state',
  'mcp-presence',
  'notification-channels',
  'notification-config',
  'notification-deliveries',
  'notification-link-base',
  'notification-project-timing',
  'notification-rules',
  'notifications',
  'notifiers',
  'permissions',
  'phase-report',
  'projects',
  'provider-attachments',
  'provider-connections',
  'provider-transfers',
  'providers',
  'remote-auth',
  'remote-checkouts',
  'remote-identity',
  'remotes',
  'resolve',
  'setup',
  'standby-settings',
  'stores',
  'terminal-limits',
  'terminal-settings',
  'terminals',
  'tickets',
  'tts',
  'views',
  'webhooks',
  'ws',
] as const;

const PREFIXES = new Set<string>(PEER_GATEWAY_PREFIXES);
const STRIPPED_HEADERS = ['host', 'x-hotsheet-secret', 'authorization', 'connection', 'upgrade', 'content-length'];

export function isPeerGatewayPath(pathname: string): boolean {
  const first = /^\/([^/?#]+)/.exec(pathname)?.[1];
  return first !== undefined && PREFIXES.has(first);
}

/** The upstream URL for a gateway request, with any presented machine secret removed. */
export function peerGatewayUrl(serverUrl: string, requestUrl: string): URL {
  const incoming = new URL(requestUrl, 'http://gateway.invalid');
  const url = new URL(incoming.pathname, serverUrl);
  for (const [name, value] of incoming.searchParams) if (name !== 'secret') url.searchParams.append(name, value);
  return url;
}

/** The caller's own headers minus anything that could claim this host's machine authority. */
export function peerGatewayHeaders(headers: Headers): Headers {
  const forwarded = new Headers(headers);
  for (const name of STRIPPED_HEADERS) forwarded.delete(name);
  return forwarded;
}

export type ServerUrlResolver = () => Promise<string>;

/** HTTP half of the gateway; mounted after `/__hotsheet` and before the static application. */
export function peerGateway(
  resolveServer: ServerUrlResolver = bootstrapServerUrl,
  forward: typeof fetch = fetch,
): MiddlewareHandler {
  return async (context, next) => {
    if (!remoteBrowserAuthRequired() || !isPeerGatewayPath(context.req.path)) return next();
    let serverUrl: string;
    try {
      serverUrl = await resolveServer();
    } catch {
      return context.json({ error: 'The Hot Sheet server is unavailable.' }, 503);
    }
    const request = context.req.raw;
    const bodyless = request.method === 'GET' || request.method === 'HEAD';
    try {
      const response = await forward(peerGatewayUrl(serverUrl, request.url), {
        method: request.method,
        headers: peerGatewayHeaders(request.headers),
        body: bodyless ? undefined : await request.arrayBuffer(),
        redirect: 'manual',
        signal: request.signal,
      });
      return new Response(response.body, { status: response.status, headers: response.headers });
    } catch {
      return context.json({ error: 'The Hot Sheet server is unavailable.' }, 503);
    }
  };
}

/** Headers a gateway WebSocket carries upstream: the caller's session, Origin and CSRF only. */
export function peerGatewaySocketHeaders(request: Pick<IncomingMessage, 'headers'>): Record<string, string> {
  const headers: Record<string, string> = {};
  for (const name of ['cookie', 'origin', 'x-hotsheet-csrf', 'user-agent']) {
    const value = request.headers[name];
    if (typeof value === 'string') headers[name] = value;
  }
  return headers;
}

/** WebSocket half of the gateway (`/ws/*`, terminal attach) for the production local host. */
export function installPeerWebSocketGateway(
  server: {
    on(event: 'upgrade', listener: (request: IncomingMessage, socket: Duplex, head: Buffer) => void): unknown;
  },
  resolveServer: ServerUrlResolver = bootstrapServerUrl,
): void {
  const gateway = new WebSocketServer({ noServer: true });
  server.on('upgrade', (request, socket, head) => {
    const pathname = new URL(request.url ?? '/', 'http://gateway.invalid').pathname;
    if (!remoteBrowserAuthRequired() || !isPeerGatewayPath(pathname)) return;
    socket.on('error', () => {
      socket.destroy();
    });
    void resolveServer()
      .then((serverUrl) => {
        if (socket.destroyed) return;
        const target = peerGatewayUrl(serverUrl, request.url ?? '/');
        target.protocol = target.protocol === 'https:' ? 'wss:' : 'ws:';
        gateway.handleUpgrade(request, socket, head, (caller) => {
          pipeSockets(caller, target, request);
        });
      })
      .catch(() => socket.destroy());
  });
}

function pipeSockets(caller: WebSocket, target: URL, request: IncomingMessage): void {
  const upstream = new WebSocket(target, { headers: peerGatewaySocketHeaders(request) });
  const pending: Array<{ data: WebSocket.RawData; binary: boolean }> = [];
  caller.on('message', (data, binary) => {
    if (upstream.readyState === WebSocket.OPEN) upstream.send(data, { binary });
    else pending.push({ data, binary });
  });
  upstream.on('open', () => {
    for (const frame of pending.splice(0)) upstream.send(frame.data, { binary: frame.binary });
  });
  upstream.on('message', (data, binary) => {
    if (caller.readyState === WebSocket.OPEN) caller.send(data, { binary });
  });
  const closeCaller = () => {
    if (caller.readyState === WebSocket.OPEN) caller.close();
  };
  const closeUpstream = () => {
    if (upstream.readyState === WebSocket.OPEN || upstream.readyState === WebSocket.CONNECTING) upstream.close();
  };
  upstream.on('close', closeCaller);
  upstream.on('error', closeCaller);
  caller.on('close', closeUpstream);
  caller.on('error', closeUpstream);
}
