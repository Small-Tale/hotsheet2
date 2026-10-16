/**
 * Development-only latency budget for user interactions and server requests (HS2-ZGBQ5Q).
 *
 * From the user's point of view an action is too slow when the time from the click (or key press)
 * until the action has visibly happened exceeds 100 ms on average or 150 ms at peak. Expected
 * long-running work (AI turns, terminal launches, project setup, uploads) only has to show strong
 * feedback within that budget, so its requests are not waited for.
 *
 * This module is pure logic with injected clocks and schedulers: the budget evaluator, request
 * classification and route templating, and the interaction tracker that attributes the requests an
 * interaction started and waits for them plus the next paint. `dev-latency-entry.ts` wires it to
 * the DOM, the API client, the visible dev indicator, and the dev bridge's durable log. Nothing
 * here is imported by a production build.
 */

/** Rolling-average budget for interactions and requests. */
export const DEV_LATENCY_AVERAGE_BUDGET_MS = 100;
/** Any single interaction or request above this is flagged immediately. */
export const DEV_LATENCY_PEAK_BUDGET_MS = 150;
/** Samples in the rolling-average window. */
export const DEV_LATENCY_WINDOW = 20;
/** The average is judged only once this many samples exist, so one cold start is not an average. */
export const DEV_LATENCY_MIN_AVERAGE_SAMPLES = 5;
/**
 * A request that starts this soon after an interaction belongs to it. So does one started before the
 * settle timer actually ran, when the interaction's own synchronous work delayed that timer.
 */
export const DEV_LATENCY_SETTLE_MS = 50;
/** An interaction still waiting after this long is reported as-is rather than tracked forever. */
export const DEV_LATENCY_MAX_WAIT_MS = 60_000;
/** `localStorage` switch: `on` enables the monitor under automation, `off` disables it in dev. */
export const DEV_LATENCY_STORAGE_KEY = 'hotsheet-dev-latency';
/** Dev bridge endpoint that keeps the durable report log. */
export const DEV_LATENCY_ENDPOINT = '/__hotsheet/dev/latency';

export type LatencyKind = 'interaction' | 'request';
export type LatencyReason = 'peak' | 'average';

export interface RequestTiming {
  /** Method plus templated route, for example `PATCH /checkouts/{id}/tickets/{id}`. */
  label: string;
  duration_ms: number;
  /** Server handler time from the response's `Server-Timing: handler` entry, when present. */
  server_ms?: number;
}

export interface LatencySample {
  kind: LatencyKind;
  label: string;
  duration_ms: number;
  /** The requests an interaction waited for (interactions only). */
  requests?: RequestTiming[];
  /** Server handler time (requests only). */
  server_ms?: number;
}

export interface LatencyViolation {
  sample: LatencySample;
  reasons: LatencyReason[];
  /** Rolling average of this kind's window, including the sample. */
  average_ms: number;
  average_budget_ms: number;
  peak_budget_ms: number;
}

/**
 * Judge each sample against the peak budget and its kind's rolling average. The average violation
 * fires once when the average crosses above budget and re-arms only after it falls back within it,
 * so a sustained slow period produces one average report instead of one per sample.
 */
export function createLatencyBudget(
  options: { averageMs?: number; peakMs?: number; window?: number; minAverageSamples?: number } = {},
) {
  const averageMs = options.averageMs ?? DEV_LATENCY_AVERAGE_BUDGET_MS,
    peakMs = options.peakMs ?? DEV_LATENCY_PEAK_BUDGET_MS,
    windowSize = options.window ?? DEV_LATENCY_WINDOW,
    minSamples = options.minAverageSamples ?? DEV_LATENCY_MIN_AVERAGE_SAMPLES;
  const windows: Record<LatencyKind, number[]> = { interaction: [], request: [] },
    averageFlagged: Record<LatencyKind, boolean> = { interaction: false, request: false };
  return {
    record(sample: LatencySample): LatencyViolation | undefined {
      const window = windows[sample.kind];
      window.push(sample.duration_ms);
      if (window.length > windowSize) window.shift();
      const average = window.reduce((sum, value) => sum + value, 0) / window.length;
      const reasons: LatencyReason[] = [];
      if (sample.duration_ms > peakMs) reasons.push('peak');
      if (window.length >= minSamples && average > averageMs) {
        if (!averageFlagged[sample.kind]) reasons.push('average');
        averageFlagged[sample.kind] = true;
      } else averageFlagged[sample.kind] = false;
      if (reasons.length === 0) return undefined;
      return { sample, reasons, average_ms: average, average_budget_ms: averageMs, peak_budget_ms: peakMs };
    },
    /** The current rolling average for a kind (0 with no samples). */
    average(kind: LatencyKind): number {
      const window = windows[kind];
      return window.length === 0 ? 0 : window.reduce((sum, value) => sum + value, 0) / window.length;
    },
  };
}

