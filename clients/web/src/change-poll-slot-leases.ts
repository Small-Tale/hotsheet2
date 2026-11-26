/**
 * Direct-poll slot leases for contexts without Web Locks (HS2-QS2P6B).
 *
 * Plain-HTTP origins (an insecure context) have no `navigator.locks`, and some Web Locks
 * implementations refuse a context. Without a cross-context lock, the direct change polls
 * of every tab and worker still share the browser's per-origin HTTP/1.1 pool, so they keep
 * the same origin-wide slot cap (HS2-GDF4ZJ) by coordinating leases over a
 * `BroadcastChannel`, and a slot holder keeps a genuine long poll, never a fixed-interval
 * request loop.
 *
 * Protocol, per slot request:
 * - a context posts a tentative `claim` ranked by when it began waiting, lets it settle for
 *   `LEASE_SETTLE_MS`, then confirms it with a `hold` (carrying an absolute lease expiry) only
 *   if fewer than `capacity` confirmed holds or better-ranked claims are ahead of it, and
 *   otherwise withdraws it, so the oldest waiter wins a race;
 * - `release` frees a slot; a context that opens the channel sends `query`, and holders
 *   re-announce their holds;
 * - leases and tentative claims expire, so a context that vanishes cannot keep a slot;
 * - HS2-S3RHFP: a page that hides (closes, navigates away, or enters the back/forward
 *   cache) releases its leases at once, as Web Locks do, so peers need not wait for expiry;
 *   one restored from that cache forgets what it knew of others and queries again;
 * - a context that has to wait posts `waiting`, so holders yield after their minimum turn.
 * Waiting never touches the network: it wakes on a release, an expiry, or an abort.
 * Without any `BroadcastChannel` a context cannot see the others, so it holds one slot.
 */
export const LEASE_SETTLE_MS = 50;
/** The longest a leased poll holds its connection; the caller's default window is 25 s. */
export const LEASE_HOLD_MAX_MS = 30_000;
/** A confirmed lease outlives its poll by this much before other contexts reclaim it. */
export const LEASE_GRACE_MS = 5_000;
/** A tentative claim whose context never confirms or withdraws it stops blocking after this. */
export const LEASE_CLAIM_TTL_MS = 1_000;
/** A remote wait announcement marks grants contended for this long. */
export const LEASE_WAITING_TTL_MS = 2_000;
const CHANNEL_NAME = 'hotsheet-direct-change-poll-leases';

export type SlotLeaseMessage =
  | { type: 'query'; from: string }
  | { type: 'claim'; from: string; id: string; rank: number }
  | { type: 'hold'; from: string; id: string; rank: number; until: number }
  | { type: 'release'; from: string; id: string }
  | { type: 'waiting'; from: string };

/** Carries lease messages to the other contexts of the origin (never back to the sender). */
export interface SlotLeaseBus {
  post(message: SlotLeaseMessage): void;
  listen(listener: (message: SlotLeaseMessage) => void): () => void;
}

/** Page lifecycle events of this context; workers and tests have none. */
export interface PageLifecycle {
  /** `listener` runs when the page hides or shows again; returns an unsubscribe. */
  listen(listener: (event: { type: 'pagehide' | 'pageshow'; persisted: boolean }) => void): () => void;
}

export interface SlotLeasesOptions {
  /** `null` means this context cannot see any other, so it holds one slot. */
  bus: SlotLeaseBus | null;
  /** Slots shared by every context on the bus. */
  capacity: number;
  now?: () => number;
  /** This context's id; random by default. */
  contextId?: string;
  /** `null` ignores page lifecycle events; the default listens on the global page. */
  lifecycle?: PageLifecycle | null;
}

interface Claim {
  from: string;
  rank: number;
  confirmed: boolean;
  until: number;
}

export const abortedError = () => new DOMException('The change poll was aborted.', 'AbortError');

export function abortableDelay(milliseconds: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(abortedError());
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => {
        signal.removeEventListener('abort', onAbort);
        resolve();
      },
      Math.max(0, milliseconds),
    );
    const onAbort = () => {
      clearTimeout(timer);
      reject(abortedError());
    };
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

const isMessage = (value: unknown): value is SlotLeaseMessage => {
  if (typeof value !== 'object' || value === null) return false;
  const message = value as Record<string, unknown>;
  if (typeof message.from !== 'string') return false;
  switch (message.type) {
    case 'query':
    case 'waiting':
      return true;
    case 'release':
      return typeof message.id === 'string';
    case 'claim':
      return typeof message.id === 'string' && typeof message.rank === 'number';
    case 'hold':
      return typeof message.id === 'string' && typeof message.rank === 'number' && typeof message.until === 'number';
    default:
      return false;
  }
};

