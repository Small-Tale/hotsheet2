/**
 * Cross-context direct-poll slots for bridges without change multiplexing (HS2-GDF4ZJ).
 *
 * Four Web Locks bound the direct change long polls held by every tab and worker of the
 * origin, leaving HTTP/1.1 connections for ordinary API work. HS2-EZN3NY: a granted poll
 * learns whether any other poll is waiting for a slot, so it can hold the caller's whole
 * long-poll window when nobody waits and rotate on a short cap only under real contention.
 * A poll that has to wait announces it, so holders in other contexts yield their turn.
 * HS2-QS2P6B: a context without Web Locks, or one whose Web Locks implementation refuses it,
 * keeps the same cap with `BroadcastChannel` leases (`change-poll-slot-leases.ts`) and still
 * long polls; it never falls back to fixed-interval requests.
 */
import {
  abortedError,
  broadcastLeaseBus,
  LEASE_HOLD_MAX_MS,
  type SlotLeaseBus,
  SlotLeases,
} from './change-poll-slot-leases';

const SLOT_COUNT = 4;
const LOCK_PREFIX = 'hotsheet-direct-change-poll-';
const CHANNEL_NAME = 'hotsheet-direct-change-poll';

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
  /** Lease bus for contexts without Web Locks; `null` means this context sees no other. */
  leaseBus?: SlotLeaseBus | null;
  now?: () => number;
}

const aborted = abortedError;

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
  private readonly channel: SlotContentionChannel | undefined;
  private readonly leases: SlotLeases;

  constructor(options: DirectPollSlotsOptions = {}) {
    this.channel = options.channel === null ? undefined : (options.channel ?? broadcastContention());
    this.leases = new SlotLeases({
      bus: options.leaseBus === undefined ? broadcastLeaseBus() : options.leaseBus,
      capacity: SLOT_COUNT,
      ...(options.now ? { now: options.now } : {}),
    });
  }

  /** Run `listener` whenever a poll in another context (or a leased one here) waits for a slot. */
  onContention(listener: () => void): () => void {
    const stopChannel = this.channel?.listen(listener);
    const stopLeases = this.leases.onContention(listener);
    return () => {
      stopChannel?.();
      stopLeases();
    };
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
        // If a Web Locks implementation refuses this context, coordinate with leases instead.
      }
    }
    // HS2-QS2P6B: without a cross-context lock, leases keep the origin-wide cap and the holder
    // still long polls. The transport's own backoff paces an older bridge that answers early.
    return this.leases.run(signal, (contended) => poll({ contended, maxTimeoutMs: LEASE_HOLD_MAX_MS }));
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
