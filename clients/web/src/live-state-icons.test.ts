import * as lucide from 'lucide';
import { describe, expect, it } from 'vitest';

import { LIVE_STATE_ICONS, STANDBY_ICON } from './live-state-icons.js';

function pascal(name: string): string {
  return name.replace(/(^|-)([a-z0-9])/g, (_match, _dash: string, letter: string) => letter.toUpperCase());
}

describe('HS2-CSPKQX glance state icons', () => {
  it('pins the maintainer-standardized state-to-icon map', () => {
    expect(Object.fromEntries(Object.entries(LIVE_STATE_ICONS).map(([word, glyph]) => [word, glyph.name]))).toEqual({
      active: 'activity',
      waiting_on_you: 'hand',
      may_be_stuck: 'hourglass',
      stalled: 'octagon-alert',
      halted: 'zap-off',
      stopped: 'circle-pause',
    });
    expect(STANDBY_ICON.name).toBe('moon');
  });

  it('gives each state a distinct official Lucide asset whose node matches its name', () => {
    const glyphs = [...Object.values(LIVE_STATE_ICONS), STANDBY_ICON];
    expect(new Set(glyphs.map((glyph) => glyph.name)).size).toBe(glyphs.length);
    for (const glyph of glyphs) expect(glyph.icon).toBe((lucide as Record<string, unknown>)[pascal(glyph.name)]);
  });
});
