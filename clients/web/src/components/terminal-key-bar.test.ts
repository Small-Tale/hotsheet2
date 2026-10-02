import { describe, expect, it } from 'vitest';

import { NO_TERMINAL_MODIFIERS } from '../terminal-keys';
import { TerminalKeyBar } from './terminal-key-bar';

describe('TerminalKeyBar', () => {
  it('keeps the Fn group first and presents the ordinary keys in one toolbar', () => {
    const markup = String(TerminalKeyBar({ modifiers: NO_TERMINAL_MODIFIERS }));
    expect(markup).toContain('role="toolbar" aria-label="Terminal keys"');
    expect(markup).toContain('class="terminal-key-bar__group" role="group" aria-label="Function row"');
    expect(markup).toContain('aria-label="Escape, tab, and modifier keys"');
    expect(markup).toContain('aria-label="Arrow keys"');
    expect(markup.indexOf('aria-label="Function row"')).toBeLessThan(markup.indexOf('aria-label="Arrow keys"'));
    expect(markup).not.toContain('data-component="toolbar-control-group"');
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
    expect(markup).not.toContain('data-key="ArrowRight"');
    // HS2-FRB545: the Fn row's clipboard group follows the modifiers, before the navigation keys.
    expect(markup).toContain('aria-label="Clipboard"');
    expect(markup.indexOf('aria-label="Clipboard"')).toBeLessThan(markup.indexOf('aria-label="Navigation keys"'));
    expect(markup).toContain('data-action="copy-terminal-text"');
    expect(markup).toContain('data-action="paste-terminal-text"');
    expect(markup.match(/tabindex="-1"/g)).toHaveLength(24);
  });
});
