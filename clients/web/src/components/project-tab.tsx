import './project-tab.css';

import { CircleAlert, Cloud, FolderGit2, WifiOff, X } from 'lucide';

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
}

export function ProjectTab({ id, name, location, selected = false, busy = false, disconnected = false, attention = false, closable = true }: ProjectTabProps) {
  const locationIcon = location === 'local' ? FolderGit2 : Cloud;
  return <div class="project-tab" data-component="project-tab" data-project-id={id} data-selected={String(selected)} data-location={location} data-busy={String(busy)} data-disconnected={String(disconnected)} data-attention={String(attention)}>
    {closable && <button type="button" class="project-tab__close" data-action="close-project-tab" data-project-id={id} aria-label={`Close ${name}`} title={`Close ${name}`}><LucideIcon icon={X} name="x" /></button>}
    <button type="button" class="project-tab__select" role="tab" aria-selected={String(selected)} data-action="select-project-tab" data-project-id={id} tabindex={selected ? '0' : '-1'}>
      <LucideIcon icon={locationIcon} name={location === 'local' ? 'folder-git-2' : 'cloud'} />
      <span class="project-tab__name">{name}</span>
      {busy && <span class="project-tab__busy"><LoadingSpinner label="Project busy" /></span>}
      {!busy && disconnected && <LucideIcon icon={WifiOff} name="wifi-off" class="project-tab__state" />}
      {!busy && !disconnected && attention && <LucideIcon icon={CircleAlert} name="circle-alert" class="project-tab__state project-tab__state--attention" />}
    </button>
  </div>;
}
