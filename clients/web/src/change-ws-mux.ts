/** One bridge WebSocket carries live changes for all projects in a tab (HS2-QJR1K6). */
export const CHANGE_WS_MUX_PATH = '/__hotsheet/change-mux/ws';
export const CHANGE_WS_MUX_VERSION = 1;

export type ChangeWsCommand = {
  version: 1;
  type: 'subscribe' | 'unsubscribe';
  project: string;
  client: string;
  subscription: number;
};
export type ChangeWsMessage =
  | { version: 1; type: 'subscribed' | 'closed'; project: string; subscription: number }
  | { version: 1; type: 'event'; project: string; subscription: number; data: string };

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isSubscription = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) > 0;

export function parseChangeWsCommand(value: unknown): ChangeWsCommand | undefined {
  if (!isRecord(value) || value.version !== CHANGE_WS_MUX_VERSION) return undefined;
  if (value.type !== 'subscribe' && value.type !== 'unsubscribe') return undefined;
  if (typeof value.project !== 'string' || !value.project || typeof value.client !== 'string') return undefined;
  if (!isSubscription(value.subscription)) return undefined;
  return {
    version: 1,
    type: value.type,
    project: value.project,
    client: value.client,
    subscription: value.subscription,
  };
}

export function parseChangeWsMessage(value: unknown): ChangeWsMessage | undefined {
  if (!isRecord(value) || value.version !== CHANGE_WS_MUX_VERSION) return undefined;
  if (typeof value.project !== 'string' || !value.project) return undefined;
  if (!isSubscription(value.subscription)) return undefined;
  if (value.type === 'subscribed' || value.type === 'closed')
    return { version: 1, type: value.type, project: value.project, subscription: value.subscription };
  if (value.type === 'event' && typeof value.data === 'string')
    return { version: 1, type: 'event', project: value.project, subscription: value.subscription, data: value.data };
  return undefined;
}

export function changeWsMuxUrl(pollMuxUrl: string): string {
  const url = new URL(CHANGE_WS_MUX_PATH, pollMuxUrl);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.href;
}

type OpenSocket = (url: string) => WebSocket;

/** A project-local socket facade for the existing cursor/replay transport. */
class ProjectSocket extends EventTarget {
  readyState = 0;
  private direct: WebSocket | undefined;
  constructor(
    readonly project: string,
    readonly directUrl: string,
    readonly subscription: number,
    private readonly owner: ChangeWebSocketMultiplexer,
  ) {
    super();
  }

  opened(): void {
    if (this.readyState !== 0) return;
    this.readyState = 1;
    this.dispatchEvent(new Event('open'));
  }

  message(data: string): void {
    if (this.readyState !== 1) return;
    this.dispatchEvent(new MessageEvent('message', { data }));
  }

  ended(): void {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.dispatchEvent(new Event('close'));
  }

  useDirect(openSocket: OpenSocket): void {
    if (this.readyState === 3 || this.direct) return;
    try {
      const direct = openSocket(this.directUrl);
      this.direct = direct;
      direct.addEventListener('open', () => {
        this.opened();
      });
      direct.addEventListener('message', (event) => {
        this.message(String(event.data));
      });
      direct.addEventListener('error', () => this.dispatchEvent(new Event('error')));
      direct.addEventListener('close', () => {
        this.ended();
      });
    } catch {
      this.dispatchEvent(new Event('error'));
      this.ended();
    }
  }

  close(): void {
    if (this.readyState !== 3) this.direct?.close();
    this.owner.remove(this);
    this.ended();
  }
}

/** Shared connection state. Reconnecting projects keep their own cursor in the transport. */
export class ChangeWebSocketMultiplexer {
  private bridge: WebSocket | undefined;
  private readonly projects = new Map<string, ProjectSocket>();
  private supported = false;
  private unavailable = false;
  private nextSubscription = 1;

