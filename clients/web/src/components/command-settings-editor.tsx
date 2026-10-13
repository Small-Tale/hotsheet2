import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/textarea/textarea.js';
import '@kerfjs/ui/surface-scaffold.css';
import './command-settings-editor.css';

import { ChoiceChips } from '@kerfjs/ui/choice-chips';
import { foregroundColor, foregroundColorVar, px, uiColor } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { Select } from '@kerfjs/ui/select';
import { PopoverDialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { Bot, GripVertical, MoreHorizontal, Pencil, RotateCcw, Trash2 } from 'lucide';

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
        <LucideIcon size="s" icon={GripVertical} name="grip-vertical" color={uiColor('neutral-on-quiet')} />
      </span>
      <span
        class="command-settings-editor__row-icon"
        data-transparent={transparent ? 'true' : undefined}
        style={commandIconStyle(command)}
        aria-hidden="true"
      >
        <LucideIcon
          size={18}
          icon={icon.icon}
          name={icon.name}
          color={transparent ? uiColor('neutral-on-normal') : foregroundColorVar('--command-text-color')}
        />
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
    <List gap="2xs" className="command-settings-editor__ai-selection">
      {/* A stacked field: the ListHeader label matches the Web Awesome field labels above and below,
          and the PopupMenu's group (the cataloged menu-trigger composition, HS2-CSRJ9Y) gets the full
          row width so the label never truncates at phone width (HS2-XZ6Q5A). */}
      <ListHeader label="AI configuration" headingLevel={3} density="compact" inline />
      <Toolbar
        dividerSides=""
        leading={
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
      <Text flush tone="quiet" size="compact">
        Project Default follows this project's AI settings; choose an override only for this command.
      </Text>
    </List>
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
  // Web Awesome fields own their labels, hints, and control chrome; Kerf Grid pairs the label and
  // type and collapses to one column in a narrow dialog, and List stacks the full-width fields
  // (HS2-WNXRG3). Fields keep the `input` event autosave contract through `data-command-field`.
  return (
    <List gap="m" rootAttributes={{ 'data-command-id': command.id }}>
      <Grid minColumnWidth={px(224)} gap="m">
        <wa-input label="Button label" name="title" data-command-field required value={command.title} />
        <Select
          name="kind"
          label="Type"
          value={type}
          choices={(['program', 'shell', 'ai'] as const).map((value) => ({ value, label: TYPE_LABELS[value] }))}
        />
      </Grid>
      {type === 'program' && (
        <>
          <wa-input
            label="Program"
            name="program"
            data-command-field
            required
            value={command.program ?? ''}
            placeholder="npm"
          />
          <wa-textarea
            label="Arguments"
            name="args"
            data-command-field
            spellcheck="false"
            rows={3}
            resize="vertical"
            placeholder={'run\ntest'}
            hint="One exact argument per line."
            value={command.args?.join('\n') ?? ''}
          />
        </>
      )}
      {type === 'shell' && (
        <wa-textarea
          label="Shell command"
          name="command"
          data-command-field
          required
          spellcheck="false"
          rows={3}
          resize="vertical"
          placeholder="npm run test"
          hint="Runs in the project root; use cd within the command if needed."
          value={command.command ?? ''}
        />
      )}
      {type === 'ai' && (
        <>
          <wa-textarea
            label="Prompt"
            name="prompt"
            data-command-field
            required
            rows={3}
            resize="vertical"
            placeholder="Review the current changes"
            value={command.prompt ?? ''}
          />
          <AiCommandSelection command={command} tools={aiTools} defaults={aiDefaults} />
        </>
      )}
      <wa-input
        label="Confirmation message"
        name="confirmation"
        data-command-field
        value={command.confirmation ?? ''}
        placeholder="Optional confirmation before running"
      />
      {/* Kerf ChoiceChips: a wrapping radio set of color-dot pills, including the struck-through
          transparent swatch (KF-EJPYWD, HS2-EBY4E3). */}
      <div class="command-settings-editor__legend" id="command-color-label">
        Button color
      </div>
      <ChoiceChips
        name="color"
        labelledBy="command-color-label"
        action={COMMANDS_AND_AI_ACTIONS.selectCommandColor.value}
        size="compact"
        value={resolveCommandColor(command.color)}
        choices={COMMAND_CUSTOMIZATION_COLORS.map((option) => ({
          value: option.value as string,
          label: option.label,
          swatch: option.value === 'transparent' ? 'transparent' : foregroundColor(option.value),
        }))}
      />
      <fieldset class="command-settings-editor__icons">
        <legend class="command-settings-editor__legend">Button icon</legend>
        <LucideIconPicker
          value={command.icon}
          query={iconSearch}
          searchName="command-icon-search"
          selectAction="select-command-icon"
        />
      </fieldset>
    </List>
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
      {/* A Kerf ListHeader names the group; an empty group's delete is its logical-end action
          (HS2-7BBRJT, HS2-EBY4E3). `inline` drops its outer margin so the label sits on the
          settings text edge with the description and row grips (HS2-1711Z5). */}
      {group &&
        (empty ? (
          <ListHeader
            label={group}
            inline
            action={COMMANDS_AND_AI_ACTIONS.deleteCommandGroup.value}
            actionLabel={`Delete empty group ${group}`}
            actionIcon={<LucideIcon icon={Trash2} name="trash-2" />}
            triggerAttributes={{ 'data-group': group }}
          />
        ) : (
          <ListHeader label={group} inline />
        ))}
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
      {/* The workspace toolbar titles the page (HS2-QEQ1YB) and carries its Add group… and Add
          command… actions (HS2-YKB7HS). */}
      {/* Kerf Text item geometry puts the copy on the settings text edge shared with the group
          headers and row grips (HS2-1711Z5). */}
      <Text tone="quiet">
        Create the buttons shown in this project's sidebar. Drag to reorder or move between groups; changes save
        automatically.
      </Text>
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
        <EmptyState title="No custom commands yet." detail="Add a command to configure its label and action." />
      )}
      <footer>
        <span role="status">{message}</span>
      </footer>
      {/* Kerf's PopoverDialogSurface owns the native popover="auto" chrome, size, and backdrop (HS2-T1MVPV). */}
      <PopoverDialogSurface
        id={COMMAND_EDITOR_DIALOG_ID}
        label="Edit command"
        bodyLayout="fill"
        rootAttributes={{ 'data-command-editor-dialog': 'true' }}
      >
        {editing && editingIcon ? (
          <div class="command-settings-editor__dialog-content">
            <Toolbar
              dividerSides=""
              leading={
                <>
                  <ToolbarControlGroup single appearance="borderless">
                    <span
                      class="command-settings-editor__dialog-icon"
                      data-transparent={
                        resolveCommandColor(editing.color) === TRANSPARENT_CUSTOMIZATION_COLOR ? 'true' : undefined
                      }
                      style={commandIconStyle(editing)}
                    >
                      <LucideIcon
                        size="s"
                        icon={editingIcon.icon}
                        name={editingIcon.name}
                        color={
                          resolveCommandColor(editing.color) === TRANSPARENT_CUSTOMIZATION_COLOR
                            ? uiColor('neutral-on-normal')
                            : foregroundColorVar('--command-text-color')
                        }
                      />
                    </span>
                  </ToolbarControlGroup>
                  <ToolbarText
                    text="Edit command"
                    id="command-editor-title"
                    size="xlarge"
                    description="Changes save automatically."
                  />
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
            <div class="command-settings-editor__dialog-body">
              <CommandDetailFields
                command={editing}
                iconSearch={iconSearch}
                aiTools={aiTools}
                aiDefaults={aiDefaults}
              />
            </div>
          </div>
        ) : null}
      </PopoverDialogSurface>
    </div>
  );
}
