import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import './command-settings-editor.css';
import './native-popover-dialog.css';

import {LucideIcon} from '@kerfjs/ui/lucide-icon';
import {PanelHeader} from '@kerfjs/ui/panel-header';
import {FolderPlus,GripVertical,MoreHorizontal,Pencil,Plus,Trash2} from 'lucide';

import type {CommandDefinition} from '../api';
import {commandGroupSections} from '../command-order';
import {lucideCatalogVersion} from '../lucide-catalog';
import {resolveCommandIcon} from './command-icon';
import {COMMAND_CUSTOMIZATION_COLORS,customizationContrastColor,resolveCommandColor,TRANSPARENT_CUSTOMIZATION_COLOR} from './customization-palette';
import {LucideIconPicker} from './lucide-icon-picker';

/** DOM id of the native "Edit command" popover dialog, opened imperatively from a row's edit action. */
export const COMMAND_EDITOR_DIALOG_ID='command-editor-dialog';

export interface CommandSettingsEditorProps{
  commands:CommandDefinition[];
  /** Group names added via "Add group" that may still be empty; pins their display order at the end. */
  extraGroups?:readonly string[];
  /** The command whose details dialog is open, if any. */
  editingId?:string;
  /** Ids of the rows in the current multi-selection (highlighted; dragged together). */
  selectedIds?:readonly string[];
  /** The current icon-picker search query for the open dialog. */
  iconSearch?:string;
  message?:string;
}

const kind=(command:CommandDefinition)=>command.kind??'program';
const TYPE_LABELS:Record<'program'|'shell'|'ai',string>={program:'Program',shell:'Shell',ai:'AI'};

/** The inline style setting a command row/dialog icon tile to its color; undefined when transparent. */
function commandIconStyle(command:CommandDefinition){const color=resolveCommandColor(command.color);if(color===TRANSPARENT_CUSTOMIZATION_COLOR)return undefined;return `--command-color:${color};--command-text-color:${customizationContrastColor(color)}`}

function CommandRow({command,editing,selected}:{command:CommandDefinition;editing:boolean;selected:boolean}){
  const icon=resolveCommandIcon(command.icon),transparent=resolveCommandColor(command.color)===TRANSPARENT_CUSTOMIZATION_COLOR,label=command.title||'Untitled command';
  return <li class="command-settings-editor__row" data-command-id={command.id} data-editing={editing?'true':undefined} data-selected={selected?'true':undefined} aria-selected={selected?'true':undefined} draggable="true">
    <span class="command-settings-editor__row-grip" aria-hidden="true"><LucideIcon icon={GripVertical} name="grip-vertical"/></span>
    <span class="command-settings-editor__row-icon" data-transparent={transparent?'true':undefined} style={commandIconStyle(command)} aria-hidden="true"><LucideIcon icon={icon.icon} name={icon.name}/></span>
    <span class="command-settings-editor__row-text"><strong>{label}</strong><small>{TYPE_LABELS[kind(command)]}</small></span>
    <wa-dropdown class="command-settings-editor__row-menu" distance={4}>
      <button slot="trigger" type="button" class="command-settings-editor__row-menu-trigger" aria-label={`Actions for ${label}`} aria-haspopup="menu"><LucideIcon icon={MoreHorizontal} name="more-horizontal"/></button>
      <wa-dropdown-item data-action="edit-command-setting"><span slot="icon"><LucideIcon icon={Pencil} name="pencil"/></span>Edit</wa-dropdown-item>
      <wa-dropdown-item variant="danger" data-action="delete-command-setting"><span slot="icon"><LucideIcon icon={Trash2} name="trash-2"/></span>Delete</wa-dropdown-item>
    </wa-dropdown>
  </li>;
}

