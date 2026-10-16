import type { PollResponse } from './api';
import type { ProjectChangeResync } from './project-change-poll';

/**
 * Message contract between the main thread and the shared change-stream worker (HS2-SH9179).
 * Only change events cross the boundary, never ticket collections. Every per-project message
 * carries the generation the main thread assigned when it opened that project, so a message
 * from a closed or restarted subscription can always be recognized and dropped.
 */
export const CHANGE_STREAM_WORKER_PROTOCOL = 1;

export interface ChangeStreamWorkerOpen {
  type: 'open';
  project: string;
  generation: number;
  /** Absolute API base, for example `https://host/__hotsheet/projects/<id>`. */
  apiUrl: string;
  /** Absolute `ws:`/`wss:` change-stream URL, or undefined for long-poll only. */
  webSocketUrl?: string;
  /**
   * Absolute URL of the bridge's multiplexed change long-poll (HS2-6W1NNF), when the bridge
   * advertised one. Every project with the same URL shares one long-poll request.
   */
  changeMuxUrl?: string;
  clientId: string;
  /** Present when the stream continues one another owner followed (see transport `resume`). */
  resume?: { cursor?: number };
}

export type ChangeStreamMainMessage =
  | { type: 'hello'; protocol: number }
  | ChangeStreamWorkerOpen
  | { type: 'close'; project: string; generation: number }
  | { type: 'ack'; project: string; generation: number; seq: number };

export type ChangeStreamDelivery =
  | { kind: 'resync'; reason: ProjectChangeResync; cursor: number }
  | { kind: 'deliver'; response: PollResponse; force: boolean };

export type ChangeStreamWorkerMessage =
  | { type: 'ready'; protocol: number }
  | ({ type: 'batch'; project: string; generation: number; seq: number } & ChangeStreamDelivery)
  | { type: 'error'; project: string; generation: number; message: string };

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isCount = (value: unknown): value is number => Number.isSafeInteger(value) && (value as number) >= 0;
const RESYNC_REASONS = new Set<unknown>(['initial', 'reconnect', 'overflow']);

const isPollResponse = (value: unknown): value is PollResponse =>
  isRecord(value) && isCount(value.cursor) && typeof value.overflow === 'boolean' && Array.isArray(value.events);

/** Validate a message the worker received; anything unrecognized is ignored, never thrown. */
export function parseChangeStreamMainMessage(value: unknown): ChangeStreamMainMessage | undefined {
  if (!isRecord(value)) return undefined;
  if (value.type === 'hello') return isCount(value.protocol) ? { type: 'hello', protocol: value.protocol } : undefined;
  if (typeof value.project !== 'string' || !isCount(value.generation)) return undefined;
  const { project, generation } = value;
  if (value.type === 'close') return { type: 'close', project, generation };
  if (value.type === 'ack')
    return isCount(value.seq) ? { type: 'ack', project, generation, seq: value.seq } : undefined;
  if (value.type !== 'open' || typeof value.apiUrl !== 'string' || typeof value.clientId !== 'string') return undefined;
  if (value.webSocketUrl !== undefined && typeof value.webSocketUrl !== 'string') return undefined;
  if (value.changeMuxUrl !== undefined && typeof value.changeMuxUrl !== 'string') return undefined;
  let resume: { cursor?: number } | undefined;
  if (value.resume !== undefined) {
    if (!isRecord(value.resume)) return undefined;
    if (value.resume.cursor !== undefined && !isCount(value.resume.cursor)) return undefined;
    resume = value.resume.cursor === undefined ? {} : { cursor: value.resume.cursor };
  }
  return {
    type: 'open',
    project,
    generation,
    apiUrl: value.apiUrl,
    clientId: value.clientId,
    ...(value.webSocketUrl === undefined ? {} : { webSocketUrl: value.webSocketUrl }),
    ...(value.changeMuxUrl === undefined ? {} : { changeMuxUrl: value.changeMuxUrl }),
    ...(resume ? { resume } : {}),
  };
}

