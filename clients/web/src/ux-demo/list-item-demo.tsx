import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Archive, Layers3 } from 'lucide';

import { ProviderIcon } from '../components/provider-icon';

export function ListItemDemo() {
  return (
    <section class="list-item-demo" aria-label="ListItem demo">
      <div>
        <h2>Standard</h2>
        <ListItem
          action="demo-menu-item"
          icon={<LucideIcon icon={Archive} name="archive" />}
          label="Archive"
          trailing={<small>241</small>}
        />
      </div>
      <div>
        <h2>Selected</h2>
        <ListItem
          action="demo-menu-item"
          selected
          icon={<LucideIcon icon={Layers3} name="layers-3" />}
          label="Queue"
          trailing={<small>12</small>}
        />
      </div>
      <div>
        <h2>Multiline</h2>
        <ListItem
          action="demo-menu-item"
          multiline
          state="modified"
          icon={<span aria-hidden="true">M</span>}
          label={
            <span class="list-item-demo__copy">
              src/components/example.ts<small>Secondary detail</small>
            </span>
          }
        />
      </div>
      <div>
        <h2>Provider identities</h2>
        {(['github', 'gitlab', 'jira'] as const).map((kind) => (
          <ListItem
            action="demo-menu-item"
            icon={<ProviderIcon kind={kind} />}
            label={kind === 'github' ? 'GitHub Issues' : kind === 'gitlab' ? 'GitLab Issues' : 'Jira Cloud'}
          />
        ))}
      </div>
      <div>
        <h2>Disabled</h2>
        <ListItem
          action="demo-menu-item"
          disabled
          icon={<LucideIcon icon={Archive} name="archive" />}
          label="Already connected"
        />
      </div>
      <p>Icons, labels, trailing values, and selection boundaries share one alignment grid.</p>
    </section>
  );
}
