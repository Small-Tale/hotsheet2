/**
 * The app's keyboard-shortcut registry: the single, complete source of truth for every
 * keyboard interaction the client documents, plus the device-local override store and the
 * chord helpers the editor and the runtime handlers share (HS2-QT6PGR).
 *
 * `editable` shortcuts are command chords the user can rebind; they are resolved through
 * {@link matchesShortcut} at the runtime handlers (the central keydown dispatcher, the ticket
 * clipboard policy, and ticket-row selection). Editable ones include the global chords plus the
 * app-level ticket clipboard (copy/cut/paste) and select-all (HS2-9PR10F). `editable: false`
 * shortcuts are fixed ARIA structural/accessibility affordances (list arrows, tab navigation,
 * gallery, control activation, dismissal) — listed for reference but not rebindable, since
 * rebinding ARIA navigation would break screen-reader and platform expectations.
 */

export interface ShortcutChord {
  /** Normalized key: a lowercase character (`k`), or a named key exactly as `KeyboardEvent.key` (`ArrowUp`, `Enter`, `Escape`). */
  key: string;
  /** The primary command modifier — Cmd on macOS, Ctrl elsewhere. */
  mod?: boolean;
  shift?: boolean;
  alt?: boolean;
}

export interface ShortcutDef {
  id: string;
  label: string;
  description: string;
  group: string;
  defaultChord: ShortcutChord;
  editable: boolean;
}

export const KEYBOARD_SHORTCUT_STORAGE_KEY = 'hotsheet.keyboard-shortcuts';

/** Group order for display. */
export const KEYBOARD_SHORTCUT_GROUPS = [
  'Global',
  'Views & panels',
  'Tickets',
  'Navigation & tabs',
  'Media gallery',
] as const;

