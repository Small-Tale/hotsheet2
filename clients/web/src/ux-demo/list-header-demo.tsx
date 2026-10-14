import { CatalogExample } from '@kerfjs/ui/catalog';
import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Plus } from 'lucide';

import type { DemoModule } from './demo-module';

export function ListHeaderDemo() {
  return (
    <>
      <CatalogExample
        label="With Action"
        note="Section labels align with ListItem icons whether the header has an action or toggles a group."
        viewport={{ width: 'compact' }}
      >
        <ListHeader
          label="Views"
          action="demo-add"
          actionLabel="Add view"
          actionIcon={<LucideIcon icon={Plus} name="plus" />}
        />
      </CatalogExample>
      <CatalogExample label="Group Toggle" viewport={{ width: 'compact' }}>
        <ListHeader label="Project Commands" action="demo-toggle" expanded toggle />
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'list-header': { catalogExamples: true, Demo: ListHeaderDemo },
  },
};
