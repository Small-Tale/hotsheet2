import { CatalogExample } from '@kerfjs/ui/catalog';

import { ThemePicker } from '../components/theme-picker';
import type { DemoModule } from './demo-module';

/** The Theme picker with each choice selected (HS2-EFTHS6); choosing a tile here changes nothing. */
export function ThemePickerDemo() {
  return (
    <>
      <CatalogExample label="Auto (Default)">
        <ThemePicker value="auto" name="demo-theme-auto" />
      </CatalogExample>
      <CatalogExample label="Light">
        <ThemePicker value="light" name="demo-theme-light" />
      </CatalogExample>
      <CatalogExample label="Dark">
        <ThemePicker value="dark" name="demo-theme-dark" />
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders (HS2-EFTHS6). */
export const demoModule: DemoModule = {
  entries: {
    'theme-picker': { catalogExamples: true, Demo: ThemePickerDemo },
  },
};
