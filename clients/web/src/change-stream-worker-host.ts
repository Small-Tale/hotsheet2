import {
  CHANGE_STREAM_WORKER_PROTOCOL,
  type ChangeStreamMainMessage,
  parseChangeStreamWorkerMessage,
} from './change-stream-worker-protocol';
import {
  createProjectChangeConsumer,
  type ProjectChangeSink,
  type ProjectChangeStreamOptions,
  startProjectChangeStream,
} from './project-change-poll';

/** The subset of `Worker` the host uses, so tests can drive it with an in-memory fake. */
export interface ChangeStreamWorkerLike {
  postMessage(message: ChangeStreamMainMessage): void;
  addEventListener(type: 'message' | 'error' | 'messageerror', listener: (event: Event) => void): void;
  terminate(): void;
}

export interface ChangeStreamEndpoints {
  apiUrl: string;
  webSocketUrl?: string;
}

export type ChangeStreamHostMode = 'worker' | 'fallback';

export interface ChangeStreamWorkerHostOptions {
  /** Throws (or returns undefined) when workers are unavailable; the host then falls back. */
  createWorker(): ChangeStreamWorkerLike | undefined;
  clientId: string;
  readyTimeoutMs?: number;
  /** Crash restarts allowed within `restartWindowMs` before the host falls back for good. */
  maxRestarts?: number;
  restartWindowMs?: number;
  restartDelayMs?: number;
  /** The main-thread stream used for fallback (injected by tests). */
  startFallback?: typeof startProjectChangeStream;
  onModeChange?(mode: ChangeStreamHostMode, reason?: string): void;
  now?: () => number;
}

interface HostedProject {
  generation: number;
  options: ProjectChangeStreamOptions;
  endpoints: ChangeStreamEndpoints;
  controller: AbortController;
  consumer: ProjectChangeSink;
  /** Newest cursor the main thread finished processing; the replay point after a restart. */
  cursor?: number;
  /** Serializes this project's deliveries, across worker restarts, so order is preserved. */
  processing: Promise<void>;
  fallbackStop?: () => void;
}

/**
 * Main-thread owner of the shared change-stream worker (HS2-SH9179). The worker owns transport —
 * WebSockets, long polls, JSON validation, replay cursors, and backoff — for every open project;
 * the host applies each delivery through the same consumer as the main-thread stream, one at a
 * time per project, and acknowledges it so the worker sends the next (coalesced) page.
 *
 * Resilience: a protocol-version mismatch or a missing `ready` falls back to the main-thread
 * stream; a worker crash restarts it with bounded backoff and reopens every project from its last
 * acknowledged cursor (authoritative replay); repeated crashes fall back permanently.
 */
