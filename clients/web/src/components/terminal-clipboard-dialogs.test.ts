import { describe, expect, it } from 'vitest';

import { TerminalCopyDialog, TerminalEditMenu, TerminalPasteDialog } from './terminal-clipboard-dialogs';

describe('TerminalCopyDialog', () => {
  it('renders a closed dialog with no state', () => {
    const markup = String(TerminalCopyDialog({}));
    expect(markup).toContain('data-component="terminal-copy-dialog"');
    expect(markup).toContain('data-controlled-open="false"');
  });

  it('shows the snapshot in a read-only, keyed text view with Done and Copy', () => {
    const markup = String(
      TerminalCopyDialog({ state: { open: true, title: 'Build', generation: 3, text: '$ make <all>' } }),
    );
    expect(markup).toContain('data-controlled-open="true"');
    expect(markup).toContain('select part of Build');
    expect(markup).toContain('name="terminal-copy-text"');
    expect(markup).toContain('data-key="terminal-copy-text:3"');
    expect(markup).toMatch(/<textarea[^>]*readonly/);
    expect(markup).toContain('$ make &lt;all&gt;');
    expect(markup).toContain('data-action="close-terminal-copy"');
    expect(markup).toContain('data-action="confirm-terminal-copy"');
    expect(markup).toContain('data-lucide="copy"');
  });

  it('keeps the snapshot while closing so the hide animation does not empty it', () => {
    const markup = String(TerminalCopyDialog({ state: { open: false, title: 'Build', generation: 3, text: 'kept' } }));
    expect(markup).toContain('data-controlled-open="false"');
    expect(markup).toContain('kept');
  });
});

describe('TerminalPasteDialog', () => {
  it('explains a denied read and offers an empty editable field', () => {
    const markup = String(
      TerminalPasteDialog({ state: { open: true, title: 'Shell', generation: 4, reason: 'denied' } }),
    );
    expect(markup).toContain('Clipboard access was not allowed.');
    expect(markup).toContain('send it to Shell');
    expect(markup).toContain('data-action="submit-terminal-paste"');
    expect(markup).toContain('data-action="cancel-terminal-paste"');
    expect(markup).toContain('data-key="terminal-paste-text:4"');
    expect(markup).not.toMatch(/<textarea[^>]*readonly/);
    expect(markup).toContain('data-lucide="clipboard-paste"');
  });

  it('explains an unavailable clipboard API', () => {
    const markup = String(
      TerminalPasteDialog({ state: { open: true, title: 'Shell', generation: 1, reason: 'unavailable' } }),
    );
    expect(markup).toContain('This browser does not let Hot Sheet read the clipboard.');
    expect(String(TerminalPasteDialog({}))).toContain('data-controlled-open="false"');
  });
});

describe('TerminalEditMenu (HS2-KKP8YJ)', () => {
  it('renders nothing until a long-press opens it', () => {
    expect(TerminalEditMenu({})).toBeNull();
  });

  it('anchors a context PopupMenu at the press with Copy Text… and Paste reusing the clipboard actions', () => {
    const markup = String(TerminalEditMenu({ state: { x: 120, y: 340 } }));
    expect(markup).toContain('data-component="terminal-edit-menu"');
    expect(markup).toContain('aria-label="Terminal edit actions"');
    expect(markup).toContain('data-context-anchor-x="120"');
    expect(markup).toContain('data-context-anchor-y="340"');
    expect(markup).toContain('data-context-menu="terminal-edit"');
    expect(markup.match(/<wa-dropdown-item/g)).toHaveLength(2);
    expect(markup).toContain('data-action="copy-terminal-text"');
    expect(markup).toContain('data-action="paste-terminal-text"');
    expect(markup).toContain('Copy Text…');
    expect(markup).not.toContain('data-action="copy-terminal-selection"');
    // Copy Text… opens a sheet for selecting text, so the plain `copy` icon stays with Copy (HS2-EYR96N).
    expect(markup).toContain('data-lucide="text-select"');
    expect(markup).not.toContain('data-lucide="copy"');
    expect(markup).toContain('data-lucide="clipboard-paste"');
  });

  it('leads with Copy for a long-press selection (HS2-EYR96N)', () => {
    const markup = String(TerminalEditMenu({ state: { x: 120, y: 340, selection: true } }));
    expect(markup.match(/<wa-dropdown-item/g)).toHaveLength(3);
    const order = ['copy-terminal-selection', 'copy-terminal-text', 'paste-terminal-text'].map((action) =>
      markup.indexOf(`data-action="${action}"`),
    );
    expect(order.every((index) => index >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(markup).toMatch(/data-action="copy-terminal-selection"[\s\S]*?data-lucide="copy"[\s\S]*?Copy</);
  });
});
