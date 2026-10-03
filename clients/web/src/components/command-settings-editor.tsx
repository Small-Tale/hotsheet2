import './heading.css';
import './command-settings-editor.css';
import './native-popover-dialog.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { Bot, FolderPlus, GripVertical, MoreHorizontal, Pencil, Plus, RotateCcw, Trash2 } from 'lucide';

import type { AiToolDefaults, AiToolDescriptor, CommandDefinition } from '../api';
import { commandGroupSections } from '../command-order';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { lucideCatalogVersion } from '../lucide-catalog';
import { resolveCommandIcon } from './command-icon';
import {
  COMMAND_CUSTOMIZATION_COLORS,
  customizationContrastColor,
  resolveCommandColor,
  TRANSPARENT_CUSTOMIZATION_COLOR,
} from './customization-palette';
import { LucideIconPicker } from './lucide-icon-picker';
import { providerModelEffortEntries } from './provider-model-effort-menu';

/** DOM id of the native "Edit command" popover dialog, opened imperatively from a row's edit action. */
export const COMMAND_EDITOR_DIALOG_ID = 'command-editor-dialog';

export interface CommandSettingsEditorProps {
  commands: CommandDefinition[];
  /** Group names added via "Add group" that may still be empty; pins their display order at the end. */
  extraGroups?: readonly string[];
  /** The command whose details dialog is open, if any. */
  editingId?: string;
  /** Ids of the rows in the current multi-selection (highlighted; dragged together). */
  selectedIds?: readonly string[];
  /** The current icon-picker search query for the open dialog. */
  iconSearch?: string;
  message?: string;
  aiTools?: readonly AiToolDescriptor[];
  aiDefaults?: AiToolDefaults;
}

const kind = (command: CommandDefinition) => command.kind ?? 'program';
const TYPE_LABELS: Record<'program' | 'shell' | 'ai', string> = { program: 'Program', shell: 'Shell', ai: 'AI' };

/** The inline style setting a command row/dialog icon tile to its color; undefined when transparent. */
function commandIconStyle(command: CommandDefinition) {
  const color = resolveCommandColor(command.color);
  if (color === TRANSPARENT_CUSTOMIZATION_COLOR) return undefined;
  return `--command-color:${color};--command-text-color:${customizationContrastColor(color)}`;
}

function CommandRow({
  command,
  editing,
  selected,
}: {
  command: CommandDefinition;
  editing: boolean;
  selected: boolean;
}) {
  const icon = resolveCommandIcon(command.icon),
    transparent = resolveCommandColor(command.color) === TRANSPARENT_CUSTOMIZATION_COLOR,
    label = command.title || 'Untitled command';
  return (
    <li
      class="command-settings-editor__row"
      data-command-id={command.id}
      data-editing={editing ? 'true' : undefined}
      data-selected={selected ? 'true' : undefined}
      aria-selected={selected ? 'true' : undefined}
      draggable="true"
    >
      <span class="command-settings-editor__row-grip" aria-hidden="true">
        <LucideIcon size="s" icon={GripVertical} name="grip-vertical" />
      </span>
      <span
        class="command-settings-editor__row-icon"
        data-transparent={transparent ? 'true' : undefined}
        style={commandIconStyle(command)}
        aria-hidden="true"
      >
        <LucideIcon size={18} icon={icon.icon} name={icon.name} />
      </span>
      <span class="command-settings-editor__row-text">
        <strong>{label}</strong>
        <small>{TYPE_LABELS[kind(command)]}</small>
      </span>
      <span class="command-settings-editor__row-menu">
        <PopupMenu
          label={`Actions for ${label}`}
          icon={<LucideIcon icon={MoreHorizontal} name="more-horizontal" />}
          caret={false}
          placement="bottom-end"
          items={[
            { label: 'Edit', action: 'edit-command-setting', icon: <LucideIcon icon={Pencil} name="pencil" /> },
            {
              label: 'Delete',
              action: 'delete-command-setting',
              tone: 'danger',
              icon: <LucideIcon icon={Trash2} name="trash-2" />,
            },
          ]}
        />
      </span>
    </li>
  );
}

