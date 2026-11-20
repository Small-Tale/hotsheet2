import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ChangeEvent } from '../api';
import {
  FEATURE_AREAS,
  featureOn,
  featureSettings,
  type FeatureSettingsChange,
  type FeatureSettingsView,
  resetFeatureSettings,
} from '../feature-areas';
import { createFeatureAreasController, type FeatureAreasApi } from './feature-areas';

const view = (effective: Record<string, boolean>): FeatureSettingsView => ({
  defaults: Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, area.defaultOn])),
  configured: {},
  effective: { ...Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, area.defaultOn])), ...effective },
});
const event = (cursor: number | undefined, kind = 'feature_settings'): ChangeEvent => ({
  cursor,
  store: '',
  kind,
  id: '',
  slug: '',
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

afterEach(() => {
  resetFeatureSettings();
});

describe('feature areas controller (HS2-3GJZGG)', () => {
  it('reads once, re-reads once per new event, ignores repeats and other kinds, and re-reads on resync', async () => {
    const read = vi.fn(async () => view({ drive: true }));
    const api: FeatureAreasApi = { featureSettings: read, saveFeatureSettings: vi.fn() };
    const controller = createFeatureAreasController({ api: () => api });
    await controller.refresh();
    expect(featureOn('drive')).toBe(true);
    expect(controller.applyEvents([event(3, 'ticket_updated')])).toBeUndefined();
    await controller.applyEvents([event(4)]);
    // The same event through a second project stream reads nothing more.
    expect(controller.applyEvents([event(4)])).toBeUndefined();
    await controller.applyEvents([event(5)]);
    expect(read).toHaveBeenCalledTimes(3);
    // A restarted server restarts its cursors: a resync forgets the last one.
    await controller.resync();
    await controller.applyEvents([event(1)]);
    expect(read).toHaveBeenCalledTimes(5);
  });

  it('coalesces events that arrive during a read into one more read', async () => {
    const reads = [deferred<FeatureSettingsView>(), deferred<FeatureSettingsView>()];
    let call = 0;
    const read = vi.fn(() => reads[call++].promise);
    const api: FeatureAreasApi = { featureSettings: read, saveFeatureSettings: vi.fn() };
    const controller = createFeatureAreasController({ api: () => api });
    const first = controller.applyEvents([event(1)]);
    void controller.applyEvents([event(2)]);
    void controller.applyEvents([event(3)]);
    reads[0].resolve(view({ drive: false }));
    await Promise.resolve();
    await Promise.resolve();
    reads[1].resolve(view({ drive: true }));
    await first;
    expect(read).toHaveBeenCalledTimes(2);
    expect(featureOn('drive')).toBe(true);
  });

  it('saves a checkbox at once, restores defaults, and recovers the truth after a failed save', async () => {
    const save = vi.fn(async (change: FeatureSettingsChange) => view(change as Record<string, boolean>));
    const api: FeatureAreasApi = {
      featureSettings: vi.fn(async () => view({ standby: true })),
      saveFeatureSettings: save,
    };
    const controller = createFeatureAreasController({ api: () => api });
    await controller.setFeature('standby', false);
    expect(save).toHaveBeenLastCalledWith({ standby: false });
    expect(featureOn('standby')).toBe(false);
    expect(controller.setFeature('teleport', true)).toBeUndefined();
    await controller.restoreDefaults();
    const restored = save.mock.calls.at(-1)![0];
    expect(Object.values(restored).every((value) => value === null)).toBe(true);

    save.mockRejectedValueOnce(new Error('settings.json is read-only'));
    await controller.setFeature('standby', false);
    expect(controller.settingsMessage.value).toBe('settings.json is read-only');
    expect(featureOn('standby')).toBe(true);
    // A later successful save clears the message.
    await controller.setFeature('drive', true);
    expect(controller.settingsMessage.value).toBe('');
  });

  it('keeps defaults when no project is open or the server has no feature route', async () => {
    const none = createFeatureAreasController({ api: () => undefined });
    await none.refresh();
    expect(featureSettings.value).toBeUndefined();
    const old = createFeatureAreasController({
      api: () => ({
        featureSettings: () => Promise.reject(new Error('404')),
        saveFeatureSettings: vi.fn(),
      }),
    });
    await old.refresh();
    expect(featureOn('workers-view')).toBe(true);
    expect(featureOn('drive')).toBe(false);
    expect(old.settingsMessage.value).toBe('404');
  });
});
