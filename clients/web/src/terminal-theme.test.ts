import { readFileSync } from 'node:fs';

import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  buildTerminalTheme,
  hexColor,
  LIGHT_TERMINAL_ANSI,
  parseComputedColor,
  resolveTerminalSearchDecorations,
  terminalSchemeFor,
} from './terminal-theme';

const luminance = (hex: string) => {
  const value = Number.parseInt(hex.slice(1), 16),
    channel = (byte: number) => {
      const srgb = byte / 255;
      return srgb <= 0.03928 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
    };
  return 0.2126 * channel(value >> 16) + 0.7152 * channel((value >> 8) & 255) + 0.0722 * channel(value & 255);
};

describe('terminal theme (HS2-QNSS9C)', () => {
  it('parses the computed rgb and rgba forms', () => {
    expect(parseComputedColor('rgb(255, 255, 255)')).toEqual([255, 255, 255, 1]);
    expect(parseComputedColor('rgba(29, 29, 31, 0.5)')).toEqual([29, 29, 31, 0.5]);
    expect(parseComputedColor('rgb(0 0 0 / 40%)')).toEqual([0, 0, 0, 0.4]);
    expect(parseComputedColor('light-dark(#fff, #000)')).toBeUndefined();
  });

  it('classifies the resolved background as the light or dark scheme', () => {
    expect(terminalSchemeFor('rgb(255, 255, 255)')).toBe('light');
    expect(terminalSchemeFor('rgb(0, 0, 0)')).toBe('dark');
    // An unparseable value keeps the original dark terminal.
    expect(terminalSchemeFor('')).toBe('dark');
  });

  it('keeps the dark theme on xterm stock ANSI colors and derives cursor and selection', () => {
    const theme = buildTerminalTheme('rgb(0, 0, 0)', 'rgb(255, 255, 255)');
    expect(theme).toEqual({
      background: 'rgb(0, 0, 0)',
      foreground: 'rgb(255, 255, 255)',
      cursor: 'rgb(255, 255, 255)',
      cursorAccent: 'rgb(0, 0, 0)',
      selectionBackground: 'rgba(255, 255, 255, 0.3)',
    });
  });

  it('gives the light theme a palette that stays legible on white', () => {
    const theme = buildTerminalTheme('rgb(255, 255, 255)', 'rgb(29, 29, 31)');
    expect(theme).toMatchObject({
      background: 'rgb(255, 255, 255)',
      foreground: 'rgb(29, 29, 31)',
      cursor: 'rgb(29, 29, 31)',
      cursorAccent: 'rgb(255, 255, 255)',
      selectionBackground: 'rgba(29, 29, 31, 0.22)',
      ...LIGHT_TERMINAL_ANSI,
    });
    // Every ANSI color except the deliberately pale "white" pair reaches 4.5:1 against white.
    for (const [name, color] of Object.entries(LIGHT_TERMINAL_ANSI)) {
      const ratio = 1.05 / (luminance(color as string) + 0.05);
      expect(ratio, name).toBeGreaterThanOrEqual(name === 'white' || name === 'brightWhite' ? 3 : 4.5);
    }
  });

  it('omits the selection color when the foreground is unparseable', () => {
    expect(buildTerminalTheme('rgb(255, 255, 255)', 'canvastext')).not.toHaveProperty('selectionBackground');
  });
});

describe('terminal search highlight colors (HS2-M5QPED)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('converts computed colors to the #rrggbb form xterm decorations require', () => {
    expect(hexColor('rgb(253, 230, 138)')).toBe('#fde68a');
    expect(hexColor('rgba(0, 0, 0, 0.5)')).toBe('#000000');
    expect(hexColor('rgb(255.4, 0.6, 16)')).toBe('#ff0110');
    expect(hexColor('canvastext')).toBeUndefined();
  });

  it('resolves both tokens through a probe and falls back when a token is unresolvable', () => {
    const appended: { style: { color: string }; remove: () => void }[] = [];
    const element = {
      ownerDocument: {
        createElement: () => {
          const probe = { hidden: false, style: { color: '' }, remove: vi.fn() };
          return probe;
        },
      },
      append: (probe: { style: { color: string }; remove: () => void }) => appended.push(probe),
    } as unknown as HTMLElement;
    const computed: Record<string, string> = {
      'var(--hs-terminal-search-match, #fde68a)': 'rgb(92, 72, 19)',
      'var(--hs-terminal-search-active-match, #f59e0b)': 'oklch(0.6 0.1 60)',
    };
    vi.stubGlobal('getComputedStyle', (probe: { style: { color: string } }) => ({
      color: computed[probe.style.color] ?? '',
    }));
    expect(resolveTerminalSearchDecorations(element)).toEqual({
      matchBackground: '#5c4813',
      matchOverviewRuler: '#5c4813',
      activeMatchBackground: '#f59e0b',
      activeMatchColorOverviewRuler: '#f59e0b',
    });
    expect(appended).toHaveLength(1);
    expect(appended[0].remove).toHaveBeenCalledOnce();
  });

  it('keeps terminal text legible on every highlight in both schemes', () => {
    const tokens = readFileSync(new URL('./hot-sheet-tokens.css', import.meta.url), 'utf8'),
      pair = (name: string) => {
        const match = new RegExp(`${name}: light-dark\\((#[0-9a-f]{6}), (#[0-9a-f]{6})\\);`).exec(tokens);
        expect(match, name).not.toBeNull();
        return { light: match![1], dark: match![2] };
      },
      contrast = (a: string, b: string) => {
        const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
        return (high + 0.05) / (low + 0.05);
      };
    for (const name of ['--hs-terminal-search-match', '--hs-terminal-search-active-match']) {
      const { light, dark } = pair(name);
      // Light terminals draw near-black text; dark terminals draw near-white text.
      expect(contrast(light, '#1d1d1f'), `${name} light`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(dark, '#f5f5f7'), `${name} dark`).toBeGreaterThanOrEqual(4.5);
    }
  });
});
