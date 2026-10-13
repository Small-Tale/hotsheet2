import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CommandDropTarget } from '../command-order';
import {
  COMMAND_FIELD_SELECTOR,
  COMMANDS_AND_AI_ACTIONS,
  COMMANDS_AND_AI_TARGETS,
} from '../interaction-attrs/commands-and-ai';
import { COMMAND_EDITOR_DIALOG_ID } from './command-settings-editor';

/** A command's AI menu choice: inherit the default, or pick a tool, model, or effort. */
export type CommandAiChoice = { kind: 'default' } | { kind: 'tool' | 'model' | 'effort'; value: string };

/**
 * State and side effects for the CommandSettingsEditor (HS2-RWY78R). Production passes the
 * project's commands controller; the UX demo passes a fixture command list, so both run the same
 * row selection, editing, drag reorder, field, icon, and AI-menu handlers.
 */
export interface CommandSettingsEditorBindings {
  /** Whether the editor's current command list owns `id` (a stale row after a project switch may not). */
  readonly ownsCommand: (id: string) => boolean;
  readonly openEditor: (id: string) => void;
  readonly closeEditor: () => void;
  /** The edit dialog closed by any route. */
  readonly onEditorClosed: () => void;
  readonly addCommand: () => void;
  readonly deleteCommand: (id: string) => void;
  readonly addGroup: () => void;
  readonly deleteGroup: (group: string) => void;
  /** The selected command ids. */
  readonly selection: () => readonly string[];
  /** Every command id in display order. */
  readonly orderedIds: () => readonly string[];
  readonly selectRow: (id: string, intent: { toggle?: boolean; range?: boolean }) => void;
  /** Make `id` the only selected command, as a single-row drag does. */
  readonly selectOnly: (id: string) => void;
  readonly reorder: (sourceIds: readonly string[], target: CommandDropTarget) => void;
  readonly updateField: (id: string, field: string, value: string) => void;
  readonly setIconSearch: (query: string) => void;
  readonly chooseAi: (id: string, choice: CommandAiChoice) => void;
  readonly openManualModel: (id: string) => void;
}

// Each command row is a Kerf ListActionRow with this hook class and its command id (HS2-7BBRJT).
const rowSelector = '.command-settings-editor__row';
/** The row's trailing overflow PopupMenu, which owns its own clicks. */
const rowMenuSelector = '[data-component="popup-menu"]';
const commandIdOf = (target: Element) => target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;

