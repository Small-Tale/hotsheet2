import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './terminal-visibility-dialog.css';

import { AppTab } from '@kerfjs/ui/app-tab';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { Eye, EyeOff, Globe, MessageSquare, Pencil, Plus, Sparkles, Terminal, Trash2 } from 'lucide';

import { contextPopupMenuAnchor } from '../context-menu-position';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import {
  DEFAULT_TERMINAL_VISIBILITY_GROUP_ID,
  TERMINAL_VISIBILITY_TYPES,
  terminalVisibilityItems,
  type TerminalVisibilityState,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import type { TerminalDashboardGroup } from './terminal-dashboard';

export interface TerminalVisibilityDialogProps {
  open: boolean;
  state: TerminalVisibilityState;
  scope: string;
  groups: TerminalDashboardGroup[];
  types?: readonly TerminalVisibilityType[];
  contextMenu?: { id: string; x: number; y: number };
}

/** Browser tiles are a separate (unshipped) kind, so the choice stays visible but unavailable. */
export const TERMINAL_VISIBILITY_TYPE_CHOICES: readonly SelectChoice<TerminalVisibilityType | 'browser'>[] = [
  { value: 'shell', label: 'Shell Terminals', icon: Terminal, iconName: 'terminal' },
  { value: 'ai', label: 'AI Terminals', icon: Sparkles, iconName: 'sparkles' },
  { value: 'chat', label: 'AI Chat', icon: MessageSquare, iconName: 'message-square' },
  {
    value: 'browser',
    label: 'Web Browsers',
    icon: Globe,
    iconName: 'globe',
    disabled: true,
    disabledReason: 'Web browser tiles are not available yet.',
  },
];

export interface TerminalVisibilityNamePrompt {
  mode: 'add' | 'rename';
  groupId?: string;
  value: string;
}

export function TerminalVisibilityNameDialog({ prompt }: { prompt?: TerminalVisibilityNamePrompt }) {
  const adding = prompt?.mode === 'add';
  return (
    <wa-dialog
      data-terminal-visibility-name-dialog
      label={adding ? 'Add Visibility Group' : 'Rename Visibility Group'}
      open={Boolean(prompt)}
      data-controlled-open={String(Boolean(prompt))}
    >
      <form class="terminal-visibility-name-dialog" {...TERMINALS_ACTIONS.submitTerminalVisibilityName.attrs}>
        <wa-input
          name="terminal-visibility-group-name"
          label="Group name"
          value={prompt?.value ?? ''}
          required
          autofocus
        />
        <footer>
          <wa-button appearance="plain" type="button" {...TERMINALS_ACTIONS.cancelTerminalVisibilityName.attrs}>
            Cancel
          </wa-button>
          <wa-button appearance="accent" type="submit">
            {adding ? 'Add' : 'Rename'}
          </wa-button>
        </footer>
      </form>
    </wa-dialog>
  );
}

export function TerminalVisibilityDialog({
  open,
  state,
  scope,
  groups,
  types = TERMINAL_VISIBILITY_TYPES,
  contextMenu,
}: TerminalVisibilityDialogProps) {
  const activeId = state.activeByScope[scope] ?? DEFAULT_TERMINAL_VISIBILITY_GROUP_ID;
  const active = state.groups.find((group) => group.id === activeId) ?? state.groups[0];
  const visibleGroups = terminalVisibilityItems(groups, scope, types);
  const hidden = new Set(active.hiddenKeys);

  return (
    <wa-dialog
      data-terminal-visibility-dialog
      label="Manage Workspace Visibility"
      aria-label="Manage Workspace Visibility"
      open={open}
      data-controlled-open={String(open)}
    >
      <section class="terminal-visibility-dialog">
        <div class="terminal-visibility-dialog__toolbar">
          <div class="terminal-visibility-dialog__tabs" role="tablist" aria-label="Terminal visibility groups">
            {state.groups.map((group) => (
              <AppTab
                id={group.id}
                name={group.name}
                selected={group.id === active.id}
                closable={false}
                className="terminal-visibility-dialog__tab"
                selectAction="select-terminal-visibility-tab"
                rootAttributes={{ 'data-visibility-group-id': group.id }}
              />
            ))}
            <button
              type="button"
              class="terminal-visibility-dialog__add"
              {...TERMINALS_ACTIONS.addTerminalVisibilityGroup.attrs}
              aria-label="Add visibility group"
              title="Add visibility group"
            >
              <LucideIcon icon={Plus} name="plus" size={16} />
            </button>
          </div>
        </div>
        <div class="terminal-visibility-dialog__filter">
          <Select
            name="terminal-visibility-types"
            label="Item types"
            placeholderText="No types selected"
            multiple
            value={types}
            choices={TERMINAL_VISIBILITY_TYPE_CHOICES}
            selectAllLabel="Select all"
            clearLabel="Clear"
          />
        </div>
        <div class="terminal-visibility-dialog__body">
          {visibleGroups.length === 0 ? (
            <p>No workspace items match the selected types.</p>
          ) : (
            visibleGroups.map((group) => (
              <section class="terminal-visibility-dialog__project" data-key={group.projectId}>
                <ListHeader label={group.projectName} />
                <div class="terminal-visibility-dialog__rows">
                  {group.items.map((item) => {
                    const { key, label } = item;
                    const visible = !hidden.has(key);
                    return (
                      <ListItem
                        action="toggle-terminal-visibility"
                        itemId={key}
                        accessibleLabel={`${visible ? 'Hide' : 'Show'} ${label}`}
                        state={visible ? 'visible' : 'hidden'}
                        icon={<LucideIcon icon={visible ? Eye : EyeOff} name={visible ? 'eye' : 'eye-off'} />}
                        label={label}
                        trailing={
                          <span class="terminal-visibility-dialog__row-state">{visible ? 'Visible' : 'Hidden'}</span>
                        }
                      />
                    );
                  })}
                </div>
              </section>
            ))
          )}
        </div>
        <footer class="terminal-visibility-dialog__footer">
          <wa-button
            appearance="plain"
            type="button"
            data-action="hide-all-terminals-in-group"
            disabled={visibleGroups.length === 0}
          >
            Hide listed
          </wa-button>
          <wa-button
            appearance="plain"
            type="button"
            data-action="show-all-terminals-in-group"
            disabled={visibleGroups.length === 0}
          >
            Show listed
          </wa-button>
        </footer>
        {contextMenu && contextMenu.id !== DEFAULT_TERMINAL_VISIBILITY_GROUP_ID && (
          // A context-mode Kerf PopupMenu anchored at the pointer (HS2-84751P); the app owns the open
          // state through `contextMenu` and reveals the rendered menu with `revealContextPopupMenu`.
          <div
            class="terminal-visibility-dialog__context-menu"
            role="menu"
            aria-label="Visibility group actions"
            data-context-group-id={contextMenu.id}
            {...contextPopupMenuAnchor(contextMenu.x, contextMenu.y)}
          >
            <PopupMenu
              context
              label="Visibility group actions"
              rootAttributes={{ 'data-context-menu': 'terminal-visibility-group' }}
              items={[
                {
                  label: 'Rename…',
                  action: 'rename-terminal-visibility-group',
                  icon: <LucideIcon icon={Pencil} name="pencil" />,
                },
                {
                  label: 'Delete',
                  action: 'remove-terminal-visibility-group',
                  tone: 'danger',
                  icon: <LucideIcon icon={Trash2} name="trash-2" />,
                },
              ]}
            />
          </div>
        )}
      </section>
    </wa-dialog>
  );
}
