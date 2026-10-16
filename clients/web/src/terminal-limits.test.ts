import { afterEach, describe, expect, it, vi } from 'vitest';

import type { TerminalLimits } from './api';
import { createTerminalLimitsSettings, terminalLimitChoiceValue } from './features/terminal-limits-settings';
import { parseTerminalLimits, ServerResponseShapeError } from './server-response-parsers';
import {
  applyTerminalLimits,
  CONSERVATIVE_TERMINAL_LIMITS,
  currentTerminalLimits,
  isPhoneDevice,
  magnifiedHistoryLines,
  onTerminalLimitsChange,
  PHONE_TERMINAL_LIMITS,
  resolveClientTerminalLimits,
} from './terminal-limits';
import { terminalScrollbackLimit } from './terminal-viewport';

const large: TerminalLimits = {
  physical_memory_bytes: 64 * 1024 ** 3,
  defaults: { history_lines: 20_000, replay_bytes: 3_840_000, warm_terminals: 24 },
  configured: { history_lines: null, replay_bytes: null, warm_terminals: null },
  effective: { history_lines: 20_000, replay_bytes: 3_840_000, warm_terminals: 24 },
};
const local = { remote: false, phone: false },
  remote = { remote: true, phone: false },
  phone = { remote: true, phone: true };

afterEach(() => {
  applyTerminalLimits(undefined, remote);
});

