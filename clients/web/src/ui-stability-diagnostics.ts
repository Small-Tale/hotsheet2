import type { ReviewAttachment } from './dev-review';
import type { RenderMetricsSnapshot } from './render-metrics';

const QUICK_DISMISS_MS = 1_000;
const USER_INTENT_GRACE_MS = 150;
const THRASH_WINDOW_MS = 10_000;
const THRASH_COUNT = 3;
const REPORT_COOLDOWN_MS = 60_000;
const RENDER_STORM_WINDOW_MS = 2_000;
const RENDER_STORM_COUNT = 12;
const STARTUP_GRACE_MS = 5_000;
const EVENT_LIMIT = 120;

export interface RenderStormState {
  passes: number[];
  reported: boolean;
}

export interface DismissalThrashState {
  dismissals: number[];
  reported: boolean;
}

export interface UiStabilityEvent {
  at: string;
  kind: string;
  target?: string;
  detail?: Record<string, unknown>;
}

export interface UiStabilityDiagnostics {
  attachment(): ReviewAttachment;
  recordRender(metrics: RenderMetricsSnapshot, automaticReportSuppressed?: string): void;
  destroy(): void;
}

interface UiStabilityOptions {
  document?: Document;
  now?: () => number;
  onThrash?: (attachment: ReviewAttachment) => void | Promise<void>;
}

export function isUnexpectedQuickDismiss(openedAt: number, dismissedAt: number, lastUserIntentAt: number): boolean {
  return dismissedAt - openedAt <= QUICK_DISMISS_MS && dismissedAt - lastUserIntentAt > USER_INTENT_GRACE_MS;
}

export function hasDismissalThrash(dismissals: readonly number[], now: number): boolean {
  return dismissals.filter(value => now - value <= THRASH_WINDOW_MS).length >= THRASH_COUNT;
}

/** Tracks one continuous dismissal episode and rearms after its bounded window clears. */
export function advanceDismissalThrash(state: DismissalThrashState, now: number): DismissalThrashState & { shouldReport: boolean } {
  const dismissals = [...state.dismissals.filter(value => now - value <= THRASH_WINDOW_MS), now];
  const thrashing = dismissals.length >= THRASH_COUNT;
  return { dismissals, reported: thrashing, shouldReport: thrashing && !state.reported };
}

/** Tracks one continuous root-render storm and rearms only after a quiet window. */
export function advanceRenderStorm(state: RenderStormState, now: number, suppressed = false): RenderStormState & { shouldReport: boolean } {
  if (suppressed) return { passes: [], reported: false, shouldReport: false };
  const passes = [...state.passes.filter(value => now - value <= RENDER_STORM_WINDOW_MS), now];
  const storming = passes.length >= RENDER_STORM_COUNT;
  return { passes, reported: storming, shouldReport: storming && !state.reported };
}

