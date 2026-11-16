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
import { parseRemoteProjectId } from '../remote-projects/identity';
import { startVersionedLongPoll } from '../versioned-long-poll';

export interface OrchestrationTargetLinesClient {
  orchestrationTargetLines: (
    checkout: string,
    after?: string,
    signal?: AbortSignal,
  ) => Promise<OrchestrationTargetLines>;
}

/** The two reads an API client offers for target lines (HS2-3D8D6V). */
export interface TargetLinesApi extends OrchestrationTargetLinesClient {
  orchestrationRemoteTargetLines: (
    registration: string,
    checkout: string,
    after?: string,
    signal?: AbortSignal,
  ) => Promise<OrchestrationTargetLines>;
}

/** HS2-SHBSQF: the two halves of a combined version, this machine's then the remote server's; `-` is unread. */
const COMBINED = '~';
const combinedVersion = (here?: OrchestrationTargetLines, there?: OrchestrationTargetLines) =>
  `${here?.version ?? '-'}${COMBINED}${there?.version ?? '-'}`;

/**
 * HS2-SHBSQF: one answer from both halves. This machine's lines win per ticket; the remote server's lines (what
 * other machines pushed to it, HS2-KB2Y51) fill the tickets this machine does not plan.
 */
export function mergeTargetLines(
  here: OrchestrationTargetLines | undefined,
  there: OrchestrationTargetLines | undefined,
): OrchestrationTargetLines {
  const mine = here?.lines ?? [],
    taken = new Set(mine.map((line) => targetLineKey(line.item.target.connection_id, line.item.target.native_id)));
  return {
    version: combinedVersion(here, there),
    orchestrated: Boolean(here?.orchestrated) || Boolean(there?.orchestrated),
    lines: [
      ...mine,
      ...(there?.lines ?? []).filter(
        (line) => !taken.has(targetLineKey(line.item.target.connection_id, line.item.target.native_id)),
      ),
    ],
  };
}

type Half = 'here' | 'there';
type Settled = { half: Half; value: OrchestrationTargetLines } | { half: Half; error: unknown };

/**
 * HS2-3D8D6V, HS2-SHBSQF: where a shown project's lines come from. A local project asks its own server. A remote
 * project's tab with a local project open reads both halves: this machine's server, through that local project,
 * for the lines its orchestration projects plan through a remote binding (the plan never leaves this machine),
 * and the remote project's own server for the lines other machines pushed to it (HS2-KB2Y51). The two cursors
 * travel as one combined version; a long poll waits on both halves at once and answers when either changes,
 * cancelling the other. A half that fails keeps its last answer while the other still waits; only when both fail
 * does the read fail, so the caller's bounded backoff applies. With no local project open the tab asks the remote
 * project's server alone.
 */
export function targetLinesClientFor(
  project: Project,
  open: readonly Project[],
  api: (apiPath: string) => TargetLinesApi,
): OrchestrationTargetLinesClient {
  const remote = parseRemoteProjectId(project.id);
  const local = remote ? open.find((candidate) => !parseRemoteProjectId(candidate.id)) : undefined;
  if (!remote || !local) return api(project.apiPath);
  const here = api(local.apiPath),
    there = api(project.apiPath);
  const last: Partial<Record<Half, OrchestrationTargetLines>> = {};
  const read = (half: Half, checkout: string, after: string | undefined, signal: AbortSignal): Promise<Settled> =>
    (half === 'here'
      ? here.orchestrationRemoteTargetLines(remote.bindingId, remote.checkoutId, after, signal)
      : there.orchestrationTargetLines(checkout, after, signal)
    ).then(
      (value) => ({ half, value }),
      (error: unknown) => ({ half, error }),
    );
  return {
    orchestrationTargetLines: async (checkout, after, signal) => {
      const halves: Half[] = ['here', 'there'];
      const current = combinedVersion(last.here, last.there);
      if (after === undefined) {
        // A first read: both halves now, with no cursor.
        const answers = await Promise.all(
          halves.map((half) => read(half, checkout, undefined, signal ?? new AbortController().signal)),
        );
        for (const answer of answers) if ('value' in answer) last[answer.half] = answer.value;
        if (answers.every((answer) => 'error' in answer)) throw (answers[0] as { error: unknown }).error;
        return mergeTargetLines(last.here, last.there);
      }
      // A cursor from before this client's state (or a stale one) is answered with what is known now.
      if (after !== current) return mergeTargetLines(last.here, last.there);
      const inner = new AbortController();
      const stop = () => {
        inner.abort();
      };
      signal?.addEventListener('abort', stop, { once: true });
      try {
        // A half never read waits with no cursor, which answers at once; a read half waits on its own version.
        const pending = new Map<Half, Promise<Settled>>(
          halves.map((half) => [half, read(half, checkout, last[half]?.version, inner.signal)]),
        );
        let failure: unknown;
        while (pending.size) {
          const settled = await Promise.race(pending.values());
          pending.delete(settled.half);
          if ('value' in settled) {
            last[settled.half] = settled.value;
            return mergeTargetLines(last.here, last.there);
          }
          if (signal?.aborted) throw settled.error;
          failure = settled.error;
        }
        throw failure;
      } finally {
        signal?.removeEventListener('abort', stop);
        inner.abort();
      }
    },
  };
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