/** The editable detail form for one command, shown inside the popover dialog. */
function AiCommandSelection({
  command,
  tools,
  defaults,
}: {
  command: CommandDefinition;
  tools: readonly AiToolDescriptor[];
  defaults: AiToolDefaults;
}) {
  const overridden = Boolean(command.tool || command.model || command.effort),
    toolId = command.tool ?? defaults.tool,
    active = tools.find((tool) => tool.id === toolId) ?? tools.at(0),
    modelId =
      command.model ??
      (!command.tool && toolId === defaults.tool ? defaults.model : undefined) ??
      active?.default_model ??
      active?.models.at(0)?.id ??
      '',
    model = active?.models.find((item) => item.id === modelId),
    customModel = modelId && !model ? modelId : undefined,
    efforts = model?.effort_levels ?? [],
    effort =
      command.effort ??
      (!command.tool && !command.model && modelId === defaults.model ? defaults.effort : undefined) ??
      active?.default_effort ??
      efforts.at(0),
    summary = overridden
      ? [active?.display_name ?? toolId, model?.label ?? modelId, effort].filter(Boolean).join(' · ')
      : 'Project Default';
  return (
    <div class="command-settings-editor__wide command-settings-editor__ai-selection">
      {/* A settings row: the field label is the toolbar's identity and the PopupMenu's group is its
          trailing control, the cataloged composition for a menu trigger (HS2-CSRJ9Y). */}
      <Toolbar
        dividerSides=""
        leading={<ToolbarText text="AI configuration" />}
        trailing={
          <ToolbarControlGroup single nestedDropdown>
            <PopupMenu
              text={summary}
              icon={<LucideIcon icon={Bot} name="bot" />}
              placement="bottom-start"
              rootAttributes={{ 'data-command-ai-menu': 'true' }}
              items={[
                {
                  label: 'Project Default',
                  action: 'select-command-ai-default',
                  checked: !overridden,
                  icon: <LucideIcon icon={RotateCcw} name="rotate-ccw" />,
                },
                ...(active
                  ? [
                      { kind: 'divider' } as const,
                      ...providerModelEffortEntries({
                        actions: {
                          provider: 'select-command-ai-tool',
                          model: 'select-command-ai-model',
                          effort: 'select-command-ai-effort',
                          manualModel: 'open-command-manual-model',
                        },
                        providers: {
                          choices: tools.map((tool) => ({ id: tool.id, label: tool.display_name })),
                          currentId: active.id,
                          currentLabel: active.display_name,
                        },
                        model: {
                          choices: active.models.map((item) => ({ id: item.id, label: item.label })),
                          currentId: model?.id,
                          currentLabel: model?.label ?? (modelId || 'Provider default'),
                          customModel,
                        },
                        effort: { efforts, current: effort },
                      }),
                    ]
                  : []),
              ]}
            />
          </ToolbarControlGroup>
        }
      />
      <small class="command-settings-editor__hint">
        Project Default follows this project's AI settings; choose an override only for this command.
      </small>
    </div>
  );
}