  constructor(
    private readonly url: string,
    private readonly client: string,
    private readonly openSocket: OpenSocket = (target) => new WebSocket(target),
    private readonly openTimeoutMs = 1_500,
  ) {}

  open(project: string, directUrl: string): WebSocket {
    const old = this.projects.get(project);
    old?.close();
    const virtual = new ProjectSocket(project, directUrl, this.nextSubscription++, this);
    this.projects.set(project, virtual);
    if (this.unavailable) virtual.useDirect(this.openSocket);
    else if (this.bridge?.readyState === 1) this.send('subscribe', virtual);
    else this.connect();
    return virtual as unknown as WebSocket;
  }

  remove(socket: ProjectSocket): void {
    if (this.projects.get(socket.project) !== socket) return;
    this.projects.delete(socket.project);
    if (this.bridge?.readyState === 1) this.send('unsubscribe', socket);
    if (!this.projects.size) {
      this.bridge?.close();
      this.bridge = undefined;
    }
  }

  private send(type: ChangeWsCommand['type'], socket: ProjectSocket): void {
    this.bridge?.send(
      JSON.stringify({
        version: 1,
        type,
        project: socket.project,
        client: this.client,
        subscription: socket.subscription,
      } satisfies ChangeWsCommand),
    );
  }

  private connect(): void {
    if (this.bridge || !this.projects.size) return;
    let bridge: WebSocket;
    try {
      bridge = this.openSocket(this.url);
    } catch {
      this.fallback();
      return;
    }
    this.bridge = bridge;
    let bridgeOpened = false;
    const openTimer = globalThis.setTimeout(() => {
      if (this.bridge === bridge && bridge.readyState === 0) this.fallback();
    }, this.openTimeoutMs);
    bridge.addEventListener('open', () => {
      globalThis.clearTimeout(openTimer);
      if (this.bridge !== bridge) return;
      bridgeOpened = true;
      this.supported = true;
      for (const socket of this.projects.values()) this.send('subscribe', socket);
    });
    bridge.addEventListener('message', (event) => {
      if (this.bridge !== bridge) return;
      let message: ChangeWsMessage | undefined;
      try {
        message = parseChangeWsMessage(JSON.parse(String(event.data)));
      } catch {
        // A malformed frame is a failed bridge contract, not a project event.
      }
      if (!message) {
        this.fallback();
        return;
      }
      const socket = this.projects.get(message.project);
      if (socket?.subscription !== message.subscription) return;
      if (message.type === 'subscribed') socket.opened();
      else if (message.type === 'event') socket.message(message.data);
      else socket.ended();
    });
    bridge.addEventListener('error', () => {
      // The close event completes the transition, including an unsupported endpoint.
    });
    bridge.addEventListener('close', () => {
      globalThis.clearTimeout(openTimer);
      if (this.bridge !== bridge) return;
      this.bridge = undefined;
      if (!bridgeOpened) this.fallback();
      else {
        const sockets = [...this.projects.values()];
        this.projects.clear();
        for (const socket of sockets) socket.ended();
      }
    });
  }

  private fallback(): void {
    this.unavailable = true;
    const bridge = this.bridge;
    this.bridge = undefined;
    bridge?.close();
    if (this.supported) {
      const sockets = [...this.projects.values()];
      this.projects.clear();
      for (const socket of sockets) socket.ended();
    } else for (const socket of this.projects.values()) socket.useDirect(this.openSocket);
  }
}

export function createChangeWebSocketMultiplexers(openSocket?: OpenSocket) {
  const byEndpoint = new Map<string, ChangeWebSocketMultiplexer>();
  return (pollMuxUrl: string, client: string): ChangeWebSocketMultiplexer => {
    const url = changeWsMuxUrl(pollMuxUrl);
    const key = `${url}\n${client}`;
    let mux = byEndpoint.get(key);
    if (!mux) {
      mux = new ChangeWebSocketMultiplexer(url, client, openSocket);
      byEndpoint.set(key, mux);
    }
    return mux;
  };
}
