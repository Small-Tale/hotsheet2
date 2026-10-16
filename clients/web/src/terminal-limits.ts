/**
 * Terminal memory limits (HS2-Y5DYC0): the history an emulator keeps and how many kept-alive terminals
 * stay warm out of the page.
 *
 * The limits are machine-wide server settings (`GET`/`PUT /terminal-limits`, App Settings → Terminal
 * memory, or `hotsheet-cli settings set terminal.history_lines <n> --scope global`). The server reports
 * the defaults it derived from the machine's physical memory, because browsers expose no usable memory
 * signal (Safari has none; Chromium's `navigator.deviceMemory` is rounded and capped at 8 GB). Those
 * defaults describe the server's machine, so a client on another device, or a phone, keeps the
 * conservative defaults below for any limit that is not set explicitly. An explicit setting always wins.
 */
import type { TerminalLimits } from './api';
import { isMobileViewport } from './mobile-layout';

/** What the web client applies. */
export interface ClientTerminalLimits {
  /** Scrollback lines a dedicated (drawer) terminal keeps. */
  historyLines: number;
  /** Scrollback lines a magnified dashboard card keeps. */
  magnifiedHistoryLines: number;
  /** Kept-alive terminals that stay warm out of the page. */
  warmTerminals: number;
}

/** Where this client runs relative to the server. */
export interface TerminalLimitContext {
  /** The client is on a different device than the server. */
  remote: boolean;
  /** The client is a phone-sized device. */
  phone: boolean;
}

/** The historical limits: the default for a client on another device, or before the server answered. */
export const CONSERVATIVE_TERMINAL_LIMITS = { historyLines: 5_000, warmTerminals: 12 } as const;
/** A phone keeps less: its memory is small and the OS discards background tabs. */
export const PHONE_TERMINAL_LIMITS = { historyLines: 2_500, warmTerminals: 4 } as const;
/** A magnified card keeps a fifth of the dedicated history, never fewer than this. */
const MINIMUM_MAGNIFIED_HISTORY = 200;

/** The magnified-card history for a dedicated history of `historyLines`. */
export function magnifiedHistoryLines(historyLines: number): number {
  return Math.min(historyLines, Math.max(MINIMUM_MAGNIFIED_HISTORY, Math.round(historyLines / 5)));
}

/** Resolve the client's limits from the server's answer and where the client runs. */
export function resolveClientTerminalLimits(
  limits: TerminalLimits | undefined,
  context: TerminalLimitContext,
): ClientTerminalLimits {
  const fallback = context.phone
      ? PHONE_TERMINAL_LIMITS
      : context.remote || !limits
        ? CONSERVATIVE_TERMINAL_LIMITS
        : { historyLines: limits.defaults.history_lines, warmTerminals: limits.defaults.warm_terminals },
    historyLines = limits?.configured.history_lines ?? fallback.historyLines;
  return {
    historyLines,
    magnifiedHistoryLines: magnifiedHistoryLines(historyLines),
    warmTerminals: limits?.configured.warm_terminals ?? fallback.warmTerminals,
  };
}

/** A phone-sized device: its larger screen side is below the desktop layout floor. */
export function isPhoneDevice(screenValue: Pick<Screen, 'width' | 'height'> = window.screen): boolean {
  return isMobileViewport(Math.max(screenValue.width, screenValue.height));
}

let current: ClientTerminalLimits = resolveClientTerminalLimits(undefined, { remote: true, phone: false });
const listeners = new Set<(limits: ClientTerminalLimits) => void>();

/** The limits in effect now. Emulators read them when they mount. */
export function currentTerminalLimits(): ClientTerminalLimits {
  return current;
}

/** Apply the server's answer; returns the resolved limits. */
export function applyTerminalLimits(limits: TerminalLimits | undefined, context: TerminalLimitContext) {
  current = resolveClientTerminalLimits(limits, context);
  for (const listener of listeners) listener(current);
  return current;
}

/** Follow later changes (for example, the warm cap evicting extra parked terminals). */
export function onTerminalLimitsChange(listener: (limits: ClientTerminalLimits) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
