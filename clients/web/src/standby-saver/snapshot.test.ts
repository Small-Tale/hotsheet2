import { describe, expect, it } from 'vitest';

import { DEMO_LIVE_NOW_MS, demoLiveState } from '../ux-demo/live-section-fixtures';
import { parseStandbySnapshot, saverView, type StandbySnapshot } from './snapshot';

const NOW = DEMO_LIVE_NOW_MS;
const snapshot = (overrides: Partial<StandbySnapshot> = {}): StandbySnapshot => ({
  version: 1,
  written_at_ms: NOW - 30_000,
  valid_until_ms: NOW + 150_000,
  live: {
    epoch_ms: 1,
    stores: [demoLiveState('attention'), { ...demoLiveState('stalled'), store: 'second', name: 'Beta' }],
  },
  ...overrides,
});

describe('standby screen saver snapshot (HS2-MZM988)', () => {
  it('parses what the server writes and rejects other versions and shapes', () => {
    expect(parseStandbySnapshot(snapshot())).toEqual(snapshot());
    for (const bad of [
      null,
      {},
      { ...snapshot(), version: 2 },
      { ...snapshot(), written_at_ms: 'now' },
      { ...snapshot(), valid_until_ms: undefined },
      { ...snapshot(), live: { epoch_ms: 1, stores: [{ store: 'x' }] } },
    ])
      expect(() => parseStandbySnapshot(bad)).toThrow(/standby snapshot|live state/);
  });

  it('derives the same display as the app: rows in name order, alerts for problem states', () => {
    const view = saverView(snapshot(), NOW, 'en-US');
    expect(view.model.scopeLabel).toBe('All projects');
    expect(view.model.rows.map((row) => row.title)).toEqual(['Beta', 'Hot Sheet 2']);
    expect(view.model.alerts.map((alert) => alert.title)).toEqual(['Beta stalled', 'Hot Sheet 2 is waiting on you']);
    expect(view.freshness).toEqual({ live: true, label: expect.stringMatching(/^live · updated /u) });
  });

  it('says the connection was lost once the snapshot stops being vouched for', () => {
    const view = saverView(snapshot({ valid_until_ms: NOW - 1 }), NOW, 'en-US');
    expect(view.freshness.live).toBe(false);
    expect(view.freshness.label).toMatch(/^connection lost · last update /u);
  });

  it('shows an empty, not-live display before any snapshot has arrived', () => {
    const view = saverView(undefined, NOW, 'en-US');
    expect(view.model.rows).toEqual([]);
    expect(view.freshness).toEqual({ live: false, label: 'connection lost' });
  });

  it('ages a snapshot with the local clock and never extends its vouching', () => {
    const written = snapshot({ valid_until_ms: NOW + 60_000 });
    expect(saverView(written, NOW + 59_000, 'en-US').freshness.live).toBe(true);
    expect(saverView(written, NOW + 60_000, 'en-US').freshness.live).toBe(false);
  });

  it('refills after an empty store list', () => {
    expect(saverView(snapshot({ live: { epoch_ms: 1, stores: [] } }), NOW, 'en-US').model.rows).toEqual([]);
    expect(saverView(snapshot(), NOW, 'en-US').model.rows).toHaveLength(2);
  });
});