export const KEYBOARD_SHORTCUTS: readonly ShortcutDef[] = [
  // Global command chords — rebindable, resolved at the central dispatcher.
  { id: 'open-search', label: 'Open search', description: 'Focus the workspace ticket search.', group: 'Global', defaultChord: { key: 'k', mod: true }, editable: true },
  { id: 'undo', label: 'Undo', description: 'Undo the last ticket change.', group: 'Global', defaultChord: { key: 'z', mod: true }, editable: true },
  { id: 'redo', label: 'Redo', description: 'Redo the last undone ticket change.', group: 'Global', defaultChord: { key: 'z', mod: true, shift: true }, editable: true },
  // Views & panels — rebindable command chords resolved at the central dispatcher (HS2-9SHYWD).
  // Defaults follow VS Code where it has an equivalent, except where the browser owns the chord.
  // In particular Safari consumes ⌘⌥B (bookmarks), ⌘, (settings), and the ⌘⌥+Arrow family
  // (browser tabs/tab groups) before the page can handle them (HS2-Q1BH0V). `mod` is Cmd on Apple,
  // Ctrl elsewhere.
  { id: 'toggle-left-sidebar', label: 'Toggle left sidebar', description: 'Show or hide the project sidebar.', group: 'Views & panels', defaultChord: { key: 'b', mod: true }, editable: true },
  { id: 'toggle-right-sidebar', label: 'Toggle right sidebar', description: 'Show or hide the ticket inspector.', group: 'Views & panels', defaultChord: { key: 'b', mod: true, alt: true, shift: true }, editable: true },
  { id: 'toggle-bottom-drawer', label: 'Toggle bottom drawer', description: 'Show or hide the terminal drawer.', group: 'Views & panels', defaultChord: { key: 'j', mod: true }, editable: true },
  { id: 'view-list', label: 'List view', description: 'Switch the workspace to the list view.', group: 'Views & panels', defaultChord: { key: 'l', mod: true, shift: true }, editable: true },
  { id: 'view-board', label: 'Column view', description: 'Switch the workspace to the column (board) view.', group: 'Views & panels', defaultChord: { key: 'b', mod: true, shift: true }, editable: true },
  { id: 'view-notifications', label: 'Notifications view', description: 'Switch the workspace to the notifications view.', group: 'Views & panels', defaultChord: { key: 'm', mod: true, shift: true }, editable: true },
  { id: 'view-settings', label: 'Settings view', description: 'Switch the workspace to the settings view.', group: 'Views & panels', defaultChord: { key: 's', mod: true, alt: true }, editable: true },
  { id: 'view-workspace-grid', label: 'Workspace grid', description: 'Toggle the all-project workspace terminal grid and the last selected project.', group: 'Views & panels', defaultChord: { key: 'g', mod: true, shift: true }, editable: true },
  { id: 'view-all-stats', label: 'All-project stats', description: 'Toggle the all-project stats dashboard and the last selected project.', group: 'Views & panels', defaultChord: { key: 'd', mod: true, shift: true }, editable: true },
  // Ticket clipboard & select-all — app-level chords, rebindable (resolved via matchesShortcut).
  { id: 'select-all-tickets', label: 'Select all tickets', description: 'Select every ticket in the current view.', group: 'Tickets', defaultChord: { key: 'a', mod: true }, editable: true },
  { id: 'copy-tickets', label: 'Copy tickets', description: 'Copy the selected tickets.', group: 'Tickets', defaultChord: { key: 'c', mod: true }, editable: true },
  { id: 'cut-tickets', label: 'Cut tickets', description: 'Cut the selected tickets.', group: 'Tickets', defaultChord: { key: 'x', mod: true }, editable: true },
  { id: 'paste-tickets', label: 'Paste tickets', description: 'Paste tickets from the clipboard.', group: 'Tickets', defaultChord: { key: 'v', mod: true }, editable: true },
  // Create a new ticket. A bare `c` follows the GitHub/Linear "create" convention; it only fires when no
  // text field is focused (HS2-9SHYWD). Rebind to a modifier chord in Settings → Keyboard if preferred.
  { id: 'new-ticket', label: 'New ticket', description: 'Open the new-ticket composer.', group: 'Tickets', defaultChord: { key: 'c' }, editable: true },
  { id: 'move-selection-up', label: 'Move selection up', description: 'Focus and select the previous ticket.', group: 'Tickets', defaultChord: { key: 'ArrowUp' }, editable: false },
  { id: 'move-selection-down', label: 'Move selection down', description: 'Focus and select the next ticket.', group: 'Tickets', defaultChord: { key: 'ArrowDown' }, editable: false },
  { id: 'activate', label: 'Activate control', description: 'Activate the focused row, control, or menu item.', group: 'Navigation & tabs', defaultChord: { key: 'Enter' }, editable: false },
  { id: 'dismiss', label: 'Dismiss', description: 'Close the open menu, popup, gallery, or overlay.', group: 'Navigation & tabs', defaultChord: { key: 'Escape' }, editable: false },
  { id: 'tab-previous', label: 'Previous tab', description: 'Move focus to the previous tab in a tab strip.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowLeft' }, editable: false },
  { id: 'tab-next', label: 'Next tab', description: 'Move focus to the next tab in a tab strip.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowRight' }, editable: false },
  { id: 'tab-first', label: 'First tab', description: 'Move focus to the first tab in a tab strip.', group: 'Navigation & tabs', defaultChord: { key: 'Home' }, editable: false },
  { id: 'tab-last', label: 'Last tab', description: 'Move focus to the last tab in a tab strip.', group: 'Navigation & tabs', defaultChord: { key: 'End' }, editable: false },
  { id: 'close-tab', label: 'Close tab', description: 'Close the focused terminal or chat tab.', group: 'Navigation & tabs', defaultChord: { key: 'Delete' }, editable: false },
  { id: 'reorder-tab-left', label: 'Move tab left', description: 'Reorder the focused drawer tab toward the start.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowLeft', alt: true, shift: true }, editable: false },
  { id: 'reorder-tab-right', label: 'Move tab right', description: 'Reorder the focused drawer tab toward the end.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowRight', alt: true, shift: true }, editable: false },
  // Cycle the active project / drawer tab (rebindable, resolved at the central dispatcher — HS2-9SHYWD).
  { id: 'project-tab-previous', label: 'Previous project tab', description: 'Activate the previous open project tab.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowLeft', mod: true, alt: true, shift: true }, editable: true },
  { id: 'project-tab-next', label: 'Next project tab', description: 'Activate the next open project tab.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowRight', mod: true, alt: true, shift: true }, editable: true },
  { id: 'drawer-tab-previous', label: 'Previous drawer tab', description: 'Select the previous bottom-drawer tab.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowUp', mod: true, alt: true, shift: true }, editable: true },
  { id: 'drawer-tab-next', label: 'Next drawer tab', description: 'Select the next bottom-drawer tab.', group: 'Navigation & tabs', defaultChord: { key: 'ArrowDown', mod: true, alt: true, shift: true }, editable: true },
  { id: 'gallery-previous', label: 'Previous attachment', description: 'Show the previous image in the attachment gallery.', group: 'Media gallery', defaultChord: { key: 'ArrowLeft' }, editable: false },
  { id: 'gallery-next', label: 'Next attachment', description: 'Show the next image in the attachment gallery.', group: 'Media gallery', defaultChord: { key: 'ArrowRight' }, editable: false },
  { id: 'gallery-toggle-playback', label: 'Play or pause video', description: 'Toggle playback of the gallery video.', group: 'Media gallery', defaultChord: { key: ' ' }, editable: false },
];

