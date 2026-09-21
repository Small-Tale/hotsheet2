import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronDown, Plus } from 'lucide';

export function ListHeaderDemo() {
  return (
    <section class="list-item-demo" aria-label="ListHeader demo">
      <div>
        <ListHeader
          label="Views"
          action="demo-add"
          actionLabel="Add view"
          actionIcon={<LucideIcon icon={Plus} name="plus" />}
        />
      </div>
      <div>
        <ListHeader
          label="Project Commands"
          action="demo-toggle"
          actionIcon={<LucideIcon icon={ChevronDown} name="chevron-down" />}
          expanded
          toggle
        />
      </div>
      <p>Section labels align with ListItem icons whether the header has an action or toggles a group.</p>
    </section>
  );
}
