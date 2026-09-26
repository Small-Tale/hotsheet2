import '@xterm/xterm/css/xterm.css';

import { FitAddon } from '@xterm/addon-fit';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';

import { isMobileViewport } from './mobile-layout';
import {
  DEFAULT_MOBILE_TERMINAL_COLUMNS,
  MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT,
  normalizeMobileTerminalColumns,
} from './mobile-terminal-columns';
import { registerTerminalTicketLinkProvider } from './terminal-ticket-links';
import {
  isTerminalReplacementReplay,
  parseTerminalSizeMessage,
  stripLeadingZshPromptEolMark,
  TERMINAL_DASHBOARD_COLS,
  TERMINAL_DASHBOARD_FONT_SIZE,
  TERMINAL_DASHBOARD_LINE_HEIGHT,
  TERMINAL_DASHBOARD_ROWS,
  TERMINAL_DRAWER_RESIZE_END_EVENT,
  TERMINAL_PREVIEW_NATURAL_HEIGHT,
  TERMINAL_PREVIEW_NATURAL_WIDTH,
  TERMINAL_RESIZE_SETTLE_MS,
  terminalDedicatedGridSize,
  terminalFittedFontSize,
  terminalInverseScalePercent,
  terminalPhysicalScale,
  terminalPreviewScale,
  terminalReconnectDelay,
  terminalResizeClaim,
  terminalScrollbackLimit,
  terminalShouldUseWebgl,
  terminalUsesMobile80xM,
  terminalViewportClaimsSizing,
  terminalViewportScale,
} from './terminal-viewport';

type OwnTerminalResource = (dispose: () => void) => void;

// Reset modes, erase scrollback + the visible display, and home the cursor in the same parser
// batch as the authoritative replay. RIS alone does not erase every xterm buffer row.
const TERMINAL_RESET_BYTES = new Uint8Array([
  0x1b, 0x63, 0x1b, 0x5b, 0x33, 0x4a, 0x1b, 0x5b, 0x32, 0x4a, 0x1b, 0x5b, 0x48,
]);

function terminalReplacementPayload(value: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  const replacement = new Uint8Array(TERMINAL_RESET_BYTES.length + value.length);
  replacement.set(TERMINAL_RESET_BYTES);
  replacement.set(value, TERMINAL_RESET_BYTES.length);
  return replacement;
}

function resizeTerminalPreservingScroll(terminal: Terminal, cols: number, rows: number): void {
  if (terminal.cols === cols && terminal.rows === rows) return;
  const buffer = terminal.buffer.active,
    marker =
      buffer.viewportY < buffer.baseY
        ? terminal.registerMarker(buffer.viewportY - buffer.baseY - buffer.cursorY)
        : undefined;
  terminal.resize(cols, rows);
  if (marker) {
    if (marker.line >= 0) terminal.scrollToLine(marker.line);
    marker.dispose();
  }
}

function mountWithCleanup(initialize: (own: OwnTerminalResource) => void): () => void {
  const cleanups: Array<() => void> = [];
  const dispose = () => {
    for (const cleanup of cleanups.splice(0).reverse()) cleanup();
  };
  try {
    initialize((cleanup) => cleanups.push(cleanup));
  } catch (error) {
    dispose();
    throw error;
  }
  return dispose;
}

export function mountStaticTerminalViewportRuntime(
  element: HTMLElement,
  options: { output: string; autoFocus?: boolean },
): () => void {
  return mountWithCleanup((own) => {
    initializeStaticTerminalViewport(element, options, own);
  });
}

