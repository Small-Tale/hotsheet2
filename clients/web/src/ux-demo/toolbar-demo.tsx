import { CatalogExample } from '@kerfjs/ui/catalog';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { PanelLeftOpen, PanelRightOpen } from 'lucide';

import type { DemoModule } from './demo-module';

export function ToolbarDemo() {
  return (
    <>
      <CatalogExample
        label="Three slots"
        viewport={{ width: 'full', layout: 'grid', frame: 'solid', surface: 'lowered' }}
      >
        <Toolbar
          label="Example three-slot toolbar"
          leading={
            <ToolbarControlGroup appearance="borderless" single>
              <button type="button" aria-label="Show left sidebar">
                <LucideIcon icon={PanelLeftOpen} name="panel-left-open" />
              </button>
            </ToolbarControlGroup>
          }
          center={<ToolbarText text="Hot Sheet 2" size="large" />}
          trailing={
            <ToolbarControlGroup appearance="borderless" single>
              <button type="button" aria-label="Show right sidebar">
                <LucideIcon icon={PanelRightOpen} name="panel-right-open" />
              </button>
            </ToolbarControlGroup>
          }
        />
      </CatalogExample>
      <CatalogExample
        label="Without divider"
        viewport={{ width: 'full', layout: 'grid', frame: 'solid', surface: 'lowered' }}
      >
        <Toolbar
          label="Toolbar without divider"
          dividerSides=""
          center={<ToolbarText text="HS2-C1TY0F" size="small" />}
        />
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    toolbar: { catalogExamples: true, Demo: ToolbarDemo },
  },
};
