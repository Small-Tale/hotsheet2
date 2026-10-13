import { CatalogExample } from '@kerfjs/ui/catalog';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { Archive, Layers3 } from 'lucide';

import { ProviderIcon } from '../components/provider-icon';
import type { DemoModule } from './demo-module';

const count = (value: number) => (
  <Text variant="span" size="compact" tone="quiet">
    {value}
  </Text>
);

export function ListItemDemo() {
  return (
    <>
      <CatalogExample
        label="Standard"
        note="Icons, labels, trailing values, and selection boundaries share one alignment grid."
        viewport={{ width: 'compact' }}
      >
        <ListItem
          action="demo-menu-item"
          icon={<LucideIcon icon={Archive} name="archive" />}
          label="Archive"
          trailing={count(241)}
        />
      </CatalogExample>
      <CatalogExample label="Selected" viewport={{ width: 'compact' }}>
        <ListItem
          action="demo-menu-item"
          selected
          icon={<LucideIcon icon={Layers3} name="layers-3" />}
          label="Queue"
          trailing={count(12)}
        />
      </CatalogExample>
      <CatalogExample label="Multiline" viewport={{ width: 'compact' }}>
        <ListItem
          action="demo-menu-item"
          multiline
          state="modified"
          icon={<span aria-hidden="true">M</span>}
          label="src/components/example.ts"
          description="Secondary detail"
        />
      </CatalogExample>
      <CatalogExample label="Provider identities" viewport={{ width: 'compact' }}>
        {(['github', 'gitlab', 'jira'] as const).map((kind) => (
          <ListItem
            action="demo-menu-item"
            icon={<ProviderIcon kind={kind} />}
            label={kind === 'github' ? 'GitHub Issues' : kind === 'gitlab' ? 'GitLab Issues' : 'Jira Cloud'}
          />
        ))}
      </CatalogExample>
      <CatalogExample label="Disabled" viewport={{ width: 'compact' }}>
        <ListItem
          action="demo-menu-item"
          disabled
          icon={<LucideIcon icon={Archive} name="archive" />}
          label="Already connected"
        />
      </CatalogExample>
    </>
  );
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'list-item': { catalogExamples: true, Demo: ListItemDemo },
  },
};
