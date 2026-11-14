/**
 * HS2-S3FKJ7: the orchestration line of a project's own tickets in its List and Columns. The project is
 * a target of an orchestration project on this machine; the server reads that plan from the target side
 * (`GET /checkouts/{checkout}/orchestration/target-lines`). One read when the ticket views show; while
 * the project is orchestrated, a cursor long poll keeps the lines current. Nothing polls on a timer, and
 * a project nothing orchestrates is read again only when its ticket views show again.
 */
import { effect, type ReadonlySignal, signal } from 'kerfjs';

import type { TicketRowOrchestration } from '../components/ticket-row';
import type { Project } from '../interactions/types';
import type { OrchestrationTargetLines } from '../orchestration-timeline';
import { targetLineKey, targetLineMap } from '../orchestration-timeline-board';
import { startVersionedLongPoll } from '../versioned-long-poll';

export interface OrchestrationTargetLinesClient {
  orchestrationTargetLines: (
    checkout: string,
    after?: string,
    signal?: AbortSignal,
  ) => Promise<OrchestrationTargetLines>;
}

export interface OrchestrationTargetLinesDependencies {
  /** The project whose ticket views show, or `undefined` while none does (another view, settings, no project). */
  readonly shown: ReadonlySignal<Project | undefined>;
  readonly client: (project: Project) => OrchestrationTargetLinesClient;
  readonly delay?: (ms: number, signal: AbortSignal) => Promise<void>;
  /**
   * Run the first read after the next paint, so a warm project switch paints its cached tickets (and the
   * lines last read for them) first (HS2-8YHY6V). Returns a cancel. Tests pass an immediate one.
   */
  readonly afterPaint?: (run: () => void) => () => void;
}

const afterNextPaint = (run: () => void) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const frame =
    typeof requestAnimationFrame === 'function'
      ? requestAnimationFrame(() => {
          timer = setTimeout(run, 0);
        })
      : undefined;
  if (frame === undefined) timer = setTimeout(run, 0);
  return () => {
    if (frame !== undefined) cancelAnimationFrame(frame);
    if (timer !== undefined) clearTimeout(timer);
  };
};

export function createOrchestrationTargetLines(dependencies: OrchestrationTargetLinesDependencies) {
  /** The lines last read per project, so returning to a project paints them with its cached tickets. */
  const lines = signal<Readonly<Partial<Record<string, ReadonlyMap<string, TicketRowOrchestration>>>>>({});
  let stop: (() => void) | undefined;
  const halt = () => {
    stop?.();
    stop = undefined;
  };
  const dispose = effect(() => {
    const project = dependencies.shown.value;
    halt();
    if (!project) return;
    const client = dependencies.client(project);
    const apply = (value: OrchestrationTargetLines) => {
      lines.value = {
        ...lines.peek(),
        [project.id]: value.lines.length ? targetLineMap(value.lines) : undefined,
      };
    };
    let current: (() => void) | undefined;
    const controller = new AbortController();
    // The first read decides: nothing orchestrates the project, so no long poll holds a request open.
    const cancelRead = (dependencies.afterPaint ?? afterNextPaint)(() => {
      void client
        .orchestrationTargetLines(project.id, undefined, controller.signal)
        .then((first) => {
          if (controller.signal.aborted) return;
          apply(first);
          if (!first.orchestrated) return;
          let seen = first.version;
          current = startVersionedLongPoll({
            read: (after, signal) => client.orchestrationTargetLines(project.id, after ?? seen, signal),
            onValue: (value) => {
              seen = value.version;
              apply(value);
            },
            onError: () => {
              // Keep the last lines while the read backs off; the next answer replaces them.
            },
            delay: dependencies.delay,
          });
        })
        .catch(() => {
          // A server without the route, or a failed first read, shows no lines.
        });
    });
    stop = () => {
      cancelRead();
      controller.abort();
      current?.();
    };
  });
  return {
    /** The line of one of the shown project's tickets, when an orchestration project plans it. */
    lineFor(projectId: string | undefined, ticket: { connection_id: string; native_id: string }) {
      if (projectId === undefined) return undefined;
      return lines.value[projectId]?.get(targetLineKey(ticket.connection_id, ticket.native_id));
    },
    dispose() {
      dispose();
      halt();
    },
  };
}

export type OrchestrationTargetLinesController = ReturnType<typeof createOrchestrationTargetLines>;
