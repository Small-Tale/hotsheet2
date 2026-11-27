import { signal } from 'kerfjs';

import type { OrchestrationCapacity } from './api';
import { describeRequestFailure } from './bounded-backoff';
import { startVersionedLongPoll } from './versioned-long-poll';

/** What the Systems screen last read from one machine's capacity stream. */
export interface SystemsCapacityState {
  capacity?: OrchestrationCapacity;
  /** The last read failed; the last snapshot (if any) stays and may be out of date. */
  error?: string;
}

/** The read one stream makes: the hosted capacity snapshot through a project's path. */
export type SystemsCapacityRead = (
  projectId: string,
  after: string | undefined,
  signal: AbortSignal,
) => Promise<OrchestrationCapacity>;

/** One machine the Systems screen should be reading, through which open project. */
export interface SystemsCapacityTarget {
  machineId: string;
  projectId: string;
}

/** An older host sends no version; its answers never change one, so its stream backs off. */
const UNVERSIONED = 'unversioned';

/**
 * The Systems screen's capacity streams (HS2-DR0ZSQ): one cursor long poll per machine over the
 * hosted capacity snapshot (`GET /checkouts/{checkout}/target-capacity?after=<version>&wait_ms=`,
 * HS2-P5GTSW). Each read passes the version it last saw, so the server answers when the machine's
 * room changes; failures back off 1 s doubling to 30 s (`versioned-long-poll.ts`). `sync` starts the
 * streams the screen needs and stops the rest; `stop` ends them all. Nothing reads on a timer, and
 * the last reading of a stopped stream stays for the next time the screen shows.
 */
export function createSystemsCapacityFeeds(
  read: SystemsCapacityRead,
  options: { delay?: (ms: number, signal: AbortSignal) => Promise<void>; now?: () => number } = {},
) {
  const states = signal<ReadonlyMap<string, SystemsCapacityState>>(new Map());
  const running = new Map<string, { projectId: string; stop: () => void }>();

  function update(machineId: string, change: (state: SystemsCapacityState) => SystemsCapacityState) {
    const next = new Map(states.peek());
    next.set(machineId, change(next.get(machineId) ?? {}));
    states.value = next;
  }

  function start({ machineId, projectId }: SystemsCapacityTarget) {
    const stop = startVersionedLongPoll({
      read: async (after, abort) => {
        const capacity = await read(projectId, after === UNVERSIONED ? undefined : after, abort);
        return { capacity, version: capacity.version ?? UNVERSIONED };
      },
      onValue: ({ capacity }) => {
        if (running.get(machineId)?.stop !== stop) return;
        update(machineId, () => ({ capacity }));
      },
      onError: (reason) => {
        if (running.get(machineId)?.stop !== stop) return;
        update(machineId, (state) => ({ ...state, error: describeRequestFailure(reason) }));
      },
      ...(options.delay ? { delay: options.delay } : {}),
      ...(options.now ? { now: options.now } : {}),
    });
    running.set(machineId, { projectId, stop });
  }

  /** Read exactly these machines: start the new ones, restart one whose project changed, stop the rest. */
  function sync(targets: readonly SystemsCapacityTarget[]) {
    const wanted = new Map(targets.map((target) => [target.machineId, target]));
    for (const [machineId, stream] of running) {
      const target = wanted.get(machineId);
      if (target && target.projectId === stream.projectId) continue;
      stream.stop();
      running.delete(machineId);
    }
    for (const target of wanted.values()) if (!running.has(target.machineId)) start(target);
  }

  /** Stop every stream and abort its open read. */
  function stop() {
    sync([]);
  }

  /** Forget a machine's readings (it was unpaired or its last tab closed). */
  function forget(machineId: string) {
    if (!states.peek().has(machineId)) return;
    const next = new Map(states.peek());
    next.delete(machineId);
    states.value = next;
  }

  return { states, sync, stop, forget, running: () => [...running.keys()] };
}

export type SystemsCapacityFeeds = ReturnType<typeof createSystemsCapacityFeeds>;
