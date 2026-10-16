import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { Text } from '@kerfjs/ui/text';
import { toElement } from 'kerfjs';

import { browserRandomId } from './browser-id';
import { isRecord, parseJson } from './json-value';
import type { TerminalModifiers } from './terminal-keys';
import { currentTerminalLimits } from './terminal-limits';

export interface TerminalSizeMessage {
  pty_size: { cols: number; rows: number };
  driven_by?: string | null;
}

export function terminalResizeClaim(
  viewerId: string,
  cols: number,
  rows: number,
  focus: boolean,
  visible: boolean,
  interacting = false,
): string {
  const dimension = (value: number) => (Number.isFinite(value) ? Math.max(1, Math.floor(value)) : 1);
  return JSON.stringify({
    resize: { viewer_id: viewerId, cols: dimension(cols), rows: dimension(rows), focus, visible, interacting },
  });
}

export function parseTerminalSizeMessage(value: string): TerminalSizeMessage | undefined {
  try {
    const parsed = parseJson(value);
    if (!isRecord(parsed) || !isRecord(parsed.pty_size)) return undefined;
    const { cols, rows } = parsed.pty_size;
    if (
      typeof cols === 'number' &&
      typeof rows === 'number' &&
      Number.isInteger(cols) &&
      Number.isInteger(rows) &&
      cols > 0 &&
      rows > 0 &&
      (parsed.driven_by === undefined || parsed.driven_by === null || typeof parsed.driven_by === 'string')
    )
      return parsed as unknown as TerminalSizeMessage;
  } catch {
    /* terminal text input is not a control frame */
  }
  return undefined;
}

export function isTerminalReplacementReplay(value: string): boolean {
  try {
    const parsed = parseJson(value);
    return isRecord(parsed) && parsed.terminal_replay === 'replace';
  } catch {
    return false;
  }
}

/** Liveness protocol (HS2-51HQ3S): the server's attach hello, probe, and reply frames. */
export const TERMINAL_PING_INTERVAL_MS = 5_000;
/** No inbound frame for this long, despite answered probes, means the socket is dead. */
export const TERMINAL_STALE_AFTER_MS = 15_000;

export function terminalPing(seq: number): string {
  return JSON.stringify({ terminal_ping: seq });
}

/** True for the liveness frames this viewer handles itself (`terminal_liveness`, `terminal_pong`). */
export function terminalLivenessFrame(value: string): 'hello' | 'pong' | undefined {
  try {
    const parsed = parseJson(value);
    if (!isRecord(parsed)) return undefined;
    if (parsed.terminal_liveness !== undefined) return 'hello';
    if (parsed.terminal_pong !== undefined) return 'pong';
  } catch {
    return undefined;
  }
  return undefined;
}

/**
 * Whether an OPEN socket is actually dead (HS2-51HQ3S). A half-open connection (a slept laptop, a
 * changed network, a restarted proxy) can stay OPEN in the browser while nothing flows, so the
 * viewport froze on stale output and swallowed keystrokes. Only a server that announced liveness
 * support answers probes, so silence from an older server is never mistaken for a dead socket.
 */
export function terminalSocketStale(input: {
  livenessSupported: boolean;
  lastInboundAt: number;
  now: number;
}): boolean {
  return input.livenessSupported && input.now - input.lastInboundAt > TERMINAL_STALE_AFTER_MS;
}

/** Typing after this long with nothing received first confirms the socket with a probe (HS2-J52ZJ7). */
export const TERMINAL_QUIET_INPUT_AFTER_MS = 3_000;
/** A held keystroke's probe unanswered for this long means the socket is dead: reconnect and replay. */
export const TERMINAL_INPUT_PROBE_TIMEOUT_MS = 2_000;

/**
 * Whether input typed now should be held behind a liveness probe instead of sent (HS2-J52ZJ7).
 * Keystrokes sent into a half-open socket before the 15 s stale window noticed it were lost, so
 * after a quiet spell the viewport holds them, probes at once, and sends them on the reply (or
 * replays them on a fresh socket if none comes). Input keeps queueing behind a probe already in
 * flight so it stays in order. A server without liveness support is never probed.
 */
export function terminalInputNeedsProbe(input: {
  livenessSupported: boolean;
  lastInboundAt: number;
  now: number;
  probing: boolean;
}): boolean {
  if (!input.livenessSupported) return false;
  return input.probing || input.now - input.lastInboundAt > TERMINAL_QUIET_INPUT_AFTER_MS;
}

