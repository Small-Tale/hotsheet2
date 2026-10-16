/**
 * Development-only wiring for the latency monitor (HS2-ZGBQ5Q). Loaded through a dynamic import from
 * `installDevelopmentDiagnostics`, so a production build never contains it.
 *
 * - Interactions: trusted clicks and action keys (Enter, Escape, Space on controls, and Cmd/Ctrl
 *   shortcuts) are timed until the requests they started finish and the result paints.
 * - Requests: every API request except idle long-polls is timed; expected long-running requests are
 *   exempt. The server's `Server-Timing: handler` value separates server time from queueing.
 * - Reports: a console warning, a non-blocking dev toast, and a debounced POST to the dev bridge's
 *   durable log (`/__hotsheet/dev/latency`). Reporting is event-driven, never polled.
 */
import '@awesome.me/webawesome/dist/components/toast/toast.js';

import {
  createInteractionTracker,
  createLatencyBudget,
  describeViolation,
  DEV_LATENCY_ENDPOINT,
  DEV_LATENCY_STORAGE_KEY,
  devLatencyEnabled,
  interactionLabel,
  isLongRunningRequest,
  type LatencySample,
  type LatencyViolation,
  requestRouteLabel,
  serverHandlerMs,
  summarizeViolation,
} from './dev-latency';
import { setRequestObserver } from './request-observer';

export interface DevLatencyReport {
  at: string;
  kind: LatencySample['kind'];
  label: string;
  duration_ms: number;
  reasons: LatencyViolation['reasons'];
  average_ms: number;
  server_ms?: number;
  requests?: LatencySample['requests'];
  message: string;
}

export interface DevLatencyApi {
  reports(): DevLatencyReport[];
}

declare global {
  interface Window {
    __hotsheetDevLatency?: DevLatencyApi;
  }
}

/** Reports kept in memory for `window.__hotsheetDevLatency`. */
const REPORT_MEMORY = 100;
/** Coalesce bridge posts so a burst of slow requests becomes one write. */
const BRIDGE_FLUSH_MS = 1_000;

const ACTION_KEYS = new Set(['Enter', 'Escape', ' ']);

function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return Boolean(
    target.closest('textarea, [contenteditable=""], [contenteditable="true"], wa-textarea') ||
    (target.closest('input, wa-input') &&
      !['checkbox', 'radio', 'button', 'submit'].includes(target.getAttribute('type') ?? '')),
  );
}

function readSetting(): string | null {
  try {
    return localStorage.getItem(DEV_LATENCY_STORAGE_KEY);
  } catch {
    return null;
  }
}

