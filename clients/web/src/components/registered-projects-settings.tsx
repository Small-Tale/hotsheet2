import { Badge } from '@kerfjs/ui/badge';
import { rem } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { Folder, FolderX, MoreHorizontal, X } from 'lucide';

import type { RegisteredProjectsState } from '../features/registered-projects';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { type RegisteredProject, registeredProjectName } from '../registered-projects-model';

export interface RegisteredProjectsSettingsProps {
  state?: RegisteredProjectsState;
  /** The host's answer, already sorted; absent while loading. */
  list?: readonly RegisteredProject[];
  /** The last failure or refusal, shown until the next change or successful load. */
  message?: string;
  /** Projects with a removal in flight. */
  busy?: readonly string[];
}

const ACTIONS = NOTIFICATIONS_AND_LINKS_ACTIONS;

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;

/**
 * App Settings → Projects (HS2-R806FZ): every project registered with Hot Sheet on this computer,
 * following the Devices grammar. A row's "…" menu (which the row itself and a right-click also open)
 * removes the project from Hot Sheet; that unregisters it and never deletes its files. A folder that
 * is gone, or on a volume that is not mounted, shows a Folder Missing badge. The delegated handlers
 * live in `registered-projects-settings-interactions.ts`.
 */
export function RegisteredProjectsSettings({
  state = 'idle',
  list,
  message = '',
  busy = [],
}: RegisteredProjectsSettingsProps) {
  const loading = !list && state === 'loading',
    projects = list ?? [];
  return (
    <div data-component="registered-projects-settings">
      <List gap="l">
        <List gap="xs">
          <Text variant="h2">Projects</Text>
          <Text tone="quiet">
            Projects registered with Hot Sheet on this computer. Removing one never deletes its files or ticket store.
          </Text>
          {message ? (
            <Text tone="danger" size="compact" role="alert">
              {message}
            </Text>
          ) : null}
        </List>
        {loading ? (
          <List gap={rem(0.125)}>
            <ListActionRow
              label="Loading"
              action={ACTIONS.openRegisteredProjectActions.value}
              placeholder
              trailingMenu={{ label: 'Project Actions', icon: menuIcon, items: [] }}
            />
          </List>
        ) : !list ? null : projects.length === 0 ? (
          <EmptyState
            icon={<LucideIcon icon={Folder} name="folder" />}
            title="No projects are registered."
            detail="Opening a project folder in Hot Sheet registers it here."
          />
        ) : (
          <List gap={rem(0.125)}>
            {projects.map((project) => {
              const name = registeredProjectName(project),
                attributes = { 'data-project-id': project.id };
              return (
                <ListActionRow
                  label={name}
                  description={project.root}
                  status={
                    project.available ? undefined : (
                      <Badge tone="warning" appearance="quiet" size="compact">
                        Folder Missing
                      </Badge>
                    )
                  }
                  icon={
                    project.available ? (
                      <LucideIcon icon={Folder} name="folder" />
                    ) : (
                      <LucideIcon icon={FolderX} name="folder-x" />
                    )
                  }
                  iconTile="neutral"
                  action={ACTIONS.openRegisteredProjectActions.value}
                  itemId={project.id}
                  busy={busy.includes(project.id)}
                  rootAttributes={{
                    'data-registered-project': project.id,
                    'data-project-available': String(project.available),
                  }}
                  trailingMenu={{
                    label: `Actions for ${name}`,
                    icon: menuIcon,
                    items: [
                      {
                        label: 'Remove from Hot Sheet',
                        action: ACTIONS.removeRegisteredProject.value,
                        tone: 'danger' as const,
                        icon: <LucideIcon icon={X} name="x" />,
                        attributes,
                      },
                    ],
                  }}
                />
              );
            })}
          </List>
        )}
      </List>
    </div>
  );
}