/** The editable detail form for one command, shown inside the popover dialog. */
function CommandDetailFields({command,iconSearch}:{command:CommandDefinition;iconSearch?:string}){
  const type=kind(command);
  return <div class="command-settings-editor__grid" data-command-id={command.id}>
    <label>Button label<input name="title" data-command-field required value={command.title}/></label>
    <label>Type<select name="kind" data-command-field><option value="program" selected={type==='program'}>Program</option><option value="shell" selected={type==='shell'}>Shell</option><option value="ai" selected={type==='ai'}>AI</option></select></label>
    {type==='program'&&<><label class="command-settings-editor__wide">Program<input name="program" data-command-field required value={command.program??''} placeholder="npm"/></label><label class="command-settings-editor__wide">Arguments<textarea name="args" data-command-field spellcheck="false" placeholder={'run\ntest'}>{command.args?.join('\n')??''}</textarea><small>One exact argument per line.</small></label></>}
    {type==='shell'&&<label class="command-settings-editor__wide">Shell command<textarea name="command" data-command-field required spellcheck="false" placeholder="npm run test">{command.command??''}</textarea><small>Runs in the project root; use cd within the command if needed.</small></label>}
    {type==='ai'&&<><label class="command-settings-editor__wide">Prompt<textarea name="prompt" data-command-field required placeholder="Review the current changes">{command.prompt??''}</textarea></label><label class="command-settings-editor__wide">AI tool<input name="tool" data-command-field value={command.tool??''} placeholder="Project default"/></label></>}
    <label class="command-settings-editor__wide">Confirmation message<input name="confirmation" data-command-field value={command.confirmation??''} placeholder="Optional confirmation before running"/></label>
    <fieldset class="command-settings-editor__wide command-settings-editor__swatches"><legend>Button color</legend>{COMMAND_CUSTOMIZATION_COLORS.map(option=><label class="command-settings-editor__swatch" data-transparent={option.value==='transparent'?'true':undefined} style={`--swatch:${option.value}`} title={option.label}><input type="radio" name="color" data-command-field value={option.value} checked={resolveCommandColor(command.color)===option.value}/><span aria-hidden="true"></span><span class="command-settings-editor__swatch-label">{option.label}</span></label>)}</fieldset>
    <fieldset class="command-settings-editor__wide command-settings-editor__icons"><legend>Button icon</legend><LucideIconPicker value={command.icon} query={iconSearch} searchName="command-icon-search" selectAction="select-command-icon"/></fieldset>
  </div>;
}

/** One display section: the ungrouped rows (blank group) or a named, droppable, deletable-when-empty group. */
function CommandGroup({group,commands,editingId,selectedIds}:{group:string;commands:CommandDefinition[];editingId?:string;selectedIds:ReadonlySet<string>}){
  const empty=commands.length===0;
  return <li class="command-settings-editor__section" data-command-group={group||undefined} data-ungrouped={group?undefined:'true'}>
    {group&&<div class="command-settings-editor__group-header"><span class="command-settings-editor__group-label">{group}</span>{empty&&<button type="button" class="command-settings-editor__group-delete" data-action="delete-command-group" data-group={group} aria-label={`Delete empty group ${group}`}><LucideIcon icon={Trash2} name="trash-2"/></button>}</div>}
    <ul class="command-settings-editor__group-items" data-command-group-drop={group}>
      {commands.map(command=><CommandRow command={command} editing={command.id===editingId} selected={selectedIds.has(command.id)}/>)}
      {empty&&<li class="command-settings-editor__group-empty" aria-hidden="true">Drag commands here</li>}
    </ul>
  </li>;
}

export function CommandSettingsEditor({commands,extraGroups=[],editingId,selectedIds=[],iconSearch='',message=''}:CommandSettingsEditorProps){
  const editing=editingId?commands.find(command=>command.id===editingId):undefined;
  const selection=new Set(selectedIds);
  const sections=commandGroupSections(commands,extraGroups);
  const editingIcon=editing?resolveCommandIcon(editing.icon):undefined;
  return <div class="command-settings-editor" data-component="command-settings-editor" data-icon-catalog={lucideCatalogVersion.value}>
    <header class="command-settings-editor__heading"><div><h2>Custom commands</h2><p>Create the buttons shown in this project's sidebar. Drag to reorder or move between groups; changes save automatically.</p></div><div class="command-settings-editor__heading-actions"><button type="button" data-action="add-command-group"><LucideIcon icon={FolderPlus} name="folder-plus"/> Add group</button><button type="button" class="command-settings-editor__add-command" data-action="add-command-setting"><LucideIcon icon={Plus} name="plus"/> Add command</button></div></header>
    {sections.length>0
      ?<ul class="command-settings-editor__list" aria-label="Custom commands" aria-multiselectable="true">{sections.map(section=><CommandGroup group={section.group} commands={section.commands} editingId={editingId} selectedIds={selection}/>)}</ul>
      :<div class="command-settings-editor__blank"><p>No custom commands yet.</p><p>Add a command to configure its label and action.</p></div>}
    <footer><span role="status">{message}</span></footer>
    <section popover="auto" id={COMMAND_EDITOR_DIALOG_ID} class="dialog-surface command-settings-editor__dialog" data-component="command-editor-dialog" role="dialog" aria-label="Edit command">
      {editing&&editingIcon?<>
        <PanelHeader title="Edit command" titleId="command-editor-title" summary="Changes save automatically." icon={<span class="command-settings-editor__dialog-icon" data-transparent={resolveCommandColor(editing.color)===TRANSPARENT_CUSTOMIZATION_COLOR?'true':undefined} style={commandIconStyle(editing)}><LucideIcon icon={editingIcon.icon} name={editingIcon.name}/></span>} actions={<button type="button" class="command-settings-editor__dialog-done" data-action="close-command-editor">Done</button>}/>
        <div class="command-settings-editor__dialog-body"><CommandDetailFields command={editing} iconSearch={iconSearch}/></div>
      </>:null}
    </section>
  </div>;
}