export function createChangeStreamWorkerHost(options: ChangeStreamWorkerHostOptions) {
  const readyTimeoutMs = options.readyTimeoutMs ?? 2_000,
    maxRestarts = options.maxRestarts ?? 3,
    restartWindowMs = options.restartWindowMs ?? 60_000,
    restartDelayMs = options.restartDelayMs ?? 250,
    startFallback = options.startFallback ?? startProjectChangeStream,
    now = options.now ?? (() => Date.now());
  const projects = new Map<string, HostedProject>();
  const crashes: number[] = [];
  let worker: ChangeStreamWorkerLike | undefined,
    mode: ChangeStreamHostMode = 'worker',
    ready = false,
    readyTimer: ReturnType<typeof setTimeout> | undefined,
    restartTimer: ReturnType<typeof setTimeout> | undefined,
    nextGeneration = 1,
    disposed = false;

  const resumeOf = (hosted: HostedProject): { cursor?: number } | undefined =>
    hosted.cursor !== undefined ? { cursor: hosted.cursor } : undefined;

  const post = (message: ChangeStreamMainMessage) => {
    worker?.postMessage(message);
  };
  const openInWorker = (project: string, hosted: HostedProject, resume?: { cursor?: number }) => {
    post({
      type: 'open',
      project,
      generation: hosted.generation,
      apiUrl: hosted.endpoints.apiUrl,
      ...(hosted.endpoints.webSocketUrl ? { webSocketUrl: hosted.endpoints.webSocketUrl } : {}),
      clientId: options.clientId,
      ...(resume ? { resume } : {}),
    });
  };

  const clearTimers = () => {
    if (readyTimer !== undefined) clearTimeout(readyTimer);
    if (restartTimer !== undefined) clearTimeout(restartTimer);
    readyTimer = restartTimer = undefined;
  };
  const stopWorker = () => {
    clearTimers();
    worker?.terminate();
    worker = undefined;
    ready = false;
  };

  const startMainThread = (hosted: HostedProject) => {
    const resume = resumeOf(hosted);
    hosted.fallbackStop = startFallback({
      ...hosted.options,
      ...(resume ? { resume } : {}),
    });
  };

  const fallBack = (reason: string) => {
    if (mode === 'fallback' || disposed) return;
    mode = 'fallback';
    stopWorker();
    options.onModeChange?.('fallback', reason);
    // Continue each project from what the main thread already applied; the stream replays the rest.
    for (const hosted of projects.values()) {
      hosted.generation = nextGeneration++;
      void hosted.processing.then(() => {
        if (!hosted.controller.signal.aborted && mode === 'fallback' && !hosted.fallbackStop) startMainThread(hosted);
      });
    }
  };

  const handleMessage = (event: Event) => {
    const message = parseChangeStreamWorkerMessage((event as MessageEvent).data);
    if (!message) return;
    if (message.type === 'ready') {
      if (message.protocol !== CHANGE_STREAM_WORKER_PROTOCOL) {
        fallBack(`Change-stream worker protocol ${message.protocol} does not match ${CHANGE_STREAM_WORKER_PROTOCOL}.`);
        return;
      }
      ready = true;
      if (readyTimer !== undefined) clearTimeout(readyTimer);
      readyTimer = undefined;
      return;
    }
    const hosted = projects.get(message.project);
    if (!hosted || hosted.generation !== message.generation || mode !== 'worker') return;
    if (message.type === 'error') {
      hosted.options.onError?.(new Error(message.message));
      return;
    }
    const { generation, seq } = message;
    hosted.processing = hosted.processing.then(async () => {
      if (hosted.controller.signal.aborted || hosted.generation !== generation) return;
      if (message.kind === 'resync') {
        hosted.cursor = Math.max(hosted.cursor ?? 0, message.cursor);
        await hosted.consumer.resync(message.reason, message.cursor);
      } else {
        const applied = hosted.cursor;
        // Replay after a restart can repeat what was already applied; drop those events.
        const events =
          applied === undefined
            ? message.response.events
            : message.response.events.filter((item) => item.cursor === undefined || item.cursor > applied);
        await hosted.consumer.deliver({ ...message.response, events }, message.force);
        hosted.cursor = Math.max(hosted.cursor ?? 0, message.response.cursor);
      }
      if (hosted.generation === generation && mode === 'worker')
        post({ type: 'ack', project: message.project, generation, seq });
    });
  };

  const crashed = (reason: string) => {
    if (mode !== 'worker' || disposed) return;
    stopWorker();
    const at = now();
    crashes.push(at);
    while (crashes.length && at - crashes[0] > restartWindowMs) crashes.shift();
    if (crashes.length > maxRestarts) {
      fallBack(`Change-stream worker crashed ${crashes.length} times: ${reason}`);
      return;
    }
    const delay = restartDelayMs * 2 ** (crashes.length - 1);
    restartTimer = setTimeout(() => {
      restartTimer = undefined;
      launch(true);
    }, delay);
  };

  function launch(restart: boolean) {
    if (disposed || mode !== 'worker') return;
    let created: ChangeStreamWorkerLike | undefined;
    try {
      created = options.createWorker();
    } catch {
      created = undefined;
    }
    if (!created) {
      fallBack('Change-stream worker is unavailable.');
      return;
    }
    worker = created;
    created.addEventListener('message', (event) => {
      if (worker === created) handleMessage(event);
    });
    created.addEventListener('error', (event) => {
      event.preventDefault();
      if (worker === created) crashed('error');
    });
    created.addEventListener('messageerror', () => {
      if (worker === created) crashed('messageerror');
    });
    post({ type: 'hello', protocol: CHANGE_STREAM_WORKER_PROTOCOL });
    readyTimer = setTimeout(() => {
      readyTimer = undefined;
      if (!ready) fallBack('Change-stream worker did not become ready.');
    }, readyTimeoutMs);
    for (const [project, hosted] of projects) {
      if (restart) hosted.generation = nextGeneration++;
      // A restarted worker resumes from the last applied cursor, or reconciles as a reconnect.
      openInWorker(project, hosted, restart ? (resumeOf(hosted) ?? {}) : resumeOf(hosted));
    }
  }

  return {
    /** Follow one project's change stream; the returned function stops it. */
    start(project: string, streamOptions: ProjectChangeStreamOptions, endpoints: ChangeStreamEndpoints): () => void {
      projects.get(project)?.controller.abort();
      const controller = new AbortController();
      const hosted: HostedProject = {
        generation: nextGeneration++,
        options: streamOptions,
        endpoints,
        controller,
        consumer: createProjectChangeConsumer(streamOptions, controller.signal),
        processing: Promise.resolve(),
      };
      projects.set(project, hosted);
      if (mode === 'fallback') startMainThread(hosted);
      else if (!worker && restartTimer === undefined) launch(false);
      else if (worker) openInWorker(project, hosted);
      return () => {
        if (projects.get(project) !== hosted) return;
        projects.delete(project);
        controller.abort();
        hosted.fallbackStop?.();
        if (mode === 'worker') post({ type: 'close', project, generation: hosted.generation });
      };
    },
    mode: () => mode,
    /** Testing hook: generation currently assigned to a project. */
    generation: (project: string) => projects.get(project)?.generation,
    dispose(): void {
      disposed = true;
      for (const hosted of projects.values()) {
        hosted.controller.abort();
        hosted.fallbackStop?.();
      }
      projects.clear();
      stopWorker();
    },
  };
}
