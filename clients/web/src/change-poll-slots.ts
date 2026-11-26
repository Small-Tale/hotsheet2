/**
 * Cross-context direct-poll slots for bridges without change multiplexing (HS2-GDF4ZJ).
 *
 * Four Web Locks bound the direct change long polls held by every tab and worker of the
 * origin, leaving HTTP/1.1 connections for ordinary API work. HS2-EZN3NY: a granted poll
 * learns whether any other poll is waiting for a slot, so it can hold the caller's whole
 * long-poll window when nobody waits and rotate on a short cap only under real contention.
 * A poll that has to wait announces it, so holders in other contexts yield their turn.
 */
const SLOT_COUNT = 4;
const LOCK_PREFIX = 'hotsheet-direct-change-poll-';
const CHANNEL_NAME = 'hotsheet-direct-change-poll';
const FALLBACK_SPACING_MS = 250;

/** What a slot holder may do with its connection. */
export interface SlotGrant {
  /** Another direct poll, in this or another context, is waiting for a slot. */
  contended: boolean;
  /** The longest this poll may hold its connection; absent means the caller's own window. */
  maxTimeoutMs?: number;
}

/** Tells slot holders in other contexts that a poll is waiting for a slot. */
export interface SlotContentionChannel {
  announce(): void;
  listen(listener: () => void): () => void;
}

export interface DirectPollSlotsOptions {
  /** `null` disables cross-context announcements; the default uses a `BroadcastChannel`. */
  channel?: SlotContentionChannel | null;
}

const aborted = () => new DOMException('The change poll was aborted.', 'AbortError');

function wait(milliseconds: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(aborted());
  if (milliseconds <= 0) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve();
    }, milliseconds);
    const onAbort = () => {
      clearTimeout(timer);
      reject(aborted());
    };
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

/** A lazily opened `BroadcastChannel`, or nothing where the context has none. */
function broadcastContention(): SlotContentionChannel | undefined {
  const Channel = Reflect.get(globalThis, 'BroadcastChannel') as typeof BroadcastChannel | undefined;
  if (typeof Channel !== 'function') return undefined;
  let channel: BroadcastChannel | undefined;
  const listeners = new Set<() => void>();
  const open = (): BroadcastChannel => {
    if (channel) return channel;
    const opened = new Channel(CHANNEL_NAME);
    opened.onmessage = () => {
      for (const listener of [...listeners]) listener();
    };
    // Node's channel would otherwise keep a test process alive; browsers have no `unref`.
    (opened as unknown as { unref?: () => void }).unref?.();
    channel = opened;
    return opened;
  };
  return {
    announce: () => {
      open().postMessage('waiting');
    },
    listen: (listener) => {
      open();
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) {
          channel?.close();
          channel = undefined;
        }
      };
    },
  };
}

export class DirectPollSlots {
  private nextSlot = 0;
  private nextFallbackStart = 0;
  private readonly channel: SlotContentionChannel | undefined;

  constructor(options: DirectPollSlotsOptions = {}) {
    this.channel = options.channel === null ? undefined : (options.channel ?? broadcastContention());
  }

  /** Run `listener` whenever a poll in another context starts waiting for a slot. */
  onContention(listener: () => void): () => void {
    return this.channel?.listen(listener) ?? (() => undefined);
  }

  async run<T>(signal: AbortSignal, poll: (grant: SlotGrant) => Promise<T>): Promise<T> {
    if (signal.aborted) throw aborted();
    const host = Reflect.get(globalThis, 'navigator') as Navigator | undefined;
    const locks = host?.locks;
    if (locks) {
      const state = { acquired: false };
      const hold = async (): Promise<T> => {
        state.acquired = true;
        return poll({ contended: await othersWaiting(locks) });
      };
      try {
        const first = this.nextSlot++ % SLOT_COUNT;
        // Take any free slot before queueing behind a held one.
        for (let offset = 0; offset < SLOT_COUNT; offset += 1) {
          const name = `${LOCK_PREFIX}${(first + offset) % SLOT_COUNT}`;
          const granted = await locks.request(name, { mode: 'exclusive', ifAvailable: true }, (lock) =>
            lock ? hold().then((value) => ({ value })) : undefined,
          );
          if (granted) return granted.value;
        }
        this.channel?.announce();
        return await locks.request(`${LOCK_PREFIX}${first}`, { mode: 'exclusive', signal }, hold);
      } catch (reason) {
        if (state.acquired || (reason instanceof Error && reason.name === 'AbortError')) throw reason;
        // If a Web Locks implementation refuses this context, use short requests instead.
      }
    }
    // With no cross-context lock, never hold a connection through a long poll.
    // Pacing avoids a busy loop when an older bridge returns an empty page immediately.
    const start = Math.max(Date.now(), this.nextFallbackStart);
    this.nextFallbackStart = start + FALLBACK_SPACING_MS;
    await wait(start - Date.now(), signal);
    return poll({ contended: true, maxTimeoutMs: 0 });
  }
}

/** Whether a direct poll anywhere in the origin is queued for a slot. */
async function othersWaiting(locks: LockManager): Promise<boolean> {
  if (typeof locks.query !== 'function') return false;
  try {
    const snapshot = await locks.query();
    return (snapshot.pending ?? []).some((lock) => lock.name?.startsWith(LOCK_PREFIX) === true);
  } catch {
    return false;
  }
}
