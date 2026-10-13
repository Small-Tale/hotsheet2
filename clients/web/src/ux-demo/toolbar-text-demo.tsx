import { CatalogExample } from '@kerfjs/ui/catalog';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';

import type { DemoModule } from './demo-module';

export function ToolbarTextDemo() {
  return (
    <>
      <CatalogExample label="Large" align="glyph">
        <ToolbarText text="Hot Sheet 2" size="large" />
      </CatalogExample>
      <CatalogExample label="Default" align="glyph">
        <ToolbarText text="Queue" />
      </CatalogExample>
      <CatalogExample label="Small" align="glyph">
        <ToolbarText text="HS2-C1TY0F" size="small" />
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'toolbar-text': { catalogExamples: true, Demo: ToolbarTextDemo },
  },
};