/** Install the monitor; returns its disposer (or a no-op when disabled). */
export function installDevLatencyMonitor(): () => void {
  if (!devLatencyEnabled(readSetting(), navigator.webdriver)) return () => {};
  const reports: DevLatencyReport[] = [];
  let queued: DevLatencyReport[] = [],
    flushTimer: ReturnType<typeof setTimeout> | undefined,
    slowCount = 0;

  const toastHost = document.createElement('wa-toast');
  toastHost.dataset.devLatency = '';
  (toastHost as HTMLElement & { placement: string }).placement = 'bottom-start';
  document.body.append(toastHost);

  const flush = () => {
    flushTimer = undefined;
    const batch = queued;
    queued = [];
    if (batch.length === 0) return;
    void fetch(DEV_LATENCY_ENDPOINT, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ page: location.pathname + location.hash, reports: batch }),
      keepalive: true,
    }).catch(() => {
      // The bridge exists only under the Vite dev server; the console warning still stands.
    });
  };

  const report = (violation: LatencyViolation) => {
    const message = describeViolation(violation),
      { sample } = violation;
    const entry: DevLatencyReport = {
      at: new Date().toISOString(),
      kind: sample.kind,
      label: sample.label,
      duration_ms: Math.round(sample.duration_ms),
      reasons: violation.reasons,
      average_ms: Math.round(violation.average_ms),
      ...(sample.server_ms === undefined ? {} : { server_ms: Math.round(sample.server_ms) }),
      ...(sample.requests?.length ? { requests: sample.requests } : {}),
      message,
    };
    reports.push(entry);
    if (reports.length > REPORT_MEMORY) reports.shift();
    slowCount += 1;
    console.warn(`[hotsheet dev latency] ${message}`, entry);
    queued.push(entry);
    flushTimer ??= setTimeout(flush, BRIDGE_FLUSH_MS);
    // One toast at a time. An interaction report (what the user felt) is not replaced by a later
    // request report while it is still showing; the full detail stays in the console and the log.
    const showing = toastHost.querySelector<HTMLElement>('[data-dev-latency-toast]');
    if (entry.kind === 'request' && showing?.dataset.devLatencyToast === 'interaction') return;
    toastHost.replaceChildren();
    // Re-open the stack so it is raised above any dialog or drawer opened since it was shown.
    try {
      if (toastHost.matches(':popover-open')) toastHost.hidePopover();
    } catch {
      // Not yet upgraded: create() opens it.
    }
    void toastHost
      .create(summarizeViolation(violation, slowCount), { duration: 8_000, variant: 'warning' })
      .then((item: HTMLElement) => {
        item.dataset.devLatencyToast = entry.kind;
      });
  };

  const budget = createLatencyBudget();
  const judge = (sample: LatencySample) => {
    const violation = budget.record(sample);
    if (violation) report(violation);
  };
  const tracker = createInteractionTracker({
    now: () => performance.now(),
    afterPaint: (callback) => requestAnimationFrame(() => setTimeout(callback, 0)),
    setTimer: (callback, ms) => {
      setTimeout(callback, ms);
    },
    onComplete: judge,
  });

  const removeObserver = setRequestObserver((method, path) => {
    const label = requestRouteLabel(method, path),
      longRunning = isLongRunningRequest(method, path),
      started = performance.now(),
      end = tracker.requestStarted(label, longRunning);
    return (response) => {
      const duration_ms = performance.now() - started,
        server_ms = serverHandlerMs(response?.headers.get('server-timing'));
      end({ label, duration_ms, ...(server_ms === undefined ? {} : { server_ms }) });
      if (!longRunning)
        judge({ kind: 'request', label, duration_ms, ...(server_ms === undefined ? {} : { server_ms }) });
    };
  });

  const ownToast = (target: EventTarget | null) => target instanceof Element && toastHost.contains(target);
  const onClick = (event: MouseEvent) => {
    if (!event.isTrusted || ownToast(event.target)) return;
    tracker.begin(interactionLabel(event.target as Element | null, 'click'), event.timeStamp);
  };
  const onKeydown = (event: KeyboardEvent) => {
    if (!event.isTrusted || event.repeat || ownToast(event.target)) return;
    const shortcut = event.metaKey || event.ctrlKey;
    if (!shortcut && !ACTION_KEYS.has(event.key)) return;
    if (!shortcut && event.key !== 'Escape' && isTextEntry(event.target)) return;
    const key = `${shortcut ? (event.metaKey ? 'Cmd+' : 'Ctrl+') : ''}${event.key === ' ' ? 'Space' : event.key}`;
    tracker.begin(interactionLabel(event.target as Element | null, `key ${key} on`), event.timeStamp);
  };
  document.addEventListener('click', onClick, true);
  document.addEventListener('keydown', onKeydown, true);
  window.__hotsheetDevLatency = { reports: () => [...reports] };

  return () => {
    document.removeEventListener('click', onClick, true);
    document.removeEventListener('keydown', onKeydown, true);
    removeObserver();
    if (flushTimer !== undefined) clearTimeout(flushTimer);
    flush();
    toastHost.remove();
    delete window.__hotsheetDevLatency;
  };
}
