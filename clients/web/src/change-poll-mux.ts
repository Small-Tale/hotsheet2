/**
 * The browser half of the multiplexed change long-poll (HS2-6W1NNF).
 *
 * Each open project keeps its own change transport (cursor, replay, gap checks, backoff,
 * and sink; see `project-change-poll.ts`). Only the long-poll HTTP request is shared: a
 * project's `pollEvents` call joins one in-flight bridge request that carries every
 * waiting project's poll, so N projects whose WebSockets fail hold one browser connection
 * instead of N and cannot fill the browser's per-origin HTTP/1.1 pool.
 *
 * States per multiplexer: `mux` (sharing) or `direct` (an older bridge or an invalid
 * answer; every poll goes straight to the project's own `/ws/poll`, permanently).
 * Within `mux`, the request is idle, coalescing (a flush is scheduled), or in flight:
 * - a poll that joins, or a project that closes, while a request is in flight aborts it
 *   and reissues one with every pending project and its remaining deadline;
 * - each answered project resolves (or rejects with its own upstream status); projects
 *   without an answer stay pending for the reissued request;
 * - a failed request rejects every project it carried, and each transport then backs off
 *   on its own bounded schedule, so a failure can never become a tight request loop.
 * No timer here issues a request on its own: the only deferral is a short coalescing
 * window after a project asked to poll.
 */
import type { PollResponse } from './api';
import { CHANGE_MUX_MAX_POLLS, CHANGE_MUX_VERSION, type ChangeMuxPoll } from './change-mux-bridge';
import { DirectPollSlots, type SlotGrant } from './change-poll-slots';

export type ChangePollFn = (since?: number, signal?: AbortSignal, timeoutMs?: number) => Promise<PollResponse>;

export type ChangePollMuxMode = 'mux' | 'direct';

export interface ChangePollMultiplexerOptions {
  /** The bridge's multiplexed poll URL. */
  endpoint: string;
  clientId: string;
  /** SharedWorker v2: several tab clients may poll the same project in one browser request. */
  sharedAcrossClients?: boolean;
  /** Older bridges have no mux endpoint, but still need bounded direct polling. */
  directOnly?: boolean;
  /** Disable cross-context coordination only for isolated multiplexer tests. */
  crossContextSlots?: boolean;
  /** Cross-context slots to use instead of the default ones (tests). */
  directSlots?: Pick<DirectPollSlots, 'run' | 'onContention'>;
  parse(value: unknown): PollResponse;
  fetch?: typeof fetch;
  now?: () => number;
  /** How long to gather polls before sending; keeps a burst of re-polls in one request. */
  coalesceMs?: number;
  onModeChange?(mode: ChangePollMuxMode, reason: string): void;
}

interface Pending {
  key: string;
  project: string;
  client: string;
  since?: number;
  deadline: number;
  signal?: AbortSignal;
  directController: AbortController;
  direct: ChangePollFn;
  resolve(response: PollResponse): void;
  reject(reason: unknown): void;
  detach(): void;
}

/** One direct request for a pending project (HS2-EZN3NY). */
interface DirectAttempt {
  entry: Pending;
  /** Aborts this request only; the project's poll stays pending when it is preempted. */
  controller: AbortController;
  /** Set when the request is issued (after its slot is granted). */
  started?: number;
  timeout?: number;
  /** A full-window poll that contention may preempt after its minimum turn. */
  held: boolean;
  preempted: boolean;
  timer?: ReturnType<typeof setTimeout>;
  release(): void;
}

interface InFlight {
  controller: AbortController;
  members: Map<string, Pending>;
}

const DEFAULT_TIMEOUT_MS = 25_000;
/** Leave two HTTP/1.1 origin connections available for ordinary API work in direct fallback. */
export const DIRECT_POLL_MAX_IN_FLIGHT = 4;
/**
 * Under contention (more projects than slots, or another context waiting for a slot), direct
 * polls rotate on this cap, and a held full-window poll yields once it has had this long.
 * Without contention a direct poll holds the caller's whole window (HS2-EZN3NY).
 */
