import { afterEach, describe, expect, it, vi } from 'vitest';

import type { CalibrationReport, LiveStoreState } from '../api';
import { STANDBY_SETTING_FIELDS } from '../components/standby-settings';
import type { HostHealthSnapshot } from '../host-health';
import { createLiveStateStore, type LiveStateStore } from '../live-state';
import type { StandbySettingsView } from '../standby';
import { createStandbyController, type StandbyApi } from './standby';

const view = (enabled: boolean, idle: number, configured: Partial<StandbySettingsView['configured']> = {}) => ({
  defaults: { enabled: true, idle_minutes: 15 },
  configured: { enabled: null, idle_minutes: null, ...configured },
  effective: { enabled, idle_minutes: idle },
});

function harness(
  options: {
    api?: StandbyApi;
    touch?: boolean;
    liveStates?: LiveStateStore;
    openProjectIds?: () => readonly string[];
    hostHealth?: () => HostHealthSnapshot | undefined;
    completions?: (projectIds: readonly string[]) => Promise<readonly CalibrationReport[]>;
  } = {},
) {
  let clock = 1_000_000;
  const intervals = new Map<number, () => void>();
  let nextInterval = 0;
  const released = vi.fn(() => Promise.resolve());
  const request = vi.fn(() => Promise.resolve({ release: released }));
  const controller = createStandbyController({
    openProjectIds: options.openProjectIds ?? (() => []),
    liveStates: options.liveStates ?? createLiveStateStore(),
    ...(options.hostHealth ? { hostHealth: options.hostHealth } : {}),
    ...(options.completions ? { completions: options.completions } : {}),
    api: () => options.api,
    // Fake timers drive the idle tests through the real clock.
    now: () => (vi.isFakeTimers() ? Date.now() : clock),
    touchDevice: () => options.touch ?? false,
    wakeLock: () => ({ request }),
    streamGraceMs: 20_000,
    setInterval: (callback) => {
      intervals.set(++nextInterval, callback);
      return nextInterval;
    },
    clearInterval: (handle) => {
      intervals.delete(handle as number);
    },
  });
  return {
    controller,
    intervals,
    request,
    released,
    tick: (ms: number) => {
      clock += ms;
      for (const callback of intervals.values()) callback();
    },
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('standby controller (HS2-EQVZTZ)', () => {
  it('forced entry always shows all projects (HS2-JGW031), ticks only a local display timer, and exits once', () => {
    const { controller, intervals } = harness();
    controller.enter();
    expect(controller.scope.value).toEqual({ kind: 'all' });
    expect(intervals.size).toBe(1);
    // A repeated entry is ignored.
    controller.enter();
    expect(intervals.size).toBe(1);
    controller.exit();
    expect(controller.scope.value).toBeUndefined();
    expect(intervals.size).toBe(0);
    controller.exit();
  });

  it('enters after the configured idle threshold and not while disabled', async () => {
    vi.useFakeTimers();
    let settings = view(true, 5);
    const api: StandbyApi = {
      standbySettings: vi.fn(() => Promise.resolve(settings)),
      saveStandbySettings: vi.fn(() => Promise.resolve(settings)),
    };
    const { controller } = harness({ api });
    await controller.refreshSettings();
    // Without `wire()` the idle timer has not started; settings alone re-arm it from construction.
    vi.advanceTimersByTime(5 * 60_000);
    expect(controller.scope.value).toEqual({ kind: 'all' });
    controller.exit();
    settings = view(false, 5, { enabled: false });
    await controller.refreshSettings();
    vi.advanceTimersByTime(60 * 60_000);
    expect(controller.scope.value).toBeUndefined();
  });

  it('re-reads pushed settings changes once per new event and on resync, re-arming idle entry (HS2-A5BTTK)', async () => {
    vi.useFakeTimers();
    let settings = view(true, 30);
    const standbySettings = vi.fn(() => Promise.resolve(settings));
    const api: StandbyApi = { standbySettings, saveStandbySettings: vi.fn(() => Promise.resolve(settings)) };
    const { controller } = harness({ api });
    await controller.refreshSettings();
    expect(standbySettings).toHaveBeenCalledTimes(1);
    const event = (cursor: number, kind = 'standby_settings') => ({ cursor, store: '', kind, id: '', slug: '' });
    // Unrelated events read nothing.
    expect(controller.applySettingsEvents([event(1, 'host_health')])).toBeUndefined();
    // The CLI lowered the threshold: one read re-arms the timer from the last activity.
    settings = view(true, 5, { idle_minutes: 5 });
    await controller.applySettingsEvents([event(2)]);
    expect(standbySettings).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(5 * 60_000);
    expect(controller.scope.value).toEqual({ kind: 'all' });
    controller.exit();
    // The same event seen again through another project stream of this server reads nothing.
    expect(controller.applySettingsEvents([event(2)])).toBeUndefined();
    expect(standbySettings).toHaveBeenCalledTimes(2);
    // Standby was turned off elsewhere: idle entry stops.
    settings = view(false, 5, { enabled: false });
    await controller.applySettingsEvents([event(3)]);
    vi.advanceTimersByTime(60 * 60_000);
    expect(controller.scope.value).toBeUndefined();
    // A resync re-reads even without an event, and forgets the old cursor (a restarted server).
    settings = view(true, 10);
    await controller.resyncSettings();
    expect(standbySettings).toHaveBeenCalledTimes(4);
    await controller.applySettingsEvents([event(1)]);
    expect(standbySettings).toHaveBeenCalledTimes(5);
    vi.advanceTimersByTime(10 * 60_000);
    expect(controller.scope.value).toEqual({ kind: 'all' });
  });

  it('coalesces a burst of settings events into the read in flight plus one more (HS2-A5BTTK)', async () => {
    const pending: ((value: StandbySettingsView) => void)[] = [];
    const standbySettings = vi.fn(
      () =>
        new Promise<StandbySettingsView>((resolve) => {
          pending.push(resolve);
        }),
    );
    const { controller } = harness({ api: { standbySettings, saveStandbySettings: vi.fn() } });
    const event = (cursor: number) => ({ cursor, store: '', kind: 'standby_settings', id: '', slug: '' });
    const first = controller.applySettingsEvents([event(1)]);
    void controller.applySettingsEvents([event(2)]);
    void controller.applySettingsEvents([event(3)]);
    void controller.resyncSettings();
    expect(standbySettings).toHaveBeenCalledTimes(1);
    pending.shift()!(view(true, 20));
    await vi.waitFor(() => {
      expect(standbySettings).toHaveBeenCalledTimes(2);
    });
    pending.shift()!(view(true, 25));
    await first;
    expect(standbySettings).toHaveBeenCalledTimes(2);
    expect(controller.settings.value?.effective.idle_minutes).toBe(25);
  });

  it('holds a wake lock only on touch devices and releases it on exit', async () => {
    const touch = harness({ touch: true });
    touch.controller.enter();
    await Promise.resolve();
    await Promise.resolve();
    expect(touch.request).toHaveBeenCalledWith('screen');
    touch.controller.exit();
    expect(touch.released).toHaveBeenCalledTimes(1);
    const desk = harness();
    desk.controller.enter();
    await Promise.resolve();
    expect(desk.request).not.toHaveBeenCalled();
  });

  it('releases a wake lock granted after exit', async () => {
    const touch = harness({ touch: true });
    let grant!: (value: { release: typeof touch.released }) => void;
    touch.request.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          grant = resolve;
        }),
    );
    touch.controller.enter();
    touch.controller.exit();
    grant({ release: touch.released });
    await Promise.resolve();
    await Promise.resolve();
    expect(touch.released).toHaveBeenCalledTimes(1);
  });

  it('tracks stream drops by first drop time and clears them on recovery or close', () => {
    const { controller } = harness();
    controller.streamHealth('demo-ab12', true);
    expect(controller.streams.value.get('demo-ab12')).toBeNull();
    controller.streamHealth('demo-ab12', false);
    const since = controller.streams.value.get('demo-ab12');
    expect(typeof since).toBe('number');
    // A retry that fails again keeps the first drop time.
    const before = controller.streams.value;
    controller.streamHealth('demo-ab12', false);
    expect(controller.streams.value).toBe(before);
    controller.streamHealth('demo-ab12', true);
    expect(controller.streams.value.get('demo-ab12')).toBeNull();
    controller.streamHealth('demo-ab12', undefined);
    expect(controller.streams.value.has('demo-ab12')).toBe(false);
    const empty = controller.streams.value;
    controller.streamHealth('demo-ab12', undefined);
    expect(controller.streams.value).toBe(empty);
  });

  it('saves both settings from the form and keeps a failure visible with what is stored', async () => {
    const stored = view(true, 15);
    const save = vi.fn((value: StandbySettingsView['configured']) =>
      Promise.resolve(view(value.enabled ?? true, value.idle_minutes ?? 15, value)),
    );
    const api: StandbyApi = {
      standbySettings: vi.fn(() => Promise.resolve(stored)),
      saveStandbySettings: save,
    };
    const { controller } = harness({ api });
    const form = (checked: boolean, minutes: string) =>
      ({
        querySelector: (selector: string) =>
          selector.includes(STANDBY_SETTING_FIELDS.enabled) ? { checked } : { value: minutes },
      }) as unknown as HTMLElement;
    await controller.saveSettings(form(false, '30'));
    expect(save).toHaveBeenCalledWith({ enabled: false, idle_minutes: 30 });
    expect(controller.settings.value?.effective).toEqual({ enabled: false, idle_minutes: 30 });
    await controller.saveSettings(form(true, ''));
    expect(save).toHaveBeenLastCalledWith({ enabled: true, idle_minutes: null });

    save.mockRejectedValueOnce(new Error('idle_minutes must be a whole number'));
    await controller.saveSettings(form(true, '30'));
    expect(controller.settingsMessage.value).toBe('idle_minutes must be a whole number');
    expect(controller.settings.value).toBe(stored);
  });

  it('keeps idle entry off when the server has no standby route', async () => {
    vi.useFakeTimers();
    const api: StandbyApi = {
      standbySettings: vi.fn(() => Promise.reject(new Error('Not Found'))),
      saveStandbySettings: vi.fn(),
    };
    const { controller } = harness({ api });
    await controller.refreshSettings();
    expect(controller.settingsMessage.value).toBe('Not Found');
    vi.advanceTimersByTime(24 * 60 * 60_000);
    expect(controller.scope.value).toBeUndefined();
    // No API (no project open) is a no-op.
    const none = harness();
    await none.controller.refreshSettings();
    await none.controller.saveSettings({} as HTMLElement);
    expect(none.controller.settings.value).toBeUndefined();
  });

  describe('away digest (HS2-JJ9RYN)', () => {
    const live = (revision: number, overrides: Partial<LiveStoreState> = {}): LiveStoreState => ({
      store: 'stw',
      root: '/work/stw.hs2',
      name: 'Small Tale Website',
      checkouts: ['p1'],
      epoch_ms: 1,
      revision,
      state: 'active',
      activity: {
        word: 'active',
        active_since_ms: 1,
        stopped_at_ms: null,
        last_progress_ms: 1,
        idle_deadline_ms: null,
        stalled: [],
        halted: [],
        causes: [],
      },
      claims: [],
      workers: { live: 1, expected: 1 },
      waiting: { count: 0, tickets: [] },
      rates: {
        tokens: { window_minutes: 5, minutes: [], peak_per_minute: 1 },
        tickets_updated: { window_minutes: 60, at_ms: [], peak_per_hour: 1 },
      },
      ...overrides,
    });
    const report = (at: string[]): CalibrationReport => ({
      completions: at.length,
      scored: 0,
      bands: [],
      events: at.map((completed_at, index) => ({ slug: `STW-${String(index)}`, completed_at, outcome: 'pending' })),
    });

    it('an uneventful standby exits silently, after one completions read', async () => {
      const liveStates = createLiveStateStore();
      liveStates.apply(live(1));
      const completions = vi.fn(() => Promise.resolve([report([])]));
      const { controller } = harness({ liveStates, openProjectIds: () => ['p1'], completions });
      controller.enter();
      // A snapshot that changes nothing that matters.
      liveStates.apply(live(2));
      controller.exit();
      await vi.waitFor(() => {
        expect(completions).toHaveBeenCalledWith(['p1']);
      });
      await Promise.resolve();
      expect(controller.awayDigest.value).toBeUndefined();
    });

    it('records pushed changes while standby shows and opens the digest on exit', async () => {
      const liveStates = createLiveStateStore();
      liveStates.apply(live(1));
      const host: HostHealthSnapshot | undefined = undefined;
      const completions = vi.fn(() =>
        // One completion during standby, one long before it.
        Promise.resolve([report([new Date(1_000_500).toISOString(), new Date(0).toISOString()])]),
      );
      const { controller, tick } = harness({
        liveStates,
        openProjectIds: () => ['p1'],
        hostHealth: () => host,
        completions,
      });
      controller.enter();
      tick(60_000);
      liveStates.apply(
        live(2, {
          state: 'waiting_on_you',
          waiting: { count: 1, tickets: [{ ticket: 'STW-7', slug: 'STW-7', title: 'Copy', reason: 'feedback' }] },
        }),
      );
      // Snapshots before entry and after exit are not recorded.
      controller.exit();
      liveStates.apply(live(3, { activity: { ...live(3).activity, stalled: [{ slug: 'STW-8', worker: 'w' }] } }));
      await vi.waitFor(() => {
        expect(controller.awayDigest.value).toBeDefined();
      });
      const digest = controller.awayDigest.value!;
      expect(digest.events).toMatchObject([{ kind: 'waiting', slug: 'STW-7', projectId: 'p1' }]);
      expect(digest.needYou).toBe(1);
      expect(digest.completed).toBe(1);
      controller.dismissAwayDigest();
      expect(controller.awayDigest.value).toBeUndefined();
    });

    it('re-entering standby while completions load drops the stale digest', async () => {
      const liveStates = createLiveStateStore();
      liveStates.apply(live(1));
      let resolve: (value: readonly CalibrationReport[]) => void = () => undefined;
      const completions = vi.fn(
        () =>
          new Promise<readonly CalibrationReport[]>((done) => {
            resolve = done;
          }),
      );
      const { controller } = harness({ liveStates, openProjectIds: () => ['p1'], completions });
      controller.enter();
      liveStates.apply(live(2, { activity: { ...live(2).activity, stalled: [{ slug: 'STW-8', worker: 'w' }] } }));
      controller.exit();
      controller.enter();
      resolve([]);
      await Promise.resolve();
      await Promise.resolve();
      expect(controller.awayDigest.value).toBeUndefined();
      controller.exit();
    });

    it('a failed completions read still shows the events', async () => {
      const liveStates = createLiveStateStore();
      liveStates.apply(live(1));
      const { controller } = harness({
        liveStates,
        openProjectIds: () => ['p1'],
        completions: () => Promise.reject(new Error('offline')),
      });
      controller.enter();
      liveStates.apply(live(2, { activity: { ...live(2).activity, stalled: [{ slug: 'STW-8', worker: 'w' }] } }));
      controller.exit();
      await vi.waitFor(() => {
        expect(controller.awayDigest.value?.events).toMatchObject([{ kind: 'stalled', slug: 'STW-8' }]);
      });
      expect(controller.awayDigest.value?.completed).toBe(0);
    });
  });
});
