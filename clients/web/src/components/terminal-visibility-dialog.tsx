import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './terminal-visibility-dialog.css';

import { Eye,EyeOff,Pencil,Plus,Trash2 } from 'lucide';

import { DEFAULT_TERMINAL_VISIBILITY_GROUP_ID,type TerminalVisibilityState } from '../terminal-visibility';
import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';
import type { TerminalDashboardGroup } from './terminal-dashboard';

export interface TerminalVisibilityDialogProps {
  open: boolean;
  state: TerminalVisibilityState;
  scope: string;
  groups: TerminalDashboardGroup[];
  contextMenu?:{id:string;x:number;y:number};
}

export interface TerminalVisibilityNamePrompt {mode:'add'|'rename';groupId?:string;value:string}

export function TerminalVisibilityNameDialog({prompt}:{prompt?:TerminalVisibilityNamePrompt}){
  const adding=prompt?.mode==='add';
  return <wa-dialog data-terminal-visibility-name-dialog label={adding?'Add Visibility Group':'Rename Visibility Group'} open={Boolean(prompt)}>
    <form class="terminal-visibility-name-dialog" data-action="submit-terminal-visibility-name"><wa-input name="terminal-visibility-group-name" label="Group name" value={prompt?.value??''} required autofocus></wa-input><footer><wa-button appearance="plain" type="button" data-action="cancel-terminal-visibility-name">Cancel</wa-button><wa-button appearance="accent" type="submit">{adding?'Add':'Rename'}</wa-button></footer></form>
  </wa-dialog>;
}

export function TerminalVisibilityDialog({ open, state, scope, groups,contextMenu }: TerminalVisibilityDialogProps) {
  const activeId = state.activeByScope[scope] ?? DEFAULT_TERMINAL_VISIBILITY_GROUP_ID;
  const active = state.groups.find(group => group.id === activeId) ?? state.groups[0];
  const projectId = scope.startsWith('project:') ? scope.slice('project:'.length) : undefined;
  const visibleGroups = projectId ? groups.filter(group => group.projectId === projectId) : groups;
  const hidden = new Set(active.hiddenKeys);

  return <wa-dialog data-terminal-visibility-dialog label="Show / Hide Terminals" aria-label="Show / Hide Terminals" open={open}>
    <section class="terminal-visibility-dialog">
      <div class="terminal-visibility-dialog__toolbar"><div class="terminal-visibility-dialog__tabs" role="tablist" aria-label="Terminal visibility groups">{state.groups.map(group=><button type="button" class="terminal-visibility-dialog__tab app-tab" role="tab" aria-selected={String(group.id===active.id)} data-selected={String(group.id===active.id)} data-action="select-terminal-visibility-tab" data-item-id={group.id} data-visibility-group-id={group.id}>{group.name}</button>)}<button type="button" class="terminal-visibility-dialog__add" data-action="add-terminal-visibility-group" aria-label="Add visibility group" title="Add visibility group"><LucideIcon icon={Plus} name="plus"/></button></div></div>
      <div class="terminal-visibility-dialog__body">{visibleGroups.length===0?<p>No terminals are available.</p>:visibleGroups.map(group=><section class="terminal-visibility-dialog__project" data-key={group.projectId}><MenuHeader label={group.projectName}/><div class="terminal-visibility-dialog__rows">{group.sessions.map(session=>{const key=`${session.projectId}:${session.id}`,visible=!hidden.has(key);return <MenuItem className="terminal-visibility-dialog__row" action="toggle-terminal-visibility" itemId={key} accessibleLabel={`${visible?'Hide':'Show'} ${session.title??session.id}`} state={visible?'visible':'hidden'} icon={<LucideIcon icon={visible?Eye:EyeOff} name={visible?'eye':'eye-off'}/>} label={session.title??session.id} trailing={<span class="terminal-visibility-dialog__row-state">{visible?'Visible':'Hidden'}</span>}/>})}</div></section>)}</div>
      <footer class="terminal-visibility-dialog__footer"><wa-button appearance="plain" type="button" data-action="hide-all-terminals-in-group">Hide all</wa-button><wa-button appearance="plain" type="button" data-action="show-all-terminals-in-group">Show all</wa-button></footer>
      {contextMenu&&contextMenu.id!==DEFAULT_TERMINAL_VISIBILITY_GROUP_ID&&<div class="terminal-visibility-dialog__context-menu" role="menu" aria-label="Visibility group actions" style={`left:${contextMenu.x}px;top:${contextMenu.y}px`} data-visibility-group-id={contextMenu.id}><wa-dropdown-item data-action="rename-terminal-visibility-group"><span slot="icon"><LucideIcon icon={Pencil} name="pencil"/></span>Rename…</wa-dropdown-item><wa-dropdown-item data-action="remove-terminal-visibility-group" variant="danger"><span slot="icon"><LucideIcon icon={Trash2} name="trash-2"/></span>Delete</wa-dropdown-item></div>}
    </section>
  </wa-dialog>;
}
