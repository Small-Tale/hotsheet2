import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';

import { CatalogExample } from '@kerfjs/ui/catalog';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { SegmentedControl } from '@kerfjs/ui/segmented-control';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
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

import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule } from './demo-module';

export const toolbarGroupDemoMode = signal<'list' | 'board' | 'settings'>('list');

export function ToolbarControlGroupDemo() {
  return (
    <>
      <CatalogExample label="Segmented Choices" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup>
              <SegmentedControl
                id="toolbar-group-demo-mode"
                label="View mode"
                value={toolbarGroupDemoMode.value}
                action="set-toolbar-group-demo-mode"
                appearance="toolbar"
                shape="pill"
                choices={[
                  { value: 'list', label: 'List View', content: <LucideIcon icon={List} name="list" /> },
                  { value: 'board', label: 'Columns View', content: <LucideIcon icon={Columns3} name="columns-3" /> },
                  {
                    value: 'settings',
                    label: 'Settings View',
                    content: <LucideIcon icon={Settings} name="settings" />,
                  },
                ]}
              />
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample label="Popup Menu" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup single nestedDropdown>
              <PopupMenu
                label="Sort Tickets"
                icon={<LucideIcon icon={ArrowDownAZ} name="arrow-down-a-z" />}
                items={[{ label: 'Recently Updated' }, { label: 'Priority' }]}
              />
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample label="Button Group" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup label="View Actions">
              <button type="button" aria-label="Favorite View">
                <LucideIcon icon={Star} name="star" />
              </button>
              <button type="button" aria-label="More Actions">
                <LucideIcon icon={MoreHorizontal} name="ellipsis" />
              </button>
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample label="Single Button" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup single>
              <button type="button" aria-label="Pin View">
                <LucideIcon icon={Pin} name="pin" />
              </button>
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample label="Borderless Group" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup appearance="borderless" single>
              <button type="button" aria-label="Show Sidebar">
                <LucideIcon icon={PanelLeftOpen} name="panel-left-open" />
              </button>
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample label="Push Button, Resting" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup buttonAppearance="push" single>
              <button type="button" aria-label="Resting Comparison" aria-pressed="false">
                <LucideIcon icon={GitCompare} name="git-compare" />
              </button>
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample label="Push Button, Pressed" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup buttonAppearance="push" single>
              <button type="button" aria-label="Pressed Comparison" aria-pressed="true">
                <LucideIcon icon={GitCompare} name="git-compare" />
              </button>
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample label="Dark Group" align="glyph">
        <Toolbar
          leading={
            <ToolbarControlGroup label="Dark Navigation" tone="dark">
              <button type="button" aria-label="Previous">
                <LucideIcon icon={ChevronLeft} name="chevron-left" />
              </button>
              <button type="button" aria-label="Next">
                <LucideIcon icon={ChevronRight} name="chevron-right" />
              </button>
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireToolbarControlGroupDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.setToolbarGroupDemoMode.selector, (_event, target) => {
      toolbarGroupDemoMode.value = (target as HTMLElement).dataset.segmentValue as typeof toolbarGroupDemoMode.value;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'toolbar-control-group': { catalogExamples: true, Demo: ToolbarControlGroupDemo },
  },
  wire: wireToolbarControlGroupDemo,
};