/** Validate a message the main thread received from the worker. */
export function parseChangeStreamWorkerMessage(value: unknown): ChangeStreamWorkerMessage | undefined {
  if (!isRecord(value)) return undefined;
  if (value.type === 'ready') return isCount(value.protocol) ? { type: 'ready', protocol: value.protocol } : undefined;
  if (typeof value.project !== 'string' || !isCount(value.generation)) return undefined;
  const { project, generation } = value;
  if (value.type === 'error')
    return typeof value.message === 'string'
      ? { type: 'error', project, generation, message: value.message }
      : undefined;
  if (value.type !== 'batch' || !isCount(value.seq)) return undefined;
  const { seq } = value;
  if (value.kind === 'resync')
    return RESYNC_REASONS.has(value.reason) && isCount(value.cursor)
      ? {
          type: 'batch',
          project,
          generation,
          seq,
          kind: 'resync',
          reason: value.reason as ProjectChangeResync,
          cursor: value.cursor,
        }
      : undefined;
  if (value.kind === 'deliver' && isPollResponse(value.response) && typeof value.force === 'boolean')
    return { type: 'batch', project, generation, seq, kind: 'deliver', response: value.response, force: value.force };
  return undefined;
}

/**
 * The worker's per-project outbound queue. One delivery is in flight to the main thread at a
 * time; adjacent event pages queued behind it coalesce into one page (preserving order, OR-ing
 * overflow and forced refresh, keeping the newest cursor). Past `maxEvents` queued events the
 * queue collapses into a single overflow page, which makes the main thread reconcile from the
 * server instead of holding an unbounded backlog — the same contract as a server-side overflow.
 */
export class ChangeStreamDeliveryQueue {
  private readonly queued: ChangeStreamDelivery[] = [];
  private inFlight: number | undefined;
  private nextSeq = 1;
  private queuedEvents = 0;
  constructor(
    private readonly send: (seq: number, delivery: ChangeStreamDelivery) => void,
    private readonly maxEvents = 1_000,
  ) {}

  /** Queued plus in-flight deliveries, for tests and diagnostics. */
  get size(): number {
    return this.queued.length + (this.inFlight === undefined ? 0 : 1);
  }

  push(delivery: ChangeStreamDelivery): void {
    const last = this.queued.at(-1);
    if (delivery.kind === 'deliver' && last?.kind === 'deliver') {
      last.response = {
        cursor: delivery.response.cursor,
        overflow: last.response.overflow || delivery.response.overflow,
        events: [...last.response.events, ...delivery.response.events],
      };
      last.force ||= delivery.force;
    } else
      this.queued.push(delivery.kind === 'deliver' ? { ...delivery, response: { ...delivery.response } } : delivery);
    if (delivery.kind === 'deliver') this.queuedEvents += delivery.response.events.length;
    if (this.queuedEvents > this.maxEvents) this.collapse();
    this.flush();
  }

  /** The main thread finished `seq`; a stale or repeated acknowledgement is ignored. */
  ack(seq: number): void {
    if (seq !== this.inFlight) return;
    this.inFlight = undefined;
    this.flush();
  }

  private collapse(): void {
    let cursor = 0,
      force = false;
    for (const item of this.queued) {
      cursor = Math.max(cursor, item.kind === 'deliver' ? item.response.cursor : item.cursor);
      if (item.kind === 'deliver') force ||= item.force;
    }
    this.queued.splice(0, this.queued.length, {
      kind: 'deliver',
      response: { cursor, events: [], overflow: true },
      force,
    });
    this.queuedEvents = 0;
  }

  private flush(): void {
    if (this.inFlight !== undefined) return;
    const next = this.queued.shift();
    if (!next) return;
    if (next.kind === 'deliver') this.queuedEvents -= next.response.events.length;
    this.inFlight = this.nextSeq;
    this.nextSeq += 1;
    this.send(this.inFlight, next);
  }
}
