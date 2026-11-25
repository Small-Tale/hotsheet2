import { signal } from 'kerfjs';

import type { Project } from '../interactions/types';
import { remoteFailureKind, remoteFailureText } from '../remote-session-lifetime';
import { remoteProjectId, remoteProjectTabName } from './identity';

/**
 * Remembered remote project tabs that are not open yet (HS2-EJ5K09, HS2-WNK73S).
 *
 * Startup shows one placeholder tab per selected remote checkout before any network call, then opens
 * every checkout in parallel; each tab becomes the real project on its own. A checkout whose host does
 * not answer keeps its placeholder in a reconnecting state. Retries are per host binding with
 * exponential backoff to a cap, and a retry first makes one cheap reachability probe of the host
 * before reopening its checkouts, so an idle unreachable host costs one probe per backoff interval no
 * matter how many of its projects were selected. Network-online, window focus and an explicit tab
 * selection wake a retry early; nothing here polls on a fixed interval. Only `forget` (an explicit
 * close) removes a placeholder; a failure never drops one.
 *
 * HS2-4B0KSD: a probe that reports an expired or revoked approval moves the host's placeholders to
 * that state and stops retrying it, because retrying an expired credential never succeeds. Only an
 * explicit Check Again (`retryNow`) or a renewal tries again. Any other failure keeps the backoff and
 * shows its text.
 */

/** The first attempt is still running (`loading`), at least one attempt failed (`reconnecting`), or the
 * host refused this computer's approval as expired or revoked (HS2-4B0KSD), which no retry fixes. */
export type RemoteTabPlaceholderPhase = 'loading' | 'reconnecting' | 'expired' | 'revoked';

/** One reachability probe's answer: reachable, or why not (a remote failure code). A bare boolean is an
 * unclassified answer. */
export type RemoteProbeResult = boolean | { reachable: boolean; failure?: string | null };

export interface RemoteTabPlaceholder {
  /** The browser project id the opened tab will carry. */
  id: string;
  bindingId: string;
  checkoutId: string;
  name: string;
  /** Saved tab position: the placeholder and the opened tab keep it. */
  rank: number;
  phase: RemoteTabPlaceholderPhase;
  /** An open or reachability probe for this placeholder's host is in flight. */
  connecting: boolean;
  error?: string;
  /** This computer asked the host to renew its approval (HS2-4B0KSD). */
  renewalRequested?: boolean;
}

export interface RemoteTabSelection {
  binding_id: string;
  checkout_id: string;
}

export interface RemoteTabBinding {
  id: string;
  label: string;
  /** The reviewed origin; with it, a placeholder is named like the opened tab (HS2-GN0WKS). */
  origin?: string;
  last_confirmed_checkouts?: ReadonlyArray<{ id: string; name: string }>;
}

/** The placeholder tab id: never a project id, so no project code mistakes a placeholder for a session. */
export const REMOTE_PLACEHOLDER_TAB_PREFIX = 'remote-placeholder:';
export const remotePlaceholderTabId = (projectId: string) => `${REMOTE_PLACEHOLDER_TAB_PREFIX}${projectId}`;
export const remotePlaceholderProjectId = (tabId: string) =>
  tabId.startsWith(REMOTE_PLACEHOLDER_TAB_PREFIX) ? tabId.slice(REMOTE_PLACEHOLDER_TAB_PREFIX.length) : undefined;

/** Remote tabs sort after every remembered local project, in the registry's saved order. */
export const REMOTE_TAB_RANK_BASE = 1_000_000;

/**
 * The placeholder seeds from this machine's registry and this device's host-name cache: named
 * `<host> · <project>` exactly like the opened tab (HS2-HGG426, HS2-GN0WKS), with the host the peer last
 * reported (`hostNames`, by binding id), else the origin's host, and the checkout name the host last
 * confirmed. A binding without an origin keeps its label. An unknown checkout falls back to its id;
 * an invalid identity is skipped.
 */
