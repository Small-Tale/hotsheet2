import { describe, expect, it } from 'vitest';

import { buildTerminalTheme, LIGHT_TERMINAL_ANSI, parseComputedColor, terminalSchemeFor } from './terminal-theme';

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
