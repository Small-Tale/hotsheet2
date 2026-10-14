import { CatalogExample } from '@kerfjs/ui/catalog';
import { flex, pct } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';

import type { DemoModule } from './demo-module';

/** Package-owned list geometry with passive content and an explicitly bounded scroll example. */
export function ListDemo() {
  const rows = (count: number) =>
    Array.from({ length: count }, (_, index) => <ListInsetText>{`Row ${index + 1}`}</ListInsetText>);
  return (
    <>
      <CatalogExample label="No Gap" viewport={{ width: 'compact' }}>
        <List>{rows(3)}</List>
      </CatalogExample>
      <CatalogExample label="Standard Gap" viewport={{ width: 'compact' }}>
        <List gap>{rows(3)}</List>
      </CatalogExample>
      <CatalogExample
        label="Custom Gap and Scrolling"
        note="A bounded viewport scrolls the list inside its divider edges."
        viewport={{ width: 'compact', height: 'short', layout: 'flex' }}
      >
        <List gap="m" flex={flex(1, 1, pct(0))} scrollable dividerSides="trbl">
          {rows(12)}
        </List>
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    list: { catalogExamples: true, Demo: ListDemo },
  },
};