function initializeStaticTerminalViewport(
  element: HTMLElement,
  { output, autoFocus = false }: { output: string; autoFocus?: boolean },
  own: OwnTerminalResource,
): void {
  const scaledPreview = element.dataset.displayMode === 'scaled-preview',
    fixedDashboardGrid = element.dataset.gridPolicy === 'dashboard-80x24',
    background = getComputedStyle(element).getPropertyValue('--hs-terminal-background').trim() || '#000';
  if (scaledPreview) {
    element.style.width = `${TERMINAL_PREVIEW_NATURAL_WIDTH}px`;
    element.style.height = `${TERMINAL_PREVIEW_NATURAL_HEIGHT}px`;
    element.dataset.naturalSize = `${TERMINAL_PREVIEW_NATURAL_WIDTH}x${TERMINAL_PREVIEW_NATURAL_HEIGHT}`;
  }
  const terminal = new Terminal({
    ...(fixedDashboardGrid
      ? { cols: TERMINAL_DASHBOARD_COLS, rows: TERMINAL_DASHBOARD_ROWS, lineHeight: TERMINAL_DASHBOARD_LINE_HEIGHT }
      : {}),
    cursorBlink: !scaledPreview,
    disableStdin: scaledPreview,
    convertEol: false,
    scrollback: terminalScrollbackLimit(element.dataset.displayMode, fixedDashboardGrid || !scaledPreview),
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
    fontSize: fixedDashboardGrid ? TERMINAL_DASHBOARD_FONT_SIZE : 12,
    theme: { background },
  });
  own(() => {
    terminal.dispose();
  });
  terminal.open(element);
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  element.dataset.renderer = 'dom';
  element.dataset.connection = 'connected';
  element.dataset.driving = 'true';
  element.dataset.ptySize = `${terminal.cols}x${terminal.rows}`;
  element.dataset.gridSize = `${terminal.cols}x${terminal.rows}`;
  element.dataset.sizingFocus = String(terminalViewportClaimsSizing(scaledPreview, fixedDashboardGrid));
  element.dataset.viewportVisible = 'true';
  if (fixedDashboardGrid) {
    element.dataset.fontSize = String(TERMINAL_DASHBOARD_FONT_SIZE);
    element.dataset.letterSpacing = '0';
    element.dataset.lineHeight = String(TERMINAL_DASHBOARD_LINE_HEIGHT);
  }
  element.dataset.scrollbackLimit = String(
    terminalScrollbackLimit(element.dataset.displayMode, fixedDashboardGrid || !scaledPreview),
  );
  element.dataset.geometryReady = 'false';
  let frame: number | undefined,
    disposed = false,
    initialFocusPending = autoFocus;
  own(() => {
    disposed = true;
    if (frame !== undefined) window.cancelAnimationFrame(frame);
  });
  const fill = () => {
    frame = undefined;
    if (disposed || !terminal.element) return;
    const screen = terminal.element.querySelector<HTMLElement>('.xterm-screen');
    if (!screen || screen.offsetWidth <= 0 || screen.offsetHeight <= 0) return;
    const target = element.parentElement ?? element;
    if (!fixedDashboardGrid) {
      fit.fit();
      terminal.element.style.transform = '';
      element.dataset.scale = '1';
      element.dataset.physicalScale = '1';
      element.dataset.ptySize = `${terminal.cols}x${terminal.rows}`;
      element.dataset.gridSize = `${terminal.cols}x${terminal.rows}`;
      element.dataset.geometryReady = 'true';
      if (initialFocusPending) {
        initialFocusPending = false;
        if (document.activeElement === document.body || document.activeElement === null) terminal.focus();
      }
      return;
    }
    const scale = terminalPhysicalScale(
      screen.offsetWidth,
      screen.offsetHeight,
      Math.max(1, target.clientWidth - 1),
      Math.max(1, target.clientHeight - 1),
    );
    if (scale <= 0) return;
    terminal.element.style.transform = `scale(${scale})`;
    element.dataset.scale = String(scale);
    element.dataset.physicalScale = String(scale);
    element.dataset.geometryReady = 'true';
    if (initialFocusPending) {
      initialFocusPending = false;
      if (document.activeElement === document.body || document.activeElement === null) terminal.focus();
    }
  };
  const schedule = () => {
    if (frame === undefined) frame = window.requestAnimationFrame(fill);
  };
  const render = terminal.onRender(schedule);
  own(() => {
    render.dispose();
  });
  const resize = new ResizeObserver(schedule);
  own(() => {
    resize.disconnect();
  });
  resize.observe(element.parentElement ?? element);
  terminal.write(output, schedule);
  schedule();
  const focus = () => {
    terminal.focus();
  };
  if (!scaledPreview) {
    element.addEventListener('click', focus);
    own(() => {
      element.removeEventListener('click', focus);
    });
  }
}

type TerminalRuntimeOptions = {
  url: string;
  viewerId: string;
  autoFocus?: boolean;
  onTicketReference?: (reference: string) => void;
  mobileColumns?: () => number;
};

