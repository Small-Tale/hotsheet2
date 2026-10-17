import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { KEYBOARD_SHORTCUTS } from '../keyboard-shortcuts';
import { KeyboardSettings } from './keyboard-settings';

describe('KeyboardSettings', () => {
  it('renders each shortcut group as a base-surface Kerf FramedList with a highlighted recording row (HS2-4GX2YS)', () => {
    const css = readFileSync(new URL('./keyboard-settings.css', import.meta.url), 'utf8');
    expect(css).not.toMatch(/\.keyboard-settings__(list|row)\b/u);
    const markup = String(KeyboardSettings({ overrides: {}, capturingId: 'new-ticket', apple: true }));
    expect(markup).toMatch(
      /data-component="framed-list"[^>]*data-surface="base"|data-surface="base"[^>]*data-component="framed-list"/u,
    );
    const capturing = markup.match(/<li[^>]*data-shortcut-id="new-ticket"[^>]*>/u)?.[0] ?? '';
    expect(capturing).toContain('data-highlighted="true"');
    expect(capturing).toContain('data-capturing="true"');
  });

  it('keeps the intro copy on the row text edge through Kerf Text item geometry (HS2-V8YYXA)', () => {
    const markup = String(KeyboardSettings({ overrides: {}, apple: true }));
    const intro = markup.match(/<p[^>]*>\s*Rebind the command shortcuts/)?.[0];
    expect(intro).toBeDefined();
    expect(intro).not.toContain('data-flush');
  });

  it('lists every shortcut grouped, with editable rows editable and fixed rows marked System', () => {
    const markup = String(KeyboardSettings({ overrides: {}, capturingId: undefined, apple: true }));
    expect(markup).toContain('data-component="keyboard-settings"');
    expect(markup).toContain('data-component="list"');
    expect(markup).toContain('data-gap="true"');
    expect(markup).toContain('data-component="row"');
    expect(markup).toContain('data-wrap="true"');
    for (const shortcut of KEYBOARD_SHORTCUTS) expect(markup).toContain(`data-shortcut-id="${shortcut.id}"`);
    for (const group of ['Global', 'Tickets', 'Navigation &amp; tabs', 'Media gallery'])
      expect(markup).toContain(`>${group}</h3>`);
    // Editable global chords expose an edit control and render their chord (⌘ on Apple).
    expect(markup).toMatch(/data-shortcut-id="open-search"[\s\S]*?data-action="edit-shortcut"/);
    expect(markup).toContain('⌘K');
    // Fixed ARIA shortcuts are labeled System and have no edit control.
    expect(markup).toMatch(/data-shortcut-id="move-selection-up"[^]*?>System</);
    const fixedRow = markup.slice(
      markup.indexOf('data-shortcut-id="move-selection-up"'),
      markup.indexOf('data-shortcut-id="move-selection-down"'),
    );
    expect(fixedRow).not.toContain('data-action="edit-shortcut"');
    // The ticket clipboard and select-all chords are now editable (HS2-9PR10F).
    const copyRow = markup.slice(
      markup.indexOf('data-shortcut-id="copy-tickets"'),
      markup.indexOf('data-shortcut-id="cut-tickets"'),
    );
    expect(copyRow).toContain('data-action="edit-shortcut"');
    const selectAllRow = markup.slice(
      markup.indexOf('data-shortcut-id="select-all-tickets"'),
      markup.indexOf('data-shortcut-id="copy-tickets"'),
    );
    expect(selectAllRow).toContain('data-action="edit-shortcut"');
    // Reset-all is disabled when there are no overrides.
    expect(markup).toMatch(/data-action="reset-all-shortcuts"[^>]*disabled/);
  });

  it('shows the recording state, the override chord, an enabled reset, and a conflict warning', () => {
    const overrides = { 'open-search': { key: 'z', mod: true }, undo: { key: 'z', mod: true } };
    const markup = String(KeyboardSettings({ overrides, capturingId: 'redo', apple: false }));
    // The overridden open-search shows its custom chord (Ctrl on non-Apple) and an enabled reset.
    expect(markup).toContain('Ctrl+Z');
    expect(markup).toMatch(/data-shortcut-id="open-search"[^]*?data-overridden="true"/);
    const searchRow = markup.slice(
      markup.indexOf('data-shortcut-id="open-search"'),
      markup.indexOf('data-shortcut-id="undo"'),
    );
    expect(searchRow).toMatch(
      /<button(?=[^>]*data-action="reset-shortcut")(?=[^>]*data-shortcut-id="open-search")(?![^>]*disabled)/,
    );
    // open-search and undo now collide → each flags a conflict.
    expect(searchRow).toContain('keyboard-settings__conflict');
    // The row being recorded shows the capture control and no static chord.
    expect(markup).toMatch(/data-shortcut-id="redo"[^]*?data-capturing="true"/);
    expect(markup).toContain('data-shortcut-capture="redo"');
    // reset-all is enabled once overrides exist.
    expect(markup).toMatch(/data-action="reset-all-shortcuts"(?![^>]*disabled)/);
  });
  it('distinguishes Control from Command and projects conflicts on the supplied platform (HS2-835BZD)', () => {
    const overrides = { undo: { key: 'k', ctrl: true } };
    const apple = String(KeyboardSettings({ overrides, apple: true }));
    expect(apple).toContain('⌃K');
    expect(apple).toContain('⌘K');
    expect(apple).not.toContain('keyboard-settings__conflict');
    const other = String(KeyboardSettings({ overrides, apple: false }));
    expect(other).toContain('Ctrl+K');
    expect(other.match(/class="[^"]*keyboard-settings__conflict[^"]*"/g)).toHaveLength(2);
  });
});
