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
}

export type MobileTerminalFocusEvent =
  | { type: 'terminal-focus'; mobile: boolean; terminalId: string; viewport: MobileTerminalViewport }
  | { type: 'viewport-change'; viewport: MobileTerminalViewport }
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
  if (event.type === 'terminal-focus')
    return event.mobile
      ? { active: true, terminalId: event.terminalId, viewport: event.viewport }
      : INACTIVE_MOBILE_TERMINAL_FOCUS;
  if (event.type === 'viewport-change') return state.active ? { ...state, viewport: event.viewport } : state;
  return state.active ? INACTIVE_MOBILE_TERMINAL_FOCUS : state;
}
