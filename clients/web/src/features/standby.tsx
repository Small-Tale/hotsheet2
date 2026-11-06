import { effect, mount, signal } from 'kerfjs';

import { StandbyScreen } from '../components/standby-screen';
import { STANDBY_SETTING_FIELDS } from '../components/standby-settings';
import type { LiveStateStore } from '../live-state';
import {
  createStandbyIdleTimer,
  formatStandbyClock,
  standbyFreshnessLabel,
  standbyModel,
  type StandbyScope,
  type StandbySettingsView,
} from '../standby';

/** The server calls standby makes: only its machine-wide settings. The display itself reads nothing. */
export interface StandbyApi {
  standbySettings(): Promise<StandbySettingsView>;
  saveStandbySettings(value: StandbySettingsView['configured']): Promise<StandbySettingsView>;
}

/** The slice of the Screen Wake Lock API standby uses. */
interface WakeLockLike {
  request(type: 'screen'): Promise<{ release(): Promise<void> }>;
}

export interface StandbyControllerDependencies {
  liveStates: LiveStateStore;
  /** The open project tabs; standby shows only their stores (HS2-JGW031). */
  openProjectIds: () => readonly string[];
  /** The API of the server this client talks to, once a project is open. */
  api: () => StandbyApi | undefined;
  now?: () => number;
  /** The clock and ages re-render on this local display timer; it never issues a request. */
  tickMs?: number;
  /** Whether this device is a phone or tablet (Wake Lock, "tap" wording). */
  touchDevice?: () => boolean;
  wakeLock?: () => WakeLockLike | undefined;
  /** How long a dropped stream may reconnect before the freshness line says so. */
  streamGraceMs?: number;
  /** Injected for tests. */
  setInterval?: (callback: () => void, ms: number) => unknown;
  clearInterval?: (handle: unknown) => void;
}

/** The events that count as direct human interaction for idle entry. */
const ACTIVITY_EVENTS = ['pointerdown', 'pointermove', 'keydown', 'wheel', 'touchstart'] as const;

const coarsePointer = () =>
  typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches && navigator.maxTouchPoints > 0;
const browserWakeLock = (): WakeLockLike | undefined =>
  typeof navigator !== 'undefined' && 'wakeLock' in navigator
    ? (navigator as Navigator & { wakeLock: WakeLockLike }).wakeLock
    : undefined;
const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * Standby (HS2-EQVZTZ, docs/34-glance-surfaces.md §34.5). It enters after the machine-wide idle
 * threshold passes with no direct interaction, or when the moon button forces it, and exits on any
 * click, tap, or key. While it shows, it renders only the pushed live state model: the clock and ages
 * tick on a local display timer, and nothing here issues a request. On a phone or tablet it holds a
 * Screen Wake Lock so the display stays on.
 */
