import './terminal-viewport-error.css';

import { browserRandomId } from './browser-id';
import type { TerminalModifiers } from './terminal-keys';

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
    const parsed = JSON.parse(value) as { pty_size?: { cols?: unknown; rows?: unknown }; driven_by?: string | null },
      cols = parsed.pty_size?.cols,
      rows = parsed.pty_size?.rows;
    if (Number.isInteger(cols) && Number.isInteger(rows) && (cols as number) > 0 && (rows as number) > 0)
      return parsed as TerminalSizeMessage;
  } catch {
    /* terminal text input is not a control frame */
  }
  return undefined;
}

export function isTerminalReplacementReplay(value: string): boolean {
  try {
    return (JSON.parse(value) as { terminal_replay?: unknown }).terminal_replay === 'replace';
  } catch {
    return false;
  }
}

export const terminalReconnectDelay = (attempt: number) => Math.min(8_000, 250 * 2 ** Math.max(0, attempt));
/** Sent to a live viewport its owner moves out of the page to keep it warm (HS2-WGTQ6X). */
export const TERMINAL_VIEWPORT_PARK_EVENT = 'hs-terminal-viewport-park';
/** Sent when a parked viewport returns to the page; `detail.focus` requests input focus. */
export const TERMINAL_VIEWPORT_RESUME_EVENT = 'hs-terminal-viewport-resume';
/** Dispatched by a parked viewport whose socket closed, so its owner evicts it. */
export const TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT = 'hs-terminal-viewport-parked-closed';
/** How long an automatic initial focus waits for a second attempt, after layout and mounts settle. */
export const TERMINAL_FOCUS_RETRY_MS = 120;
export const TERMINAL_DRAWER_RESIZE_END_EVENT = 'hotsheet-terminal-drawer-resize-end';
export const TERMINAL_DASHBOARD_COLS = 80;
export const TERMINAL_DASHBOARD_ROWS = 24;
export const TERMINAL_DASHBOARD_FONT_SIZE = 24;
export const TERMINAL_DASHBOARD_LINE_HEIGHT = 1.085;
export const TERMINAL_PREVIEW_NATURAL_WIDTH = 1280;
export const TERMINAL_PREVIEW_NATURAL_HEIGHT = 768;
export const TERMINAL_PREVIEW_SCROLLBACK = 0;
export const TERMINAL_MAGNIFIED_SCROLLBACK = 1_000;
export const TERMINAL_DEDICATED_SCROLLBACK = 5_000;

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
  return fixedDashboardGrid ? TERMINAL_MAGNIFIED_SCROLLBACK : TERMINAL_DEDICATED_SCROLLBACK;
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
    element.querySelector('.terminal-viewport-error')?.remove();
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
      const message = element.ownerDocument.createElement('p');
      message.className = 'terminal-viewport-error';
      message.setAttribute('role', 'alert');
      message.textContent = 'Terminal could not start. Reload the page to try again.';
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