function CommandDetailFields({
  command,
  iconSearch,
  aiTools,
  aiDefaults,
}: {
  command: CommandDefinition;
  iconSearch?: string;
  aiTools: readonly AiToolDescriptor[];
  aiDefaults: AiToolDefaults;
}) {
  const type = kind(command);
  return (
    <div class="command-settings-editor__grid" data-command-id={command.id}>
      <label class="command-settings-editor__field">
        Button label
        <input
          class="command-settings-editor__control"
          name="title"
          data-command-field
          required
          value={command.title}
        />
      </label>
      <label class="command-settings-editor__field">
        Type
        <select class="command-settings-editor__control" name="kind" data-command-field>
          <option value="program" selected={type === 'program'}>
            Program
          </option>
          <option value="shell" selected={type === 'shell'}>
            Shell
          </option>
          <option value="ai" selected={type === 'ai'}>
            AI
          </option>
        </select>
      </label>
      {type === 'program' && (
        <>
          <label class="command-settings-editor__field command-settings-editor__wide">
            Program
            <input
              class="command-settings-editor__control"
              name="program"
              data-command-field
              required
              value={command.program ?? ''}
              placeholder="npm"
            />
          </label>
          <label class="command-settings-editor__field command-settings-editor__wide">
            Arguments
            <textarea
              class="command-settings-editor__control"
              name="args"
              data-command-field
              spellcheck="false"
              placeholder={'run\ntest'}
            >
              {command.args?.join('\n') ?? ''}
            </textarea>
            <small class="command-settings-editor__hint">One exact argument per line.</small>
          </label>
        </>
      )}
      {type === 'shell' && (
        <label class="command-settings-editor__field command-settings-editor__wide">
          Shell command
          <textarea
            class="command-settings-editor__control"
            name="command"
            data-command-field
            required
            spellcheck="false"
            placeholder="npm run test"
          >
            {command.command ?? ''}
          </textarea>
          <small class="command-settings-editor__hint">
            Runs in the project root; use cd within the command if needed.
          </small>
        </label>
      )}
      {type === 'ai' && (
        <>
          <label class="command-settings-editor__field command-settings-editor__wide">
            Prompt
            <textarea
              class="command-settings-editor__control"
              name="prompt"
              data-command-field
              required
              placeholder="Review the current changes"
            >
              {command.prompt ?? ''}
            </textarea>
          </label>
          <AiCommandSelection command={command} tools={aiTools} defaults={aiDefaults} />
        </>
      )}
      <label class="command-settings-editor__field command-settings-editor__wide">
        Confirmation message
        <input
          class="command-settings-editor__control"
          name="confirmation"
          data-command-field
          value={command.confirmation ?? ''}
          placeholder="Optional confirmation before running"
        />
      </label>
      <fieldset class="command-settings-editor__wide command-settings-editor__swatches">
        <legend class="command-settings-editor__legend">Button color</legend>
        {COMMAND_CUSTOMIZATION_COLORS.map((option) => (
          <label
            class="command-settings-editor__swatch"
            data-transparent={option.value === 'transparent' ? 'true' : undefined}
            style={`--swatch:${option.value}`}
            title={option.label}
          >
            <input
              type="radio"
              name="color"
              data-command-field
              value={option.value}
              checked={resolveCommandColor(command.color) === option.value}
            />
            <span aria-hidden="true"></span>
            <span class="command-settings-editor__swatch-label">{option.label}</span>
          </label>
        ))}
      </fieldset>
      <fieldset class="command-settings-editor__wide command-settings-editor__icons">
        <legend class="command-settings-editor__legend">Button icon</legend>
        <LucideIconPicker
          value={command.icon}
          query={iconSearch}
          searchName="command-icon-search"
          selectAction="select-command-icon"
        />
      </fieldset>
    </div>
  );
}

/** One display section: the ungrouped rows (blank group) or a named, droppable, deletable-when-empty group. */
function CommandGroup({
  group,
  commands,
  editingId,
  selectedIds,
}: {
  group: string;
  commands: CommandDefinition[];
  editingId?: string;
  selectedIds: ReadonlySet<string>;
}) {
  const empty = commands.length === 0;
  return (
    <li
      class="command-settings-editor__section"
      data-command-group={group || undefined}
      data-ungrouped={group ? undefined : 'true'}
    >
      {group && (
        <div class="command-settings-editor__group-header">
          <span class="command-settings-editor__group-label">{group}</span>
          {empty && (
            <button
              type="button"
              class="command-settings-editor__button command-settings-editor__group-delete"
              {...COMMANDS_AND_AI_ACTIONS.deleteCommandGroup.attrs}
              data-group={group}
              aria-label={`Delete empty group ${group}`}
            >
              <LucideIcon size={14} icon={Trash2} name="trash-2" />
            </button>
          )}
        </div>
      )}
      <ul class="command-settings-editor__group-items" data-command-group-drop={group}>
        {commands.map((command) => (
          <CommandRow command={command} editing={command.id === editingId} selected={selectedIds.has(command.id)} />
        ))}
        {empty && (
          <li class="command-settings-editor__group-empty" aria-hidden="true">
            Drag commands here
          </li>
        )}
      </ul>
    </li>
  );
}

