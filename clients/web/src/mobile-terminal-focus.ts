export interface MobileTerminalViewport {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface MobileTerminalFocusState {
  active: boolean;
  terminalId?: string;
  viewport: MobileTerminalViewport;
  /**
   * The full-screen terminal shows its search bar in place of its floating controls (HS2-7HPXCY). Any
   * exit from full screen ends it; the query itself lives in the terminal search model and survives.
   */
  search?: boolean;
}

export type MobileTerminalFocusEvent =
  | {
      type: 'terminal-focus';
      mobile: boolean;
      terminalId: string;
      viewport: MobileTerminalViewport;
      /** Open the search bar with full screen; omitted keeps an open search bar on the same terminal. */
      search?: boolean;
    }
  | { type: 'viewport-change'; viewport: MobileTerminalViewport }
  | { type: 'search'; open: boolean }
  | { type: 'exit' }
  | { type: 'context-invalid' };

export const EMPTY_MOBILE_TERMINAL_VIEWPORT: MobileTerminalViewport = { left: 0, top: 0, width: 1, height: 1 };
export const INACTIVE_MOBILE_TERMINAL_FOCUS: MobileTerminalFocusState = {
  active: false,
  viewport: EMPTY_MOBILE_TERMINAL_VIEWPORT,
};

/** Visible viewport geometry in layout-viewport coordinates, including virtual-keyboard shrinkage. */
export function mobileTerminalViewport(
  visualViewport: Pick<VisualViewport, 'offsetLeft' | 'offsetTop' | 'width' | 'height'> | null | undefined,
  fallback: Pick<Window, 'innerWidth' | 'innerHeight'>,
): MobileTerminalViewport {
  return visualViewport
    ? {
        left: Math.max(0, visualViewport.offsetLeft),
        top: Math.max(0, visualViewport.offsetTop),
        width: Math.max(1, visualViewport.width),
        height: Math.max(1, visualViewport.height),
      }
    : { left: 0, top: 0, width: Math.max(1, fallback.innerWidth), height: Math.max(1, fallback.innerHeight) };
}

/** Latest-event-wins transition policy for the ephemeral mobile terminal focus surface. */
export function transitionMobileTerminalFocus(
  state: MobileTerminalFocusState,
  event: MobileTerminalFocusEvent,
): MobileTerminalFocusState {
  if (event.type === 'terminal-focus') {
    if (!event.mobile) return INACTIVE_MOBILE_TERMINAL_FOCUS;
    // Tapping into the searched terminal keeps its search bar; another terminal starts without one.
    const search = event.search ?? (state.active && state.terminalId === event.terminalId && state.search === true);
    return { active: true, terminalId: event.terminalId, viewport: event.viewport, ...(search ? { search } : {}) };
  }
  if (event.type === 'viewport-change') return state.active ? { ...state, viewport: event.viewport } : state;
  if (event.type === 'search') {
    if (!state.active || (state.search === true) === event.open) return state;
    if (event.open) return { ...state, search: true };
    return { active: state.active, terminalId: state.terminalId, viewport: state.viewport };
  }
  return state.active ? INACTIVE_MOBILE_TERMINAL_FOCUS : state;
}

/** The on-screen keyboard covers at least this much of the layout viewport when it is presented. */
export const MOBILE_VIRTUAL_KEYBOARD_MIN_INSET = 120;

/** Whether a virtual keyboard is covering part of the layout viewport. Pinch zoom also shrinks the
 * visual viewport, so compare its unscaled height rather than its CSS-pixel height. */
export function mobileVirtualKeyboardVisible(
  visualViewport: Pick<VisualViewport, 'height' | 'scale'> | null | undefined,
  layoutHeight: number,
): boolean {
  if (!visualViewport) return false;
  const scale = Number.isFinite(visualViewport.scale) && visualViewport.scale > 0 ? visualViewport.scale : 1;
  return layoutHeight - visualViewport.height * scale > MOBILE_VIRTUAL_KEYBOARD_MIN_INSET;
}
