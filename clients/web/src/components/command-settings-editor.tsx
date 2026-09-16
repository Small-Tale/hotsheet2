import './command-settings-editor.css';
import './native-popover-dialog.css';

import {LucideIcon} from '@kerfjs/ui/lucide-icon';
import {PanelHeader} from '@kerfjs/ui/panel-header';
import {ChevronDown,ChevronUp,Pencil,Plus,Trash2} from 'lucide';

import type {CommandDefinition} from '../api';
import {COMMAND_ICONS,isCommandNavigationIcon} from './command-navigation';
import {CUSTOMIZATION_COLORS,customizationContrastColor,resolveCustomizationColor} from './customization-palette';

/** DOM id of the native "Edit command" popover dialog, opened imperatively from a row's edit action. */
export const COMMAND_EDITOR_DIALOG_ID='command-editor-dialog';

export interface CommandSettingsEditorProps{
  commands:CommandDefinition[];
  /** The command whose details dialog is open, if any. */
  editingId?:string;
  message?:string;
}

const kind=(command:CommandDefinition)=>command.kind??'program';
const TYPE_LABELS:Record<'program'|'shell'|'ai',string>={program:'Program',shell:'Shell',ai:'AI'};

function commandIcon(command:CommandDefinition){
  const key=command.icon&&isCommandNavigationIcon(command.icon)?command.icon:COMMAND_ICONS[0].key;
  return COMMAND_ICONS.find(option=>option.key===key)??COMMAND_ICONS[0];
}

/** One WYSIWYG list row: colored icon, name, type, and inline edit/reorder/delete actions. */
function CommandRow({command,globalIndex,total,editing}:{command:CommandDefinition;globalIndex:number;total:number;editing:boolean}){
  const icon=commandIcon(command),color=resolveCustomizationColor(command.color),textColor=customizationContrastColor(color);
  return <li class="command-settings-editor__row" data-command-id={command.id} data-editing={editing?'true':undefined}>
    <span class="command-settings-editor__row-icon" style={`--command-color:${color};--command-text-color:${textColor}`} aria-hidden="true"><LucideIcon icon={icon.icon} name={icon.name}/></span>
    <span class="command-settings-editor__row-text"><strong>{command.title||'Untitled command'}</strong><small>{TYPE_LABELS[kind(command)]}</small></span>
    <span class="command-settings-editor__row-actions">
      <button type="button" data-action="edit-command-setting" aria-label={`Edit ${command.title||'command'}`}><LucideIcon icon={Pencil} name="pencil"/></button>
      <button type="button" data-action="move-command-setting" data-direction="up" aria-label={`Move ${command.title||'command'} up`} disabled={globalIndex===0}><LucideIcon icon={ChevronUp} name="chevron-up"/></button>
      <button type="button" data-action="move-command-setting" data-direction="down" aria-label={`Move ${command.title||'command'} down`} disabled={globalIndex===total-1}><LucideIcon icon={ChevronDown} name="chevron-down"/></button>
      <button type="button" class="command-settings-editor__row-delete" data-action="delete-command-setting" aria-label={`Delete ${command.title||'command'}`}><LucideIcon icon={Trash2} name="trash-2"/></button>
    </span>
  </li>;
}

