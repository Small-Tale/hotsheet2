import './project-tab.css';

import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bell, CircleAlert, Cloud, WifiOff } from 'lucide';

import { AppTab } from './app-tab';

export type ProjectTabLocation = 'local' | 'remote';

export interface ProjectTabProps {
  id: string;
  name: string;
  location: ProjectTabLocation;
  selected?: boolean;
  busy?: boolean;
  disconnected?: boolean;
  attention?: boolean;
  closable?: boolean;
  notificationCount?: number;
  upNextCount?: number;
  activeTicketCount?: number;
}

export function projectTabUpNextLabel(count: number): string {
  return Math.max(0, Math.trunc(count)) > 99 ? '99+' : String(Math.max(0, Math.trunc(count)));
}

export function projectTabActivityDash(count:number):string {
  const segments=Math.max(1,Math.trunc(count)),round=(value:number)=>Number(value.toFixed(4)),circumference=2*Math.PI*9;
  return `${round(circumference*.75/segments)} ${round(circumference*.25/segments)}`;
}

function ProjectTabActivityRing({count}:{count:number}) {
  return <svg class="project-tab__activity-ring" data-segments={String(count)} viewBox="0 0 24 24" aria-hidden="true"><circle class="project-tab__activity-track" cx="12" cy="12" r="9"/><circle class="project-tab__activity-segments" cx="12" cy="12" r="9" stroke-dasharray={projectTabActivityDash(count)}/></svg>;
}

export function ProjectTab({ id, name, location, selected = false, busy = false, disconnected = false, attention = false, closable = true,notificationCount=0,upNextCount=0,activeTicketCount=0 }: ProjectTabProps) {
  const normalizedUpNextCount=Math.max(0,Math.trunc(upNextCount)),normalizedActiveTicketCount=Math.max(0,Math.trunc(activeTicketCount));
  const workLabel=[normalizedUpNextCount>0?`${normalizedUpNextCount} Up Next ticket${normalizedUpNextCount===1?'':'s'}`:'',normalizedActiveTicketCount>0?`${normalizedActiveTicketCount} active ticket${normalizedActiveTicketCount===1?'':'s'}`:''].filter(Boolean).join(', ');
  const visibleCount=normalizedActiveTicketCount||normalizedUpNextCount;
  const work=workLabel?<span class="project-tab__work" data-active={String(normalizedActiveTicketCount>0)} data-active-count={String(normalizedActiveTicketCount)} data-up-next-count={String(normalizedUpNextCount)} aria-label={workLabel} title={workLabel}>{normalizedActiveTicketCount>0&&<ProjectTabActivityRing count={normalizedActiveTicketCount}/>}<span class="project-tab__work-count" aria-hidden="true">{projectTabUpNextLabel(visibleCount)}</span></span>:undefined;
  const notification=notificationCount>0?<span class="project-tab__notification" aria-label={`${notificationCount} pending notification${notificationCount===1?'':'s'}`} title={`${notificationCount} pending notification${notificationCount===1?'':'s'}`}><LucideIcon icon={Bell} name="bell"/><span aria-hidden="true">{notificationCount}</span></span>:undefined;
  const trailing=work||notification?<span class="project-tab__indicators">{notification}{work}</span>:busy?<span class="project-tab__busy"><LoadingSpinner label="Project busy" /></span>:disconnected?<LucideIcon icon={WifiOff} name="wifi-off" className="project-tab__state" />:attention?<LucideIcon icon={CircleAlert} name="circle-alert" className="project-tab__state project-tab__state--attention" />:undefined;
  return <AppTab kind="project" id={id} name={name} selected={selected} closable={closable} leading={location==='remote'?<LucideIcon icon={Cloud} name="cloud"/>:undefined} trailing={trailing} rootAttributes={{'data-ticket-drop-project':id,'data-location':location,'data-busy':String(busy),'data-disconnected':String(disconnected),'data-attention':String(attention)}}/>;
}
