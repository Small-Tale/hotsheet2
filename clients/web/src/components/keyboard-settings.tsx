import './keyboard-settings.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pencil, RotateCcw, TriangleAlert, X } from 'lucide';

import { findChordConflict, formatChord, KEYBOARD_SHORTCUT_GROUPS, KEYBOARD_SHORTCUTS, resolveChord, type ShortcutChord } from '../keyboard-shortcuts';

export interface KeyboardSettingsProps {
  /** Device-local rebindings for editable shortcuts. */
  overrides: Record<string, ShortcutChord>;
  /** The shortcut id currently recording a new chord, if any. */
  capturingId?: string;
  /** Whether to display Cmd/⌘ (Apple) rather than Ctrl. */
  apple: boolean;
}

/**
 * The App Settings → Keyboard screen: a complete, grouped reference of every documented keyboard
 * shortcut. Editable global chords can be rebound (record a new chord, reset to default), with
 * conflict warnings; fixed structural/accessibility shortcuts are shown for reference (HS2-QT6PGR).
 */
export function KeyboardSettings({ overrides, capturingId, apple }: KeyboardSettingsProps) {
  const hasOverrides = Object.keys(overrides).length > 0;
  return <section class="keyboard-settings" data-component="keyboard-settings" aria-label="Keyboard shortcuts">
    <header class="keyboard-settings__header">
      <p class="keyboard-settings__intro">Rebind the global command shortcuts below. Structural navigation, activation, dismissal, and clipboard shortcuts are shown for reference and use platform conventions. Changes are saved on this device.</p>
      <button type="button" class="keyboard-settings__reset-all" data-action="reset-all-shortcuts" disabled={!hasOverrides}><LucideIcon icon={RotateCcw} name="rotate-ccw" />Reset all to defaults</button>
    </header>
    {KEYBOARD_SHORTCUT_GROUPS.map(group => {
      const shortcuts = KEYBOARD_SHORTCUTS.filter(shortcut => shortcut.group === group);
      if (!shortcuts.length) return undefined;
      return <div class="keyboard-settings__group">
        <h3 class="keyboard-settings__group-title">{group}</h3>
        <ul class="keyboard-settings__list">
          {shortcuts.map(shortcut => {
            const chord = resolveChord(shortcut.id, overrides);
            const capturing = capturingId === shortcut.id;
            const overridden = shortcut.editable && Boolean(overrides[shortcut.id]);
            const conflict = shortcut.editable && !capturing && chord ? findChordConflict(shortcut.id, chord, overrides) : undefined;
            return <li class="keyboard-settings__row" data-shortcut-id={shortcut.id} data-editable={String(shortcut.editable)} data-capturing={String(capturing)} data-overridden={String(overridden)}>
              <div class="keyboard-settings__meta">
                <span class="keyboard-settings__label">{shortcut.label}</span>
                <span class="keyboard-settings__description">{shortcut.description}</span>
                {conflict && <span class="keyboard-settings__conflict" role="status"><LucideIcon icon={TriangleAlert} name="triangle-alert" />Also used by “{conflict.label}”</span>}
              </div>
              <div class="keyboard-settings__controls">
                {capturing
                  ? <button type="button" class="keyboard-settings__capture" data-shortcut-capture={shortcut.id} aria-label={`Recording new shortcut for ${shortcut.label}. Press a key combination, or Escape to cancel.`}>Press keys…</button>
                  : <kbd class="keyboard-settings__chord">{formatChord(chord, apple)}</kbd>}
                {shortcut.editable
                  ? (capturing
                    ? <button type="button" class="keyboard-settings__action" data-action="cancel-shortcut-capture" aria-label={`Cancel editing ${shortcut.label}`} title="Cancel"><LucideIcon icon={X} name="x" /></button>
                    : <>
                      <button type="button" class="keyboard-settings__action" data-action="edit-shortcut" data-shortcut-id={shortcut.id} aria-label={`Change shortcut for ${shortcut.label}`} title="Change shortcut"><LucideIcon icon={Pencil} name="pencil" /></button>
                      <button type="button" class="keyboard-settings__action" data-action="reset-shortcut" data-shortcut-id={shortcut.id} disabled={!overridden} aria-label={`Reset ${shortcut.label} to its default`} title="Reset to default"><LucideIcon icon={RotateCcw} name="rotate-ccw" /></button>
                    </>)
                  : <span class="keyboard-settings__fixed" title="Fixed system shortcut">System</span>}
              </div>
            </li>;
          })}
        </ul>
      </div>;
    })}
  </section>;
}
