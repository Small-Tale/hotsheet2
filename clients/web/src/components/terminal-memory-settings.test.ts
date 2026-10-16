import { describe, expect, it } from 'vitest';

import { DEMO_TERMINAL_LIMITS, DEMO_TERMINAL_LIMITS_CONFIGURED } from '../ux-demo/fixtures';
import {
  formatMemoryBytes,
  TERMINAL_LIMIT_DEFAULT,
  terminalLimitChoices,
  TerminalMemorySettings,
} from './terminal-memory-settings';

describe('TerminalMemorySettings (HS2-Y5DYC0)', () => {
  it('formats byte sizes', () => {
    expect(formatMemoryBytes(512)).toBe('512 bytes');
    expect(formatMemoryBytes(256 * 1024)).toBe('256 KB');
    expect(formatMemoryBytes(3_840_000)).toBe('3.7 MB');
    expect(formatMemoryBytes(32 * 1024 ** 3)).toBe('32 GB');
  });

  it('lists the default with its value, the presets, and an off-preset configured value', () => {
    const format = (value: number) => `${value}`;
    expect(terminalLimitChoices([1, 2], 2, null, format).map((choice) => choice.value)).toEqual([
      TERMINAL_LIMIT_DEFAULT,
      '1',
      '2',
    ]);
    const withCustom = terminalLimitChoices([1, 4], 4, 3, format);
    expect(withCustom.map((choice) => choice.value)).toEqual([TERMINAL_LIMIT_DEFAULT, '1', '3', '4']);
    expect(withCustom[0]?.label).toBe('Default (4)');
  });

  it('renders the memory-derived defaults and the selected explicit values', () => {
    const defaults = String(TerminalMemorySettings({ limits: DEMO_TERMINAL_LIMITS }));
    expect(defaults).toContain('32 GB of memory');
    expect(defaults).toContain('Default (20,000 lines)');
    expect(defaults).toContain('Default (24 terminals)');
    expect(defaults).toContain('data-action="save-terminal-limits"');
    expect(defaults).not.toContain('data-role="terminal-limits-conservative"');

    const configured = String(TerminalMemorySettings({ limits: DEMO_TERMINAL_LIMITS_CONFIGURED }));
    expect(configured).toContain('8,000 lines');
    // The unset replay names the size that follows the configured history.
    expect(configured).toContain('Default (1.5 MB)');
  });

  it('names this device’s smaller defaults on another device, and shows loading and failures', () => {
    const remote = String(
      TerminalMemorySettings({
        limits: DEMO_TERMINAL_LIMITS,
        deviceDefaults: { historyLines: 5_000, warmTerminals: 12 },
      }),
    );
    expect(remote).toContain('data-role="terminal-limits-conservative"');
    expect(remote).toContain('Default (5,000 lines)');
    expect(remote).toContain('Default (12 terminals)');
    expect(
      String(TerminalMemorySettings({ limits: { ...DEMO_TERMINAL_LIMITS, physical_memory_bytes: null } })),
    ).toContain('did not report its memory');
    expect(String(TerminalMemorySettings({ message: 'boom' }))).toContain('boom');
  });
});
