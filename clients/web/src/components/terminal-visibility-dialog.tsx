import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './terminal-visibility-dialog.css';

import { Eye,EyeOff,Plus,Trash2 } from 'lucide';

import { DEFAULT_TERMINAL_VISIBILITY_GROUP_ID,type TerminalVisibilityState } from '../terminal-visibility';
import { LucideIcon } from './lucide-icon';
import { MenuItem } from './menu-item';
import type { TerminalDashboardGroup } from './terminal-dashboard';

export interface TerminalVisibilityDialogProps {
  open: boolean;
  state: TerminalVisibilityState;
  scope: string;
  groups: TerminalDashboardGroup[];
}

export function TerminalVisibilityDialog({ open, state, scope, groups }: TerminalVisibilityDialogProps) {
  const activeId = state.activeByScope[scope] ?? DEFAULT_TERMINAL_VISIBILITY_GROUP_ID;
  const active = state.groups.find(group => group.id === activeId) ?? state.groups[0];
  const projectId = scope.startsWith('project:') ? scope.slice('project:'.length) : undefined;
  const visibleGroups = projectId ? groups.filter(group => group.projectId === projectId) : groups;
  const hidden = new Set(active.hiddenKeys);

  return <wa-dialog data-terminal-visibility-dialog label="Show / Hide Terminals" aria-label="Show / Hide Terminals" open={open}>
    <section class="terminal-visibility-dialog">
      <div class="terminal-visibility-dialog__tabs" role="tablist" aria-label="Terminal visibility groups">{state.groups.map(group=><button type="button" role="tab" aria-selected={String(group.id===active.id)} data-action="select-terminal-visibility-tab" data-item-id={group.id}>{group.name}</button>)}<button type="button" class="terminal-visibility-dialog__add" data-action="add-terminal-visibility-group" aria-label="Add visibility group" title="Add visibility group"><LucideIcon icon={Plus} name="plus"/></button></div>
      <div class="terminal-visibility-dialog__group"><wa-input name="terminal-visibility-group-name" label="Group name" value={active.name} disabled={active.id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID}></wa-input><wa-button appearance="outlined" type="button" data-action="remove-terminal-visibility-group" disabled={active.id===DEFAULT_TERMINAL_VISIBILITY_GROUP_ID} aria-label="Delete visibility group" title="Delete visibility group"><LucideIcon icon={Trash2} name="trash-2"/> Delete</wa-button></div>
      <div class="terminal-visibility-dialog__body">{visibleGroups.length===0?<p>No terminals are available.</p>:visibleGroups.map(group=><section class="terminal-visibility-dialog__project" data-key={group.projectId}><h2>{group.projectName}</h2><div class="terminal-visibility-dialog__rows">{group.sessions.map(session=>{const key=`${session.projectId}:${session.id}`,visible=!hidden.has(key);return <MenuItem className="terminal-visibility-dialog__row" action="toggle-terminal-visibility" itemId={key} accessibleLabel={`${visible?'Hide':'Show'} ${session.title??session.id}`} state={visible?'visible':'hidden'} icon={<LucideIcon icon={visible?Eye:EyeOff} name={visible?'eye':'eye-off'}/>} label={session.title??session.id} trailing={<span class="terminal-visibility-dialog__row-state">{visible?'Visible':'Hidden'}</span>}/>})}</div></section>)}</div>
      <footer class="terminal-visibility-dialog__footer"><wa-button appearance="plain" type="button" data-action="show-all-terminals-in-group">Show all</wa-button><wa-button appearance="plain" type="button" data-action="hide-all-terminals-in-group">Hide all</wa-button></footer>
    </section>
  </wa-dialog>;
}
