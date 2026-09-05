import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './project-tab-context-menu.css';

import { ArrowLeft,ArrowRight, CircleX, type IconNode,Trash2, X } from 'lucide';

import { LucideIcon } from './lucide-icon';

export type AppTabKind='project'|'terminal';
export function AppTabContextMenu({kind,id,x,y,direction='right'}:{kind:AppTabKind;id:string;x:number;y:number;direction?:'left'|'right'}){
  const directional=direction==='left'?{id:'close-left',label:'Close Tabs to the Left',icon:ArrowLeft,iconName:'arrow-left'}:{id:'close-right',label:'Close Tabs to the Right',icon:ArrowRight,iconName:'arrow-right'};
  const actions:ReadonlyArray<{id:string;label:string;icon:IconNode;iconName:string;danger?:boolean}>=[{id:'close',label:'Close Tab',icon:X,iconName:'x'},{id:'close-others',label:'Close Other Tabs',icon:CircleX,iconName:'circle-x'},directional,{id:'close-all',label:'Close All Tabs',icon:Trash2,iconName:'trash-2',danger:true}];
  const legacyId=kind==='project'?{'data-project-id':id}:{'data-terminal-id':id};
  return <div class="project-tab-context-menu app-tab-context-menu" role="menu" aria-label={`${kind==='project'?'Project':'Terminal'} tab actions`} style={`left:${x}px;top:${y}px`} data-tab-kind={kind} data-tab-id={id} {...legacyId}>
    {actions.map((item,index)=><>{index===3&&<wa-divider></wa-divider>}<wa-dropdown-item data-action={kind==='project'?'project-tab-context-action':'terminal-tab-context-action'} data-tab-action={item.id} data-tab-kind={kind} data-tab-id={id} {...legacyId} variant={item.danger?'danger':undefined}><span slot="icon"><LucideIcon icon={item.icon} name={item.iconName}/></span>{item.label}</wa-dropdown-item></>)}
  </div>;
}

export function ProjectTabContextMenu({projectId,x,y,direction='right'}:{projectId:string;x:number;y:number;direction?:'left'|'right'}){return <AppTabContextMenu kind="project" id={projectId} x={x} y={y} direction={direction}/>}
