import { describe, expect, it } from 'vitest';

import { NO_TERMINAL_MODIFIERS } from '../terminal-keys';
import { TerminalKeyBar } from './terminal-key-bar';

describe('TerminalKeyBar', () => {
  it('keeps the More group first and presents the ordinary keys in one toolbar', () => {
    const markup = String(TerminalKeyBar({ modifiers: NO_TERMINAL_MODIFIERS }));
    expect(markup).toContain('role="toolbar" aria-label="Terminal keys"');
    expect(markup).toContain('class="terminal-key-bar__group" role="group" aria-label="More Terminal Keys"');
    // Each app-owned group slot holds a Kerf ToolbarControlGroup of native buttons (HS2-NQZ2EE).
    expect(markup.match(/data-component="toolbar-control-group"/g)).toHaveLength(3);
    expect(markup).toContain('aria-label="Escape, tab, and modifier keys"');
    expect(markup).toContain('aria-label="Arrow keys"');
    expect(markup.indexOf('aria-label="More Terminal Keys"')).toBeLessThan(markup.indexOf('aria-label="Arrow keys"'));
    expect(markup).toContain('data-key="Escape"');
    expect(markup).toContain('data-key="ArrowRight"');
    expect(markup).not.toContain('aria-label="Clipboard"');
  });

  it('replaces arrows with navigation/function keys and retains locked modifiers', () => {
    const markup = String(
      TerminalKeyBar({
        modifiers: { ctrl: 'once', alt: 'locked', shift: 'off' },
        functionRow: true,
      }),
    );
    expect(markup).toContain('data-function-row="true"');
    expect(markup).toContain('aria-label="Navigation keys"');
    expect(markup).toContain('aria-label="Function keys"');
    expect(markup).toContain('data-key="F12"');
    expect(markup).toContain('aria-label="Alt (locked)"');
    expect(markup).toContain('data-lucide="lock"');
    expect(markup).not.toContain('data-key="ArrowRight"');
    // HS2-FRB545: the extra row's clipboard group follows the modifiers, before navigation keys.
    expect(markup).toContain('aria-label="Clipboard"');
    expect(markup.indexOf('aria-label="Clipboard"')).toBeLessThan(markup.indexOf('aria-label="Navigation keys"'));
    expect(markup).toContain('data-action="copy-terminal-text"');
    expect(markup).toContain('data-action="paste-terminal-text"');
    expect(markup.match(/tabindex="-1"/g)).toHaveLength(24);
  });

  it('HS2-RS96EJ labels the expanded group broadly and exposes sticky Shift there', () => {
    const collapsed = String(TerminalKeyBar({ modifiers: NO_TERMINAL_MODIFIERS }));
    const expanded = String(TerminalKeyBar({ modifiers: NO_TERMINAL_MODIFIERS, functionRow: true }));
    expect(collapsed).toContain('aria-label="More Terminal Keys"');
    expect(collapsed).toContain('data-lucide="ellipsis"');
    expect(collapsed).not.toContain('data-modifier="shift"');
    expect(expanded).toContain('data-modifier="shift"');
    expect(expanded).toContain('aria-label="Shift"');
  });
});