export function createStandbyController(dependencies: StandbyControllerDependencies) {
  const now = dependencies.now ?? Date.now,
    tickMs = dependencies.tickMs ?? 10_000,
    streamGraceMs = dependencies.streamGraceMs ?? 20_000,
    touchDevice = dependencies.touchDevice ?? coarsePointer,
    wakeLock = dependencies.wakeLock ?? browserWakeLock,
    startInterval =
      dependencies.setInterval ?? ((callback: () => void, ms: number) => globalThis.setInterval(callback, ms)),
    stopInterval =
      dependencies.clearInterval ??
      ((handle: unknown) => {
        globalThis.clearInterval(handle as ReturnType<typeof setInterval>);
      });
  const settings = signal<StandbySettingsView | undefined>(undefined),
    settingsMessage = signal(''),
    scope = signal<StandbyScope | undefined>(undefined),
    nowMs = signal(now()),
    /** Each project's change stream: `null` while connected, else when it dropped. */
    streams = signal<ReadonlyMap<string, number | null>>(new Map()),
    updatedAtMs = signal<number | undefined>(undefined);
  let ticker: unknown,
    lock: { release(): Promise<void> } | undefined,
    lockPending = false,
    returnFocus: HTMLElement | undefined,
    generation = 0;

  const idle = createStandbyIdleTimer({
    idleMs: () => {
      const effective = settings.value?.effective;
      return effective?.enabled ? effective.idle_minutes * 60_000 : undefined;
    },
    onIdle: () => {
      enter();
    },
    now,
  });

  // A snapshot arriving is the freshness line's "updated" time.
  let firstStates = true;
  const disposeFreshness = effect(() => {
    void dependencies.liveStates.states.value;
    if (firstStates) {
      firstStates = false;
      return;
    }
    updatedAtMs.value = now();
  });

  async function holdWakeLock() {
    const api = touchDevice() ? wakeLock() : undefined;
    if (!api || lock || lockPending) return;
    lockPending = true;
    try {
      const sentinel = await api.request('screen');
      if (scope.peek()) lock = sentinel;
      else void sentinel.release();
    } catch {
      // Denied (battery saver, an inactive page): standby still works, the screen may sleep.
    } finally {
      lockPending = false;
    }
  }

  function releaseWakeLock() {
    const held = lock;
    lock = undefined;
    void held?.release().catch(() => undefined);
  }

  function enter() {
    if (scope.peek()) return;
    idle.pause();
    const page = typeof document === 'undefined' ? undefined : document;
    const focused = page ? page.activeElement : null;
    returnFocus = typeof HTMLElement !== 'undefined' && focused instanceof HTMLElement ? focused : undefined;
    nowMs.value = now();
    // Always every project, whichever tab or page standby was entered from (HS2-JGW031).
    scope.value = { kind: 'all' };
    ticker = startInterval(() => {
      nowMs.value = now();
    }, tickMs);
    void holdWakeLock();
    // Take focus off any editor beneath once the top-layer watcher has shown the display.
    if (page && typeof requestAnimationFrame === 'function')
      requestAnimationFrame(() => {
        page.querySelector<HTMLElement>('[data-component="standby-screen"]')?.focus({ preventScroll: true });
      });
  }

  function exit() {
    if (!scope.peek()) return;
    scope.value = undefined;
    if (ticker !== undefined) stopInterval(ticker);
    ticker = undefined;
    releaseWakeLock();
    idle.resume();
    const focus = returnFocus;
    returnFocus = undefined;
    if (focus?.isConnected) focus.focus({ preventScroll: true });
  }

  /** A project's change stream connected (`true`), dropped (`false`), or closed (`undefined`). */
  function streamHealth(projectId: string, live: boolean | undefined) {
    const current = streams.peek(),
      previous = current.get(projectId);
    // A drop keeps its first time while the stream retries; a recovery clears it.
    if (live === undefined ? !current.has(projectId) : live ? previous === null : typeof previous === 'number') return;
    const next = new Map(current);
    if (live === undefined) next.delete(projectId);
    else next.set(projectId, live ? null : now());
    streams.value = next;
  }

  /** Live unless any project's stream has been down past the reconnect grace (judged at `at`). */
  function live(at: number): boolean {
    const down = (since: number | null | undefined) => typeof since === 'number' && at - since >= streamGraceMs;
    return ![...streams.value.values()].some(down);
  }

  async function refreshSettings(keepMessage = false) {
    const api = dependencies.api();
    if (!api) return;
    const request = ++generation;
    try {
      const value = await api.standbySettings();
      if (request !== generation) return;
      settings.value = value;
      if (!keepMessage) settingsMessage.value = '';
      idle.reconfigure();
    } catch (reason) {
      // An older server has no standby route: idle entry stays off; the moon button still works.
      if (request === generation) settingsMessage.value = message(reason);
    }
  }

  async function saveSettings(form: HTMLElement) {
    const api = dependencies.api();
    if (!api) return;
    const enabled = form.querySelector<HTMLInputElement>(`[name="${STANDBY_SETTING_FIELDS.enabled}"]`)?.checked,
      idleValue = Number(
        form.querySelector<HTMLElement & { value?: string }>(`[name="${STANDBY_SETTING_FIELDS.idleMinutes}"]`)?.value,
      );
    const request = ++generation;
    settingsMessage.value = '';
    try {
      const value = await api.saveStandbySettings({
        enabled: enabled ?? null,
        idle_minutes: Number.isInteger(idleValue) && idleValue > 0 ? idleValue : null,
      });
      if (request !== generation) return;
      settings.value = value;
      idle.reconfigure();
    } catch (reason) {
      if (request !== generation) return;
      settingsMessage.value = message(reason);
      await refreshSettings(true);
    }
  }

  /** Render the display into `host` and listen for activity and exit keys; returns a disposer. */
  function wire(host: HTMLElement = document.body): () => void {
    const root = document.createElement('div');
    root.dataset.standbyRoot = '';
    host.append(root);
    const unmount = mount(root, () => {
      const current = scope.value;
      if (!current) return <></>;
      const at = nowMs.value,
        isLive = live(at);
      return (
        <StandbyScreen
          clock={formatStandbyClock(at)}
          model={standbyModel(dependencies.liveStates.forProjects(dependencies.openProjectIds()), at)}
          freshness={{ live: isLive, label: standbyFreshnessLabel({ live: isLive, updatedAtMs: updatedAtMs.value }) }}
          touch={touchDevice()}
        />
      );
    });
    const onActivity = () => {
      idle.activity();
    };
    // Any key exits and goes no further: the app beneath never sees it (an editor, a shortcut,
    // Escape on a dialog). Clicks and taps exit through the overlay's `exit-standby` action.
    const onKey = (event: KeyboardEvent) => {
      if (!scope.peek()) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      exit();
    };
    // The browser drops a wake lock while the page is hidden; take it again on return.
    const onVisibility = () => {
      if (document.visibilityState === 'visible' && scope.peek()) void holdWakeLock();
    };
    for (const type of ACTIVITY_EVENTS) window.addEventListener(type, onActivity, { capture: true, passive: true });
    window.addEventListener('keydown', onKey, { capture: true });
    document.addEventListener('visibilitychange', onVisibility);
    idle.resume();
    return () => {
      for (const type of ACTIVITY_EVENTS) window.removeEventListener(type, onActivity, { capture: true });
      window.removeEventListener('keydown', onKey, { capture: true });
      document.removeEventListener('visibilitychange', onVisibility);
      exit();
      idle.dispose();
      disposeFreshness();
      unmount();
      root.remove();
    };
  }

  return {
    settings,
    settingsMessage,
    scope,
    streams,
    enter,
    exit,
    streamHealth,
    refreshSettings,
    saveSettings,
    wire,
  };
}

export type StandbyController = ReturnType<typeof createStandbyController>;
