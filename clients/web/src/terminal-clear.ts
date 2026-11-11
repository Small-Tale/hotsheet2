/**
 * Clear a terminal's display, like Command-K in macOS Terminal (HS2-N2AG8V).
 *
 * Clearing is a view action: the shell is not told and the running program keeps its state. Every
 * mounted viewport of the terminal in this client (dedicated, grid preview, and parked keep-alive)
 * drops its scrollback and screen but keeps the cursor's line, so the prompt stays usable. Viewports
 * talk to the emulator through DOM events, like the clipboard actions. The server's replay ring is
 * unchanged, so a viewport mounted later replays earlier output (shared clear: HS2-HA0Z3A).
 */

import { matchesShortcut } from './keyboard-shortcuts';

/** Event dispatched on a mounted viewport to clear its scrollback and screen. */
export const TERMINAL_CLEAR_EVENT = 'hotsheet-terminal-clear';

/**
 * The terminal viewport a keydown should clear: the Clear Terminal chord (physical Command-K) pressed
 * while a terminal holds the keyboard. The global shortcut dispatcher defers to it, so the shared chord
 * never opens search from inside a terminal.
 */
export function terminalClearKeyTarget(
  event: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'isComposing' | 'target'>,
): HTMLElement | undefined {
  if (event.isComposing || !matchesShortcut('clear-terminal', event, {})) return undefined;
  const target = event.target as { closest?: (selector: string) => Element | null } | null;
  return (target?.closest?.('[data-component="terminal-viewport"]') as HTMLElement | null) ?? undefined;
}

/** The terminal a viewport shows. */
export interface TerminalViewportIdentity {
  projectId: string;
  terminalId: string;
}

/** The terminal a viewport element shows, read from its data attributes. */
export function terminalViewportIdentity(viewport: HTMLElement): TerminalViewportIdentity | undefined {
  const { projectId, terminalId } = viewport.dataset;
  return projectId && terminalId ? { projectId, terminalId } : undefined;
}

/** Clear every mounted viewport of one terminal under `root`; returns how many viewports were asked. */
export function clearTerminalViewports(root: ParentNode, identity: TerminalViewportIdentity): number {
  let cleared = 0;
  for (const viewport of root.querySelectorAll<HTMLElement>('[data-component="terminal-viewport"]')) {
    const shown = terminalViewportIdentity(viewport);
    if (shown?.projectId !== identity.projectId || shown.terminalId !== identity.terminalId) continue;
    viewport.dispatchEvent(new CustomEvent(TERMINAL_CLEAR_EVENT));
    cleared += 1;
  }
  return cleared;
}