export function remoteTabPlaceholders(
  selected: readonly RemoteTabSelection[],
  bindings: readonly RemoteTabBinding[],
  hostNames: Readonly<Record<string, string>> = {},
): Array<Pick<RemoteTabPlaceholder, 'id' | 'bindingId' | 'checkoutId' | 'name' | 'rank'>> {
  const seen = new Set<string>();
  return selected.flatMap((item, index) => {
    let id: string;
    try {
      id = remoteProjectId(item.binding_id, item.checkout_id);
    } catch {
      return [];
    }
    if (seen.has(id)) return [];
    seen.add(id);
    const binding = bindings.find((candidate) => candidate.id === item.binding_id),
      checkout = binding?.last_confirmed_checkouts?.find((candidate) => candidate.id === item.checkout_id),
      project = checkout?.name || item.checkout_id;
    return [
      {
        id,
        bindingId: item.binding_id,
        checkoutId: item.checkout_id,
        name: binding?.origin
          ? remoteProjectTabName(hostNames[item.binding_id], binding.origin, project)
          : binding?.label
            ? `${binding.label} · ${project}`
            : project,
        rank: REMOTE_TAB_RANK_BASE + index,
      },
    ];
  });
}

export interface RemoteReconnectTimers {
  now: () => number;
  setTimeout: (run: () => void, ms: number) => unknown;
  clearTimeout: (handle: unknown) => void;
}

export interface RemoteReconnectDependencies {
  /** Open one checkout; resolves to the project the tab shows. */
  open: (bindingId: string, checkoutId: string) => Promise<Project>;
  /** A cheap reachability check of one host; `false` or a rejection keeps its placeholders waiting. An
   * expired or revoked failure stops retrying (HS2-4B0KSD). */
  probe: (bindingId: string) => Promise<RemoteProbeResult>;
  /** Turn a placeholder into its real tab at the placeholder's rank. A rejection keeps the placeholder. */
  opened: (placeholder: RemoteTabPlaceholder, project: Project) => Promise<void>;
  /** An open that finished after its placeholder was closed re-selected it in the registry: undo that. */
  discard: (placeholder: RemoteTabPlaceholder) => void;
  /** The same project is already open (for example from the Remote project dialog). */
  isOpen?: (projectId: string) => boolean;
  timers?: RemoteReconnectTimers;
  baseDelayMs?: number;
  maxDelayMs?: number;
  /** A focus or online wake retries a host at most this often; an explicit tab selection ignores it. */
  wakeMinIntervalMs?: number;
}

export const REMOTE_RECONNECT_BASE_DELAY_MS = 2_000;
export const REMOTE_RECONNECT_MAX_DELAY_MS = 5 * 60_000;
export const REMOTE_RECONNECT_WAKE_MIN_INTERVAL_MS = 10_000;
/** An open remote tab shows its reconnecting state once its change stream stayed down this long. */
export const REMOTE_STREAM_DOWN_GRACE_MS = 3_000;

/** The delay before retry number `failures` (1-based): doubling from the base, capped. */
export function remoteReconnectDelay(failures: number, baseMs: number, maxMs: number): number {
  return Math.min(maxMs, baseMs * 2 ** Math.max(0, Math.min(failures, 31) - 1));
}

interface BindingState {
  failures: number;
  timer?: unknown;
  inFlight?: Promise<void>;
  lastAttemptAt?: number;
}

const browserTimers: RemoteReconnectTimers = {
  now: () => Date.now(),
  setTimeout: (run, ms) => window.setTimeout(run, ms),
  clearTimeout: (handle) => {
    window.clearTimeout(handle as number);
  },
};

