/**
 * Special-key input for phone terminals (HS2-CKS78M).
 *
 * Phone keyboards have no Esc, Tab, arrow, function, or usable Ctrl/Alt keys. The terminal key bar
 * sends named keys, and sticky modifiers also apply to the next character typed on the soft
 * keyboard. Sequences follow xterm's conventions so shells, editors, and TUIs read them as they
 * would from a desktop keyboard.
 */
export type TerminalModifier = 'ctrl' | 'alt' | 'shift';
/** `once` applies to the next key and then clears; `locked` stays on until tapped again. */
export type TerminalModifierState = 'off' | 'once' | 'locked';
export type TerminalModifiers = Readonly<Record<TerminalModifier, TerminalModifierState>>;

/** Event a key bar dispatches on an interactive viewport to send a named special key. */
export const TERMINAL_KEY_EVENT = 'hotsheet-terminal-key';

export const NO_TERMINAL_MODIFIERS: TerminalModifiers = { ctrl: 'off', alt: 'off', shift: 'off' };

export type TerminalSpecialKey =
  | 'Escape'
  | 'Tab'
  | 'ArrowUp'
  | 'ArrowDown'
  | 'ArrowRight'
  | 'ArrowLeft'
  | 'Home'
  | 'End'
  | 'PageUp'
  | 'PageDown'
  | 'Insert'
  | 'Delete'
  | `F${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12}`;

const ESC = '\u001b';
const CURSOR_FINAL: Partial<Record<TerminalSpecialKey, string>> = {
  ArrowUp: 'A',
  ArrowDown: 'B',
  ArrowRight: 'C',
  ArrowLeft: 'D',
  Home: 'H',
  End: 'F',
};
const SS3_FUNCTION: Partial<Record<TerminalSpecialKey, string>> = { F1: 'P', F2: 'Q', F3: 'R', F4: 'S' };
const TILDE_CODE: Partial<Record<TerminalSpecialKey, number>> = {
  Insert: 2,
  Delete: 3,
  PageUp: 5,
  PageDown: 6,
  F5: 15,
  F6: 17,
  F7: 18,
  F8: 19,
  F9: 20,
  F10: 21,
  F11: 23,
  F12: 24,
};

/** A modifier tap: off → once → locked → off (a double tap locks). */
export function nextModifierState(state: TerminalModifierState): TerminalModifierState {
  return state === 'off' ? 'once' : state === 'once' ? 'locked' : 'off';
}

export function toggleTerminalModifier(modifiers: TerminalModifiers, modifier: TerminalModifier): TerminalModifiers {
  return { ...modifiers, [modifier]: nextModifierState(modifiers[modifier]) };
}

/** Clear one-shot modifiers after they applied to a key; locked ones stay. */
export function consumeTerminalModifiers(modifiers: TerminalModifiers): TerminalModifiers {
  return {
    ctrl: modifiers.ctrl === 'once' ? 'off' : modifiers.ctrl,
    alt: modifiers.alt === 'once' ? 'off' : modifiers.alt,
    shift: modifiers.shift === 'once' ? 'off' : modifiers.shift,
  };
}

export function terminalModifiersActive(modifiers: TerminalModifiers): boolean {
  return modifiers.ctrl !== 'off' || modifiers.alt !== 'off' || modifiers.shift !== 'off';
}

/** xterm's modifier parameter: 1 + shift(1) + alt(2) + ctrl(4); 1 means none. */
function modifierParameter(modifiers: TerminalModifiers): number {
  return (
    1 + (modifiers.shift !== 'off' ? 1 : 0) + (modifiers.alt !== 'off' ? 2 : 0) + (modifiers.ctrl !== 'off' ? 4 : 0)
  );
}

/** Encode a named special key with the active modifiers. */
export function encodeTerminalKey(
  key: TerminalSpecialKey,
  modifiers: TerminalModifiers,
  { applicationCursor = false }: { applicationCursor?: boolean } = {},
): string {
  const parameter = modifierParameter(modifiers);
  if (key === 'Escape') return modifiers.alt !== 'off' ? `${ESC}${ESC}` : ESC;
  if (key === 'Tab') {
    const tab = modifiers.shift !== 'off' ? `${ESC}[Z` : '\t';
    return modifiers.alt !== 'off' ? `${ESC}${tab}` : tab;
  }
  const cursor = CURSOR_FINAL[key];
  if (cursor)
    return parameter > 1 ? `${ESC}[1;${parameter}${cursor}` : `${ESC}${applicationCursor ? 'O' : '['}${cursor}`;
  const ss3 = SS3_FUNCTION[key];
  if (ss3) return parameter > 1 ? `${ESC}[1;${parameter}${ss3}` : `${ESC}O${ss3}`;
  const code = TILDE_CODE[key];
  if (code !== undefined) return parameter > 1 ? `${ESC}[${code};${parameter}~` : `${ESC}[${code}~`;
  return '';
}

/**
 * Apply sticky modifiers to text typed on the soft keyboard. Only a single character is modified
 * (Ctrl maps letters and `@[\]^_`/space to control codes, Shift upper-cases, Alt prefixes ESC);
 * longer input such as a paste or autocorrect replacement passes through unchanged.
 */
export function applyTerminalModifiersToText(text: string, modifiers: TerminalModifiers): string {
  if (!terminalModifiersActive(modifiers) || text.length !== 1) return text;
  let value = modifiers.shift !== 'off' ? text.toUpperCase() : text;
  if (modifiers.ctrl !== 'off') {
    const upper = value.toUpperCase(),
      code = upper.charCodeAt(0);
    if (value === ' ' || value === '2' || value === '@') value = '\u0000';
    else if (code >= 0x40 && code <= 0x5f) value = String.fromCharCode(code & 0x1f);
    else if (value === '?') value = '\u007f';
  }
  return modifiers.alt !== 'off' ? `${ESC}${value}` : value;
}
