import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';

import WebSocket, { WebSocketServer } from 'ws';

import { CHANGE_WS_MUX_PATH, type ChangeWsCommand, type ChangeWsMessage, parseChangeWsCommand } from './change-ws-mux';
import { projectChangeWebSocketUrl, projectTerminalWebSocketUrl } from './project-bridge';

const browserAttach = /^\/__hotsheet\/project-api\/([^/]+)\/terminals\/([^/]+)\/attach$/;
const browserSync = /^\/__hotsheet\/project-api\/([^/]+)\/ws\/sync$/;
type TargetResolver = (projectId: string, terminalId: string) => string | undefined | Promise<string | undefined>;
type SyncTargetResolver = (projectId: string, client?: string) => string | undefined | Promise<string | undefined>;

export function webSocketRawText(data: WebSocket.RawData): string {
  if (Array.isArray(data)) return Buffer.concat(data).toString('utf8');
  if (Buffer.isBuffer(data)) return data.toString('utf8');
  return Buffer.from(data).toString('utf8');
}

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
  return pathname === CHANGE_WS_MUX_PATH || browserSync.test(pathname) || browserAttach.test(pathname);
}

function serveChangeMux(browser: WebSocket, resolveSync: SyncTargetResolver): void {
  const subscriptions = new Map<string, { socket: WebSocket; subscription: number }>();
  const pending = new Map<string, { token: symbol; subscription: number }>();
  const send = (message: ChangeWsMessage) => {
    if (browser.readyState === WebSocket.OPEN) browser.send(JSON.stringify(message));
  };
  browser.on('message', (raw) => {
    let command: ChangeWsCommand | undefined;
    try {
      const text = webSocketRawText(raw);
      command = parseChangeWsCommand(JSON.parse(text) as unknown);
    } catch {
      // The protocol is versioned and malformed frames never reach an upstream server.
    }
    if (!command) {
      browser.close(1008, 'Invalid change mux command');
      return;
    }
    const { project } = command;
    if (command.type === 'unsubscribe') {
      if (pending.get(project)?.subscription === command.subscription) pending.delete(project);
      if (subscriptions.get(project)?.subscription === command.subscription) {
        subscriptions.get(project)?.socket.close();
        subscriptions.delete(project);
      }
      return;
    }
    const existing = subscriptions.get(project);
    if (existing) {
      if (existing.subscription === command.subscription) {
        if (existing.socket.readyState === WebSocket.OPEN)
          send({ version: 1, type: 'subscribed', project, subscription: command.subscription });
        return;
      }
      existing.socket.close();
      subscriptions.delete(project);
    }
    if (pending.get(project)?.subscription === command.subscription) return;
    const token = Symbol(project);
    pending.set(project, { token, subscription: command.subscription });
    void Promise.resolve()
      .then(() => resolveSync(project, command.client))
      .then((target) => {
        if (pending.get(project)?.token !== token) return;
        pending.delete(project);
        if (!target || browser.readyState !== WebSocket.OPEN) {
          send({ version: 1, type: 'closed', project, subscription: command.subscription });
          return;
        }
        const upstream = new WebSocket(target);
        subscriptions.set(project, { socket: upstream, subscription: command.subscription });
        upstream.on('open', () => {
          send({ version: 1, type: 'subscribed', project, subscription: command.subscription });
        });
        upstream.on('message', (data) => {
          send({
            version: 1,
            type: 'event',
            project,
            subscription: command.subscription,
            data: webSocketRawText(data),
          });
        });
        const closed = () => {
          if (subscriptions.get(project)?.socket !== upstream) return;
          subscriptions.delete(project);
          send({ version: 1, type: 'closed', project, subscription: command.subscription });
        };
        upstream.on('close', closed);
        upstream.on('error', closed);
      })
      .catch(() => {
        if (pending.get(project)?.token !== token) return;
        pending.delete(project);
        send({ version: 1, type: 'closed', project, subscription: command.subscription });
      });
  });
  browser.on('close', () => {
    pending.clear();
    for (const upstream of subscriptions.values()) upstream.socket.close();
    subscriptions.clear();
  });
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
    if (new URL(request.url ?? '/', 'http://localhost').pathname === CHANGE_WS_MUX_PATH) {
      browserServer.handleUpgrade(request, socket, head, (browser) => {
        serveChangeMux(browser, resolveSync);
      });
      return;
    }
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
