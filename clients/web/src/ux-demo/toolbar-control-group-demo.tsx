import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { SegmentedControl } from '@kerfjs/ui/segmented-control';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { signal } from 'kerfjs';
import {
  ArrowDownAZ,
  ChevronLeft,
  ChevronRight,
  Columns3,
  GitCompare,
  List,
  MoreHorizontal,
  PanelLeftOpen,
  Pin,
  Settings,
  Star,
} from 'lucide';

export const toolbarGroupDemoMode = signal<'list' | 'board' | 'settings'>('list');

export function ToolbarControlGroupDemo() {
  return (
    <section class="toolbar-control-group-demo" aria-label="ToolbarControlGroup demo">
      <div>
        <h2>Segmented choices</h2>
        <ToolbarControlGroup>
          <SegmentedControl
            id="toolbar-group-demo-mode"
            label="View mode"
            value={toolbarGroupDemoMode.value}
            action="set-toolbar-group-demo-mode"
            appearance="toolbar"
            shape="pill"
            choices={[
              { value: 'list', label: 'List view', content: <LucideIcon icon={List} name="list" /> },
              { value: 'board', label: 'Columns view', content: <LucideIcon icon={Columns3} name="columns-3" /> },
              { value: 'settings', label: 'Settings view', content: <LucideIcon icon={Settings} name="settings" /> },
            ]}
          />
        </ToolbarControlGroup>
      </div>
      <div>
        <h2>Popup menu</h2>
        <ToolbarControlGroup single>
          <wa-dropdown placement="bottom-start">
            <wa-button slot="trigger" appearance="plain" with-caret aria-label="Sort tickets">
              <LucideIcon icon={ArrowDownAZ} name="arrow-down-a-z" />
            </wa-button>
            <wa-dropdown-item>Recently updated</wa-dropdown-item>
            <wa-dropdown-item>Priority</wa-dropdown-item>
          </wa-dropdown>
        </ToolbarControlGroup>
      </div>
      <div>
        <h2>Button group</h2>
        <ToolbarControlGroup label="View actions">
          <button type="button" aria-label="Favorite view">
            <LucideIcon icon={Star} name="star" />
          </button>
          <button type="button" aria-label="More actions">
            <LucideIcon icon={MoreHorizontal} name="ellipsis" />
          </button>
        </ToolbarControlGroup>
      </div>
      <div>
        <h2>Single button</h2>
        <ToolbarControlGroup single>
          <button type="button" aria-label="Pin view">
            <LucideIcon icon={Pin} name="pin" />
          </button>
        </ToolbarControlGroup>
      </div>
      <div>
        <h2>Borderless group</h2>
        <ToolbarControlGroup appearance="borderless" single>
          <button type="button" aria-label="Show sidebar">
            <LucideIcon icon={PanelLeftOpen} name="panel-left-open" />
          </button>
        </ToolbarControlGroup>
      </div>
      <div>
        <h2>Push button, resting</h2>
        <ToolbarControlGroup buttonAppearance="push" single>
          <button type="button" aria-label="Resting comparison" aria-pressed="false">
            <LucideIcon icon={GitCompare} name="git-compare" />
          </button>
        </ToolbarControlGroup>
      </div>
      <div>
        <h2>Push button, pressed</h2>
        <ToolbarControlGroup buttonAppearance="push" single>
          <button type="button" aria-label="Pressed comparison" aria-pressed="true">
            <LucideIcon icon={GitCompare} name="git-compare" />
          </button>
        </ToolbarControlGroup>
      </div>
      <div>
        <h2>Dark group</h2>
        <ToolbarControlGroup label="Dark navigation" tone="dark">
          <button type="button" aria-label="Previous">
            <LucideIcon icon={ChevronLeft} name="chevron-left" />
          </button>
          <button type="button" aria-label="Next">
            <LucideIcon icon={ChevronRight} name="chevron-right" />
          </button>
        </ToolbarControlGroup>
      </div>
    </section>
  );
}