/** Wire the CommandSettingsEditor under `root`; drag state is private to this registration. */
export function wireCommandSettingsEditor(root: HTMLElement, bindings: CommandSettingsEditorBindings): () => void {
  const lifetime = createScope();
  let draggedIds: string[] = [];
  function clearDropIndicators() {
    // Kerf rows render `dropPosition`/`dragging` as these documented marks; the reorder wiring paints
    // them during a pointer drag, as Kerf's own wireListReorder does (KF-HWYE11).
    root
      .querySelectorAll<HTMLElement>(`${rowSelector}[data-drop-position]`)
      .forEach((element) => delete element.dataset.dropPosition);
    root
      .querySelectorAll<HTMLElement>('[data-command-drop-active]')
      .forEach((element) => delete element.dataset.commandDropActive);
  }
  function clearDrag() {
    draggedIds = [];
    root
      .querySelectorAll<HTMLElement>(`${rowSelector}[data-dragging]`)
      .forEach((element) => delete element.dataset.dragging);
    clearDropIndicators();
  }
  /** Where a drag over `over` would land: before/after another row, or into a group. */
  function dropTargetAt(drag: DragEvent): { target: CommandDropTarget; element: HTMLElement } | undefined {
    const over = drag.target as Element,
      row = over.closest<HTMLElement>('[data-command-id]');
    if (row?.dataset.commandId && !draggedIds.includes(row.dataset.commandId)) {
      const bounds = row.getBoundingClientRect();
      return {
        element: row,
        target: {
          kind: 'row',
          id: row.dataset.commandId,
          position: drag.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after',
        },
      };
    }
    const container = over.closest<HTMLElement>('[data-command-group-drop]');
    return container
      ? { element: container, target: { kind: 'group', group: container.dataset.commandGroupDrop ?? '' } }
      : undefined;
  }
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.editCommandSetting.selector, (_event, target) => {
      const id = commandIdOf(target);
      // Right after a project switch the previous project's row can still be in the DOM; a command id
      // the selected project does not own would open an empty Edit command dialog (HS2-9ZWR5D).
      if (id && bindings.ownsCommand(id)) bindings.openEditor(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.addCommandSetting.selector, () => {
      bindings.addCommand();
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.closeCommandEditor.selector, () => {
      bindings.closeEditor();
    }),
  );
  lifetime.add(
    delegateCapture(
      root,
      'toggle',
      `#${COMMAND_EDITOR_DIALOG_ID}`,
      (event) => {
        if ((event as ToggleEvent).newState === 'closed') bindings.onEditorClosed();
      },
      { match: 'direct' },
    ),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.deleteCommandSetting.selector, (_event, target) => {
      const id = commandIdOf(target);
      if (id) bindings.deleteCommand(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.addCommandGroup.selector, () => {
      bindings.addGroup();
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.deleteCommandGroup.selector, (_event, target) => {
      const group = target.closest<HTMLElement>('[data-group]')?.dataset.group;
      if (group) bindings.deleteGroup(group);
    }),
  );
  lifetime.add(
    delegate(root, 'dblclick', rowSelector, (event, target) => {
      if ((event.target as Element).closest(rowMenuSelector)) return;
      const id = (target as HTMLElement).dataset.commandId;
      if (id) bindings.openEditor(id);
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', rowSelector, (event, target) => {
      // Right-click opens the row's PopupMenu (a wa-dropdown root) in place of a separate context menu.
      const menu = target.querySelector<HTMLElement & { open: boolean }>(rowMenuSelector);
      if (!menu) return;
      event.preventDefault();
      menu.open = true;
    }),
  );
  lifetime.add(
    delegate(root, 'click', rowSelector, (event, target) => {
      if ((event.target as Element).closest(rowMenuSelector)) return;
      const id = (target as HTMLElement).dataset.commandId;
      if (!id) return;
      const mouse = event as MouseEvent;
      bindings.selectRow(id, { toggle: mouse.metaKey || mouse.ctrlKey, range: mouse.shiftKey });
    }),
  );
  lifetime.add(
    delegate(root, 'dragstart', rowSelector, (event, target) => {
      const id = (target as HTMLElement).dataset.commandId;
      if (!id) return;
      const selection = bindings.selection();
      draggedIds =
        selection.length > 1 && selection.includes(id)
          ? bindings.orderedIds().filter((commandId) => selection.includes(commandId))
          : [id];
      if (draggedIds.length <= 1) bindings.selectOnly(id);
      root.querySelectorAll<HTMLElement>(rowSelector).forEach((row) => {
        if (draggedIds.includes(row.dataset.commandId ?? '')) row.dataset.dragging = 'true';
      });
      const transfer = (event as DragEvent).dataTransfer;
      if (transfer) {
        transfer.effectAllowed = 'move';
        transfer.setData('text/plain', draggedIds.join(','));
      }
    }),
  );
  lifetime.add(
    delegate(root, 'dragover', '.command-settings-editor__list', (event) => {
      if (!draggedIds.length) return;
      const drag = event as DragEvent,
        drop = dropTargetAt(drag);
      clearDropIndicators();
      if (!drop) return;
      drag.preventDefault();
      if (drop.target.kind === 'row') drop.element.dataset.dropPosition = drop.target.position;
      else drop.element.dataset.commandDropActive = 'true';
      if (drag.dataTransfer) drag.dataTransfer.dropEffect = 'move';
    }),
  );
  lifetime.add(
    delegate(root, 'drop', '.command-settings-editor__list', (event) => {
      const sources = draggedIds;
      if (!sources.length) {
        clearDrag();
        return;
      }
      const drag = event as DragEvent;
      drag.preventDefault();
      const drop = dropTargetAt(drag);
      clearDrag();
      if (drop) bindings.reorder(sources, drop.target);
    }),
  );
  lifetime.add(delegate(root, 'dragend', rowSelector, clearDrag));
  lifetime.add(
    delegate(root, 'input', COMMAND_FIELD_SELECTOR, (_event, target) => {
      const field = (target as HTMLInputElement).name,
        id = commandIdOf(target);
      if (id && field) bindings.updateField(id, field, (target as HTMLInputElement).value);
    }),
  );
  lifetime.add(
    delegate(root, 'input', COMMANDS_AND_AI_TARGETS.commandIconSearchField.selector, (_event, target) => {
      bindings.setIconSearch((target as HTMLInputElement).value);
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.selectCommandIcon.selector, (_event, target) => {
      const id = commandIdOf(target),
        name = (target as HTMLElement).dataset.iconName;
      if (id && name) bindings.updateField(id, 'icon', name);
    }),
  );
  const aiChoices = [
    [COMMANDS_AND_AI_ACTIONS.selectCommandAiTool.selector, 'tool'],
    [COMMANDS_AND_AI_ACTIONS.selectCommandAiModel.selector, 'model'],
    [COMMANDS_AND_AI_ACTIONS.selectCommandAiEffort.selector, 'effort'],
  ] as const;
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.selectCommandAiDefault.selector, (_event, target) => {
      const id = commandIdOf(target);
      if (id) bindings.chooseAi(id, { kind: 'default' });
    }),
  );
  for (const [selector, kind] of aiChoices)
    lifetime.add(
      delegate(root, 'click', selector, (_event, target) => {
        const id = commandIdOf(target),
          value = (target as HTMLElement).dataset.value;
        if (id && value) bindings.chooseAi(id, { kind, value });
      }),
    );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.openCommandManualModel.selector, (_event, target) => {
      const id = commandIdOf(target);
      if (id) bindings.openManualModel(id);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
