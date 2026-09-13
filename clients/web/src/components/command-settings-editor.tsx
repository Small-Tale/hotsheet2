import './command-settings-editor.css';

import {LucideIcon} from '@kerfjs/ui/lucide-icon';
import {ChevronDown,ChevronUp,Plus,Trash2} from 'lucide';

import type {CommandDefinition} from '../api';

export interface CommandSettingsEditorProps{
  commands:CommandDefinition[];
  selectedId?:string;
  message?:string;
}

const kind=(command:CommandDefinition)=>command.kind??'program';

export function CommandSettingsEditor({commands,selectedId,message=''}:CommandSettingsEditorProps){
  const selected=commands.find(command=>command.id===selectedId)??(commands.length>0?commands[0]:undefined),selectedIndex=selected?commands.indexOf(selected):-1,type=selected&&kind(selected);
  return <div class="command-settings-editor" data-component="command-settings-editor">
    <header class="command-settings-editor__heading"><div><h2>Custom commands</h2><p>Create the buttons shown in this project's sidebar.</p></div><button type="button" data-action="add-command-setting"><LucideIcon icon={Plus} name="plus"/> Add command</button></header>
    <div class="command-settings-editor__layout">
      <aside aria-label="Custom commands">{commands.length>0?<nav>{commands.map(command=><button type="button" data-action="select-command-setting" data-command-id={command.id} aria-current={command===selected?'page':undefined}><span><strong>{command.title||'Untitled command'}</strong><small>{kind(command)}{command.group?` · ${command.group}`:''}</small></span></button>)}</nav>:<p>No custom commands yet.</p>}</aside>
      {selected?<section class="command-settings-editor__form" data-command-id={selected.id} aria-label="Edit custom command">
        <div class="command-settings-editor__form-heading"><strong>Edit command</strong><div><button type="button" data-action="move-command-setting" data-direction="up" aria-label="Move command up" disabled={selectedIndex===0}><LucideIcon icon={ChevronUp} name="chevron-up"/></button><button type="button" data-action="move-command-setting" data-direction="down" aria-label="Move command down" disabled={selectedIndex===commands.length-1}><LucideIcon icon={ChevronDown} name="chevron-down"/></button><button type="button" data-action="delete-command-setting" aria-label="Delete command"><LucideIcon icon={Trash2} name="trash-2"/></button></div></div>
        <div class="command-settings-editor__grid">
          <label>Button label<input name="title" data-command-field required value={selected.title}/></label>
          <label>Identifier<input name="id" data-command-field required value={selected.id}/><small>Stable key used by command history.</small></label>
          <label>Type<select name="kind" data-command-field><option value="program" selected={type==='program'}>Program</option><option value="shell" selected={type==='shell'}>Shell</option><option value="ai" selected={type==='ai'}>AI</option></select></label>
          <label>Group<input name="group" data-command-field value={selected.group??''} placeholder="Optional sidebar group"/></label>
          {type==='program'&&<><label class="command-settings-editor__wide">Program<input name="program" data-command-field required value={selected.program??''} placeholder="npm"/></label><label class="command-settings-editor__wide">Arguments<textarea name="args" data-command-field spellcheck="false" placeholder={'run\ntest'}>{selected.args?.join('\n')??''}</textarea><small>One exact argument per line.</small></label></>}
          {type==='shell'&&<label class="command-settings-editor__wide">Shell command<textarea name="command" data-command-field required spellcheck="false" placeholder="npm run test">{selected.command??''}</textarea></label>}
          {type==='ai'&&<><label class="command-settings-editor__wide">Prompt<textarea name="prompt" data-command-field required placeholder="Review the current changes">{selected.prompt??''}</textarea></label><label>AI tool<input name="tool" data-command-field value={selected.tool??''} placeholder="Project default"/></label></>}
          <label class="command-settings-editor__wide">Working directory<input name="cwd" data-command-field value={selected.cwd??''} placeholder="Project root"/></label>
          <label class="command-settings-editor__wide">Confirmation message<input name="confirmation" data-command-field value={selected.confirmation??''} placeholder="Optional confirmation before running"/></label>
        </div>
      </section>:<section class="command-settings-editor__blank"><p>Add a command to configure its label and action.</p></section>}
    </div>
    <footer><button type="button" data-action="save-command-settings">Save commands</button><span role="status">{message}</span></footer>
  </div>;
}