export const DIRECT_POLL_TIMEOUT_MS = 1_000;

const abortError = (): Error => {
  const error = new Error('The change poll was aborted.');
  error.name = 'AbortError';
  return error;
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** Whether a bridge answer is a valid multiplexed response of this wire version. */
const muxResults = (
  value: unknown,
  version: 1 | 2,
): Array<{ project: string; client?: string; status: number; body: unknown }> | undefined => {
  if (!isRecord(value) || value.version !== version || !Array.isArray(value.results)) return undefined;
  const results: Array<{ project: string; client?: string; status: number; body: unknown }> = [];
  for (const item of value.results as unknown[]) {
    if (!isRecord(item) || typeof item.project !== 'string' || typeof item.status !== 'number') return undefined;
    if (version === 2 && typeof item.client !== 'string') return undefined;
    results.push({
      project: item.project,
      ...(version === 2 ? { client: item.client as string } : {}),
      status: item.status,
      body: item.body,
    });
  }
  return results;
};

export class ChangePollMultiplexer {
  private modeValue: ChangePollMuxMode = 'mux';
  private readonly pending = new Map<string, Pending>();
  private inflight: InFlight | undefined;
  private readonly directActive = new Map<Pending, DirectAttempt>();
  private readonly directSlots: Pick<DirectPollSlots, 'run' | 'onContention'> | undefined;
  private contentionSubscribed = false;
  private flushTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly fetcher: typeof fetch;
  private readonly now: () => number;
  private readonly coalesceMs: number;
  /** Requests sent, for tests and diagnostics. */
  requests = 0;

  constructor(private readonly options: ChangePollMultiplexerOptions) {
    if (options.directOnly) this.modeValue = 'direct';
    this.fetcher = options.fetch ?? ((input, init) => fetch(input, init));
    this.now = options.now ?? Date.now;
    this.coalesceMs = options.coalesceMs ?? 5;
    this.directSlots = options.directSlots ?? (options.crossContextSlots === false ? undefined : new DirectPollSlots());
  }

  get mode(): ChangePollMuxMode {
    return this.modeValue;
  }

  private key(project: string, client: string): string {
    return this.options.sharedAcrossClients ? `${client}\u0000${project}` : project;
  }

  /** Projects with a poll waiting for an answer. */
  waiting(): string[] {
    return [...this.pending.keys()];
  }

  /** A change client for `project` whose polls ride this multiplexer. */
  client<T extends { pollEvents: ChangePollFn }>(project: string, direct: T, clientId = this.options.clientId): T {
    const directPoll: ChangePollFn = (since, signal, timeoutMs) => direct.pollEvents(since, signal, timeoutMs);
    return Object.assign(Object.create(direct) as T, {
      pollEvents: (since?: number, signal?: AbortSignal, timeoutMs?: number) =>
        this.poll(project, directPoll, since, signal, timeoutMs, clientId),
    });
  }

  /** Long-poll `project` after `since`, answered by the shared request. */
  poll(
    project: string,
    direct: ChangePollFn,
    since?: number,
    signal?: AbortSignal,
    timeoutMs = DEFAULT_TIMEOUT_MS,
    clientId = this.options.clientId,
  ): Promise<PollResponse> {
    if (signal?.aborted) return Promise.reject(abortError());
    return new Promise<PollResponse>((resolve, reject) => {
      const key = this.key(project, clientId);
      // A transport polls sequentially, so a second poll for a project supersedes the first.
      const previous = this.pending.get(key);
      if (previous)
        this.settle(previous, () => {
          previous.reject(new Error('Superseded by a newer change poll.'));
        });
      const onAbort = () => {
        if (this.pending.get(key) !== entry) return;
        this.settle(entry, () => {
          entry.reject(abortError());
        });
        if (this.inflight?.members.get(key) === entry) this.schedule();
        if (this.modeValue === 'direct') this.drainDirect();
      };
      const entry: Pending = {
        key,
        project,
        client: clientId,
        ...(since === undefined ? {} : { since }),
        deadline: this.now() + Math.max(0, timeoutMs),
        ...(signal ? { signal } : {}),
        directController: new AbortController(),
        direct,
        resolve,
        reject,
        detach: () => signal?.removeEventListener('abort', onAbort),
      };
      signal?.addEventListener('abort', onAbort, { once: true });
      this.pending.set(key, entry);
      if (this.modeValue === 'direct') this.drainDirect();
      else this.schedule();
    });
  }

  /** Drop `entry` from the pending set, then run its outcome. */
  private settle(entry: Pending, outcome: () => void) {
    if (this.pending.get(entry.key) === entry) this.pending.delete(entry.key);
    entry.detach();
    entry.directController.abort();
    outcome();
  }

  private schedule() {
    if (this.flushTimer !== undefined) return;
    this.flushTimer = setTimeout(() => {
      this.flushTimer = undefined;
      this.flush();
    }, this.coalesceMs);
  }

  private sameMembers(members: Map<string, Pending>): boolean {
    if (members.size !== this.pending.size) return false;
    for (const [project, entry] of this.pending) if (members.get(project) !== entry) return false;
    return true;
  }

  private flush() {
    if (this.modeValue === 'direct') return;
    if (this.inflight) {
      if (this.sameMembers(this.inflight.members)) return;
      this.inflight.controller.abort();
      this.inflight = undefined;
    }
    if (this.pending.size === 0) return;
    if (this.pending.size > CHANGE_MUX_MAX_POLLS) {
      this.goDirect(`More than ${CHANGE_MUX_MAX_POLLS} projects are waiting; polling each directly.`);
      return;
    }
    this.issue(new Map(this.pending));
  }

  private issue(members: Map<string, Pending>) {
    const controller = new AbortController();
    // Read through a function: the request can be aborted across any await below.
    const aborted = () => controller.signal.aborted;
    const inflight: InFlight = { controller, members };
    this.inflight = inflight;
    const now = this.now();
    const polls: ChangeMuxPoll[] = [...members.values()].map((entry) => ({
      project: entry.project,
      ...(this.options.sharedAcrossClients ? { client: entry.client } : {}),
      ...(entry.since === undefined ? {} : { since: entry.since }),
      timeout_ms: Math.max(0, Math.ceil(entry.deadline - now)),
    }));
    this.requests += 1;
    void (async () => {
      let response: Response;
      try {
        response = await this.fetcher(this.options.endpoint, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            version: this.options.sharedAcrossClients ? 2 : CHANGE_MUX_VERSION,
            client: this.options.clientId,
            polls,
          }),
          signal: controller.signal,
        });
      } catch (reason) {
        if (aborted()) return;
        this.fail(inflight, reason);
        return;
      }
      if (aborted()) return;
      // An older bridge has no such endpoint: poll each project directly from now on.
      // A 400 means it does not understand this wire version.
      if ([400, 404, 405, 501].includes(response.status)) {
        this.goDirect(`The bridge does not multiplex change polls (${response.status}).`);
        return;
      }
      if (!response.ok) {
        this.fail(inflight, new Error(`Change multiplex failed with ${response.status}.`));
        return;
      }
      let body: unknown;
      try {
        body = await response.json();
      } catch (reason) {
        if (aborted()) return;
        this.goDirect(`The bridge answered the change multiplex with invalid JSON: ${String(reason)}`);
        return;
      }
      if (aborted()) return;
      const results = muxResults(body, this.options.sharedAcrossClients ? 2 : CHANGE_MUX_VERSION);
      // A valid answer always carries at least one result; an empty one would otherwise make
      // every reissue return at once.
      if (!results || results.length === 0) {
        this.goDirect('The bridge answered the change multiplex with an unsupported response.');
        return;
      }
      if (this.inflight === inflight) this.inflight = undefined;
      for (const result of results) {
        const key = this.key(result.project, result.client ?? this.options.clientId);
        const entry = members.get(key);
        if (!entry || this.pending.get(key) !== entry) continue;
        if (result.status >= 200 && result.status < 300) {
          let page: PollResponse;
          try {
            page = this.options.parse(result.body);
          } catch (reason) {
            this.settle(entry, () => {
              entry.reject(reason);
            });
            continue;
          }
          this.settle(entry, () => {
            entry.resolve(page);
          });
        } else
          this.settle(entry, () => {
            entry.reject(new Error(`Change poll failed with ${result.status}.`));
          });
      }
      if (this.pending.size) this.schedule();
    })();
  }

  /** The whole request failed: every project it carried sees the failure and backs off. */
  private fail(inflight: InFlight, reason: unknown) {
    if (this.inflight === inflight) this.inflight = undefined;
    for (const entry of inflight.members.values())
      if (this.pending.get(entry.key) === entry)
        this.settle(entry, () => {
          entry.reject(reason);
        });
    if (this.pending.size) this.schedule();
  }

  /** Permanently poll projects directly, but reserve browser connections for ordinary API calls. */
  private goDirect(reason: string) {
    if (this.modeValue === 'direct') return;
    this.modeValue = 'direct';
    this.inflight?.controller.abort();
    this.inflight = undefined;
    if (this.flushTimer !== undefined) clearTimeout(this.flushTimer);
    this.flushTimer = undefined;
    this.options.onModeChange?.('direct', reason);
    this.drainDirect();
  }

  /**
   * HS2-AZ11V9: under contention a direct poll is capped at `DIRECT_POLL_TIMEOUT_MS` so queued
   * projects take turns (HS2-EZN3NY: only then). An empty page from that capped poll is not the end of the caller's long poll: the
   * transport treats an early empty page as the server having held its whole window and then
   * sleeps out the rest of its retry window, up to 30 s, without polling, so an event in that
   * gap (a terminal halt) arrived late. Keep polling until the caller's deadline instead.
   * A poll that came back well before its timeout (a server that ignores the timeout) still
   * resolves, so this can never become a tight request loop.
   */
  private continueDirectPoll(entry: Pending, page: PollResponse, started: number, timeout: number): boolean {
    const now = this.now();
    return (
      entry.since !== undefined &&
      page.events.length === 0 &&
      !page.overflow &&
      page.cursor === entry.since &&
      timeout > 0 &&
      (now - started) * 2 >= timeout &&
      entry.deadline - now > 0 &&
      !entry.signal?.aborted
    );
  }

  /** Projects waiting for a direct slot in this context. */
  private queuedDirect(): number {
    let queued = 0;
    for (const entry of this.pending.values()) if (!this.directActive.has(entry)) queued += 1;
    return queued;
  }

  private drainDirect() {
    if (this.modeValue !== 'direct') return;
    if (this.directSlots && !this.contentionSubscribed) {
      this.contentionSubscribed = true;
      this.directSlots.onContention(() => {
        this.preemptHeld();
      });
    }
    for (const entry of this.pending.values()) {
      if (this.directActive.size >= DIRECT_POLL_MAX_IN_FLIGHT) break;
      if (this.directActive.has(entry)) continue;
      this.startDirect(entry);
    }
    if (this.queuedDirect() > 0) this.preemptHeld();
  }

  /**
   * HS2-EZN3NY: a held full-window poll yields its slot once it has had a minimum turn. The
   * project stays pending and is reissued with the same cursor, so the server replays anything
   * that arrived meanwhile; preemption never rejects, so the transport does not back off.
   */
  private preemptHeld() {
    const now = this.now();
    for (const attempt of this.directActive.values()) {
      if (!attempt.held || attempt.preempted || attempt.timer !== undefined || attempt.started === undefined) continue;
      const preempt = () => {
        attempt.timer = undefined;
        attempt.preempted = true;
        attempt.controller.abort();
      };
      const remaining = attempt.started + DIRECT_POLL_TIMEOUT_MS - now;
      if (remaining <= 0) preempt();
      else attempt.timer = setTimeout(preempt, remaining);
    }
  }

  /** Put a still-pending project behind the others waiting for a slot. */
  private rotate(entry: Pending) {
    this.pending.delete(entry.key);
    this.pending.set(entry.key, entry);
  }

  private startDirect(entry: Pending) {
    const controller = new AbortController();
    const parent = entry.directController.signal;
    const onParentAbort = () => {
      controller.abort();
    };
    parent.addEventListener('abort', onParentAbort, { once: true });
    const attempt: DirectAttempt = {
      entry,
      controller,
      held: false,
      preempted: false,
      release: () => {
        parent.removeEventListener('abort', onParentAbort);
        if (attempt.timer !== undefined) clearTimeout(attempt.timer);
        attempt.timer = undefined;
        if (this.directActive.get(entry) === attempt) this.directActive.delete(entry);
      },
    };
    this.directActive.set(entry, attempt);
    // The timeout is chosen once the slot is granted: hold the caller's whole window unless
    // another project, here or in another context, is waiting for a slot.
    const issue = (grant: SlotGrant) => {
      const started = this.now(),
        remaining = Math.max(0, Math.ceil(entry.deadline - started)),
        contended = grant.contended || this.pending.size > DIRECT_POLL_MAX_IN_FLIGHT;
      let timeout = contended ? Math.min(DIRECT_POLL_TIMEOUT_MS, remaining) : remaining;
      if (grant.maxTimeoutMs !== undefined) timeout = Math.min(timeout, grant.maxTimeoutMs);
      attempt.started = started;
      attempt.timeout = timeout;
      attempt.held = timeout > DIRECT_POLL_TIMEOUT_MS;
      return entry.direct(entry.since, controller.signal, timeout);
    };
    void Promise.resolve()
      .then(() => (this.directSlots ? this.directSlots.run(controller.signal, issue) : issue({ contended: false })))
      .then(
        (page) => {
          attempt.release();
          if (this.pending.get(entry.key) === entry) {
            if (this.continueDirectPoll(entry, page, attempt.started ?? this.now(), attempt.timeout ?? 0))
              this.rotate(entry);
            else
              this.settle(entry, () => {
                entry.resolve(page);
              });
          }
          this.drainDirect();
        },
        (reason: unknown) => {
          attempt.release();
          if (this.pending.get(entry.key) === entry) {
            if (attempt.preempted) this.rotate(entry);
            else
              this.settle(entry, () => {
                entry.reject(reason);
              });
          }
          this.drainDirect();
        },
      );
  }
}

/** Split a bridge project API URL or path into its multiplex endpoint and project id. */
export function changeMuxTarget(apiPath: string, muxPath: string): { endpoint: string; project: string } | undefined {
  const match = /^(.*)\/__hotsheet\/project-api\/([^/?#]+)\/?$/u.exec(apiPath);
  if (!match) return undefined;
  let project: string;
  try {
    project = decodeURIComponent(match[2]);
  } catch {
    return undefined;
  }
  return { endpoint: `${match[1]}${muxPath}`, project };
}

/** One multiplexer per endpoint and tab client, shared by every project of a context (page or worker). */
export function createChangePollMultiplexers(
  options: Omit<ChangePollMultiplexerOptions, 'endpoint' | 'clientId'>,
): (endpoint: string, clientId: string) => ChangePollMultiplexer {
  const byEndpoint = new Map<string, ChangePollMultiplexer>();
  return (endpoint, clientId) => {
    const key = options.sharedAcrossClients ? endpoint : `${clientId}\n${endpoint}`;
    let mux = byEndpoint.get(key);
    if (!mux) {
      mux = new ChangePollMultiplexer({ ...options, endpoint, clientId });
      byEndpoint.set(key, mux);
    }
    return mux;
  };
}