export function createRemoteReconnect(dependencies: RemoteReconnectDependencies) {
  const timers = dependencies.timers ?? browserTimers,
    baseMs = dependencies.baseDelayMs ?? REMOTE_RECONNECT_BASE_DELAY_MS,
    maxMs = dependencies.maxDelayMs ?? REMOTE_RECONNECT_MAX_DELAY_MS,
    wakeMinMs = dependencies.wakeMinIntervalMs ?? REMOTE_RECONNECT_WAKE_MIN_INTERVAL_MS;
  const placeholders = signal<readonly RemoteTabPlaceholder[]>([]);
  const bindings = new Map<string, BindingState>();
  let disposed = false;

  const pendingFor = (bindingId: string) => placeholders.peek().filter((item) => item.bindingId === bindingId);
  function patch(ids: ReadonlySet<string>, change: (item: RemoteTabPlaceholder) => RemoteTabPlaceholder) {
    placeholders.value = placeholders.peek().map((item) => (ids.has(item.id) ? change(item) : item));
  }
  function remove(id: string) {
    placeholders.value = placeholders.peek().filter((item) => item.id !== id);
  }

  function schedule(bindingId: string, state: BindingState) {
    if (disposed || !pendingFor(bindingId).length) {
      bindings.delete(bindingId);
      return;
    }
    if (state.timer !== undefined) timers.clearTimeout(state.timer);
    state.timer = timers.setTimeout(
      () => {
        state.timer = undefined;
        void attempt(bindingId, true);
      },
      remoteReconnectDelay(state.failures, baseMs, maxMs),
    );
  }

  async function openOne(item: RemoteTabPlaceholder): Promise<boolean> {
    if (dependencies.isOpen?.(item.id)) {
      remove(item.id);
      return true;
    }
    let project: Project;
    try {
      project = await dependencies.open(item.bindingId, item.checkoutId);
    } catch (reason) {
      patch(new Set([item.id]), (current) => ({ ...current, error: errorMessage(reason) }));
      return false;
    }
    const current = placeholders.peek().find((candidate) => candidate.id === item.id);
    if (!current || disposed) {
      // Closed while the open was in flight: the open re-selected it in the registry.
      if (!disposed) dependencies.discard(item);
      return true;
    }
    try {
      await dependencies.opened(current, project);
    } catch (reason) {
      patch(new Set([item.id]), (value) => ({ ...value, error: errorMessage(reason) }));
      return false;
    }
    remove(item.id);
    return true;
  }

  /** One attempt for every placeholder of a host. A retry probes the host first. */
  function attempt(bindingId: string, probeFirst: boolean): Promise<void> {
    let state = bindings.get(bindingId);
    if (!state) {
      state = { failures: 0 };
      bindings.set(bindingId, state);
    }
    if (state.inFlight) return state.inFlight;
    if (disposed || !pendingFor(bindingId).length) {
      bindings.delete(bindingId);
      return Promise.resolve();
    }
    const owned = state;
    if (owned.timer !== undefined) {
      timers.clearTimeout(owned.timer);
      owned.timer = undefined;
    }
    owned.lastAttemptAt = timers.now();
    const ids = new Set(pendingFor(bindingId).map((item) => item.id));
    patch(ids, (item) => ({ ...item, connecting: true }));
    const run = async () => {
      let reachable = true,
        failure: string | null | undefined;
      if (probeFirst) {
        const answer = await dependencies.probe(bindingId).catch(() => false);
        reachable = typeof answer === 'boolean' ? answer : answer.reachable;
        failure = typeof answer === 'boolean' ? undefined : answer.failure;
      }
      const targets = pendingFor(bindingId),
        kind = remoteFailureKind(failure);
      if (!reachable)
        patch(new Set(targets.map((item) => item.id)), (item) => ({
          ...item,
          error: remoteFailureText(failure),
        }));
      const results = reachable ? await Promise.all(targets.map((item) => openOne(item))) : [false];
      if (disposed) return;
      // An expired or revoked approval is final until the user acts: no retry is scheduled.
      const stopped = !reachable && (kind === 'expired' || kind === 'revoked');
      patch(new Set(pendingFor(bindingId).map((item) => item.id)), (item) => ({
        ...item,
        connecting: false,
        phase: stopped ? kind : 'reconnecting',
      }));
      owned.failures = results.every(Boolean) ? 0 : owned.failures + 1;
      owned.inFlight = undefined;
      if (stopped) {
        if (owned.timer !== undefined) timers.clearTimeout(owned.timer);
        owned.timer = undefined;
        return;
      }
      schedule(bindingId, owned);
    };
    owned.inFlight = run();
    return owned.inFlight;
  }

  /** Show placeholders at once and open every host in parallel; resolves when each first attempt settled. */
  function restore(items: ReturnType<typeof remoteTabPlaceholders>): Promise<void> {
    const known = new Set(placeholders.peek().map((item) => item.id));
    const added = items.filter((item) => !known.has(item.id) && !dependencies.isOpen?.(item.id));
    if (!added.length || disposed) return Promise.resolve();
    placeholders.value = [
      ...placeholders.peek(),
      ...added.map((item) => ({ ...item, phase: 'loading' as const, connecting: true })),
    ].sort((left, right) => left.rank - right.rank);
    return Promise.all([...new Set(added.map((item) => item.bindingId))].map((id) => attempt(id, false))).then(
      () => undefined,
    );
  }

  /** The user selected this placeholder's tab or chose Check Again: try its host now, whatever the
   * backoff says. */
  function retryNow(projectId: string): Promise<void> {
    const item = placeholders.peek().find((candidate) => candidate.id === projectId);
    return item ? attempt(item.bindingId, item.phase !== 'loading') : Promise.resolve();
  }

  /** This computer asked the host to renew its approval of `bindingId` (HS2-4B0KSD). */
  function renewalRequested(bindingId: string) {
    patch(new Set(pendingFor(bindingId).map((item) => item.id)), (item) => ({ ...item, renewalRequested: true }));
  }

  /** Whether a host's placeholders wait for the user (expired or revoked), not for the network. */
  const waitsForUser = (bindingId: string) =>
    pendingFor(bindingId).some((item) => item.phase === 'expired' || item.phase === 'revoked');

  /** Network came back or the window gained focus: retry hosts not tried within the wake interval. */
  function wake() {
    const now = timers.now();
    for (const bindingId of new Set(placeholders.peek().map((item) => item.bindingId))) {
      const state = bindings.get(bindingId);
      if (state?.inFlight) continue;
      if (waitsForUser(bindingId)) continue;
      if (state?.lastAttemptAt !== undefined && now - state.lastAttemptAt < wakeMinMs) continue;
      void attempt(bindingId, true);
    }
  }

  /** An explicit close: the only way a placeholder leaves without opening. */
  function forget(projectId: string): RemoteTabPlaceholder | undefined {
    const item = placeholders.peek().find((candidate) => candidate.id === projectId);
    if (!item) return undefined;
    remove(projectId);
    const state = bindings.get(item.bindingId);
    if (state && !state.inFlight && !pendingFor(item.bindingId).length) {
      if (state.timer !== undefined) timers.clearTimeout(state.timer);
      bindings.delete(item.bindingId);
    }
    return item;
  }

  function dispose() {
    disposed = true;
    for (const state of bindings.values()) if (state.timer !== undefined) timers.clearTimeout(state.timer);
    bindings.clear();
  }

  /** Whether a retry timer is armed for a host (tests and diagnostics). */
  const scheduled = (bindingId: string) => bindings.get(bindingId)?.timer !== undefined;

  return { placeholders, restore, retryNow, renewalRequested, wake, forget, dispose, scheduled };
}

export type RemoteReconnect = ReturnType<typeof createRemoteReconnect>;

/** Wake retries on network-online and window focus; returns the disposer. */
export function wireRemoteReconnectWakeups(reconnect: Pick<RemoteReconnect, 'wake'>, target: Window = window) {
  const wake = () => {
    reconnect.wake();
  };
  target.addEventListener('online', wake);
  target.addEventListener('focus', wake);
  return () => {
    target.removeEventListener('online', wake);
    target.removeEventListener('focus', wake);
  };
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
