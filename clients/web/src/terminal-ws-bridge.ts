import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';

import WebSocket, { WebSocketServer } from 'ws';

import { projectChangeWebSocketUrl, projectTerminalWebSocketUrl } from './project-bridge';

const browserAttach = /^\/__hotsheet\/project-api\/([^/]+)\/terminals\/([^/]+)\/attach$/;
const browserSync = /^\/__hotsheet\/project-api\/([^/]+)\/ws\/sync$/;
type TargetResolver = (projectId: string, terminalId: string) => string | undefined | Promise<string | undefined>;
type SyncTargetResolver = (projectId: string, client?: string) => string | undefined | Promise<string | undefined>;

export async function browserTerminalAttachTarget(
  requestUrl: string | undefined,
  resolveTarget: TargetResolver = projectTerminalWebSocketUrl,
): Promise<string | undefined> {
  if (!requestUrl) return undefined;
  const match = new URL(requestUrl, 'http://localhost').pathname.match(browserAttach);
  if (!match) return undefined;
  try {
    return await resolveTarget(decodeURIComponent(match[1]), decodeURIComponent(match[2]));
  } catch {
    return undefined;
  }
}

export async function browserProjectWebSocketTarget(
  requestUrl: string | undefined,
  resolveTerminal: TargetResolver = projectTerminalWebSocketUrl,
  resolveSync: SyncTargetResolver = projectChangeWebSocketUrl,
): Promise<string | undefined> {
  if (!requestUrl) return undefined;
  const url = new URL(requestUrl, 'http://localhost'),
    sync = url.pathname.match(browserSync);
  if (sync) {
    try {
      return await resolveSync(decodeURIComponent(sync[1]), url.searchParams.get('client') ?? undefined);
    } catch {
      return undefined;
    }
  }
  return browserTerminalAttachTarget(requestUrl, resolveTerminal);
}

/** Whether an upgrade request names a Hot Sheet project WebSocket this bridge serves. */
export function isProjectWebSocketPath(requestUrl: string | undefined): boolean {
  if (!requestUrl) return false;
  const { pathname } = new URL(requestUrl, 'http://localhost');
  return browserSync.test(pathname) || browserAttach.test(pathname);
}

function refuseUpgrade(socket: Duplex): void {
  socket.end('HTTP/1.1 404 Not Found\r\nConnection: close\r\nContent-Length: 0\r\n\r\n');
}

export function installProjectWebSocketBridge(
  server: {
    httpServer?: {
      on(event: 'upgrade', listener: (request: IncomingMessage, socket: Duplex, head: Buffer) => void): unknown;
    } | null;
  },
  resolveTerminal: TargetResolver = projectTerminalWebSocketUrl,
  resolveSync: SyncTargetResolver = projectChangeWebSocketUrl,
): void {
  const browserServer = new WebSocketServer({ noServer: true });
  server.httpServer?.on('upgrade', (request, socket, head) => {
    // Another upgrade handler (Vite's HMR socket) owns every other path.
    if (!isProjectWebSocketPath(request.url)) return;
    // Node's HTTP server drops its own socket error handler when it hands an upgrade off. A browser
    // that resets the connection while the target is still resolving (a test closing its page, a
    // tab reload) then emitted an unhandled 'error' that crashed the whole dev server, failing every
    // later browser test with ERR_CONNECTION_REFUSED (HS2-D95TXJ). Own the socket's errors from here.
    socket.on('error', () => {
      socket.destroy();
    });
    void browserProjectWebSocketTarget(request.url, resolveTerminal, resolveSync)
      .then((target) => {
        if (socket.destroyed) return;
        if (!target) {
          // A Hot Sheet path the bridge cannot resolve (no open session, unreachable server) used to
          // be left pending forever, so the browser waited out its handshake timeout on each retry;
          // refuse it at once instead (HS2-CNWHW3).
          refuseUpgrade(socket);
          return;
        }
        browserServer.handleUpgrade(request, socket, head, (browser) => {
          const upstream = new WebSocket(target),
            pending: Array<{ data: WebSocket.RawData; binary: boolean }> = [];
          browser.on('message', (data, binary) => {
            if (upstream.readyState === WebSocket.OPEN) upstream.send(data, { binary });
            else pending.push({ data, binary });
          });
          upstream.on('open', () => {
            for (const frame of pending.splice(0)) upstream.send(frame.data, { binary: frame.binary });
          });
          upstream.on('message', (data, binary) => {
            if (browser.readyState === WebSocket.OPEN) browser.send(data, { binary });
          });
          const closeBrowser = () => {
            if (browser.readyState === WebSocket.OPEN) browser.close();
          };
          const closeUpstream = () => {
            if (upstream.readyState === WebSocket.OPEN || upstream.readyState === WebSocket.CONNECTING)
              upstream.close();
          };
          upstream.on('close', closeBrowser);
          upstream.on('error', closeBrowser);
          browser.on('close', closeUpstream);
          browser.on('error', closeUpstream);
        });
      })
      .catch(() => {
        socket.destroy();
      });
  });
}

/** Backward-compatible adapter for existing callers that only customize terminal resolution. */
export function installTerminalWebSocketBridge(
  server: Parameters<typeof installProjectWebSocketBridge>[0],
  resolveTarget: TargetResolver = projectTerminalWebSocketUrl,
): void {
  installProjectWebSocketBridge(server, resolveTarget);
}