function encodeJson(value: unknown): string {
  const bytes = new TextEncoder().encode(JSON.stringify(value, null, 2));
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function describeElement(element: Element): string {
  const name = element.getAttribute('name');
  const id = element.id;
  const component = element.closest('[data-component]')?.getAttribute('data-component');
  return `${element.localName}${id ? `#${id}` : ''}${name ? `[name="${name}"]` : ''}${component ? ` in ${component}` : ''}`;
}

/** Development-only recorder for unexpected control dismissal and render storms. */
export function installUiStabilityDiagnostics(options: UiStabilityOptions = {}): UiStabilityDiagnostics {
  const doc = options.document ?? document;
  const view = doc.defaultView ?? window;
  const now = options.now ?? (() => Date.now());
  const installedAt = now();
  const events: UiStabilityEvent[] = [];
  const openedSelects = new WeakMap<Element, number>();
  let lastUserIntentAt = Number.NEGATIVE_INFINITY;
  let dismissalThrash: DismissalThrashState = { dismissals: [], reported: false };
  let renderStorm: RenderStormState = { passes: [], reported: false };
  let previousRenderMetrics: RenderMetricsSnapshot = { passes: 0, mutations: 0 };
  let lastReportAt = Number.NEGATIVE_INFINITY;
  let reporting = false;

  const record = (kind: string, target?: Element, detail?: Record<string, unknown>) => {
    events.push({ at: new Date(now()).toISOString(), kind, target: target ? describeElement(target) : undefined, detail });
    if (events.length > EVENT_LIMIT) events.splice(0, events.length - EVENT_LIMIT);
  };
  const attachment = (): ReviewAttachment => {
    const timestamp = now();
    const payload = {
      schema: 'hotsheet/ui-stability-diagnostics/v1',
      captured_at: new Date(timestamp).toISOString(),
      page: view.location.href,
      viewport: { width: view.innerWidth, height: view.innerHeight, device_pixel_ratio: view.devicePixelRatio },
      user_agent: view.navigator.userAgent,
      events,
    };
    const text = JSON.stringify(payload, null, 2);
    return { id: `ui-stability-${timestamp}`, filename: `hotsheet-ui-diagnostics-${timestamp}.json`, dataUrl: `data:application/json;base64,${encodeJson(payload)}`, mimeType: 'application/json', size: new TextEncoder().encode(text).byteLength };
  };
  const report = (kind: string) => {
    const timestamp = now();
    if (!options.onThrash || reporting || timestamp - lastReportAt < REPORT_COOLDOWN_MS) return;
    lastReportAt = timestamp;
    reporting = true;
    record('automatic-report', undefined, { trigger: kind });
    Promise.resolve(options.onThrash(attachment())).catch(() => undefined).finally(() => { reporting = false; });
  };
  const noteUserIntent = () => { lastUserIntentAt = now(); };
  const onShow = (event: Event) => {
    const target = event.target;
    if (!(target instanceof Element) || !target.matches('wa-select')) return;
    openedSelects.set(target, now());
    record('select-opened', target);
  };
  const onHide = (event: Event) => {
    const target = event.target;
    if (!(target instanceof Element) || !target.matches('wa-select')) return;
    const dismissedAt = now();
    const openedAt = openedSelects.get(target);
    openedSelects.delete(target);
    if (openedAt === undefined) return;
    const unexpected = isUnexpectedQuickDismiss(openedAt, dismissedAt, lastUserIntentAt);
    record('select-closed', target, { open_ms: dismissedAt - openedAt, unexpected });
    if (!unexpected) return;
    const nextDismissalThrash = advanceDismissalThrash(dismissalThrash, dismissedAt);
    dismissalThrash = nextDismissalThrash;
    if (nextDismissalThrash.shouldReport) report('repeated-unexpected-select-dismissal');
  };
  const observer = new MutationObserver(records => {
    let removedTrackedSelects = 0;
    for (const mutation of records) for (const node of mutation.removedNodes) {
      if (!(node instanceof Element)) continue;
      const selects = [node.matches('wa-select') ? node : undefined, ...node.querySelectorAll('wa-select')].filter((value): value is Element => Boolean(value));
      for (const select of selects) {
        const openedAt = openedSelects.get(select);
        if (openedAt === undefined) continue;
        openedSelects.delete(select);
        removedTrackedSelects += 1;
        const timestamp = now();
        record('open-select-removed', select, { open_ms: timestamp - openedAt });
        const nextDismissalThrash = advanceDismissalThrash(dismissalThrash, timestamp);
        dismissalThrash = nextDismissalThrash;
        if (nextDismissalThrash.shouldReport) report('open-select-dom-removal');
      }
    }
    if (records.length >= 25) record('large-mutation-batch', undefined, { records: records.length, removed_open_selects: removedTrackedSelects });
  });
  observer.observe(doc.documentElement, { childList: true, subtree: true });
  const onError = (event: ErrorEvent) => { record('window-error', undefined, { message: event.message, filename: event.filename, line: event.lineno, column: event.colno }); };
  const onUnhandledRejection = (event: PromiseRejectionEvent) => { record('unhandled-rejection', undefined, { reason: event.reason instanceof Error ? event.reason.message : String(event.reason) }); };
  doc.addEventListener('pointerdown', noteUserIntent, true);
  doc.addEventListener('keydown', noteUserIntent, true);
  doc.addEventListener('wa-show', onShow, true);
  doc.addEventListener('wa-hide', onHide, true);
  view.addEventListener('error', onError);
  view.addEventListener('unhandledrejection', onUnhandledRejection);

  return {
    attachment,
    recordRender(metrics, automaticReportSuppressed) {
      const timestamp = now();
      const nextRenderStorm = advanceRenderStorm(renderStorm, timestamp, Boolean(automaticReportSuppressed));
      renderStorm = nextRenderStorm;
      record('render-pass', undefined, {
        ...metrics,
        pass_delta: metrics.passes - previousRenderMetrics.passes,
        mutation_delta: metrics.mutations - previousRenderMetrics.mutations,
        ...(automaticReportSuppressed ? { automatic_report_suppressed: automaticReportSuppressed } : {}),
      });
      previousRenderMetrics = metrics;
      if (timestamp - installedAt > STARTUP_GRACE_MS && timestamp - lastUserIntentAt > USER_INTENT_GRACE_MS && nextRenderStorm.shouldReport) report('render-storm');
    },
    destroy() {
      observer.disconnect();
      doc.removeEventListener('pointerdown', noteUserIntent, true);
      doc.removeEventListener('keydown', noteUserIntent, true);
      doc.removeEventListener('wa-show', onShow, true);
      doc.removeEventListener('wa-hide', onHide, true);
      view.removeEventListener('error', onError);
      view.removeEventListener('unhandledrejection', onUnhandledRejection);
    },
  };
}
