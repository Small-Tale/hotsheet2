import './terminal-drawer.css';

import { Eye, Grid2X2, PanelBottomClose, Plus, SquareTerminal } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { TerminalDashboard,type TerminalDashboardSession } from './terminal-dashboard';

export interface TerminalDrawerProps {projectId:string;projectName:string;sessions:TerminalDashboardSession[];width:number;height:number;fitAcross:number;fitHigh:number;selectedId:string;magnifiedKey?:string;hiddenKeys?:readonly string[];loading?:boolean;message?:string}

export function TerminalDrawer({projectId,projectName,sessions,width,height,fitAcross,fitHigh,selectedId,magnifiedKey,hiddenKeys=[],loading=false,message=''}:TerminalDrawerProps){
  const selected=sessions.some(session=>session.id===selectedId)?selectedId:'grid',shown=selected==='grid'?sessions:sessions.filter(session=>session.id===selected),hiddenCount=hiddenKeys.filter(key=>key.startsWith(`${projectId}:`)).length;
  return <section class="terminal-drawer" data-component="terminal-drawer" data-mode={selected==='grid'?'grid':'dedicated'} data-terminal-drawer-measure="true" aria-label={`${projectName} terminal drawer`}>
    <header class="terminal-drawer__rail"><div role="tablist" aria-label="Terminal drawer views"><button type="button" role="tab" aria-selected={String(selected==='grid')} data-action="select-drawer-terminal" data-terminal-id="grid" aria-label="Terminal grid"><LucideIcon icon={Grid2X2} name="grid-2x-2"/></button>{sessions.map(session=><button type="button" role="tab" aria-selected={String(selected===session.id)} data-action="select-drawer-terminal" data-terminal-id={session.id}><LucideIcon icon={SquareTerminal} name="square-terminal"/><span>{session.title??session.id}</span>{session.busy&&<i aria-label="Busy"></i>}</button>)}</div><div class="terminal-drawer__actions">{hiddenCount>0&&<button type="button" data-action="show-hidden-terminals" aria-label={`Show ${hiddenCount} hidden project terminal${hiddenCount===1?'':'s'}`} title="Show hidden terminals"><LucideIcon icon={Eye} name="eye"/><span>{hiddenCount}</span></button>}<button type="button" data-action="create-project-terminal" aria-label="New project terminal" title="New terminal"><LucideIcon icon={Plus} name="plus"/></button><button type="button" data-action="toggle-terminal-drawer" aria-label="Hide terminal drawer" title="Hide terminal drawer"><LucideIcon icon={PanelBottomClose} name="panel-bottom-close"/></button></div></header>
    <div class="terminal-drawer__content"><TerminalDashboard groups={[{projectId,projectName,sessions:shown}]} width={width} height={height} fitAcross={selected==='grid'?fitAcross:1} fitHigh={selected==='grid'?fitHigh:1} grouping="flow" magnifiedKey={magnifiedKey} hiddenKeys={hiddenKeys} loading={loading} message={message}/></div>
  </section>;
}