/** A lazily opened `BroadcastChannel` bus, or `null` where the context has none. */
export function broadcastLeaseBus(): SlotLeaseBus | null {
  const Channel = Reflect.get(globalThis, 'BroadcastChannel') as typeof BroadcastChannel | undefined;
  if (typeof Channel !== 'function') return null;
  let channel: BroadcastChannel | undefined;
  const listeners = new Set<(message: SlotLeaseMessage) => void>();
  const open = (): BroadcastChannel => {
    if (channel) return channel;
    const opened = new Channel(CHANNEL_NAME);
    opened.onmessage = (event: MessageEvent) => {
      if (!isMessage(event.data)) return;
      for (const listener of [...listeners]) listener(event.data);
    };
    // Node's channel would otherwise keep a test process alive; browsers have no `unref`.
    (opened as unknown as { unref?: () => void }).unref?.();
    channel = opened;
    return opened;
  };
  return {
    post: (message) => {
      open().postMessage(message);
    },
    listen: (listener) => {
      open();
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

/**
 * The global page's `pagehide`/`pageshow` events, or `null` where the context has none. Nothing
 * is registered until `listen` runs, which `SlotLeases` does only when its bus first opens.
 */
export function windowPageLifecycle(): PageLifecycle | null {
  const target = globalThis as Partial<Pick<Window, 'addEventListener' | 'removeEventListener'>>;
  if (typeof target.addEventListener !== 'function' || typeof target.removeEventListener !== 'function') return null;
  const add = target.addEventListener.bind(globalThis);
  const remove = target.removeEventListener.bind(globalThis);
  return {
    listen: (listener) => {
      const handle = (event: Event) => {
        listener({
          type: event.type === 'pageshow' ? 'pageshow' : 'pagehide',
          persisted: (event as Partial<PageTransitionEvent>).persisted ?? false,
        });
      };
      add('pagehide', handle);
      add('pageshow', handle);
      return () => {
        remove('pagehide', handle);
        remove('pageshow', handle);
      };
    },
  };
}

const randomId = () => `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

export class SlotLeases {
  readonly capacity: number;
  private readonly bus: SlotLeaseBus | null;
  private readonly lifecycle: PageLifecycle | null;
  private readonly now: () => number;
  private readonly self: string;
  private readonly claims = new Map<string, Claim>();
  private readonly remoteWaiting = new Map<string, number>();
  private readonly wakers = new Set<() => void>();
  private readonly contention = new Set<() => void>();
  private localWaiting = 0;
  private sequence = 0;
  private opened = false;

  constructor(options: SlotLeasesOptions) {
    this.bus = options.bus;
    this.capacity = options.bus ? Math.max(1, options.capacity) : 1;
    this.now = options.now ?? Date.now;
    this.self = options.contextId ?? randomId();
    this.lifecycle = options.lifecycle === undefined ? windowPageLifecycle() : options.lifecycle;
  }

  /** Run `listener` whenever a poll, here or in another context, starts waiting for a slot. */
  onContention(listener: () => void): () => void {
    this.contention.add(listener);
    return () => this.contention.delete(listener);
  }

  /** Slots this context currently holds (diagnostics and tests). */
  held(): number {
    let count = 0;
    for (const claim of this.claims.values()) if (claim.from === this.self && claim.confirmed) count += 1;
    return count;
  }

  /** Run `poll` while holding a slot; `contended` says whether another poll is waiting. */
  async run<T>(signal: AbortSignal, poll: (contended: boolean) => Promise<T>): Promise<T> {
    const id = await this.acquire(signal);
    try {
      return await poll(this.contended());
    } finally {
      this.release(id);
    }
  }

  private open() {
    if (this.opened || !this.bus) return;
    this.opened = true;
    this.bus.listen((message) => {
      this.receive(message);
    });
    this.bus.post({ type: 'query', from: this.self });
    // Registered here, on first use, never at import or construction (HS2-S3RHFP).
    this.lifecycle?.listen((event) => {
      if (event.type === 'pagehide') this.hide();
      else if (event.persisted) this.restore();
    });
  }

  /** The page is going away or into the back/forward cache: free every slot it holds. */
  private hide() {
    for (const [id, claim] of [...this.claims]) if (claim.from === this.self) this.release(id);
  }

  /**
   * Restored from the back/forward cache: releases may have been missed while frozen, so forget
   * other contexts' claims and ask holders to announce again. A settling claim absorbs replies.
   */
  private restore() {
    for (const [id, claim] of [...this.claims]) if (claim.from !== this.self) this.claims.delete(id);
    this.remoteWaiting.clear();
    this.bus?.post({ type: 'query', from: this.self });
    this.wake();
  }

  private receive(message: SlotLeaseMessage) {
    if (message.from === this.self) return;
    switch (message.type) {
      case 'query':
        for (const [id, claim] of this.claims)
          if (claim.from === this.self && claim.confirmed)
            this.bus?.post({ type: 'hold', from: this.self, id, rank: claim.rank, until: claim.until });
        return;
      case 'claim':
        this.claims.set(message.id, {
          from: message.from,
          rank: message.rank,
          confirmed: false,
          until: this.now() + LEASE_CLAIM_TTL_MS,
        });
        return;
      case 'hold':
        this.claims.set(message.id, { from: message.from, rank: message.rank, confirmed: true, until: message.until });
        return;
      case 'release':
        if (this.claims.delete(message.id)) this.wake();
        return;
      case 'waiting':
        this.remoteWaiting.set(message.from, this.now() + LEASE_WAITING_TTL_MS);
        this.notifyContention();
        return;
    }
  }

  private notifyContention() {
    for (const listener of [...this.contention]) listener();
  }

  private wake() {
    for (const waker of [...this.wakers]) waker();
  }

  /** Forget expired remote claims and wait announcements. */
  private prune() {
    const now = this.now();
    for (const [id, claim] of this.claims) if (claim.from !== this.self && claim.until <= now) this.claims.delete(id);
    for (const [from, until] of this.remoteWaiting) if (until <= now) this.remoteWaiting.delete(from);
  }

  /** Claims that take a slot before a claim of `rank` and `id`. */
  private ahead(rank: number, id: string): number {
    let count = 0;
    for (const [other, claim] of this.claims) {
      if (other === id) continue;
      if (claim.confirmed || claim.rank < rank || (claim.rank === rank && other < id)) count += 1;
    }
    return count;
  }

  private contended(): boolean {
    this.prune();
    return this.localWaiting > 0 || this.remoteWaiting.size > 0;
  }

  private async acquire(signal: AbortSignal): Promise<string> {
    this.open();
    const rank = this.now();
    let waiting = false;
    try {
      for (;;) {
        if (signal.aborted) throw abortedError();
        this.prune();
        const id = `${this.self}:${(this.sequence += 1).toString()}`;
        if (this.ahead(rank, id) < this.capacity) {
          this.claims.set(id, { from: this.self, rank, confirmed: false, until: Number.POSITIVE_INFINITY });
          if (this.bus) {
            this.bus.post({ type: 'claim', from: this.self, id, rank });
            try {
              await abortableDelay(LEASE_SETTLE_MS, signal);
            } catch (reason) {
              this.release(id);
              throw reason;
            }
            this.prune();
          }
          // A claim the page dropped while hiding (HS2-S3RHFP) is not held: try again.
          if (!this.claims.has(id)) continue;
          if (this.ahead(rank, id) < this.capacity) {
            this.confirm(id);
            return id;
          }
          this.release(id);
        }
        if (!waiting) {
          waiting = true;
          this.localWaiting += 1;
        }
        // Re-announce after every lost turn, so holders that started since still yield. This
        // runs only on a release, an expiry, or the first try: never on a timer of its own.
        this.bus?.post({ type: 'waiting', from: this.self });
        this.notifyContention();
        await this.changed(signal);
      }
    } finally {
      if (waiting) this.localWaiting -= 1;
    }
  }

  private confirm(id: string) {
    const claim = this.claims.get(id);
    if (!claim) return;
    claim.confirmed = true;
    claim.until = this.now() + LEASE_HOLD_MAX_MS + LEASE_GRACE_MS;
    this.bus?.post({ type: 'hold', from: this.self, id, rank: claim.rank, until: claim.until });
  }

  private release(id: string) {
    if (!this.claims.delete(id)) return;
    this.bus?.post({ type: 'release', from: this.self, id });
    this.wake();
  }

  /** Resolve when a slot may have freed: a release, the next expiry, or reject on abort. */
  private changed(signal: AbortSignal): Promise<void> {
    if (signal.aborted) return Promise.reject(abortedError());
    let expiry = Number.POSITIVE_INFINITY;
    for (const claim of this.claims.values()) if (claim.from !== this.self) expiry = Math.min(expiry, claim.until);
    return new Promise((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const finish = () => {
        this.wakers.delete(waker);
        signal.removeEventListener('abort', onAbort);
        if (timer !== undefined) clearTimeout(timer);
      };
      const waker = () => {
        finish();
        resolve();
      };
      const onAbort = () => {
        finish();
        reject(abortedError());
      };
      this.wakers.add(waker);
      signal.addEventListener('abort', onAbort, { once: true });
      if (Number.isFinite(expiry)) timer = setTimeout(waker, Math.max(0, expiry - this.now()));
    });
  }
}
