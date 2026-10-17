/**
 * The bridge half of the multiplexed change long-poll (HS2-6W1NNF).
 *
 * When WebSocket upgrades keep failing, every open project falls back to a replay long-poll
 * held for up to 25 s. Each one used to be its own browser request, so six or more open
 * projects filled the browser's per-origin HTTP/1.1 connection pool and ordinary requests
 * queued behind them. The browser now sends one request carrying every waiting project's
 * poll; the bridge forwards each to that project's own upstream `/ws/poll` (keeping its
 * cursor, timeout, and `checkout=` presence) and answers as soon as any of them completes,
 * together with any others that complete within a short grace window. The rest are aborted
 * and the browser reissues them. Nothing is consumed by an aborted poll: the browser's
 * cursor only advances on a delivered page, so replay semantics are unchanged.
 */

/** Wire version of the multiplexed poll. A client that sees any other answer polls directly. */
export const CHANGE_MUX_VERSION = 1;
/** Browser-facing endpoint path. */
export const CHANGE_MUX_PATH = '/__hotsheet/change-mux/poll';
/** How long the bridge keeps collecting other completions after the first one. */
export const CHANGE_MUX_GRACE_MS = 50;
/** Upper bound on the projects one request may carry. */
export const CHANGE_MUX_MAX_POLLS = 64;
/** The server's long-poll ceiling; a larger requested timeout is clamped. */
const MAX_TIMEOUT_MS = 30_000;

export interface ChangeMuxPoll {
  project: string;
  /** Required in v2 so several tabs can poll the same project in one request. */
  client?: string;
  since?: number;
  timeout_ms: number;
}

export interface ChangeMuxRequest {
  version: number;
  client: string;
  polls: ChangeMuxPoll[];
}

export interface ChangeMuxResult {
  project: string;
  client?: string;
  status: number;
  body: unknown;
}

export interface ChangeMuxResponse {
  version: number;
  results: ChangeMuxResult[];
}

/** Forward one project's upstream poll. Must honor `signal`. */
export type ChangeMuxForward = (project: string, path: string, signal: AbortSignal) => Promise<Response>;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** Validate a browser request body; returns an error message for a malformed one. */
export function parseChangeMuxRequest(value: unknown): ChangeMuxRequest | string {
  if (!isRecord(value)) return 'Expected a JSON object.';
  if (value.version !== CHANGE_MUX_VERSION && value.version !== 2)
    return `Unsupported change multiplex version ${String(value.version)}.`;
  if (typeof value.client !== 'string') return 'Expected a client id.';
  if (!Array.isArray(value.polls) || value.polls.length === 0) return 'Expected at least one poll.';
  if (value.polls.length > CHANGE_MUX_MAX_POLLS) return `At most ${CHANGE_MUX_MAX_POLLS} polls per request.`;
  const seen = new Set<string>();
  const polls: ChangeMuxPoll[] = [];
  for (const poll of value.polls as unknown[]) {
    if (!isRecord(poll) || typeof poll.project !== 'string' || poll.project === '') return 'Invalid poll entry.';
    if (value.version === 2 && typeof poll.client !== 'string') return 'Expected a poll client id.';
    const client = value.version === 2 ? (poll.client as string) : value.client;
    const key = `${client}\u0000${poll.project}`;
    if (seen.has(key)) return `Duplicate poll for project ${poll.project}.`;
    seen.add(key);
    if (poll.since !== undefined && (!Number.isSafeInteger(poll.since) || (poll.since as number) < 0))
      return 'Invalid poll cursor.';
    if (!Number.isSafeInteger(poll.timeout_ms) || (poll.timeout_ms as number) < 0) return 'Invalid poll timeout.';
    polls.push({
      project: poll.project,
      ...(value.version === 2 ? { client } : {}),
      ...(poll.since === undefined ? {} : { since: poll.since as number }),
      timeout_ms: Math.min(poll.timeout_ms as number, MAX_TIMEOUT_MS),
    });
  }
  return { version: value.version, client: value.client, polls };
}

/** The project-relative upstream path for one poll; the project proxy adds presence. */
export function changeMuxPollPath(poll: ChangeMuxPoll, client: string): string {
  return `/ws/poll?timeout_ms=${poll.timeout_ms}${poll.since === undefined ? '' : `&since=${poll.since}`}&client=${encodeURIComponent(client)}`;
}

const settleBody = async (response: Response): Promise<unknown> => {
  const text = await response.text();
  try {
    return text ? (JSON.parse(text) as unknown) : null;
  } catch {
    return { error: text };
  }
};

/**
 * Run one multiplexed poll: resolve with the first completion plus any that land within
 * `graceMs`, then abort the others. `signal` is the browser request's; its abort cancels
 * every upstream poll.
 */
export async function multiplexChangePolls(
  request: ChangeMuxRequest,
  forward: ChangeMuxForward,
  signal: AbortSignal,
  graceMs = CHANGE_MUX_GRACE_MS,
): Promise<ChangeMuxResponse> {
  const controller = new AbortController();
  const abortAll = () => {
    controller.abort();
  };
  if (signal.aborted) abortAll();
  else signal.addEventListener('abort', abortAll, { once: true });
  const results: ChangeMuxResult[] = [];
  let first: (() => void) | undefined;
  const firstDone = new Promise<void>((resolve) => {
    first = resolve;
  });
  const settled = request.polls.map(async (poll) => {
    let result: ChangeMuxResult;
    try {
      const response = await forward(
        poll.project,
        changeMuxPollPath(poll, poll.client ?? request.client),
        controller.signal,
      );
      result = {
        project: poll.project,
        ...(request.version === 2 ? { client: poll.client } : {}),
        status: response.status,
        body: await settleBody(response),
      };
    } catch (error) {
      if (controller.signal.aborted) return;
      result = {
        project: poll.project,
        ...(request.version === 2 ? { client: poll.client } : {}),
        status: 502,
        body: { error: error instanceof Error ? error.message : 'Project change poll failed.' },
      };
    }
    if (controller.signal.aborted) return;
    results.push(result);
    first?.();
  });
  const allDone = Promise.all(settled);
  const aborted = new Promise<void>((resolve) => {
    if (controller.signal.aborted) resolve();
    else
      controller.signal.addEventListener(
        'abort',
        () => {
          resolve();
        },
        { once: true },
      );
  });
  await Promise.race([firstDone, allDone, aborted]);
  if (!controller.signal.aborted && results.length < request.polls.length) {
    let timer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([
      allDone,
      aborted,
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, graceMs);
      }),
    ]);
    if (timer !== undefined) clearTimeout(timer);
  }
  const answered = results.slice();
  signal.removeEventListener('abort', abortAll);
  abortAll();
  return { version: request.version, results: answered };
}