const SHORTCUTS_BY_ID = new Map(KEYBOARD_SHORTCUTS.map(shortcut => [shortcut.id, shortcut]));

export function shortcutDef(id: string): ShortcutDef | undefined {
  return SHORTCUTS_BY_ID.get(id);
}

/** True when running on an Apple platform, where the primary modifier is Cmd and displays as ⌘. */
export function isAppleShortcutPlatform(navigatorLike: Pick<Navigator, 'userAgent'> = navigator): boolean {
  return /macintosh|mac os|iphone|ipad|ipod/i.test(navigatorLike.userAgent || '');
}

export function chordsEqual(a: ShortcutChord | undefined, b: ShortcutChord | undefined): boolean {
  if (!a || !b) return a === b;
  return a.key.toLowerCase() === b.key.toLowerCase()
    && Boolean(a.mod) === Boolean(b.mod)
    && Boolean(a.shift) === Boolean(b.shift)
    && Boolean(a.alt) === Boolean(b.alt);
}

function isValidChord(value: unknown): value is ShortcutChord {
  return Boolean(value) && typeof value === 'object' && typeof (value as ShortcutChord).key === 'string' && (value as ShortcutChord).key.length > 0;
}

/** Read the device-local overrides, tolerating missing/corrupt storage and ignoring unknown or non-editable ids. */
export function loadShortcutOverrides(storage: Pick<Storage, 'getItem'> = localStorage): Record<string, ShortcutChord> {
  let raw: string | null;
  try {
    raw = storage.getItem(KEYBOARD_SHORTCUT_STORAGE_KEY);
  } catch {
    return {};
  }
  if (!raw) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!parsed || typeof parsed !== 'object') return {};
  const result: Record<string, ShortcutChord> = {};
  for (const [id, chord] of Object.entries(parsed as Record<string, unknown>)) {
    const def = SHORTCUTS_BY_ID.get(id);
    if (def?.editable && isValidChord(chord)) result[id] = { key: chord.key, mod: Boolean(chord.mod), shift: Boolean(chord.shift), alt: Boolean(chord.alt) };
  }
  return result;
}

export function saveShortcutOverrides(overrides: Record<string, ShortcutChord>, storage: Pick<Storage, 'setItem' | 'removeItem'> = localStorage): void {
  try {
    if (Object.keys(overrides).length === 0) storage.removeItem(KEYBOARD_SHORTCUT_STORAGE_KEY);
    else storage.setItem(KEYBOARD_SHORTCUT_STORAGE_KEY, JSON.stringify(overrides));
  } catch {
    // Device-local persistence is a convenience; ignore quota/availability failures.
  }
}