/** Requests whose work is expected to take longer than the budget; only their feedback is judged. */
const LONG_RUNNING_ROUTES: readonly RegExp[] = [
  /\/drive\//,
  /\/turns(?:\/|$)/,
  /\/terminals(?:\/[^/]+)?\/(?:open|launch|restart|connect-ai)$/,
  /^\/terminals$/,
  /\/setup(?:\/|$)/,
  /\/setup-git/,
  /\/migrate/,
  /\/migration-jobs/,
  /\/tts\//,
  /\/commands\/[^/]+\/run$/,
  /\/lifecycle\//,
  /\/sync(?:\/|$)/,
  /\/projects\/open$/,
  /\/recover-unhealthy$/,
];

/** Whether a request is expected to be long-running (its duration is not budgeted). */
export function isLongRunningRequest(method: string, path: string): boolean {
  const pathname = path.split('?', 1)[0] ?? path;
  if (method.toUpperCase() !== 'GET' && /\/attachments(?:\/|$)/.test(pathname)) return true;
  return LONG_RUNNING_ROUTES.some((route) => route.test(pathname));
}

const ULID = /^[0-9A-HJKMNP-TV-Z]{26}$/i;
const SLUG = /^[A-Z][A-Z0-9]*-[0-9A-Z]{4,}$/;
const NUMERIC = /^\d+$/;
const HEXISH = /^[0-9a-f]{12,}$/i;
/** A checkout id (`folder-shortpathhash`): a trailing hash segment containing a digit. */
const CHECKOUT_ID = /^.+-(?=[0-9a-z]*\d)[0-9a-z]{6,}$/;

/**
 * `METHOD /route/template`: the query is dropped and id-like segments (ULIDs, slugs, numbers,
 * hashes, percent-encoded paths, and the dev bridge's project segment) become `{id}`, so reports
 * aggregate by route and never carry ticket ids or checkout paths.
 */
export function requestRouteLabel(method: string, path: string): string {
  const pathname = (path.split('?', 1)[0] ?? path).replace(/^\/__hotsheet\/project-api\/[^/]+/, '');
  const segments = pathname
    .split('/')
    .map((segment) =>
      segment.includes('%') ||
      ULID.test(segment) ||
      SLUG.test(segment) ||
      NUMERIC.test(segment) ||
      HEXISH.test(segment) ||
      CHECKOUT_ID.test(segment)
        ? '{id}'
        : segment,
    );
  return `${method.toUpperCase()} ${segments.join('/') || '/'}`;
}

/** The `handler` duration from a `Server-Timing` header, if present. */
export function serverHandlerMs(header: string | null | undefined): number | undefined {
  if (!header) return undefined;
  for (const entry of header.split(',')) {
    const [name, ...params] = entry.trim().split(';');
    if (name.trim() !== 'handler') continue;
    for (const param of params) {
      const [key, value = ''] = param.trim().split('=');
      if (key === 'dur' && value) {
        const parsed = Number(value);
        if (Number.isFinite(parsed)) return parsed;
      }
    }
  }
  return undefined;
}

export interface InteractionTrackerOptions {
  now: () => number;
  /** Run `callback` once the current update has painted (next frame, then a task). */
  afterPaint: (callback: () => void) => void;
  /** One-shot local timer (never a network request). */
  setTimer: (callback: () => void, ms: number) => void;
  onComplete: (sample: LatencySample) => void;
  settleMs?: number;
  maxWaitMs?: number;
}

interface PendingInteraction {
  label: string;
  started: number;
  settleUntil: number;
  open: number;
  settled: boolean;
  done: boolean;
  requests: RequestTiming[];
}

/**
 * Attribute requests to the interaction that started them and complete each interaction after its
 * settle window has passed, every attributed request has finished, and the result has painted.
 */
