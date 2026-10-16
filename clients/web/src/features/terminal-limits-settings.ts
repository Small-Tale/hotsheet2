import { signal } from 'kerfjs';

import type { TerminalLimits } from '../api';
import { isRemoteClient } from '../client-origin';
import {
  applyTerminalLimits,
  isPhoneDevice,
  resolveClientTerminalLimits,
  type TerminalLimitContext,
} from '../terminal-limits';

/** The server calls the terminal-limits controller makes. */
export interface TerminalLimitsApi {
  terminalLimits(): Promise<TerminalLimits>;
  saveTerminalLimits(value: TerminalLimits['configured']): Promise<TerminalLimits>;
}

export interface TerminalLimitsSettingsDependencies {
  /** The API of the server this client talks to, once a project is open. */
  api: () => TerminalLimitsApi | undefined;
  /** Where this client runs; defaults to the page origin and screen size. */
  context?: () => TerminalLimitContext;
}

const browserContext = (): TerminalLimitContext => ({ remote: isRemoteClient(), phone: isPhoneDevice() });

/**
 * Loads, applies, and saves the machine-wide terminal limits (HS2-Y5DYC0). Every answer is applied to
 * the emulators mounted afterward and to the warm-terminal cap at once; the settings page reads the same
 * signals.
 */
export function createTerminalLimitsSettings(dependencies: TerminalLimitsSettingsDependencies) {
  const limits = signal<TerminalLimits | undefined>(undefined),
    message = signal(''),
    context = dependencies.context ?? browserContext;
  let generation = 0;

  function accept(value: TerminalLimits, keepMessage = false) {
    limits.value = value;
    if (!keepMessage) message.value = '';
    applyTerminalLimits(value, context());
  }

  async function refresh(keepMessage = false) {
    const api = dependencies.api();
    if (!api) return;
    const request = ++generation;
    try {
      const value = await api.terminalLimits();
      if (request === generation) accept(value, keepMessage);
    } catch (reason) {
      // An older server has no limits route: keep the conservative defaults already in effect.
      if (request === generation) message.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  async function save(configured: TerminalLimits['configured']) {
    const api = dependencies.api();
    if (!api) return;
    const request = ++generation;
    message.value = '';
    try {
      const value = await api.saveTerminalLimits(configured);
      if (request === generation) accept(value);
    } catch (reason) {
      if (request !== generation) return;
      message.value = reason instanceof Error ? reason.message : String(reason);
      // Re-read so the controls show what is actually stored, keeping the failure visible.
      await refresh(true);
    }
  }

  /** The defaults this device uses instead of the server machine's, or `undefined` when they match. */
  function deviceDefaults(): { historyLines: number; warmTerminals: number } | undefined {
    const current = context();
    if (!current.remote && !current.phone) return undefined;
    const resolved = resolveClientTerminalLimits(undefined, current);
    return { historyLines: resolved.historyLines, warmTerminals: resolved.warmTerminals };
  }

  return { limits, message, refresh, save, deviceDefaults };
}

/** The terminal-limits controller. */
export type TerminalLimitsSettings = ReturnType<typeof createTerminalLimitsSettings>;

/** The configured value a settings choice stands for: `null` for "default", else the chosen number. */
export function terminalLimitChoiceValue(choice: string | undefined): number | null {
  if (choice === undefined || choice === '' || choice === 'default') return null;
  const value = Number(choice);
  return Number.isFinite(value) ? value : null;
}