/** The editable detail form for one command, shown inside the popover dialog. */
function CommandDetailFields({command}:{command:CommandDefinition}){
  const type=kind(command);
  return <div class="command-settings-editor__grid" data-command-id={command.id}>
    <label>Button label<input name="title" data-command-field required value={command.title}/></label>
    <label>Identifier<input name="id" data-command-field required value={command.id}/><small>Stable key used by command history.</small></label>
    <label>Type<select name="kind" data-command-field><option value="program" selected={type==='program'}>Program</option><option value="shell" selected={type==='shell'}>Shell</option><option value="ai" selected={type==='ai'}>AI</option></select></label>
    <label>Group<input name="group" data-command-field value={command.group??''} placeholder="Optional sidebar group"/></label>
    {type==='program'&&<><label class="command-settings-editor__wide">Program<input name="program" data-command-field required value={command.program??''} placeholder="npm"/></label><label class="command-settings-editor__wide">Arguments<textarea name="args" data-command-field spellcheck="false" placeholder={'run\ntest'}>{command.args?.join('\n')??''}</textarea><small>One exact argument per line.</small></label></>}
    {type==='shell'&&<label class="command-settings-editor__wide">Shell command<textarea name="command" data-command-field required spellcheck="false" placeholder="npm run test">{command.command??''}</textarea></label>}
    {type==='ai'&&<><label class="command-settings-editor__wide">Prompt<textarea name="prompt" data-command-field required placeholder="Review the current changes">{command.prompt??''}</textarea></label><label>AI tool<input name="tool" data-command-field value={command.tool??''} placeholder="Project default"/></label></>}
    <label class="command-settings-editor__wide">Working directory<input name="cwd" data-command-field value={command.cwd??''} placeholder="Project root"/></label>
    <label class="command-settings-editor__wide">Confirmation message<input name="confirmation" data-command-field value={command.confirmation??''} placeholder="Optional confirmation before running"/></label>
    <fieldset class="command-settings-editor__wide command-settings-editor__swatches"><legend>Button color</legend>{CUSTOMIZATION_COLORS.map(option=><label class="command-settings-editor__swatch" style={`--swatch:${option.value}`} title={option.label}><input type="radio" name="color" data-command-field value={option.value} checked={resolveCustomizationColor(command.color)===option.value}/><span aria-hidden="true"></span><span class="command-settings-editor__swatch-label">{option.label}</span></label>)}</fieldset>
    <fieldset class="command-settings-editor__wide command-settings-editor__icons"><legend>Button icon</legend>{COMMAND_ICONS.map(option=><label class="command-settings-editor__icon" title={option.key}><input type="radio" name="icon" data-command-field value={option.key} checked={command.icon===option.key}/><LucideIcon icon={option.icon} name={option.name}/></label>)}</fieldset>
  </div>;
}

export function CommandSettingsEditor({commands,editingId,message=''}:CommandSettingsEditorProps){
  const editing=editingId?commands.find(command=>command.id===editingId):undefined;
  const groups=commands.reduce<Map<string,CommandDefinition[]>>((result,command)=>{
    const group=command.group?.trim()||'';
    result.set(group,[...(result.get(group)??[]),command]);
    return result;
  },new Map());
  const editingIcon=editing?commandIcon(editing):undefined;
  return <div class="command-settings-editor" data-component="command-settings-editor">
    <header class="command-settings-editor__heading"><div><h2>Custom commands</h2><p>Create the buttons shown in this project's sidebar.</p></div><button type="button" data-action="add-command-setting"><LucideIcon icon={Plus} name="plus"/> Add command</button></header>
    {commands.length>0
      ?<ul class="command-settings-editor__list" aria-label="Custom commands">{[...groups].map(([group,items])=><li class="command-settings-editor__group" data-command-group={group||undefined}>{group&&<p class="command-settings-editor__group-label">{group}</p>}<ul class="command-settings-editor__group-items">{items.map(command=><CommandRow command={command} globalIndex={commands.indexOf(command)} total={commands.length} editing={command.id===editingId}/>)}</ul></li>)}</ul>
      :<div class="command-settings-editor__blank"><p>No custom commands yet.</p><p>Add a command to configure its label and action.</p></div>}
    <footer><button type="button" data-action="save-command-settings">Save commands</button><span role="status">{message}</span></footer>
    <section popover="auto" id={COMMAND_EDITOR_DIALOG_ID} class="dialog-surface command-settings-editor__dialog" data-component="command-editor-dialog" role="dialog" aria-label="Edit command">
      {editing&&editingIcon?<>
        <PanelHeader title="Edit command" titleId="command-editor-title" summary="Changes apply after you save commands." icon={<span class="command-settings-editor__dialog-icon" style={`--command-color:${resolveCustomizationColor(editing.color)};--command-text-color:${customizationContrastColor(resolveCustomizationColor(editing.color))}`}><LucideIcon icon={editingIcon.icon} name={editingIcon.name}/></span>}/>
        <div class="command-settings-editor__dialog-body"><CommandDetailFields command={editing}/></div>
        <footer class="command-settings-editor__dialog-footer"><button type="button" data-action="close-command-editor">Done</button></footer>
      </>:null}
    </section>
  </div>;
}
