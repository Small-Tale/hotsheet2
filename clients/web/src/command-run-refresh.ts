/**
 * Keeps a command run this client just started from disappearing when a command refresh that
 * began before the start settles (HS2-YA6D31). A refresh captures before it reads; every local
 * start records its run id into each open capture; the refresh merges by run id and releases.
 */

export interface LocalRunStarts {
  /** Opens a capture for one refresh read. Release it when the refresh settles. */
  capture(): ReadonlySet<string>;
  release(capture: ReadonlySet<string>): void;
  /** Records a run this client started (a command run, a shell or AI launch record). */
  record(runId: string): void;
}

export function createLocalRunStarts(): LocalRunStarts {
  const open = new Set<Set<string>>();
  return {
    capture() {
      const started = new Set<string>();
      open.add(started);
      return started;
    },
    release(capture) {
      open.delete(capture as Set<string>);
    },
    record(runId) {
      for (const started of open) started.add(runId);
    },
  };
}

/**
 * The refreshed list: the server's runs, preceded by any run started locally during the read that
 * the server's list does not have yet. A run the server already lists takes the server's state.
 */
export function mergeRefreshedCommandRuns<R extends { id: string }>(
  server: readonly R[],
  current: readonly R[],
  startedDuringRead: ReadonlySet<string>,
): R[] {
  if (startedDuringRead.size === 0) return [...server];
  const listed = new Set(server.map((run) => run.id));
  return [...current.filter((run) => startedDuringRead.has(run.id) && !listed.has(run.id)), ...server];
}
