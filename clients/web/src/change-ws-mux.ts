/** One bridge WebSocket carries live changes for all project/client pairs (HS2-QJR1K6, HS2-NP7R42). */
export const CHANGE_WS_MUX_PATH = '/__hotsheet/change-mux/ws';
export const CHANGE_WS_MUX_VERSION = 2;

export type ChangeWsCommand = {
  version: 2;
  type: 'subscribe' | 'unsubscribe';
  project: string;
  client: string;
  subscription: number;
};
export type ChangeWsMessage =
  | { version: 2; type: 'subscribed' | 'closed'; client: string; project: string; subscription: number }
  | { version: 2; type: 'event'; client: string; project: string; subscription: number; data: string };

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isSubscription = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) > 0;

export function parseChangeWsCommand(value: unknown): ChangeWsCommand | undefined {
  if (!isRecord(value) || value.version !== CHANGE_WS_MUX_VERSION) return undefined;
  if (value.type !== 'subscribe' && value.type !== 'unsubscribe') return undefined;
  if (typeof value.project !== 'string' || !value.project || typeof value.client !== 'string') return undefined;
  if (!isSubscription(value.subscription)) return undefined;
  return {
    version: 2,
    type: value.type,
    project: value.project,
    client: value.client,
    subscription: value.subscription,
  };
}

export function parseChangeWsMessage(value: unknown): ChangeWsMessage | undefined {
  if (!isRecord(value) || value.version !== CHANGE_WS_MUX_VERSION) return undefined;
  if (typeof value.project !== 'string' || !value.project || typeof value.client !== 'string') return undefined;
  if (!isSubscription(value.subscription)) return undefined;
  if (value.type === 'subscribed' || value.type === 'closed')
    return {
      version: 2,
      type: value.type,
      client: value.client,
      project: value.project,
      subscription: value.subscription,
    };
  if (value.type === 'event' && typeof value.data === 'string')
    return {
      version: 2,
      type: 'event',
      client: value.client,
      project: value.project,
      subscription: value.subscription,
      data: value.data,
    };
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
    readonly client: string,
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

  private key(client: string, project: string): string {
    return `${client}\u0000${project}`;
  }

  open(project: string, directUrl: string, client = this.client): WebSocket {
    const key = this.key(client, project);
    const old = this.projects.get(key);
    old?.close();
    const virtual = new ProjectSocket(client, project, directUrl, this.nextSubscription++, this);
    this.projects.set(key, virtual);
    if (this.unavailable) virtual.useDirect(this.openSocket);
    else if (this.bridge?.readyState === 1) this.send('subscribe', virtual);
    else this.connect();
    return virtual as unknown as WebSocket;
  }

  remove(socket: ProjectSocket): void {
    const key = this.key(socket.client, socket.project);
    if (this.projects.get(key) !== socket) return;
    this.projects.delete(key);
    if (this.bridge?.readyState === 1) this.send('unsubscribe', socket);
    if (!this.projects.size) {
      this.bridge?.close();
      this.bridge = undefined;
    }
  }

  private send(type: ChangeWsCommand['type'], socket: ProjectSocket): void {
    this.bridge?.send(
      JSON.stringify({
        version: 2,
        type,
        project: socket.project,
        client: socket.client,
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
    let bridgeOpened = false,
      contractSeen = false;
    const openTimer = globalThis.setTimeout(() => {
      if (this.bridge === bridge && bridge.readyState === 0) this.fallback();
    }, this.openTimeoutMs);
    bridge.addEventListener('open', () => {
      globalThis.clearTimeout(openTimer);
      if (this.bridge !== bridge) return;
      bridgeOpened = true;
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
      contractSeen = true;
      this.supported = true;
      const socket = this.projects.get(this.key(message.client, message.project));
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
      if (!bridgeOpened || !contractSeen) this.fallback();
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
    const key = url;
    let mux = byEndpoint.get(key);
    if (!mux) {
      mux = new ChangeWebSocketMultiplexer(url, client, openSocket);
      byEndpoint.set(key, mux);
    }
    return mux;
  };
}
