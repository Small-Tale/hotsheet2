import { describe, expect, it } from 'vitest';

import {
  applyTerminalModifiersToText,
  consumeTerminalModifiers,
  encodeTerminalKey,
  NO_TERMINAL_MODIFIERS,
  type TerminalModifiers,
  terminalModifiersActive,
  toggleTerminalModifier,
} from './terminal-keys';

const withMods = (mods: Partial<TerminalModifiers>): TerminalModifiers => ({ ...NO_TERMINAL_MODIFIERS, ...mods });

describe('terminal special keys (HS2-CKS78M)', () => {
  it('encodes unmodified keys with xterm sequences, honoring application cursor mode', () => {
    expect(encodeTerminalKey('Escape', NO_TERMINAL_MODIFIERS)).toBe('\u001b');
    expect(encodeTerminalKey('Tab', NO_TERMINAL_MODIFIERS)).toBe('\t');
    expect(encodeTerminalKey('ArrowUp', NO_TERMINAL_MODIFIERS)).toBe('\u001b[A');
    expect(encodeTerminalKey('ArrowLeft', NO_TERMINAL_MODIFIERS, { applicationCursor: true })).toBe('\u001bOD');
    expect(encodeTerminalKey('Home', NO_TERMINAL_MODIFIERS)).toBe('\u001b[H');
    expect(encodeTerminalKey('End', NO_TERMINAL_MODIFIERS, { applicationCursor: true })).toBe('\u001bOF');
    expect(encodeTerminalKey('PageUp', NO_TERMINAL_MODIFIERS)).toBe('\u001b[5~');
    expect(encodeTerminalKey('Delete', NO_TERMINAL_MODIFIERS)).toBe('\u001b[3~');
    expect(encodeTerminalKey('F1', NO_TERMINAL_MODIFIERS)).toBe('\u001bOP');
    expect(encodeTerminalKey('F5', NO_TERMINAL_MODIFIERS)).toBe('\u001b[15~');
    expect(encodeTerminalKey('F12', NO_TERMINAL_MODIFIERS)).toBe('\u001b[24~');
  });

  it('adds xterm modifier parameters to cursor, function, and tilde keys', () => {
    expect(encodeTerminalKey('ArrowRight', withMods({ ctrl: 'once' }))).toBe('\u001b[1;5C');
    expect(encodeTerminalKey('ArrowUp', withMods({ shift: 'once' }), { applicationCursor: true })).toBe('\u001b[1;2A');
    expect(encodeTerminalKey('ArrowDown', withMods({ alt: 'locked', ctrl: 'once' }))).toBe('\u001b[1;7B');
    expect(encodeTerminalKey('F2', withMods({ shift: 'once' }))).toBe('\u001b[1;2Q');
    expect(encodeTerminalKey('F10', withMods({ ctrl: 'once', shift: 'once' }))).toBe('\u001b[21;6~');
    expect(encodeTerminalKey('Tab', withMods({ shift: 'once' }))).toBe('\u001b[Z');
    expect(encodeTerminalKey('Escape', withMods({ alt: 'once' }))).toBe('\u001b\u001b');
  });

  it('applies sticky modifiers to one typed character and leaves longer input alone', () => {
    expect(applyTerminalModifiersToText('c', withMods({ ctrl: 'once' }))).toBe('\u0003');
    expect(applyTerminalModifiersToText('C', withMods({ ctrl: 'once' }))).toBe('\u0003');
    expect(applyTerminalModifiersToText('[', withMods({ ctrl: 'once' }))).toBe('\u001b');
    expect(applyTerminalModifiersToText(' ', withMods({ ctrl: 'once' }))).toBe('\u0000');
    expect(applyTerminalModifiersToText('b', withMods({ alt: 'once' }))).toBe('\u001bb');
    expect(applyTerminalModifiersToText('x', withMods({ ctrl: 'once', alt: 'once' }))).toBe('\u001b\u0018');
    expect(applyTerminalModifiersToText('a', withMods({ shift: 'once' }))).toBe('A');
    expect(applyTerminalModifiersToText('hello', withMods({ ctrl: 'once' }))).toBe('hello');
    expect(applyTerminalModifiersToText('q', NO_TERMINAL_MODIFIERS)).toBe('q');
  });

  it('walks modifier taps off → once → locked → off and consumes only one-shot modifiers', () => {
    let mods = NO_TERMINAL_MODIFIERS;
    expect(terminalModifiersActive(mods)).toBe(false);
    mods = toggleTerminalModifier(mods, 'ctrl');
    expect(mods.ctrl).toBe('once');
    mods = toggleTerminalModifier(toggleTerminalModifier(mods, 'alt'), 'alt');
    expect(mods).toEqual({ ctrl: 'once', alt: 'locked', shift: 'off' });
    expect(terminalModifiersActive(mods)).toBe(true);
    mods = consumeTerminalModifiers(mods);
    expect(mods).toEqual({ ctrl: 'off', alt: 'locked', shift: 'off' });
    mods = toggleTerminalModifier(mods, 'alt');
    expect(mods).toEqual(NO_TERMINAL_MODIFIERS);
    // Empty then refill: consuming with nothing active is a no-op, and a new tap starts again.
    expect(consumeTerminalModifiers(mods)).toEqual(NO_TERMINAL_MODIFIERS);
    expect(toggleTerminalModifier(mods, 'shift').shift).toBe('once');
  });
});
