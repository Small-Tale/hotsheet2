import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { CopyX } from 'lucide';

import type { DuplicateBacklink } from '../api';

export interface DuplicateTargetSummary {
  id: string;
  projectName: string;
  slug: string;
  title: string;
}

function DuplicateTicketItem({
  id,
  projectName,
  slug,
  title,
  accessibleLabel,
}: {
  id: string;
  projectName: string;
  slug: string;
  title: string;
  accessibleLabel: string;
}) {
  return (
    <ListItem
      action="open-duplicate-target"
      itemId={id}
      icon={<LucideIcon icon={CopyX} name="copy-x" />}
      label={`${projectName} · ${slug}`}
      description={title}
      accessibleLabel={accessibleLabel}
      multiline
    />
  );
}

export function TicketDuplicateTarget({ target }: { target: DuplicateTargetSummary }) {
  return (
    <section data-component="ticket-duplicate-target" aria-label="Duplicate target">
      <List gap="2xs" controlInsets="b">
        <ListHeader label="Duplicate of" />
        <DuplicateTicketItem
          id={target.id}
          projectName={target.projectName}
          slug={target.slug}
          title={target.title}
          accessibleLabel={`Open duplicate target ${target.slug} from ${target.projectName}`}
        />
      </List>
    </section>
  );
}

export function TicketDuplicateBacklinks({
  backlinks,
  inaccessibleProjects = [],
}: {
  backlinks: readonly DuplicateBacklink[];
  inaccessibleProjects?: readonly string[];
}) {
  if (backlinks.length === 0 && inaccessibleProjects.length === 0) return null;
  return (
    <section data-component="ticket-duplicate-backlinks" aria-label="Duplicate backlinks">
      <List gap="2xs" controlInsets="b">
        <ListHeader
          label="Duplicates"
          count={backlinks.length}
          countLabel={`${backlinks.length} ${backlinks.length === 1 ? 'duplicate' : 'duplicates'}`}
        />
        {backlinks.map((backlink) => (
          <DuplicateTicketItem
            id={backlink.reference}
            projectName={backlink.project_name}
            slug={backlink.slug}
            title={backlink.title}
            accessibleLabel={`Open duplicate ${backlink.slug} from ${backlink.project_name}`}
          />
        ))}
        {inaccessibleProjects.length > 0 && (
          <Text size="compact" lineHeight="tight" controlMargins="rl" color={uiColor('warning-on-quiet')} role="status">
            {`Could not check ${inaccessibleProjects.join(', ')} for additional duplicates.`}
          </Text>
        )}
      </List>
    </section>
  );
}