/** The effective chord for a shortcut: a saved override for an editable shortcut, else its default. */
export function resolveChord(id: string, overrides: Record<string, ShortcutChord>): ShortcutChord | undefined {
  const def = SHORTCUTS_BY_ID.get(id);
  if (!def) return undefined;
  return def.editable && Object.hasOwn(overrides, id) ? overrides[id] : def.defaultChord;
}

/** Whether a keyboard event matches a chord. `mod` maps to metaKey on Apple platforms and ctrlKey elsewhere. */
export function matchesChord(event: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'>, chord: ShortcutChord | undefined, apple = isAppleShortcutPlatform()): boolean {
  if (!chord) return false;
  const mod = apple ? event.metaKey : event.ctrlKey;
  const otherMod = apple ? event.ctrlKey : event.metaKey;
  return event.key.toLowerCase() === chord.key.toLowerCase()
    && mod === Boolean(chord.mod)
    && !otherMod
    && event.shiftKey === Boolean(chord.shift)
    && event.altKey === Boolean(chord.alt);
}

/** Resolve a shortcut id against the given overrides and test the event against its effective chord. */
export function matchesShortcut(id: string, event: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'>, overrides: Record<string, ShortcutChord>, apple = isAppleShortcutPlatform()): boolean {
  return matchesChord(event, resolveChord(id, overrides), apple);
}

const MODIFIER_KEYS = new Set(['Control', 'Meta', 'Shift', 'Alt', 'AltGraph']);

/**
 * Capture a chord from a keydown while recording a new binding, or `undefined` if the event is a
 * lone modifier (keep waiting for the real key). The primary modifier is required to be Cmd/Ctrl
 * on the matching platform.
 */
export function chordFromEvent(event: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'>, apple = isAppleShortcutPlatform()): ShortcutChord | undefined {
  if (MODIFIER_KEYS.has(event.key)) return undefined;
  const mod = apple ? event.metaKey : event.ctrlKey;
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  return { key, mod, shift: event.shiftKey, alt: event.altKey };
}

/** Another editable shortcut that already uses `chord` (a conflict), or `undefined`. */
export function findChordConflict(id: string, chord: ShortcutChord, overrides: Record<string, ShortcutChord>): ShortcutDef | undefined {
  return KEYBOARD_SHORTCUTS.find(other => other.id !== id && other.editable && chordsEqual(resolveChord(other.id, overrides), chord));
}

/** Human-readable chord, e.g. `⌘K`, `Ctrl+Shift+Z`, `⌥⇧←`, `Space`. */
export function formatChord(chord: ShortcutChord | undefined, apple = isAppleShortcutPlatform()): string {
  if (!chord) return 'Unassigned';
  const parts: string[] = [];
  if (chord.mod) parts.push(apple ? '⌘' : 'Ctrl');
  if (chord.alt) parts.push(apple ? '⌥' : 'Alt');
  if (chord.shift) parts.push(apple ? '⇧' : 'Shift');
  parts.push(formatKey(chord.key, apple));
  return apple ? parts.join('') : parts.join('+');
}

function formatKey(key: string, apple: boolean): string {
  const named: Record<string, string> = {
    arrowup: apple ? '↑' : 'Up',
    arrowdown: apple ? '↓' : 'Down',
    arrowleft: apple ? '←' : 'Left',
    arrowright: apple ? '→' : 'Right',
    ' ': 'Space',
    escape: 'Esc',
    enter: apple ? '⏎' : 'Enter',
    delete: 'Delete',
    backspace: apple ? '⌫' : 'Backspace',
    home: 'Home',
    end: 'End',
  };
  return named[key.toLowerCase()] ?? (key.length === 1 ? key.toUpperCase() : key);
}