export const terminalReconnectDelay = (attempt: number) => Math.min(8_000, 250 * 2 ** Math.max(0, attempt));
/** Most input a reconnecting terminal holds for its next socket; the oldest keystrokes give way. */
export const TERMINAL_PENDING_INPUT_LIMIT = 64 * 1024;
/**
 * Queue `value` typed while the attach socket is closed or reopening (HS2-A1R38J). It is sent when the
 * socket opens, so recovering from a dropped connection never loses what the user typed.
 */
export function bufferTerminalInput(pending: string, value: string, limit = TERMINAL_PENDING_INPUT_LIMIT): string {
  const next = pending + value;
  return next.length > limit ? next.slice(next.length - limit) : next;
}
/** Sent to a live viewport its owner moves out of the page to keep it warm (HS2-WGTQ6X). */
export const TERMINAL_VIEWPORT_PARK_EVENT = 'hs-terminal-viewport-park';
/** Sent when a parked viewport returns to the page; `detail.focus` requests input focus. */
export const TERMINAL_VIEWPORT_RESUME_EVENT = 'hs-terminal-viewport-resume';
/**
 * Sent to a live viewport its owner moved into a freshly rendered placeholder elsewhere in the page
 * (a grid-mode or full-screen change re-rendered its container), so it re-observes its new frame
 * instead of being disposed and recreated (HS2-AMNNBR, HS2-TBQQ00).
 */
export const TERMINAL_VIEWPORT_REPARENT_EVENT = 'hs-terminal-viewport-reparent';
/**
 * Sent to a live dedicated viewport its owner moved into (`detail.tile: true`) or out of (`false`) a
 * grid tile (HS2-EYD24K). In a tile the same emulator and socket keep running: the local grid mirrors
 * the PTY's size, the emulator is CSS-scaled to the tile frame with letterboxing, input is disabled,
 * and its size claims never drive the PTY. Leaving the tile refits it and lets it drive again.
 */
export const TERMINAL_VIEWPORT_PRESENTATION_EVENT = 'hs-terminal-viewport-presentation';
export interface TerminalViewportPresentationDetail {
  tile: boolean;
}
/**
 * Where a tile-presented emulator sits in its tile frame: scaled to fit the frame while keeping its
 * aspect, and centered on the axis it does not fill (letterboxing).
 */
export function terminalTileLayout(
  screenWidth: number,
  screenHeight: number,
  frameWidth: number,
  frameHeight: number,
): { scale: number; x: number; y: number } | undefined {
  if (screenWidth <= 0 || screenHeight <= 0 || frameWidth <= 0 || frameHeight <= 0) return undefined;
  const scale = Math.min(frameWidth / screenWidth, frameHeight / screenHeight);
  return { scale, x: (frameWidth - screenWidth * scale) / 2, y: (frameHeight - screenHeight * scale) / 2 };
}
/** Dispatched by a parked viewport whose socket closed, so its owner evicts it. */
export const TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT = 'hs-terminal-viewport-parked-closed';
/** How long an automatic initial focus waits for a second attempt, after layout and mounts settle. */
export const TERMINAL_FOCUS_RETRY_MS = 120;
/**
 * How long after mount a newly created terminal may reclaim its requested focus from the transient
 * menu that created it (Web Awesome returns focus to the dropdown trigger as it closes — HS2-FB4DKM).
 */
export const TERMINAL_INITIAL_FOCUS_WINDOW_MS = 1_000;
const TRANSIENT_FOCUS_OWNERS = 'wa-dropdown, wa-dropdown-item, [role="menu"], [role="menuitem"]';

/**
 * Whether the element holding focus may yield it to a terminal that requested initial focus: nothing
 * (or `<body>`) is focused, the holder already left the page, or it is the menu/dropdown that created
 * the terminal. A real field the user moved to keeps focus (HS2-FB4DKM).
 */