export function mountTerminalViewportRuntime(element: HTMLElement, options: TerminalRuntimeOptions): () => void {
  return mountWithCleanup((own) => {
    initializeTerminalViewport(element, options, own);
  });
}

function initializeTerminalViewport(
  element: HTMLElement,
  { url, viewerId, autoFocus = false, onTicketReference, mobileColumns }: TerminalRuntimeOptions,
  own: OwnTerminalResource,
): void {
  const scaledPreview = element.dataset.displayMode === 'scaled-preview',
    fixedDashboardGrid = element.dataset.gridPolicy === 'dashboard-80x24',
    magnified = Boolean(element.closest('[data-fixed-aspect-terminal-card="magnified"]')),
    settledResize = element.classList.contains('terminal-viewport--dedicated'),
    insideDrawer = Boolean(element.closest('[data-region-id="app-terminal-drawer"]'));
  const background = getComputedStyle(element).getPropertyValue('--hs-terminal-background').trim() || '#000';
  if (scaledPreview) {
    element.style.width = `${TERMINAL_PREVIEW_NATURAL_WIDTH}px`;
    element.style.height = `${TERMINAL_PREVIEW_NATURAL_HEIGHT}px`;
    element.dataset.naturalSize = `${TERMINAL_PREVIEW_NATURAL_WIDTH}x${TERMINAL_PREVIEW_NATURAL_HEIGHT}`;
  }
  const scrollback = terminalScrollbackLimit(element.dataset.displayMode, fixedDashboardGrid || magnified);
  element.dataset.scrollbackLimit = String(scrollback);
  if (fixedDashboardGrid) {
    element.dataset.geometryReady = 'false';
    element.dataset.fontSize = String(TERMINAL_DASHBOARD_FONT_SIZE);
    element.dataset.letterSpacing = '0';
    element.dataset.lineHeight = String(TERMINAL_DASHBOARD_LINE_HEIGHT);
  }
  const terminal = new Terminal({
      ...(fixedDashboardGrid
        ? { cols: TERMINAL_DASHBOARD_COLS, rows: TERMINAL_DASHBOARD_ROWS, lineHeight: TERMINAL_DASHBOARD_LINE_HEIGHT }
        : {}),
      cursorBlink: !scaledPreview,
      disableStdin: scaledPreview,
      convertEol: false,
      scrollback,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
      fontSize: fixedDashboardGrid ? TERMINAL_DASHBOARD_FONT_SIZE : 12,
      theme: { background },
    }),
    fit = new FitAddon();
  own(() => {
    terminal.dispose();
  });
  terminal.loadAddon(fit);
  terminal.open(element);
  const ticketLinks =
    !scaledPreview && onTicketReference ? registerTerminalTicketLinkProvider(terminal, onTicketReference) : undefined;
  if (ticketLinks)
    own(() => {
      ticketLinks.dispose();
    });
  let webgl: WebglAddon | undefined;
  const mobile80xM = () =>
    terminalUsesMobile80xM(
      isMobileViewport(window.innerWidth),
      fixedDashboardGrid || element.dataset.mobileGridPolicy === '80xm',
      scaledPreview,
      settledResize && insideDrawer,
    );
  if (
    element.classList.contains('terminal-viewport--dedicated') &&
    !mobile80xM() &&
    terminalShouldUseWebgl(navigator.userAgent)
  )
    try {
      webgl = new WebglAddon();
      terminal.loadAddon(webgl);
      element.dataset.renderer = 'webgl';
      webgl.onContextLoss(() => {
        webgl?.dispose();
        webgl = undefined;
        element.dataset.renderer = 'dom';
      });
    } catch {
      element.dataset.renderer = 'dom';
    }
  else element.dataset.renderer = 'dom';
  let socket: WebSocket | undefined,
    reconnect: number | undefined,
    heartbeat: number | undefined,
    fitFrame: number | undefined,
    dashboardFrame: number | undefined,
    settleClaim: number | undefined,
    attempt = 0,
    visible = false,
    disposed = false,
    focusRequested = autoFocus,
    initialReplay = true,
    connectedOnce = false,
    replacementReplayPending = false,
    serverSize: { cols: number; rows: number } | undefined;
  own(() => {
    disposed = true;
    if (reconnect !== undefined) window.clearTimeout(reconnect);
    if (heartbeat !== undefined) window.clearInterval(heartbeat);
    if (fitFrame !== undefined) window.cancelAnimationFrame(fitFrame);
    if (dashboardFrame !== undefined) window.cancelAnimationFrame(dashboardFrame);
    if (settleClaim !== undefined) window.clearTimeout(settleClaim);
    socket?.close();
  });
  const claimsSizing = () => terminalViewportClaimsSizing(scaledPreview, fixedDashboardGrid);
  // On a phone-width viewport a dedicated drawer terminal keeps a fixed column count (80 by default,
  // down to 40 for larger text — HS2-WMN626) but fills the available height with M rows, scaled to
  // fit width (HS2-S708S3).
  const mobileCols = () => normalizeMobileTerminalColumns(mobileColumns?.() ?? DEFAULT_MOBILE_TERMINAL_COLUMNS);
  let mobileGridRows = TERMINAL_DASHBOARD_ROWS,
    lastMobileClaimGrid = '',
    lastSettledGeometry: { cols: number; rows: number } | undefined;
  const proposed = () => {
    if (mobile80xM()) return { cols: mobileCols(), rows: mobileGridRows };
    if (fixedDashboardGrid) return { cols: TERMINAL_DASHBOARD_COLS, rows: TERMINAL_DASHBOARD_ROWS };
    const dimensions = fit.proposeDimensions() ?? { cols: terminal.cols, rows: terminal.rows };
    return settledResize ? terminalDedicatedGridSize(dimensions.cols, dimensions.rows) : dimensions;
  };
  const claimDimensions = () =>
    !fixedDashboardGrid && !mobile80xM() && lastSettledGeometry ? lastSettledGeometry : proposed();
  const reconcileScale = () => {
    if (!terminal.element) return;
    if (fixedDashboardGrid || mobile80xM()) {
      const scale = scaledPreview
        ? terminalPreviewScale(element.parentElement?.clientWidth ?? 0, element.parentElement?.clientHeight ?? 0)
        : 1;
      element.style.transform = scaledPreview ? `scale(${scale})` : '';
      if (!mobile80xM()) element.dataset.scale = String(scale);
      delete element.dataset.viewingLabel;
      element.removeAttribute('aria-description');
      return;
    }
    if (!serverSize) return;
    const authoritative = claimsSizing() || element.dataset.driving === 'true',
      size = proposed(),
      scale = authoritative ? 1 : terminalViewportScale(size.cols, size.rows, serverSize.cols, serverSize.rows),
      mismatch = scale < 1;
    terminal.element.style.transform = mismatch ? `scale(${scale})` : '';
    terminal.element.style.width = mismatch ? `${100 / scale}%` : '';
    terminal.element.style.height = mismatch ? `${100 / scale}%` : '';
    element.dataset.scale = String(scale);
    delete element.dataset.viewingLabel;
    element.removeAttribute('aria-description');
  };
  // A heartbeat/geometry claim (interacting:false): keeps the lease alive and reports size, but
  // must NOT advance the arbiter's recency, so one device's heartbeats can't steal size control
  // from the device the user last touched (HS2-3ZBQDG).
  const claim = () => {
    const sizingFocus = claimsSizing();
    element.dataset.sizingFocus = String(sizingFocus);
    element.dataset.viewportVisible = String(visible);
    if (socket?.readyState !== WebSocket.OPEN) return;
    if (!fixedDashboardGrid && !mobile80xM() && element.dataset.geometryReady !== 'true') return;
    const size = claimDimensions();
    socket.send(terminalResizeClaim(viewerId, size.cols, size.rows, sizingFocus, visible, false));
  };
  // A genuine user interaction (tap/click/focus/keystroke) on THIS device: an interacting claim
  // makes this the "last interacted" viewport, so it takes over PTY sizing (HS2-3ZBQDG).
  let lastInteractionAt = 0;
  const signalInteraction = () => {
    if (scaledPreview || socket?.readyState !== WebSocket.OPEN) return;
    if (!fixedDashboardGrid && !mobile80xM() && element.dataset.geometryReady !== 'true') return;
    const sizingFocus = claimsSizing();
    element.dataset.sizingFocus = String(sizingFocus);
    element.dataset.viewportVisible = String(visible);
    const size = claimDimensions();
    socket.send(terminalResizeClaim(viewerId, size.cols, size.rows, sizingFocus, visible, true));
    lastInteractionAt = Date.now();
  };
  // Typing fires per keystroke; throttle the interacting claim so steady typing doesn't flood the
  // socket while still keeping this device's recency fresh.
  const signalInteractionThrottled = () => {
    if (Date.now() - lastInteractionAt >= 1_000) signalInteraction();
  };
  const applyDashboardPhysicalFill = () => {
    const screen = terminal.element?.querySelector<HTMLElement>('.xterm-screen');
    if (!terminal.element || !screen || screen.offsetWidth <= 0 || screen.offsetHeight <= 0) return;
    const style = getComputedStyle(element),
      horizontalPadding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight),
      verticalPadding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom),
      targetWidth = Math.max(1, element.clientWidth - horizontalPadding - 1),
      targetHeight = Math.max(1, element.clientHeight - verticalPadding - 1);
    if (mobile80xM()) {
      // The mobile column count is fixed and scaled to fit the phone width; M rows chosen to fill the available
      // height at that scale (HS2-Z84F78). Resizing reflows the measured screen, so a cols/rows
      // change returns early and the next render re-measures before applying the transform.
      const cols = mobileCols();
      if (terminal.cols !== cols) {
        terminal.resize(cols, terminal.rows);
        return;
      }
      const scale = targetWidth / screen.offsetWidth;
      if (scale <= 0) return;
      const rowHeight = screen.offsetHeight / Math.max(1, terminal.rows),
        rows = Math.max(1, Math.floor(targetHeight / (scale * rowHeight)));
      if (rows !== terminal.rows) {
        mobileGridRows = rows;
        terminal.resize(cols, rows);
        element.dataset.gridSize = `${cols}x${rows}`;
        return;
      }
      mobileGridRows = rows;
      terminal.element.style.transformOrigin = 'top left';
      terminal.element.style.transform = `scale(${scale})`;
      // CSS transforms change paint geometry, not layout geometry. Give xterm's root the inverse
      // layout size so its viewport and vertical scrollbar still reach the visible right edge.
      terminal.element.style.width = terminalInverseScalePercent(scale);
      terminal.element.style.height = terminalInverseScalePercent(scale);
      element.dataset.scale = String(scale);
      element.dataset.physicalScale = String(scale);
      element.dataset.gridSize = `${cols}x${rows}`;
      element.dataset.geometryReady = 'true';
      if (autoFocus && focusRequested) terminal.focus();
      const grid = `${cols}x${rows}`;
      if (lastMobileClaimGrid !== grid) {
        lastMobileClaimGrid = grid;
        claim();
      }
      return;
    }
    const scale = terminalPhysicalScale(screen.offsetWidth, screen.offsetHeight, targetWidth, targetHeight);
    if (scale <= 0) return;
    // Keep pointer hit-testing, text selection, and link ranges aligned with glyphs for any
    // interactive fixed grid. xterm measures mouse cells before CSS transforms, so fit its font.
    if (fixedDashboardGrid && !scaledPreview) {
      const current = terminal.options.fontSize ?? TERMINAL_DASHBOARD_FONT_SIZE,
        naturalScale = terminalPhysicalScale(
          TERMINAL_PREVIEW_NATURAL_WIDTH,
          TERMINAL_PREVIEW_NATURAL_HEIGHT,
          targetWidth,
          targetHeight,
        ),
        next = Math.min(
          TERMINAL_DASHBOARD_FONT_SIZE,
          terminalFittedFontSize(TERMINAL_DASHBOARD_FONT_SIZE, naturalScale),
        );
      if (Math.abs(next - current) > 0.25) {
        terminal.options.fontSize = next;
        element.dataset.fontSize = String(next);
        element.dataset.geometryReady = 'false';
        terminal.element.style.transform = '';
        return;
      }
      terminal.element.style.transformOrigin = 'top left';
      terminal.element.style.transform = `scale(${scale})`;
      element.dataset.physicalScale = String(scale);
    } else {
      terminal.element.style.transformOrigin = '';
      terminal.element.style.transform = `scale(${scale})`;
      element.dataset.physicalScale = String(scale);
    }
    terminal.element.style.width = '';
    terminal.element.style.height = '';
    element.dataset.geometryReady = 'true';
    if (autoFocus && focusRequested) {
      terminal.focus();
      claim();
    }
  };
  const scheduleDashboardFill = () => {
    if ((!fixedDashboardGrid && !mobile80xM()) || dashboardFrame !== undefined) return;
    dashboardFrame = window.requestAnimationFrame(() => {
      dashboardFrame = undefined;
      applyDashboardPhysicalFill();
    });
  };
  const render = terminal.onRender(scheduleDashboardFill);
  own(() => {
    render.dispose();
  });
  const applySettledGeometry = () => {
    if (disposed || element.closest('[hidden]')) return;
    try {
      if (mobile80xM()) {
        if (webgl) {
          webgl.dispose();
          webgl = undefined;
          element.dataset.renderer = 'dom';
        }
        terminal.resize(mobileCols(), mobileGridRows);
        scheduleDashboardFill();
      } else if (fixedDashboardGrid) {
        terminal.resize(TERMINAL_DASHBOARD_COLS, TERMINAL_DASHBOARD_ROWS);
        scheduleDashboardFill();
      } else {
        fit.fit();
        if (settledResize) {
          const contained = terminalDedicatedGridSize(terminal.cols, terminal.rows);
          terminal.resize(contained.cols, contained.rows);
          element.dataset.containmentRows = '1';
        }
        element.dataset.geometryReady = 'true';
      }
    } catch {
      /* layout can be transiently zero-sized */
      return;
    }
    if (!fixedDashboardGrid && !mobile80xM()) lastSettledGeometry = { cols: terminal.cols, rows: terminal.rows };
    element.dataset.gridSize = `${terminal.cols}x${terminal.rows}`;
    reconcileScale();
    claim();
  };
  const drawerResizeActive = () => insideDrawer && document.body.dataset.resizingRegion === 'vertical';
  const fitAndClaim = () => {
    if (fitFrame !== undefined) window.cancelAnimationFrame(fitFrame);
    if (settleClaim !== undefined) window.clearTimeout(settleClaim);
    if (drawerResizeActive()) return;
    fitFrame = window.requestAnimationFrame(() => {
      fitFrame = undefined;
      applySettledGeometry();
      if (!fixedDashboardGrid)
        settleClaim = window.setTimeout(() => {
          settleClaim = undefined;
          applySettledGeometry();
        }, TERMINAL_RESIZE_SETTLE_MS);
    });
  };
  const finishDrawerResize = () => {
    if (!insideDrawer) return;
    if (settledResize) terminal.focus();
    applySettledGeometry();
  };
  window.addEventListener(TERMINAL_DRAWER_RESIZE_END_EVENT, finishDrawerResize);
  own(() => {
    window.removeEventListener(TERMINAL_DRAWER_RESIZE_END_EVENT, finishDrawerResize);
  });
  const refitMobileColumns = () => {
    if (mobile80xM()) applySettledGeometry();
  };
  window.addEventListener(MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT, refitMobileColumns);
  own(() => {
    window.removeEventListener(MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT, refitMobileColumns);
  });
  const connect = () => {
    if (disposed) return;
    initialReplay = true;
    element.dataset.connection = 'connecting';
    const current = new WebSocket(url);
    socket = current;
    current.binaryType = 'arraybuffer';
    current.addEventListener('open', () => {
      if (socket !== current) return;
      // Every attach begins with the broker/server's authoritative scrollback snapshot. Defer
      // replacement until that binary frame arrives so reset + replay enter xterm as one parser
      // write; clearing synchronously here exposes a blank frame while the replay is in flight.
      replacementReplayPending = connectedOnce;
      connectedOnce = true;
      attempt = 0;
      element.dataset.connection = 'connected';
      fitAndClaim();
      if (autoFocus) terminal.focus();
      heartbeat = window.setInterval(claim, 5_000);
    });
    current.addEventListener('message', (event) => {
      if (socket !== current) return;
      if (typeof event.data === 'string') {
        if (isTerminalReplacementReplay(event.data)) {
          replacementReplayPending = true;
          initialReplay = true;
          return;
        }
        const size = parseTerminalSizeMessage(event.data);
        if (!size) return;
        serverSize = size.pty_size;
        const drivenByViewer = size.driven_by === viewerId;
        const local = claimDimensions();
        resizeTerminalPreservingScroll(terminal, local.cols, local.rows);
        element.dataset.driving = String(drivenByViewer);
        element.dataset.ptySize = `${size.pty_size.cols}x${size.pty_size.rows}`;
        element.dataset.gridSize = `${terminal.cols}x${terminal.rows}`;
        queueMicrotask(reconcileScale);
        return;
      }
      const write = (value: ArrayBuffer) => {
        let bytes = new Uint8Array(value);
        if (initialReplay) {
          initialReplay = false;
          if (fixedDashboardGrid || magnified) bytes = stripLeadingZshPromptEolMark(bytes);
        }
        if (replacementReplayPending) {
          replacementReplayPending = false;
          bytes = terminalReplacementPayload(bytes);
        }
        terminal.write(bytes);
      };
      if (event.data instanceof Blob) {
        void event.data.arrayBuffer().then(write);
        return;
      }
      if (event.data instanceof ArrayBuffer) write(event.data);
    });
    const disconnected = () => {
      if (socket !== current || disposed) return;
      if (heartbeat !== undefined) window.clearInterval(heartbeat);
      heartbeat = undefined;
      element.dataset.connection = 'reconnecting';
      const delay = terminalReconnectDelay(attempt++);
      reconnect = window.setTimeout(connect, delay);
    };
    current.addEventListener('close', disconnected);
    current.addEventListener('error', () => {
      current.close();
    });
  };
  const resize = new ResizeObserver(fitAndClaim);
  own(() => {
    resize.disconnect();
  });
  resize.observe(scaledPreview ? (element.parentElement ?? element) : element);
  fitAndClaim();
  const visibilityTarget = scaledPreview ? (element.parentElement ?? element) : element,
    intersection = new IntersectionObserver((entries) => {
      const next = entries[0]?.isIntersecting ?? false;
      if (next !== visible) {
        visible = next;
        claim();
      }
    });
  own(() => {
    intersection.disconnect();
  });
  intersection.observe(visibilityTarget);
  const focus = () => {
      if (fixedDashboardGrid && element.dataset.geometryReady !== 'true') return;
      focusRequested = false;
      if (fixedDashboardGrid) claim();
      else applySettledGeometry();
    },
    focusTerminal = () => {
      terminal.focus();
    };
  if (!scaledPreview) {
    element.addEventListener('click', focusTerminal);
    element.addEventListener('focusin', focus);
    element.addEventListener('focusout', focus);
    element.addEventListener('pointerdown', signalInteraction);
    element.addEventListener('focusin', signalInteraction);
    own(() => {
      element.removeEventListener('click', focusTerminal);
      element.removeEventListener('focusin', focus);
      element.removeEventListener('focusout', focus);
      element.removeEventListener('pointerdown', signalInteraction);
      element.removeEventListener('focusin', signalInteraction);
    });
  }
  const input = terminal.onData((value) => {
    if (!scaledPreview && socket?.readyState === WebSocket.OPEN) {
      socket.send(value);
      signalInteractionThrottled();
    }
  });
  own(() => {
    input.dispose();
  });
  connect();
  if (autoFocus) {
    const focusIfCurrent = (force = false) => {
      if (disposed || !terminal.element) return;
      const active = document.activeElement,
        activeTile =
          active instanceof Element ? active.closest<HTMLElement>('[data-component="terminal-tile"]') : undefined,
        sameTerminalCard =
          activeTile?.dataset.terminalKey === `${element.dataset.projectId}:${element.dataset.terminalId}`;
      if (
        !force &&
        active !== document.body &&
        !element.contains(active) &&
        !element.closest('[data-magnified="true"]')?.contains(active) &&
        !sameTerminalCard
      )
        return;
      terminal.focus();
      claim();
    };
    focusIfCurrent(true);
    window.requestAnimationFrame(() => {
      focusIfCurrent();
      window.setTimeout(focusIfCurrent, TERMINAL_RESIZE_SETTLE_MS);
    });
  }
}
