import type { Signal } from 'kerfjs';

import { terminalDrawerMaximum, terminalDrawerMinimum } from '../app-region-resize';
import {
  MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT,
  nextMobileTerminalColumns,
  saveMobileTerminalColumns,
} from '../mobile-terminal-columns';
import {
  type MobileTerminalFocusState,
  type MobileTerminalViewport,
  mobileTerminalViewport,
  mobileVirtualKeyboardVisible,
  transitionMobileTerminalFocus,
} from '../mobile-terminal-focus';
import type { TerminalModifiers } from '../terminal-keys';
import { TERMINAL_DRAWER_RESIZE_END_EVENT } from '../terminal-viewport';

/** The visual viewport and on-screen keyboard state the phone terminal surfaces size against. */
export interface MobileViewportGeometry {
  viewport: MobileTerminalViewport;
  keyboardVisible: boolean;
}

/** The current visual viewport geometry and whether a virtual keyboard covers part of it. */
export function currentMobileViewportGeometry(): MobileViewportGeometry {
  return {
    viewport: mobileTerminalViewport(window.visualViewport, window),
    keyboardVisible: mobileVirtualKeyboardVisible(window.visualViewport, window.innerHeight),
  };
}

/** Live application bindings the terminal drawer geometry owner reads and writes (HS2-T1DX2V). */
export interface TerminalDrawerGeometryDependencies {
  terminalDrawerMin: Signal<number>;
  terminalDrawerMax: Signal<number>;
  terminalDrawerMaximized: Signal<boolean>;
  mobileTerminalFocus: Signal<MobileTerminalFocusState>;
  mobileTerminalColumns: Signal<number>;
  mobileViewportGeometry: Signal<MobileViewportGeometry>;
  terminalModifiers: Signal<TerminalModifiers>;
  terminalFunctionRow: Signal<boolean>;
  /** Whether the layout is the phone layout; its signal is created after this owner, so read lazily. */
  viewportMobile: () => boolean;
  updateTerminalDrawerBounds: (target: HTMLElement) => void;
  showToast: (message: string) => void;
}

/**
 * Owns the terminal drawer's measured geometry (minimum/maximum height, maximize, settle passes)
 * and the phone terminal viewport: focus-mode transitions, visual-viewport and keyboard geometry,
 * and the magnified column count. Extracted from the application runtime without behavior
 * changes (HS2-T1DX2V).
 */
export function createTerminalDrawerGeometryController(dependencies: TerminalDrawerGeometryDependencies) {
  const {
    terminalDrawerMin,
    terminalDrawerMax,
    terminalDrawerMaximized,
    mobileTerminalFocus,
    mobileTerminalColumns,
    mobileViewportGeometry,
    terminalModifiers,
    terminalFunctionRow,
    viewportMobile,
    updateTerminalDrawerBounds,
    showToast,
  } = dependencies;
  function syncTerminalDrawerMaximum() {
    // The drawer docks below the main column in the Workbench's center, so the column it may grow
    // into ends at the shell's bottom edge, not at the main column's (which the drawer shortens).
    const shell = document.querySelector<HTMLElement>('.app-shell'),
      workArea = shell?.querySelector<HTMLElement>('.app-shell__work-area');
    if (!shell || !workArea) return;
    // The phone drawer pads its content by the home-indicator inset (HS2-ZEC4QV), so its minimum
    // grows by the resolved inset; on desktop the inset is zero.
    const minimum = terminalDrawerMinimum(
      viewportMobile()
        ? Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hotsheet-safe-area-bottom'))
        : 0,
    );
    if (minimum !== terminalDrawerMin.value) terminalDrawerMin.value = minimum;
    const next = terminalDrawerMaximum(
      shell.getBoundingClientRect().bottom,
      workArea.getBoundingClientRect().top,
      minimum,
    );
    if (next !== terminalDrawerMax.value) terminalDrawerMax.value = next;
  }
  function settleTerminalDrawerGeometry() {
    requestAnimationFrame(() => {
      const target = document.querySelector<HTMLElement>(
        '[data-terminal-drawer-measure="true"] .terminal-drawer__content',
      );
      if (target) updateTerminalDrawerBounds(target);
      window.dispatchEvent(new CustomEvent(TERMINAL_DRAWER_RESIZE_END_EVENT));
    });
  }
  function toggleTerminalDrawerMaximized() {
    terminalDrawerMaximized.value = !terminalDrawerMaximized.value;
    settleTerminalDrawerGeometry();
  }
  function currentMobileTerminalViewport() {
    return mobileTerminalViewport(window.visualViewport, window);
  }
  function enterMobileTerminalFocus(terminalId: string) {
    mobileTerminalFocus.value = transitionMobileTerminalFocus(mobileTerminalFocus.value, {
      type: 'terminal-focus',
      mobile: viewportMobile(),
      terminalId,
      viewport: currentMobileTerminalViewport(),
    });
  }
  function syncMobileTerminalViewport() {
    mobileTerminalFocus.value = transitionMobileTerminalFocus(mobileTerminalFocus.value, {
      type: 'viewport-change',
      viewport: currentMobileTerminalViewport(),
    });
  }
  function exitMobileTerminalFocus() {
    mobileTerminalFocus.value = transitionMobileTerminalFocus(mobileTerminalFocus.value, { type: 'exit' });
  }
  function syncMobileViewportGeometry() {
    const next = currentMobileViewportGeometry(),
      current = mobileViewportGeometry.peek();
    if (
      next.keyboardVisible !== current.keyboardVisible ||
      (Object.keys(next.viewport) as (keyof typeof next.viewport)[]).some(
        (key) => next.viewport[key] !== current.viewport[key],
      )
    )
      mobileViewportGeometry.value = next;
  }
  function cycleMobileTerminalColumns() {
    mobileTerminalColumns.value = saveMobileTerminalColumns(
      localStorage,
      nextMobileTerminalColumns(mobileTerminalColumns.peek()),
    );
    window.dispatchEvent(new CustomEvent(MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT));
    // The size button no longer shows the column count inline (HS2-89JZSN); a toast reports it.
    showToast(`Terminal text size: ${mobileTerminalColumns.value} columns`);
  }
  function mobileMagnifiedTerminal() {
    if (!viewportMobile()) return undefined;
    const geometry = mobileViewportGeometry.value;
    return {
      ...geometry,
      columns: mobileTerminalColumns.value,
      keyBar: { modifiers: terminalModifiers.value, functionRow: terminalFunctionRow.value },
    };
  }
  return {
    syncTerminalDrawerMaximum,
    settleTerminalDrawerGeometry,
    toggleTerminalDrawerMaximized,
    enterMobileTerminalFocus,
    syncMobileTerminalViewport,
    exitMobileTerminalFocus,
    syncMobileViewportGeometry,
    cycleMobileTerminalColumns,
    mobileMagnifiedTerminal,
  };
}