describe('client terminal limits (HS2-Y5DYC0)', () => {
  it('uses the server memory-derived defaults only on the server machine', () => {
    expect(resolveClientTerminalLimits(large, local)).toEqual({
      historyLines: 20_000,
      magnifiedHistoryLines: 4_000,
      warmTerminals: 24,
    });
    // Another device or a phone keeps conservative defaults: the server's memory is not theirs.
    expect(resolveClientTerminalLimits(large, remote)).toMatchObject({
      historyLines: CONSERVATIVE_TERMINAL_LIMITS.historyLines,
      warmTerminals: CONSERVATIVE_TERMINAL_LIMITS.warmTerminals,
    });
    expect(resolveClientTerminalLimits(large, phone)).toMatchObject({
      historyLines: PHONE_TERMINAL_LIMITS.historyLines,
      warmTerminals: PHONE_TERMINAL_LIMITS.warmTerminals,
    });
    // Before the server answers (or on an older server), even a local client stays conservative.
    expect(resolveClientTerminalLimits(undefined, local)).toEqual({
      historyLines: 5_000,
      magnifiedHistoryLines: 1_000,
      warmTerminals: 12,
    });
  });

  it('lets an explicit setting win everywhere, including on a phone', () => {
    const configured = { ...large, configured: { history_lines: 8_000, replay_bytes: null, warm_terminals: 0 } };
    for (const context of [local, remote, phone])
      expect(resolveClientTerminalLimits(configured, context)).toEqual({
        historyLines: 8_000,
        magnifiedHistoryLines: 1_600,
        warmTerminals: 0,
      });
  });

  it('keeps a fifth of the history for a magnified card within bounds', () => {
    expect(magnifiedHistoryLines(5_000)).toBe(1_000);
    expect(magnifiedHistoryLines(500)).toBe(200);
    expect(magnifiedHistoryLines(100)).toBe(100);
  });

  it('feeds the emulator scrollback and notifies listeners when applied', () => {
    const listener = vi.fn(),
      stop = onTerminalLimitsChange(listener);
    applyTerminalLimits(large, local);
    expect(currentTerminalLimits().historyLines).toBe(20_000);
    expect(terminalScrollbackLimit('interactive', false)).toBe(20_000);
    expect(terminalScrollbackLimit('interactive', true)).toBe(4_000);
    expect(terminalScrollbackLimit('scaled-preview', true)).toBe(0);
    expect(listener).toHaveBeenCalledWith(expect.objectContaining({ warmTerminals: 24 }));
    stop();
    applyTerminalLimits(undefined, remote);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('treats a device whose larger screen side is below the desktop floor as a phone', () => {
    expect(isPhoneDevice({ width: 390, height: 844 })).toBe(true);
    expect(isPhoneDevice({ width: 1024, height: 1366 })).toBe(false);
    expect(isPhoneDevice({ width: 1512, height: 982 })).toBe(false);
  });
});

describe('terminal limits response parsing', () => {
  it('accepts the server shape and rejects a malformed one', () => {
    expect(parseTerminalLimits(large)).toEqual(large);
    expect(parseTerminalLimits({ ...large, physical_memory_bytes: null })).toMatchObject({
      physical_memory_bytes: null,
    });
    expect(() => parseTerminalLimits({ ...large, defaults: { history_lines: '5000' } })).toThrow(
      ServerResponseShapeError,
    );
    expect(() => parseTerminalLimits({ ...large, configured: { history_lines: 'x' } })).toThrow(
      ServerResponseShapeError,
    );
  });
});

describe('terminal limits settings controller', () => {
  function controller(context = local) {
    const api = {
      terminalLimits: vi.fn(() => Promise.resolve(large)),
      saveTerminalLimits: vi.fn((configured: TerminalLimits['configured']) =>
        Promise.resolve({
          ...large,
          configured,
          effective: { ...large.effective, history_lines: configured.history_lines ?? 20_000 },
        }),
      ),
    };
    return { api, settings: createTerminalLimitsSettings({ api: () => api, context: () => context }) };
  }

  it('loads, applies, and saves the limits', async () => {
    const { api, settings } = controller();
    await settings.refresh();
    expect(settings.limits.value).toEqual(large);
    expect(currentTerminalLimits().warmTerminals).toBe(24);
    await settings.save({ history_lines: 2_500, replay_bytes: null, warm_terminals: null });
    expect(api.saveTerminalLimits).toHaveBeenCalledWith({
      history_lines: 2_500,
      replay_bytes: null,
      warm_terminals: null,
    });
    expect(currentTerminalLimits().historyLines).toBe(2_500);
    expect(settings.deviceDefaults()).toBeUndefined();
  });

  it('reports a failed save and re-reads what is stored', async () => {
    const { api, settings } = controller();
    api.saveTerminalLimits.mockRejectedValueOnce(new Error('terminal.history_lines must be a whole number'));
    await settings.save({ history_lines: 5, replay_bytes: null, warm_terminals: null });
    expect(settings.message.value).toContain('whole number');
    await vi.waitFor(() => {
      expect(api.terminalLimits).toHaveBeenCalledTimes(1);
    });
  });

  it('ignores an older answer that arrives after a newer save', async () => {
    const { api, settings } = controller();
    let resolveRead: (value: TerminalLimits) => void = () => undefined;
    api.terminalLimits.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveRead = resolve;
      }),
    );
    const read = settings.refresh();
    await settings.save({ history_lines: 1_000, replay_bytes: null, warm_terminals: null });
    resolveRead(large);
    await read;
    expect(settings.limits.value?.configured.history_lines).toBe(1_000);
  });

  it('keeps conservative defaults when an older server has no limits route', async () => {
    const { api, settings } = controller();
    api.terminalLimits.mockRejectedValueOnce(new Error('Not Found'));
    await settings.refresh();
    expect(settings.limits.value).toBeUndefined();
    expect(currentTerminalLimits().historyLines).toBe(5_000);
  });

  it('names the defaults another device uses', () => {
    expect(controller(remote).settings.deviceDefaults()).toEqual({ historyLines: 5_000, warmTerminals: 12 });
    expect(controller(phone).settings.deviceDefaults()).toEqual({ historyLines: 2_500, warmTerminals: 4 });
  });

  it('maps settings choices to configured values', () => {
    expect(terminalLimitChoiceValue('default')).toBeNull();
    expect(terminalLimitChoiceValue(undefined)).toBeNull();
    expect(terminalLimitChoiceValue('')).toBeNull();
    expect(terminalLimitChoiceValue('5000')).toBe(5_000);
    expect(terminalLimitChoiceValue('nope')).toBeNull();
  });
});
