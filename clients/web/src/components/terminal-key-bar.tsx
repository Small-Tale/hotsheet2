import './terminal-key-bar.css';

import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ToolbarControlGroup, type ToolbarControlGroupProps } from '@kerfjs/ui/toolbar-control-group';
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, ClipboardPaste, Copy, Lock } from 'lucide';

import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';
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
    <button
      type="button"
      tabindex="-1"
      {...TERMINALS_ACTIONS.sendTerminalKey.attrs}
      data-key={keyName}
      aria-label={label}
    >
      {children}
    </button>
  );
}

function ModifierButton({ modifier, modifiers }: { modifier: TerminalModifier; modifiers: TerminalModifiers }) {
  return (
    <button
      type="button"
      tabindex="-1"
      {...TERMINALS_ACTIONS.toggleTerminalModifier.attrs}
      data-modifier={modifier}
      data-state={modifiers[modifier]}
      aria-pressed={String(modifiers[modifier] !== 'off')}
      aria-label={`${MODIFIER_LABEL[modifier]}${modifiers[modifier] === 'locked' ? ' (locked)' : ''}`}
    >
      {/* A locked modifier carries a lock glyph beyond the pressed chrome, so once and locked read differently. */}
      {modifiers[modifier] === 'locked' && <LucideIcon size="xs" icon={Lock} name="lock" />}
      {MODIFIER_LABEL[modifier]}
    </button>
  );
}

/**
 * One key group: a Kerf ToolbarControlGroup in an inline (forced-dark) Kerf FloatingToolbar, inside an
 * app-owned, non-shrinking slot of the bar's shared scroller, so the Fn slot can stay sticky while the
 * other groups scroll beneath it.
 */
function KeyGroup({
  label,
  content = 'text',
  children,
}: {
  label: string;
  content?: 'text' | 'icon' | 'mixed';
  children: ToolbarControlGroupProps['children'];
}) {
  return (
    <span class="terminal-key-bar__group" role="group" aria-label={label}>
      <FloatingToolbar placement="inline" label={label}>
        <ToolbarControlGroup shape="pill" size="compact" density="tight" content={content}>
          {children}
        </ToolbarControlGroup>
      </FloatingToolbar>
    </span>
  );
}

/**
 * Phone terminal accessory bar (HS2-CKS78M). The main row (Fn, Esc, Tab, sticky Ctrl/Alt, arrows)
 * fits a 390px phone without scrolling; Fn swaps in a scrolling row with Shift, the Copy/Paste clipboard
 * actions (HS2-FRB545), Home/End/PgUp/PgDn, and F1–F12, and stays at the leading edge so it is always reachable. Buttons are not focusable and
 * a capture-phase `pointerdown` guard keeps focus in the terminal, so the soft keyboard stays up.
 */
export function TerminalKeyBar({ modifiers, functionRow = false }: TerminalKeyBarProps) {
  return (
    <div
      class="terminal-key-bar"
      {...TERMINALS_TARGETS.terminalKeyBar.attrs}
      data-function-row={String(functionRow)}
      role="toolbar"
      aria-label="Terminal keys"
    >
      <KeyGroup label="Function row">
        <button
          type="button"
          tabindex="-1"
          {...TERMINALS_ACTIONS.toggleTerminalFunctionRow.attrs}
          aria-pressed={String(functionRow)}
          aria-label="Function and navigation keys"
        >
          Fn
        </button>
      </KeyGroup>
      <KeyGroup
        label="Escape, tab, and modifier keys"
        content={Object.values(modifiers).includes('locked') ? 'mixed' : 'text'}
      >
        <KeyButton keyName="Escape" label="Escape">
          Esc
        </KeyButton>
        <KeyButton keyName="Tab" label="Tab">
          Tab
        </KeyButton>
        <ModifierButton modifier="ctrl" modifiers={modifiers} />
        <ModifierButton modifier="alt" modifiers={modifiers} />
        {functionRow && <ModifierButton modifier="shift" modifiers={modifiers} />}
      </KeyGroup>
      {functionRow ? (
        <>
          <KeyGroup label="Clipboard" content="icon">
            <button
              type="button"
              tabindex="-1"
              {...TERMINALS_ACTIONS.copyTerminalText.attrs}
              aria-label="Copy terminal text"
              title="Copy terminal text"
            >
              <LucideIcon size="s" icon={Copy} name="copy" />
            </button>
            <button
              type="button"
              tabindex="-1"
              {...TERMINALS_ACTIONS.pasteTerminalText.attrs}
              aria-label="Paste"
              title="Paste"
            >
              <LucideIcon size="s" icon={ClipboardPaste} name="clipboard-paste" />
            </button>
          </KeyGroup>
          <KeyGroup label="Navigation keys">
            {NAVIGATION.map((item) => (
              <KeyButton keyName={item.key} label={item.label}>
                {item.legend}
              </KeyButton>
            ))}
          </KeyGroup>
          <KeyGroup label="Function keys">
            {FUNCTION_KEYS.map((key) => (
              <KeyButton keyName={key} label={key}>
                {key}
              </KeyButton>
            ))}
          </KeyGroup>
        </>
      ) : (
        <KeyGroup label="Arrow keys" content="icon">
          {ARROWS.map((arrow) => (
            <KeyButton keyName={arrow.key} label={arrow.label}>
              <LucideIcon icon={arrow.icon} name={arrow.name} />
            </KeyButton>
          ))}
        </KeyGroup>
      )}
    </div>
  );
}
