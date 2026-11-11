import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { LIVE_STATE_ICONS } from '../live-state-icons';
import type { StandbyModel } from '../standby';
import { StandbyScreen, type StandbyScreenProps } from './standby-screen';
import { standbyIdleChoices, StandbySettings } from './standby-settings';

const MODEL: StandbyModel = {
  scopeLabel: 'All projects',
  tokensPerMinute: 1_600,
  tokensPeakPerMinute: 9_800,
  ticketsUpdated: 17,
  rows: [
    { key: 's1', word: 'active', title: 'Hot Sheet 2', detail: 'Active · 2m · HS2-R76MMW', alert: false },
    { key: 's2', word: 'stalled', title: 'Small Tale Website', detail: 'Stalled · STW-4K2P', alert: true },
  ],
  alerts: [{ key: 's2', word: 'stalled', title: 'Small Tale Website stalled', detail: 'STW-4K2P · lease expired' }],
};

function render(props: Partial<StandbyScreenProps> = {}): string {
  return String(
    StandbyScreen({
      clock: '2:47',
      model: MODEL,
      freshness: { live: true, label: 'live · updated 2:47:12' },
      ...props,
    }),
  );
}

describe('StandbyScreen (HS2-EQVZTZ)', () => {
  it('is a top-layer exit target over the whole viewport in the app', () => {
    const markup = render();
    const root = /^<dialog\b[^>]*>/.exec(markup)?.[0] ?? '';
    expect(root).toContain('data-component="standby-screen"');
    expect(root).toContain('data-action="exit-standby"');
    expect(root).toContain('popover="manual"');
    expect(root).toContain('data-top-layer-overlay');
    expect(root).toContain('aria-label="Standby"');
    expect(markup).toContain('Click or press any key to exit');
    expect(render({ touch: true })).toContain('Tap anywhere to exit');
  });

  it('stages inline in the catalog without top-layer or exit wiring', () => {
    const markup = render({ presentation: 'inline' });
    expect(markup.startsWith('<section')).toBe(true);
    expect(markup).not.toContain('exit-standby');
    expect(markup).not.toContain('popover');
  });

  it('shows the dark-red gauge, the count, the clock, and the scope', () => {
    const markup = render();
    expect(markup).toContain('data-palette="standby"');
    expect(markup).toContain('data-size="fill"');
    expect(markup).toContain('Token rate: 1.6k tokens/min (24-hour peak 9.8k)');
    expect(markup).toContain('aria-label="Tickets updated: 17"');
    expect(markup).toContain('tickets updated · last 60 min');
    expect(markup).toMatch(/data-role="standby-clock"><svg[^>]*aria-label="Time: 2:47"[^>]*>.*>2:47<\/text>/s);
    expect(markup).toMatch(/data-role="standby-scope"[^>]*>All projects</);
  });

  it('brightens only problems: one amber callout and the problem row', () => {
    const markup = render();
    expect(markup.match(/data-role="standby-alert"/g)).toHaveLength(1);
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('Small Tale Website stalled');
    expect(markup).toMatch(/data-role="standby-row" data-word="stalled" data-alert="true"/);
    expect(markup).toMatch(/data-role="standby-row" data-word="active" data-alert="false"/);
    // State glyphs come from the standardized set (HS2-CSPKQX).
    expect(markup).toContain(`data-lucide="${LIVE_STATE_ICONS.stalled.name}"`);
    expect(markup).toContain(`data-lucide="${LIVE_STATE_ICONS.active.name}"`);
    const quiet = render({ model: { ...MODEL, alerts: [], rows: [] } });
    expect(quiet).not.toContain('standby-alert');
    expect(quiet).not.toContain('standby-row');
    expect(quiet).not.toContain('role="alert"');
  });

  it('says when the stream dropped', () => {
    expect(render()).toMatch(/data-role="standby-freshness" data-live="true"[^>]*>.*live · updated 2:47:12/s);
    const dropped = render({ freshness: { live: false, label: 'connection lost · last update 2:41:03' } });
    expect(dropped).toMatch(/data-live="false"[^>]*>.*connection lost · last update 2:41:03/s);
  });

  it('keeps standby colours on its own tokens, drifts, and respects reduced motion', () => {
    const css = readFileSync(resolve(__dirname, 'standby-screen.css'), 'utf8');
    expect(css).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(css).not.toMatch(/\.kui-/);
    expect(css).toContain('animation: standby-screen-drift');
    expect(css).toMatch(/prefers-reduced-motion: reduce\)\s*{\s*\.standby-screen__content\s*{\s*animation: none;/);
    expect(css).toContain("[data-live='false']");
    expect(css).toContain('@container standby (orientation: portrait)');
  });
});

describe('StandbySettings (HS2-EQVZTZ)', () => {
  it('saves through one delegated change and disables the threshold while off', () => {
    const on = String(
      StandbySettings({
        settings: {
          defaults: { enabled: true, idle_minutes: 15 },
          configured: { enabled: null, idle_minutes: null },
          effective: { enabled: true, idle_minutes: 15 },
        },
      }),
    );
    expect(on).toContain('data-action="save-standby-settings"');
    expect(on).toContain('name="standby-enabled"');
    expect(on).toContain('name="standby-idle-minutes"');
    expect(on).toContain('Enter standby when idle');
    const off = String(
      StandbySettings({
        settings: {
          defaults: { enabled: true, idle_minutes: 15 },
          configured: { enabled: false, idle_minutes: null },
          effective: { enabled: false, idle_minutes: 15 },
        },
        message: 'Saved elsewhere',
      }),
    );
    expect(off).toMatch(/<kui-select[^>]*name="standby-idle-minutes"[^>]*disabled/);
    expect(off).toContain('Saved elsewhere');
    const loading = String(StandbySettings({}));
    expect(loading).toMatch(/name="standby-enabled"[^>]*disabled|disabled[^>]*name="standby-enabled"/);
  });

  it('offers the presets plus a CLI-set value', () => {
    expect(standbyIdleChoices(15).map((choice) => choice.label)).toEqual([
      '5 minutes',
      '10 minutes',
      '15 minutes',
      '30 minutes',
      '1 hour',
      '2 hours',
      '4 hours',
    ]);
    expect(standbyIdleChoices(45).map((choice) => choice.value)).toEqual([
      '5',
      '10',
      '15',
      '30',
      '45',
      '60',
      '120',
      '240',
    ]);
    expect(standbyIdleChoices(undefined)).toHaveLength(7);
  });
});