export function createInteractionTracker(options: InteractionTrackerOptions) {
  const settleMs = options.settleMs ?? DEV_LATENCY_SETTLE_MS,
    maxWaitMs = options.maxWaitMs ?? DEV_LATENCY_MAX_WAIT_MS;
  const pending: PendingInteraction[] = [];
  const finish = (interaction: PendingInteraction) => {
    if (interaction.done) return;
    interaction.done = true;
    const index = pending.indexOf(interaction);
    if (index >= 0) pending.splice(index, 1);
    options.onComplete({
      kind: 'interaction',
      label: interaction.label,
      duration_ms: options.now() - interaction.started,
      requests: interaction.requests,
    });
  };
  const maybeFinish = (interaction: PendingInteraction) => {
    if (interaction.done || !interaction.settled || interaction.open > 0) return;
    options.afterPaint(() => {
      finish(interaction);
    });
  };
  return {
    /** Begin timing an interaction that the user started at `startedAt` (event time). */
    begin(label: string, startedAt = options.now()): void {
      const interaction: PendingInteraction = {
        label,
        started: startedAt,
        settleUntil: startedAt + settleMs,
        open: 0,
        settled: false,
        done: false,
        requests: [],
      };
      pending.push(interaction);
      options.setTimer(
        () => {
          interaction.settled = true;
          maybeFinish(interaction);
        },
        Math.max(0, interaction.settleUntil - options.now()),
      );
      options.setTimer(() => {
        finish(interaction);
      }, maxWaitMs);
    },
    /**
     * A request started. Returns its completion callback; an expected long-running request is not
     * attributed (the interaction then only waits for its paint, which is the strong feedback).
     */
    requestStarted(label: string, longRunning: boolean): (timing: RequestTiming) => void {
      const at = options.now();
      const owner = longRunning
        ? undefined
        : // A request the interaction's own work starts before the settle timer could run (a busy
          // main thread delays that timer) still belongs to it.
          [...pending]
            .reverse()
            .find((interaction) => !interaction.done && (!interaction.settled || at <= interaction.settleUntil));
      if (owner) owner.open += 1;
      let ended = false;
      return (timing) => {
        if (ended || !owner) return;
        ended = true;
        owner.open -= 1;
        owner.requests.push({ ...timing, label: timing.label || label });
        maybeFinish(owner);
      };
    },
    /** Interactions still being timed (test helper). */
    pendingCount(): number {
      return pending.length;
    },
  };
}

/** A short human label for the control an interaction targeted. */
export interface LabelTarget {
  closest(selector: string): LabelTarget | null;
  getAttribute(name: string): string | null;
  readonly textContent: string | null;
  readonly tagName: string;
}

export function interactionLabel(target: LabelTarget | null, kind: string): string {
  const control = target?.closest(
    'button, a, [role="button"], [role="menuitem"], [role="tab"], [role="option"], [role="row"], wa-button, wa-menu-item, input, select, summary, [data-dev-latency-label]',
  );
  const element = control ?? target;
  if (!element) return kind;
  const named =
    element.getAttribute('data-dev-latency-label') ??
    element.getAttribute('aria-label') ??
    element.getAttribute('title') ??
    (element.textContent ?? '').replace(/\s+/g, ' ').trim();
  const text = named.length > 48 ? `${named.slice(0, 47)}…` : named;
  return `${kind} ${element.tagName.toLowerCase()}${text ? ` "${text}"` : ''}`;
}

/** One human-readable line describing a violation, for the console and the dev indicator. */
export function describeViolation(violation: LatencyViolation): string {
  const { sample } = violation,
    duration = Math.round(sample.duration_ms),
    what = sample.kind === 'interaction' ? 'interaction' : 'request',
    budget = violation.reasons.includes('peak')
      ? `peak budget ${violation.peak_budget_ms} ms`
      : `average ${Math.round(violation.average_ms)} ms over ${violation.average_budget_ms} ms budget`;
  const slowest = [...(sample.requests ?? [])].sort((a, b) => b.duration_ms - a.duration_ms).at(0);
  const server = sample.server_ms ?? slowest?.server_ms;
  const cause = slowest
    ? `; slowest request ${slowest.label} ${Math.round(slowest.duration_ms)} ms`
    : sample.kind === 'interaction'
      ? '; no server request (client render)'
      : '';
  const serverPart = server === undefined ? '' : ` (server ${Math.round(server)} ms)`;
  return `Slow ${what}: ${sample.label} took ${duration} ms (${budget})${cause}${serverPart}`;
}

/** A short line for the visible dev indicator; the console and log keep the full description. */
export function summarizeViolation(violation: LatencyViolation, slowCount = 1): string {
  const { sample } = violation,
    label = sample.label.length > 56 ? `${sample.label.slice(0, 55)}…` : sample.label,
    budget = violation.reasons.includes('peak')
      ? `${violation.peak_budget_ms} ms peak`
      : `${violation.average_budget_ms} ms average`;
  const count = slowCount > 1 ? ` · ${slowCount} slow so far` : '';
  return `Dev: slow ${sample.kind}, ${Math.round(sample.duration_ms)} ms (budget ${budget}): ${label}${count}`;
}

/** Whether the monitor runs: on in a dev browser, off under automation unless explicitly enabled. */
export function devLatencyEnabled(setting: string | null, automated: boolean): boolean {
  if (setting === 'off') return false;
  if (setting === 'on') return true;
  return !automated;
}
