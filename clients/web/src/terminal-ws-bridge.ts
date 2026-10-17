import type { IncomingMessage } from 'node:http';
import type { Duplex } from 'node:stream';

import WebSocket, { WebSocketServer } from 'ws';

import { CHANGE_WS_MUX_PATH, type ChangeWsCommand, type ChangeWsMessage, parseChangeWsCommand } from './change-ws-mux';
import { projectChangeStoreIds, projectChangeWebSocketUrl, projectTerminalWebSocketUrl } from './project-bridge';

const browserAttach = /^\/__hotsheet\/project-api\/([^/]+)\/terminals\/([^/]+)\/attach$/;
const browserSync = /^\/__hotsheet\/project-api\/([^/]+)\/ws\/sync$/;
type TargetResolver = (projectId: string, terminalId: string) => string | undefined | Promise<string | undefined>;
type SyncTargetResolver = (projectId: string, client?: string) => string | undefined | Promise<string | undefined>;
type StoreResolver = (projectId: string) => string[] | undefined;

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

function serveChangeMux(browser: WebSocket, resolveSync: SyncTargetResolver, resolveStores: StoreResolver): void {
  type Subscription = {
    client: string;
    project: string;
    subscription: number;
    target: string;
    stores?: string[];
    acknowledged: boolean;
  };
  type Group = {
    base: string;
    members: Map<string, Subscription>;
    active?: WebSocket;
    candidate?: WebSocket;
    activeProjects: string;
    legacy: boolean;
    direct: Map<string, WebSocket>;
  };
  const subscriptions = new Map<string, Subscription>();
  const pending = new Map<string, { token: symbol; subscription: number }>();
  const groups = new Map<string, Group>();
  const keyOf = (client: string, project: string) => `${client}\u0000${project}`;
  const send = (message: ChangeWsMessage) => {
    if (browser.readyState === WebSocket.OPEN) browser.send(JSON.stringify(message));
  };
  const groupBase = (target: string, client: string): string => {
    const url = new URL(target);
    url.searchParams.delete('checkout');
    return `${client}\n${url.toString()}`;
  };
  const ack = (member: Subscription) => {
    if (member.acknowledged) return;
    member.acknowledged = true;
    send({
      version: 2,
      type: 'subscribed',
      client: member.client,
      project: member.project,
      subscription: member.subscription,
    });
  };
  const deliver = (group: Group, raw: WebSocket.RawData) => {
    const data = webSocketRawText(raw);
    let store: string | undefined;
    try {
      const event = JSON.parse(data) as { store?: unknown };
      if (typeof event.store === 'string') store = event.store;
    } catch {
      // An older upstream frame has no routing metadata; preserve its broad delivery.
    }
    const members = [...group.members.values()];
    const matched = store ? members.filter((member) => member.stores?.includes(store)) : [];
    // Unknown or newly linked stores keep the old machine-wide delivery semantics.
    for (const member of matched.length ? matched : members)
      if (member.acknowledged)
        send({
          version: 2,
          type: 'event',
          client: member.client,
          project: member.project,
          subscription: member.subscription,
          data,
        });
  };
  const reconcile = (group: Group) => {
    group.candidate?.close();
    group.candidate = undefined;
    if (group.members.size === 0) {
      group.active?.close();
      for (const socket of group.direct.values()) socket.close();
      groups.delete(group.base);
      return;
    }
    if (group.legacy) {
      let waiting = 0;
      for (const member of group.members.values()) {
        const key = keyOf(member.client, member.project);
        if (group.direct.has(key)) continue;
        waiting += 1;
        const socket = new WebSocket(member.target);
        group.direct.set(key, socket);
        socket.on('open', () => {
          if (group.direct.get(key) !== socket) return;
          ack(member);
          waiting -= 1;
          if (waiting === 0) {
            group.active?.close();
            group.active = undefined;
          }
        });
        socket.on('message', (data) => {
          if (group.direct.get(key) === socket && member.acknowledged)
            send({
              version: 2,
              type: 'event',
              client: member.client,
              project: member.project,
              subscription: member.subscription,
              data: webSocketRawText(data),
            });
        });
        const closed = () => {
          if (group.direct.get(key) !== socket) return;
          group.direct.delete(key);
          if (subscriptions.get(key) === member) {
            group.members.delete(key);
            subscriptions.delete(key);
            send({
              version: 2,
              type: 'closed',
              client: member.client,
              project: member.project,
              subscription: member.subscription,
            });
          }
        };
        socket.on('close', closed);
        socket.on('error', closed);
      }
      if (waiting === 0) {
        group.active?.close();
        group.active = undefined;
      }
      return;
    }
    const members = [...group.members.values()];
    const projects = members
      .map((member) => member.project)
      .sort()
      .join('\u0000');
    if (group.active?.readyState === WebSocket.OPEN && group.activeProjects === projects) {
      for (const member of members) ack(member);
      return;
    }
    const url = new URL(members[0].target);
    url.searchParams.delete('checkout');
    for (const member of members) {
      const checkout = new URL(member.target).searchParams.get('checkout');
      if (checkout) url.searchParams.append('checkout', checkout);
    }
    const candidate = new WebSocket(url.toString());
    group.candidate = candidate;
    candidate.on('open', () => {
      if (group.candidate !== candidate) return;
      const previous = group.active;
      group.active = candidate;
      group.activeProjects = projects;
      group.candidate = undefined;
      for (const member of group.members.values()) ack(member);
      previous?.close();
    });
    candidate.on('message', (data) => {
      if (group.active === candidate) deliver(group, data);
    });
    const closed = () => {
      if (group.candidate === candidate) {
        group.candidate = undefined;
        if (members.length > 1) {
          // A pre-multi-checkout server rejects duplicate checkout parameters.
          group.legacy = true;
          reconcile(group);
        } else {
          const member = members[0];
          const key = keyOf(member.client, member.project);
          if (group.members.get(key) === member) {
            group.members.delete(key);
            subscriptions.delete(key);
            send({
              version: 2,
              type: 'closed',
              client: member.client,
              project: member.project,
              subscription: member.subscription,
            });
          }
        }
      } else if (group.active === candidate) {
        group.active = undefined;
        group.candidate?.close();
        group.candidate = undefined;
        for (const member of group.members.values()) {
          subscriptions.delete(keyOf(member.client, member.project));
          send({
            version: 2,
            type: 'closed',
            client: member.client,
            project: member.project,
            subscription: member.subscription,
          });
        }
        group.members.clear();
        groups.delete(group.base);
      }
    };
    candidate.on('close', closed);
    candidate.on('error', closed);
  };
  const remove = (member: Subscription) => {
    const group = groups.get(groupBase(member.target, member.client));
    const key = keyOf(member.client, member.project);
    if (!group || group.members.get(key) !== member) return;
    group.members.delete(key);
    subscriptions.delete(key);
    group.direct.get(key)?.close();
    group.direct.delete(key);
    reconcile(group);
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
    const { project, client } = command;
    const key = keyOf(client, project);
    if (command.type === 'unsubscribe') {
      if (pending.get(key)?.subscription === command.subscription) pending.delete(key);
      if (subscriptions.get(key)?.subscription === command.subscription) {
        remove(subscriptions.get(key)!);
      }
      return;
    }
    const existing = subscriptions.get(key);
    if (existing) {
      if (existing.subscription === command.subscription) {
        if (existing.acknowledged)
          send({ version: 2, type: 'subscribed', client, project, subscription: command.subscription });
        return;
      }
      remove(existing);
    }
    if (pending.get(key)?.subscription === command.subscription) return;
    const token = Symbol(project);
    pending.set(key, { token, subscription: command.subscription });
    void Promise.resolve()
      .then(() => resolveSync(project, command.client))
      .then((target) => {
        if (pending.get(key)?.token !== token) return;
        pending.delete(key);
        if (!target || browser.readyState !== WebSocket.OPEN) {
          send({ version: 2, type: 'closed', client, project, subscription: command.subscription });
          return;
        }
        const member: Subscription = {
          client,
          project,
          subscription: command.subscription,
          target,
          stores: resolveStores(project),
          acknowledged: false,
        };
        subscriptions.set(key, member);
        const base = groupBase(target, client);
        let group = groups.get(base);
        if (!group) {
          group = { base, members: new Map(), activeProjects: '', legacy: false, direct: new Map() };
          groups.set(base, group);
        }
        group.members.set(key, member);
        reconcile(group);
      })
      .catch(() => {
        if (pending.get(key)?.token !== token) return;
        pending.delete(key);
        send({ version: 2, type: 'closed', client, project, subscription: command.subscription });
      });
  });
  browser.on('close', () => {
    pending.clear();
    for (const group of groups.values()) {
      group.active?.close();
      group.candidate?.close();
      for (const socket of group.direct.values()) socket.close();
      group.active = undefined;
      group.candidate = undefined;
      group.members.clear();
      group.direct.clear();
    }
    subscriptions.clear();
    groups.clear();
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
  resolveStores: StoreResolver = projectChangeStoreIds,
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
        serveChangeMux(browser, resolveSync, resolveStores);
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
