import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import '@kerfjs/ui/select.css';
import './terminal-visibility-dialog.css';

import { AppTab } from '@kerfjs/ui/app-tab';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CheckCheck, Eye, EyeOff, Globe, MessageSquare, Pencil, Plus, Sparkles, Terminal, Trash2, X } from 'lucide';

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
      <form class="terminal-visibility-name-dialog" data-action="submit-terminal-visibility-name">
        <wa-input
          name="terminal-visibility-group-name"
          label="Group name"
          value={prompt?.value ?? ''}
          required
          autofocus
        ></wa-input>
        <footer>
          <wa-button appearance="plain" type="button" data-action="cancel-terminal-visibility-name">
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
              data-action="add-terminal-visibility-group"
              aria-label="Add visibility group"
              title="Add visibility group"
            >
              <LucideIcon icon={Plus} name="plus" />
            </button>
          </div>
        </div>
        <div class="terminal-visibility-dialog__filter" data-selected-types={types.join(',')}>
          <wa-select
            class="kui-select"
            data-terminal-type-filter
            data-morph-skip
            name="terminal-visibility-types"
            label="Item types"
            placeholder="No types selected"
            multiple
            max-options-visible={3}
          >
            <wa-option value="shell">
              <span slot="start" class="kui-select__icon">
                <LucideIcon icon={Terminal} name="terminal" />
              </span>
              Shell Terminals
            </wa-option>
            <wa-option value="ai">
              <span slot="start" class="kui-select__icon">
                <LucideIcon icon={Sparkles} name="sparkles" />
              </span>
              AI Terminals
            </wa-option>
            <wa-option value="chat">
              <span slot="start" class="kui-select__icon">
                <LucideIcon icon={MessageSquare} name="message-square" />
              </span>
              AI Chat
            </wa-option>
            <wa-option value="browser" disabled>
              <span slot="start" class="kui-select__icon">
                <LucideIcon icon={Globe} name="globe" />
              </span>
              Web Browsers
            </wa-option>
            <wa-divider></wa-divider>
            <wa-option value="select-all">
              <span slot="start" class="kui-select__icon">
                <LucideIcon icon={CheckCheck} name="check-check" />
              </span>
              Select All
            </wa-option>
            <wa-option value="deselect-all">
              <span slot="start" class="kui-select__icon">
                <LucideIcon icon={X} name="x" />
              </span>
              Deselect All
            </wa-option>
          </wa-select>
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
                        className="terminal-visibility-dialog__row"
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
          <div
            class="terminal-visibility-dialog__context-menu"
            role="menu"
            aria-label="Visibility group actions"
            style={`left:${contextMenu.x}px;top:${contextMenu.y}px`}
            data-visibility-group-id={contextMenu.id}
          >
            <wa-dropdown-item data-action="rename-terminal-visibility-group">
              <span slot="icon">
                <LucideIcon icon={Pencil} name="pencil" />
              </span>
              Rename…
            </wa-dropdown-item>
            <wa-dropdown-item data-action="remove-terminal-visibility-group" variant="danger">
              <span slot="icon">
                <LucideIcon icon={Trash2} name="trash-2" />
              </span>
              Delete
            </wa-dropdown-item>
          </div>
        )}
      </section>
    </wa-dialog>
  );
}
