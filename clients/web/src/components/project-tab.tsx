import './project-tab.css';

import { Bell, CircleAlert, Cloud, WifiOff, X } from 'lucide';

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
  return <div class="project-tab" data-component="project-tab" data-project-id={id} data-selected={String(selected)} data-location={location} data-busy={String(busy)} data-disconnected={String(disconnected)} data-attention={String(attention)}>
    {closable && <button type="button" class="project-tab__close" data-action="close-project-tab" data-project-id={id} aria-label={`Close ${name}`} title={`Close ${name}`}><LucideIcon icon={X} name="x" /></button>}
    <button type="button" class="project-tab__select" role="tab" aria-selected={String(selected)} data-action="select-project-tab" data-project-id={id} tabindex={selected ? '0' : '-1'}>
      {location === 'remote' && <LucideIcon icon={Cloud} name="cloud" />}
      <span class="project-tab__name">{name}</span>
      {notificationCount>0?<span class="project-tab__notification" aria-label={`${notificationCount} pending notification${notificationCount===1?'':'s'}`} title={`${notificationCount} pending notification${notificationCount===1?'':'s'}`}><LucideIcon icon={Bell} name="bell"/><span aria-hidden="true">{notificationCount}</span></span>:busy?<span class="project-tab__busy"><LoadingSpinner label="Project busy" /></span>:disconnected?<LucideIcon icon={WifiOff} name="wifi-off" class="project-tab__state" />:attention?<LucideIcon icon={CircleAlert} name="circle-alert" class="project-tab__state project-tab__state--attention" />:null}
    </button>
  </div>;
}
