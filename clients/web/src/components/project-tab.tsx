import './project-tab.css';

import { Bell, CircleAlert, Cloud, WifiOff } from 'lucide';

import { AppTab } from './app-tab';
import { LoadingSpinner } from './loading-spinner';
import { LucideIcon } from './lucide-icon';

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
}

export function ProjectTab({ id, name, location, selected = false, busy = false, disconnected = false, attention = false, closable = true,notificationCount=0 }: ProjectTabProps) {
  const trailing=notificationCount>0?<span class="project-tab__notification" aria-label={`${notificationCount} pending notification${notificationCount===1?'':'s'}`} title={`${notificationCount} pending notification${notificationCount===1?'':'s'}`}><LucideIcon icon={Bell} name="bell"/><span aria-hidden="true">{notificationCount}</span></span>:busy?<span class="project-tab__busy"><LoadingSpinner label="Project busy" /></span>:disconnected?<LucideIcon icon={WifiOff} name="wifi-off" class="project-tab__state" />:attention?<LucideIcon icon={CircleAlert} name="circle-alert" class="project-tab__state project-tab__state--attention" />:undefined;
  return <AppTab kind="project" id={id} name={name} selected={selected} closable={closable} leading={location==='remote'?<LucideIcon icon={Cloud} name="cloud"/>:undefined} trailing={trailing} rootAttributes={{'data-ticket-drop-project':id,'data-location':location,'data-busy':String(busy),'data-disconnected':String(disconnected),'data-attention':String(attention)}}/>;
}
