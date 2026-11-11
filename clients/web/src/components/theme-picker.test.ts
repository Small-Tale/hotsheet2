import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { DARK_TILE, LIGHT_TILE, themeTileArt } from '../theme-tile-art';
import { ThemePicker } from './theme-picker';

describe('ThemePicker (HS2-EFTHS6)', () => {
  it('offers Auto, Light, and Dark as one named radio group with the current choice checked', () => {
    for (const value of ['auto', 'light', 'dark'] as const) {
      const html = String(ThemePicker({ value }));
      expect(html).toContain('role="radiogroup"');
      expect(html).toContain('aria-labelledby="hotsheet-theme-label"');
      expect(html.match(/type="radio"/g)).toHaveLength(3);
      expect(html.match(/name="hotsheet-theme"/g)).toHaveLength(3);
      expect(html.match(/data-action="choose-theme"/g)).toHaveLength(3);
      expect(html.match(/checked/g)).toHaveLength(1);
      expect(html).toMatch(new RegExp(`value="${value}"[^>]*checked|checked[^>]*value="${value}"`));
      for (const label of ['Auto', 'Light', 'Dark']) expect(html).toContain(`>${label}<`);
    }
  });

  it('keeps several pickers on one page independent through their names', () => {
    const html = String(ThemePicker({ value: 'dark', name: 'demo-theme' }));
    expect(html).toContain('name="demo-theme"');
    expect(html).toContain('id="demo-theme-label"');
    expect(html).toContain('id="demo-theme-hint"');
    expect(html).not.toContain('hotsheet-theme');
  });
});

describe('theme tile art (HS2-EFTHS6)', () => {
  it('paints each tile in its own frozen palette and splits Auto between both', () => {
    const light = String(themeTileArt('light')),
      dark = String(themeTileArt('dark')),
      auto = String(themeTileArt('auto'));
    expect(light).toContain(LIGHT_TILE.bg);
    expect(light).not.toContain(DARK_TILE.bg);
    expect(dark).toContain(DARK_TILE.bg);
    expect(dark).not.toContain(LIGHT_TILE.bg);
    expect(auto).toContain(LIGHT_TILE.bg);
    expect(auto).toContain(DARK_TILE.bg);
    for (const art of [light, dark, auto]) expect(art).toContain('aria-hidden="true"');
    expect(new Set([light, dark, auto].map((art) => /id="(theme-tile-clip-[a-z]+)"/.exec(art)?.[1])).size).toBe(3);
  });

  it('matches the window theme colors declared for each scheme', () => {
    const html = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
    expect(html).toContain(`content="${LIGHT_TILE.bg}" media="(prefers-color-scheme: light)"`);
    expect(html).toContain(`content="${DARK_TILE.bg}" media="(prefers-color-scheme: dark)"`);
  });
});
