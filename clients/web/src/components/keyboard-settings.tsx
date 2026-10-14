import './keyboard-settings.css';
import '@awesome.me/webawesome/dist/components/button/button.js';

import { Badge } from '@kerfjs/ui/badge';
import { flex, px, uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Pencil, RotateCcw, TriangleAlert, X } from 'lucide';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import {
  findChordConflict,
  formatChord,
  KEYBOARD_SHORTCUT_GROUPS,
  KEYBOARD_SHORTCUTS,
  resolveChord,
  type ShortcutChord,
} from '../keyboard-shortcuts';

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
 * conflict warnings; fixed ARIA structural shortcuts are shown for reference (HS2-QT6PGR, HS2-9PR10F).
 * Copy, group headers, actions, and the System badge compose Kerf and Web Awesome components; the
 * stylesheet keeps only the framed shortcut list, the keycap chip, and the recording control (HS2-8YS9SD).
 */
export function KeyboardSettings({ overrides, capturingId, apple }: KeyboardSettingsProps) {
  const hasOverrides = Object.keys(overrides).length > 0;
  return (
    <section class="keyboard-settings" data-component="keyboard-settings" aria-label="Keyboard Shortcuts">
      <List gap="l">
        <Row hAlign="space-between" vAlign="top" gap="m" wrap>
          <Row flex={flex(1, 1, px(320))}>
            <Text tone="quiet">
              Rebind the command shortcuts below, including the ticket clipboard and select-all. Fixed ARIA navigation
              shortcuts (list arrows, tab and gallery navigation, activation, and dismissal) are shown for reference and
              use platform conventions. Changes are saved on this device.
            </Text>
          </Row>
          <wa-button
            appearance="outlined"
            size="small"
            {...COMMANDS_AND_AI_ACTIONS.resetAllShortcuts.attrs}
            disabled={!hasOverrides}
          >
            <LucideIcon slot="start" size={14.4} icon={RotateCcw} name="rotate-ccw" />
            Reset All to Defaults
          </wa-button>
        </Row>
        {KEYBOARD_SHORTCUT_GROUPS.map((group) => {
          const shortcuts = KEYBOARD_SHORTCUTS.filter((shortcut) => shortcut.group === group);
          if (!shortcuts.length) return undefined;
          return (
            <List gap="2xs">
              <ListHeader label={group} headingLevel={3} inline />
              <ul class="keyboard-settings__list">
                {shortcuts.map((shortcut) => {
                  const chord = resolveChord(shortcut.id, overrides);
                  const capturing = capturingId === shortcut.id;
                  const overridden = shortcut.editable && Boolean(overrides[shortcut.id]);
                  const conflict =
                    shortcut.editable && !capturing && chord
                      ? findChordConflict(shortcut.id, chord, overrides, apple)
                      : undefined;
                  return (
                    <li
                      class="keyboard-settings__row"
                      data-shortcut-id={shortcut.id}
                      data-editable={String(shortcut.editable)}
                      data-capturing={String(capturing)}
                      data-overridden={String(overridden)}
                    >
                      <Row hAlign="space-between" vAlign="middle" gap="m" wrap>
                        <List gap="2xs" flex={flex(1, 1, px(240))}>
                          <Text variant="span">
                            <strong>{shortcut.label}</strong>
                          </Text>
                          <Text variant="span" tone="quiet" size="compact">
                            {shortcut.description}
                          </Text>
                          {conflict ? (
                            <Text
                              variant="span"
                              size="compact"
                              color={uiColor('warning-on-quiet')}
                              role="status"
                              class="keyboard-settings__conflict"
                            >
                              <LucideIcon inline size={12.8} icon={TriangleAlert} name="triangle-alert" /> Also used by
                              “{conflict.label}”
                            </Text>
                          ) : (
                            <></>
                          )}
                        </List>
                        <Row vAlign="middle" gap="2xs">
                          {capturing ? (
                            <button
                              type="button"
                              class="keyboard-settings__capture"
                              data-shortcut-capture={shortcut.id}
                              aria-label={`Recording new shortcut for ${shortcut.label}. Press a key combination, or Escape to cancel.`}
                            >
                              Press Keys…
                            </button>
                          ) : (
                            <kbd class="keyboard-settings__chord">{formatChord(chord, apple)}</kbd>
                          )}
                          {shortcut.editable ? (
                            capturing ? (
                              <wa-button
                                appearance="outlined"
                                size="small"
                                {...COMMANDS_AND_AI_ACTIONS.cancelShortcutCapture.attrs}
                                title="Cancel"
                              >
                                <LucideIcon size={14.4} icon={X} name="x" label={`Cancel editing ${shortcut.label}`} />
                              </wa-button>
                            ) : (
                              <>
                                <wa-button
                                  appearance="outlined"
                                  size="small"
                                  {...COMMANDS_AND_AI_ACTIONS.editShortcut.attrs}
                                  data-shortcut-id={shortcut.id}
                                  title="Change shortcut"
                                >
                                  <LucideIcon
                                    size={14.4}
                                    icon={Pencil}
                                    name="pencil"
                                    label={`Change shortcut for ${shortcut.label}`}
                                  />
                                </wa-button>
                                <wa-button
                                  appearance="outlined"
                                  size="small"
                                  {...COMMANDS_AND_AI_ACTIONS.resetShortcut.attrs}
                                  data-shortcut-id={shortcut.id}
                                  disabled={!overridden}
                                  title="Reset to default"
                                >
                                  <LucideIcon
                                    size={14.4}
                                    icon={RotateCcw}
                                    name="rotate-ccw"
                                    label={`Reset ${shortcut.label} to its default`}
                                  />
                                </wa-button>
                              </>
                            )
                          ) : (
                            <span title="Fixed system shortcut">
                              <Badge className="keyboard-settings__fixed" tone="neutral" appearance="quiet">
                                System
                              </Badge>
                            </span>
                          )}
                        </Row>
                      </Row>
                    </li>
                  );
                })}
              </ul>
            </List>
          );
        })}
      </List>
    </section>
  );
}