export function focusYieldsToNewTerminal(active: Element | null, documentValue: Document = document): boolean {
  if (active === null || active === documentValue.body || !active.isConnected) return true;
  return active.closest(TRANSIENT_FOCUS_OWNERS) !== null;
}
export const TERMINAL_DRAWER_RESIZE_END_EVENT = 'hotsheet-terminal-drawer-resize-end';
export const TERMINAL_DASHBOARD_COLS = 80;
export const TERMINAL_DASHBOARD_ROWS = 24;
export const TERMINAL_DASHBOARD_FONT_SIZE = 24;
export const TERMINAL_DASHBOARD_LINE_HEIGHT = 1.085;
export const TERMINAL_PREVIEW_NATURAL_WIDTH = 1280;
export const TERMINAL_PREVIEW_NATURAL_HEIGHT = 768;
export const TERMINAL_PREVIEW_SCROLLBACK = 0;

/** Chromium's WebGL renderer delayed real PTY delivery in the sustained-output profile;
 * Apple WebKit also needs DOM for glyph compatibility. Firefox keeps WebGL.
 * Renderer selection alone does not prove that glyphs painted. */
export function terminalShouldUseWebgl(userAgent: string): boolean {
  const appleWebKit = /AppleWebKit/i.test(userAgent),
    chromium = /(?:Chrome|Chromium|Edg|OPR)\//i.test(userAgent);
  return !appleWebKit && !chromium;
}

