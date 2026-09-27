import './terminal-key-bar.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp } from 'lucide';

import type { TerminalModifier, TerminalModifiers, TerminalSpecialKey } from '../terminal-keys';

export interface TerminalKeyBarProps {
  modifiers: TerminalModifiers;
  /** Show the F1–F12 / navigation row instead of the arrows (the Fn key toggles it). */
  functionRow?: boolean;
}

const MODIFIER_LABEL: Record<TerminalModifier, string> = { ctrl: 'Ctrl', alt: 'Alt', shift: 'Shift' };
const ARROWS = [
  { key: 'ArrowLeft', label: 'Left arrow', icon: ArrowLeft, name: 'arrow-left' },
  { key: 'ArrowUp', label: 'Up arrow', icon: ArrowUp, name: 'arrow-up' },
  { key: 'ArrowDown', label: 'Down arrow', icon: ArrowDown, name: 'arrow-down' },
  { key: 'ArrowRight', label: 'Right arrow', icon: ArrowRight, name: 'arrow-right' },
] as const;
const NAVIGATION: readonly { key: TerminalSpecialKey; legend: string; label: string }[] = [
  { key: 'Home', legend: 'Home', label: 'Home' },
  { key: 'End', legend: 'End', label: 'End' },
  { key: 'PageUp', legend: 'PgUp', label: 'Page up' },
  { key: 'PageDown', legend: 'PgDn', label: 'Page down' },
];
const FUNCTION_KEYS = Array.from({ length: 12 }, (_, index) => `F${index + 1}` as TerminalSpecialKey);

function KeyButton({ keyName, label, children }: { keyName: TerminalSpecialKey; label: string; children: unknown }) {
  return (
    <button type="button" tabindex="-1" data-action="send-terminal-key" data-key={keyName} aria-label={label}>
      {children}
    </button>
  );
}

function ModifierButton({ modifier, modifiers }: { modifier: TerminalModifier; modifiers: TerminalModifiers }) {
  return (
    <button
      type="button"
      tabindex="-1"
      data-action="toggle-terminal-modifier"
      data-modifier={modifier}
      data-state={modifiers[modifier]}
      aria-pressed={String(modifiers[modifier] !== 'off')}
      aria-label={`${MODIFIER_LABEL[modifier]}${modifiers[modifier] === 'locked' ? ' (locked)' : ''}`}
    >
      {MODIFIER_LABEL[modifier]}
    </button>
  );
}

/**
 * Phone terminal accessory bar (HS2-CKS78M). The main row (Fn, Esc, Tab, sticky Ctrl/Alt, arrows)
 * fits a 390px phone without scrolling; Fn swaps in a scrolling row with Shift, Home/End/PgUp/PgDn,
 * and F1–F12, and stays at the leading edge so it is always reachable. Buttons are not focusable and
 * a capture-phase `pointerdown` guard keeps focus in the terminal, so the soft keyboard stays up.
 */
export function TerminalKeyBar({ modifiers, functionRow = false }: TerminalKeyBarProps) {
  const group = { tone: 'dark', size: 'compact', density: 'tight' } as const;
  return (
    <div
      class="terminal-key-bar"
      data-component="terminal-key-bar"
      data-function-row={String(functionRow)}
      role="toolbar"
      aria-label="Terminal keys"
    >
      <ToolbarControlGroup label="Function row" {...group} content="text" buttonAppearance="push" single>
        <button
          type="button"
          tabindex="-1"
          data-action="toggle-terminal-function-row"
          aria-pressed={String(functionRow)}
          aria-label="Function and navigation keys"
        >
          Fn
        </button>
      </ToolbarControlGroup>
      <ToolbarControlGroup label="Escape, tab, and modifier keys" {...group} content="text" buttonAppearance="push">
        <KeyButton keyName="Escape" label="Escape">
          Esc
        </KeyButton>
        <KeyButton keyName="Tab" label="Tab">
          Tab
        </KeyButton>
        <ModifierButton modifier="ctrl" modifiers={modifiers} />
        <ModifierButton modifier="alt" modifiers={modifiers} />
        {functionRow && <ModifierButton modifier="shift" modifiers={modifiers} />}
      </ToolbarControlGroup>
      {functionRow ? (
        <>
          <ToolbarControlGroup label="Navigation keys" {...group} content="text">
            {NAVIGATION.map((item) => (
              <KeyButton keyName={item.key} label={item.label}>
                {item.legend}
              </KeyButton>
            ))}
          </ToolbarControlGroup>
          <ToolbarControlGroup label="Function keys" {...group} content="text">
            {FUNCTION_KEYS.map((key) => (
              <KeyButton keyName={key} label={key}>
                {key}
              </KeyButton>
            ))}
          </ToolbarControlGroup>
        </>
      ) : (
        <ToolbarControlGroup label="Arrow keys" {...group}>
          {ARROWS.map((arrow) => (
            <KeyButton keyName={arrow.key} label={arrow.label}>
              <LucideIcon icon={arrow.icon} name={arrow.name} />
            </KeyButton>
          ))}
        </ToolbarControlGroup>
      )}
    </div>
  );
}
