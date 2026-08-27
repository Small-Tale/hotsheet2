import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import { ArrowDownAZ, Columns3, List, MoreHorizontal, Pin, Settings, Star } from 'lucide';
import { LucideIcon } from '../components/lucide-icon';
import { ToolbarControlGroup } from '../components/toolbar-control-group';

export function ToolbarControlGroupDemo() {
  return <section class="toolbar-control-group-demo" aria-label="ToolbarControlGroup demo">
    <div><h2>Segmented choices</h2><ToolbarControlGroup label="View mode">
      <button class="view-mode-switcher__button" aria-label="List view" aria-pressed="true"><LucideIcon icon={List} name="list" /></button>
      <button class="view-mode-switcher__button" aria-label="Columns view" aria-pressed="false"><LucideIcon icon={Columns3} name="columns-3" /></button>
      <button class="view-mode-switcher__button" aria-label="Settings view" aria-pressed="false"><LucideIcon icon={Settings} name="settings" /></button>
    </ToolbarControlGroup></div>
    <div><h2>Popup menu</h2><ToolbarControlGroup single>
      <wa-dropdown placement="bottom-start"><wa-button slot="trigger" appearance="plain" with-caret aria-label="Sort tickets"><LucideIcon icon={ArrowDownAZ} name="arrow-down-a-z" /></wa-button><wa-dropdown-item>Recently updated</wa-dropdown-item><wa-dropdown-item>Priority</wa-dropdown-item></wa-dropdown>
    </ToolbarControlGroup></div>
    <div><h2>Button group</h2><ToolbarControlGroup label="View actions">
      <wa-button appearance="plain" aria-label="Favorite view"><LucideIcon icon={Star} name="star" /></wa-button>
      <wa-button appearance="plain" aria-label="More actions"><LucideIcon icon={MoreHorizontal} name="ellipsis" /></wa-button>
    </ToolbarControlGroup></div>
    <div><h2>Single button</h2><ToolbarControlGroup single><wa-button appearance="plain" aria-label="Pin view"><LucideIcon icon={Pin} name="pin" /></wa-button></ToolbarControlGroup></div>
  </section>;
}
