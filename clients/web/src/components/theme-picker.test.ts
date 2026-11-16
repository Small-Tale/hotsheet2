import { existsSync, readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { DARK_TILE, LIGHT_TILE, themeTileArt } from '../theme-tile-art';
import { ThemePicker } from './theme-picker';

describe('ThemePicker (HS2-EFTHS6, HS2-VW7JVV)', () => {
  it('offers Auto, Light, and Dark as one Kerf tile radio group with the current choice checked', () => {
    for (const value of ['auto', 'light', 'dark'] as const) {
      const html = String(ThemePicker({ value }));
      expect(html).toContain('data-component="radio-group"');
      expect(html).toContain('data-presentation="tiles"');
      expect(html).toContain('role="radiogroup"');
      expect(html).toContain('data-theme-picker=""');
      expect(html).toMatch(/<legend[^>]*>Theme<\/legend>/);
      expect(html).toContain('aria-description="Auto follows this device&#39;s light or dark appearance.');
      expect(html.match(/type="radio"/g)).toHaveLength(3);
      expect(html.match(/name="hotsheet-theme"/g)).toHaveLength(3);
      expect(html.match(/data-action="choose-theme"/g)).toHaveLength(3);
      expect(html.match(/checked/g)).toHaveLength(1);
      expect(html).toMatch(new RegExp(`value="${value}"[^>]*checked|checked[^>]*value="${value}"`));
      for (const label of ['Auto', 'Light', 'Dark']) expect(html).toContain(`>${label}<`);
      // Each choice's preview is the app's own artwork, not a Kerf placeholder.
      expect(html.match(/preserveAspectRatio="xMidYMid slice"/g)).toHaveLength(3);
    }
  });

  it('keeps several pickers on one page independent through their names', () => {
    const html = String(ThemePicker({ value: 'dark', name: 'demo-theme' }));
    expect(html.match(/name="demo-theme"/g)).toHaveLength(3);
    expect(html).not.toContain('hotsheet-theme');
  });

  it('no longer ships an app stylesheet for the tiles (HS2-VW7JVV)', () => {
    expect(existsSync(new URL('./theme-picker.css', import.meta.url))).toBe(false);
    expect(readFileSync(new URL('./theme-picker.tsx', import.meta.url), 'utf8')).not.toMatch(/import '.*\.css'/);
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
    expect(auto).toContain('id="theme-tile-clip-auto-light"');
    expect(auto).toContain('id="theme-tile-clip-auto-dark"');
    expect(light).not.toContain('clipPath');
    expect(dark).not.toContain('clipPath');
  });

  it('matches the window theme colors declared for each scheme', () => {
    const html = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
    expect(html).toContain(`content="${LIGHT_TILE.bg}" media="(prefers-color-scheme: light)"`);
    expect(html).toContain(`content="${DARK_TILE.bg}" media="(prefers-color-scheme: dark)"`);
  });
});
