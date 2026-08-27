import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import { Ellipsis, Plus } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { ProjectTab, type ProjectTabProps } from './project-tab';
import './project-tab-bar.css';

export interface ProjectTabBarProps {
  tabs: ProjectTabProps[];
  label?: string;
}

export function ProjectTabBar({ tabs, label = 'Open projects' }: ProjectTabBarProps) {
  return <nav class="project-tab-bar" data-component="project-tab-bar" aria-label={label}>
    <div class="project-tab-bar__tabs" role="tablist" aria-label={label}>{tabs.map(tab => <ProjectTab {...tab} />)}</div>
    <div class="project-tab-bar__actions">
      <wa-button appearance="plain" data-action="add-project" aria-label="Add project" title="Add project"><LucideIcon icon={Plus} name="plus" /></wa-button>
      <wa-dropdown placement="bottom-end">
        <wa-button slot="trigger" appearance="plain" aria-label="More projects" title="More projects"><LucideIcon icon={Ellipsis} name="ellipsis" /></wa-button>
        {tabs.map(tab => <wa-dropdown-item data-action="select-project-tab" data-project-id={tab.id} value={tab.id}>{tab.name}<small slot="details">{tab.location}</small></wa-dropdown-item>)}
      </wa-dropdown>
    </div>
  </nav>;
}