export function CommandSettingsEditor({
  commands,
  extraGroups = [],
  editingId,
  selectedIds = [],
  iconSearch = '',
  message = '',
  aiTools = [],
  aiDefaults = { tool: 'codex' },
}: CommandSettingsEditorProps) {
  const editing = editingId ? commands.find((command) => command.id === editingId) : undefined;
  const selection = new Set(selectedIds);
  const sections = commandGroupSections(commands, extraGroups);
  const editingIcon = editing ? resolveCommandIcon(editing.icon) : undefined;
  return (
    <div
      class="command-settings-editor"
      data-component="command-settings-editor"
      data-icon-catalog={lucideCatalogVersion.value}
    >
      <header class="command-settings-editor__heading">
        <div>
          <h2>Custom commands</h2>
          <p>
            Create the buttons shown in this project's sidebar. Drag to reorder or move between groups; changes save
            automatically.
          </p>
        </div>
        <div class="command-settings-editor__heading-actions">
          <button
            type="button"
            class="command-settings-editor__button"
            {...COMMANDS_AND_AI_ACTIONS.addCommandGroup.attrs}
          >
            <LucideIcon size="s" icon={FolderPlus} name="folder-plus" /> Add group
          </button>
          <button
            type="button"
            class="command-settings-editor__button command-settings-editor__add-command"
            {...COMMANDS_AND_AI_ACTIONS.addCommandSetting.attrs}
          >
            <LucideIcon size="s" icon={Plus} name="plus" /> Add command
          </button>
        </div>
      </header>
      {sections.length > 0 ? (
        <ul class="command-settings-editor__list" aria-label="Custom commands" aria-multiselectable="true">
          {sections.map((section) => (
            <CommandGroup
              group={section.group}
              commands={section.commands}
              editingId={editingId}
              selectedIds={selection}
            />
          ))}
        </ul>
      ) : (
        <div class="command-settings-editor__blank">
          <p>No custom commands yet.</p>
          <p>Add a command to configure its label and action.</p>
        </div>
      )}
      <footer>
        <span role="status">{message}</span>
      </footer>
      <section
        popover="auto"
        id={COMMAND_EDITOR_DIALOG_ID}
        class="dialog-surface command-settings-editor__dialog"
        data-component="command-editor-dialog"
        role="dialog"
        aria-label="Edit command"
      >
        {editing && editingIcon ? (
          <>
            <div class="app-heading" data-component="heading" data-has-icon="true">
              <Toolbar
                dividerSides=""
                leading={
                  <>
                    <ToolbarControlGroup single className="app-heading__icon">
                      <span
                        class="command-settings-editor__dialog-icon"
                        data-transparent={
                          resolveCommandColor(editing.color) === TRANSPARENT_CUSTOMIZATION_COLOR ? 'true' : undefined
                        }
                        style={commandIconStyle(editing)}
                      >
                        <LucideIcon size="s" icon={editingIcon.icon} name={editingIcon.name} />
                      </span>
                    </ToolbarControlGroup>
                    <ToolbarText text="Edit command" id="command-editor-title" size="xlarge" />
                  </>
                }
                trailing={
                  <ToolbarControlGroup label="Command actions" appearance="borderless" single>
                    <button type="button" {...COMMANDS_AND_AI_ACTIONS.closeCommandEditor.attrs}>
                      Done
                    </button>
                  </ToolbarControlGroup>
                }
              />
              <p class="app-heading__summary">Changes save automatically.</p>
            </div>
            <div class="command-settings-editor__dialog-body">
              <CommandDetailFields
                command={editing}
                iconSearch={iconSearch}
                aiTools={aiTools}
                aiDefaults={aiDefaults}
              />
            </div>
          </>
        ) : null}
      </section>
    </div>
  );
}
