import { afterEach, describe, expect, it } from 'vitest';

import {
  FEATURE_AREAS,
  featureDefault,
  featureOn,
  featureSettings,
  type FeatureSettingsView,
  isFeatureId,
  parseFeatureSettings,
  resetFeatureSettings,
  restoreFeatureDefaults,
  shellModeEnabled,
  shellModeFeature,
  viewModeShown,
} from './feature-areas';
import { matchesShortcut } from './keyboard-shortcuts';

const view = (effective: Record<string, boolean>): FeatureSettingsView => ({
  defaults: Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, area.defaultOn])),
  configured: Object.fromEntries(Object.entries(effective)),
  effective: { ...Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, area.defaultOn])), ...effective },
});

afterEach(() => {
  resetFeatureSettings();
});

describe('feature areas (HS2-3GJZGG)', () => {
  it('defaults experimental areas off and optional areas on, with the four named areas experimental', () => {
    for (const area of FEATURE_AREAS) expect(area.defaultOn, area.id).toBe(area.group !== 'experimental');
    for (const id of ['ai-chat', 'drive', 'project-stats', 'cross-project-stats'] as const)
      expect(featureDefault(id)).toBe(false);
    expect(featureDefault('ticket-groups')).toBe(true);
    expect(new Set(FEATURE_AREAS.map((area) => area.id)).size).toBe(FEATURE_AREAS.length);
  });

  it('reads the server choice, falls back to the default while unread, and resets', () => {
    expect(featureOn('drive')).toBe(false);
    featureSettings.value = view({ drive: true, standby: false });
    expect(featureOn('drive')).toBe(true);
    expect(featureOn('standby')).toBe(false);
    // A server that predates a feature omits it: the default applies.
    featureSettings.value = { defaults: {}, configured: {}, effective: {} };
    expect(featureOn('standby')).toBe(true);
    featureSettings.value = view({ drive: true });
    resetFeatureSettings();
    expect(featureOn('drive')).toBe(false);
  });

  it('parses the server view and rejects a malformed one', () => {
    const value = view({ drive: true });
    expect(parseFeatureSettings({ ...value, configured: { drive: true, standby: null } })).toBeTruthy();
    // Unknown ids from a newer server are accepted and ignored.
    expect(parseFeatureSettings({ ...value, effective: { ...value.effective, teleport: true } })).toBeTruthy();
    expect(() => parseFeatureSettings({ ...value, effective: { drive: 'yes' } })).toThrow();
    expect(() => parseFeatureSettings({ ...value, defaults: { drive: null } })).toThrow();
    expect(() => parseFeatureSettings(null)).toThrow();
  });

  it('restores every feature to its default and recognises registered ids', () => {
    const change = restoreFeatureDefaults();
    expect(Object.keys(change)).toHaveLength(FEATURE_AREAS.length);
    expect(Object.values(change).every((value) => value === null)).toBe(true);
    expect(isFeatureId('drive')).toBe(true);
    expect(isFeatureId('teleport')).toBe(false);
  });

  it('gates dashboards, views and shortcuts by their feature, in both directions', () => {
    expect(shellModeFeature('terminals', undefined)).toBe('workspace-grid');
    expect(shellModeFeature('stats', undefined)).toBe('cross-project-stats');
    expect(shellModeFeature('stats', 'project-1')).toBe('project-stats');
    expect(shellModeFeature('project', undefined)).toBeUndefined();
    // Defaults: the Grid shows, both stats pages do not.
    expect(shellModeEnabled('terminals', undefined)).toBe(true);
    expect(shellModeEnabled('stats', undefined)).toBe(false);
    expect(shellModeEnabled('stats', 'project-1')).toBe(false);
    expect(viewModeShown('workers')).toBe(true);
    const grid = { key: 'g', metaKey: true, ctrlKey: false, shiftKey: true, altKey: false } as KeyboardEvent;
    const stats = { key: 'd', metaKey: true, ctrlKey: false, shiftKey: true, altKey: false } as KeyboardEvent;
    expect(matchesShortcut('view-workspace-grid', grid, {}, true)).toBe(true);
    expect(matchesShortcut('view-all-stats', stats, {}, true)).toBe(false);
    // Turn the stats on and the grid and Workers view off, then back.
    featureSettings.value = view({ 'cross-project-stats': true, 'workspace-grid': false, 'workers-view': false });
    expect(shellModeEnabled('stats', undefined)).toBe(true);
    expect(shellModeEnabled('terminals', undefined)).toBe(false);
    expect(viewModeShown('workers')).toBe(false);
    expect(viewModeShown('list')).toBe(true);
    expect(matchesShortcut('view-workspace-grid', grid, {}, true)).toBe(false);
    expect(matchesShortcut('view-all-stats', stats, {}, true)).toBe(true);
    resetFeatureSettings();
    expect(matchesShortcut('view-workspace-grid', grid, {}, true)).toBe(true);
    expect(viewModeShown('workers')).toBe(true);
  });
});
