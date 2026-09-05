import './terminal-drawer.css';

import { Eye, Grid2X2, PanelBottomClose, Plus, SquareTerminal } from 'lucide';

import { AppTab } from './app-tab';
import { LucideIcon } from './lucide-icon';
import { TerminalDashboard,type TerminalDashboardSession,TerminalSession } from './terminal-dashboard';

export interface TerminalDrawerProps {projectId:string;projectName:string;sessions:TerminalDashboardSession[];width:number;height:number;fitAcross:number;fitHigh:number;selectedId:string;magnifiedKey?:string;hiddenKeys?:readonly string[];loading?:boolean;message?:string;maximized?:boolean}

export function TerminalDrawer({projectId,projectName,sessions,width,height,fitAcross,fitHigh,selectedId,magnifiedKey,hiddenKeys=[],loading=false,message='',maximized=false}:TerminalDrawerProps){
  const selected=sessions.some(session=>session.id===selectedId)?selectedId:'grid',selectedSession=sessions.find(session=>session.id===selected),hiddenCount=hiddenKeys.filter(key=>key.startsWith(`${projectId}:`)).length;
  return <section class="terminal-drawer" data-component="terminal-drawer" data-mode={selected==='grid'?'grid':'dedicated'} data-maximized={String(maximized)} data-terminal-drawer-measure="true" aria-label={`${projectName} terminal drawer`}>
    <header class="terminal-drawer__rail" data-action="toggle-terminal-drawer-maximize" title={`Double-click to ${maximized?'restore':'maximize'} terminal drawer`}>
      <div class="terminal-drawer__views" role="tablist" aria-label="Terminal drawer views"><button type="button" role="tab" aria-selected={String(selected==='grid')} data-action="select-drawer-terminal" data-terminal-id="grid" aria-label="Terminal grid"><LucideIcon icon={Grid2X2} name="grid-2x-2"/></button><div class="terminal-drawer__tabs">{sessions.map(session=><AppTab kind="terminal" id={session.id} name={session.title??session.id} selected={selected===session.id} leading={<LucideIcon icon={SquareTerminal} name="square-terminal"/>} trailing={session.busy?<i aria-label="Busy"></i>:undefined}/>)}</div></div>
      <button type="button" class="terminal-drawer__create" data-action="create-project-terminal" aria-label="New project terminal" title="New terminal"><LucideIcon icon={Plus} name="plus"/></button>
      <div class="terminal-drawer__actions">{hiddenCount>0&&<button type="button" data-action="show-hidden-terminals" aria-label={`Show ${hiddenCount} hidden project terminal${hiddenCount===1?'':'s'}`} title="Show hidden terminals"><LucideIcon icon={Eye} name="eye"/><span>{hiddenCount}</span></button>}<button type="button" data-action="toggle-terminal-drawer" aria-label="Hide terminal drawer" title="Hide terminal drawer"><LucideIcon icon={PanelBottomClose} name="panel-bottom-close"/></button></div>
    </header>
    <div class="terminal-drawer__content">{selectedSession?<TerminalSession session={selectedSession}/>:<TerminalDashboard groups={[{projectId,projectName,sessions}]} width={width} height={height} fitAcross={fitAcross} fitHigh={fitHigh} grouping="flow" magnifiedKey={magnifiedKey} hiddenKeys={hiddenKeys} loading={loading} message={message}/>}</div>
  </section>;
}