export function terminalUsesMobile80xM(
  mobile: boolean,
  fixedDashboardGrid: boolean,
  scaledPreview: boolean,
  dedicatedDrawer: boolean,
): boolean {
  return mobile && !scaledPreview && (fixedDashboardGrid || dedicatedDrawer);
}
// ANSI ESC is the external terminal protocol byte intentionally recognized here.
// eslint-disable-next-line no-control-regex
const ZSH_PROMPT_EOL_MARK = /^(?:\u001b\[(?:0|1)m)*\u001b\[7m%\u001b\[27m(?:\u001b\[(?:0|1|27)m)*(?:\r?\n)?/;

/** A late zsh attachment can begin at its reverse-video partial-line marker. The marker
 * described output that preceded the bounded replay, so presenting it as a new first row
 * is a reconstruction artifact rather than terminal content. */
export function stripLeadingZshPromptEolMark(value: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  const text = new TextDecoder().decode(value),
    trimmed = text.replace(ZSH_PROMPT_EOL_MARK, '');
  return trimmed === text ? value : new TextEncoder().encode(trimmed);
}
export function terminalScrollbackLimit(displayMode: string | undefined, fixedDashboardGrid: boolean): number {
  if (displayMode === 'scaled-preview') return TERMINAL_PREVIEW_SCROLLBACK;
  // Configurable, memory-aware history (HS2-Y5DYC0); a magnified card keeps a fifth of it.
  const limits = currentTerminalLimits();
  return fixedDashboardGrid ? limits.magnifiedHistoryLines : limits.historyLines;
}
export interface TerminalFocusRequest {
  projectId: string;
  terminalId: string;
}
export function terminalViewportShouldAutoFocus(
  request: TerminalFocusRequest | undefined,
  projectId: string,
  terminalId: string,
): boolean {
  return request?.projectId === projectId && request.terminalId === terminalId;
}
export function terminalViewportScale(
  viewportCols: number,
  viewportRows: number,
  ptyCols: number,
  ptyRows: number,
): number {
  return Math.max(0, Math.min(1, viewportCols / ptyCols, viewportRows / ptyRows));
}
/**
 * The scale that fits a preview's content (the 1280×768 canvas by default, or a mirroring preview's
 * grid footprint on it — HS2-RBS46R) inside its frame.
 */
export function terminalPreviewScale(
  frameWidth: number,
  frameHeight: number,
  contentWidth: number = TERMINAL_PREVIEW_NATURAL_WIDTH,
  contentHeight: number = TERMINAL_PREVIEW_NATURAL_HEIGHT,
): number {
  if (!(contentWidth > 0) || !(contentHeight > 0)) return 0;
  return Math.max(0, Math.min(frameWidth / contentWidth, frameHeight / contentHeight));
}

/**
 * The aspect (width / height) of a mirroring preview's rendered grid, which a container can give its
 * frame so the grid fills it on both axes (HS2-RBS46R); `undefined` until the grid has a size.
 */
export function terminalPreviewGridAspect(screenWidth: number, screenHeight: number): number | undefined {
  if (!Number.isFinite(screenWidth) || !Number.isFinite(screenHeight) || screenWidth <= 0 || screenHeight <= 0)
    return undefined;
  return Math.round((screenWidth / screenHeight) * 10_000) / 10_000;
}

export function terminalPhysicalScale(
  screenWidth: number,
  screenHeight: number,
  targetWidth: number,
  targetHeight: number,
): number {
  if ([screenWidth, screenHeight, targetWidth, targetHeight].some((value) => !Number.isFinite(value) || value <= 0))
    return 0;
  return Math.min(targetWidth / screenWidth, targetHeight / screenHeight);
}

export function terminalInverseScalePercent(physicalScale: number): string {
  if (!Number.isFinite(physicalScale) || physicalScale <= 0) return '100%';
  return `${100 / physicalScale}%`;
}

export function terminalFittedFontSize(current: number, physicalScale: number): number {
  if (!Number.isFinite(current) || !Number.isFinite(physicalScale) || current <= 0 || physicalScale <= 0) return 4;
  return Math.max(4, current * physicalScale);
}

export function terminalDedicatedGridSize(cols: number, rows: number) {
  return { cols: Math.max(1, cols), rows: Math.max(1, rows - 1) };
}

export function terminalViewportClaimsSizing(scaledPreview: boolean, fixedDashboardGrid: boolean): boolean {
  return fixedDashboardGrid || !scaledPreview;
}

export function terminalBrowserWebSocketUrl(
  apiPath: string,
  terminalId: string,
  locationValue: Pick<Location, 'protocol' | 'host'> = location,
): string {
  const protocol = locationValue.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${locationValue.host}${apiPath}/terminals/${encodeURIComponent(terminalId)}/attach`;
}

export interface TerminalViewportOptions {
  url: string;
  viewerId?: string;
  autoFocus?: boolean;
  onTicketReference?: (reference: string) => void;
  /** Column count for a phone-width interactive terminal (80xM policy); defaults to 80. */
  mobileColumns?: () => number;
  /** Sticky key-bar modifiers applied to special keys and the next typed character (HS2-CKS78M). */
  modifiers?: { current: () => TerminalModifiers; consume: () => void };
}

export function mountTerminalViewport(
  element: HTMLElement,
  { url, viewerId, autoFocus = false, onTicketReference, mobileColumns, modifiers }: TerminalViewportOptions,
): () => void {
  return mountTerminalRuntime(element, async () => {
    const { mountTerminalViewportRuntime } = await import('./terminal-viewport-runtime');
    return () =>
      mountTerminalViewportRuntime(element, {
        url,
        viewerId: viewerId ?? browserRandomId(),
        autoFocus,
        onTicketReference,
        mobileColumns,
        modifiers,
      });
  });
}

function mountTerminalRuntime(element: HTMLElement, load: () => Promise<() => () => void>): () => void {
  let disposed = false,
    disposeRuntime: (() => void) | undefined;
  if (element.dataset.connection === 'error') {
    element.querySelector('[data-terminal-viewport-error]')?.remove();
    if (element.dataset.displayMode === 'scaled-preview') element.setAttribute('aria-hidden', 'true');
  }
  element.dataset.connection = 'loading';
  void load()
    .then((mount) => {
      if (!disposed) disposeRuntime = mount();
    })
    .catch(() => {
      if (disposed) return;
      element.dataset.connection = 'error';
      element.removeAttribute('aria-hidden');
      element.style.width = '';
      element.style.height = '';
      element.style.transform = '';
      // Kerf Text owns the centered, wrapping, compact copy (KF-MMAWEG, HS2-JEJTAD); the viewport
      // (TerminalDashboard) centers the block.
      const message = toElement(
        Text({
          variant: 'p',
          role: 'alert',
          'data-terminal-viewport-error': '',
          color: foregroundColorVar('--hs-terminal-foreground'),
          size: 'compact',
          wrap: 'anywhere',
          align: 'center',
          children: 'Terminal could not start. Reload the page to try again.',
        }),
      );
      element.replaceChildren(message);
    });
  return () => {
    if (disposed) return;
    disposed = true;
    disposeRuntime?.();
  };
}

export function mountStaticTerminalViewport(
  element: HTMLElement,
  { output, autoFocus = false }: { output: string; autoFocus?: boolean },
): () => void {
  return mountTerminalRuntime(element, async () => {
    const { mountStaticTerminalViewportRuntime } = await import('./terminal-viewport-runtime');
    return () => mountStaticTerminalViewportRuntime(element, { output, autoFocus });
  });
}
