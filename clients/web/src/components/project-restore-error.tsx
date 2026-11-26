import { Button } from '@kerfjs/ui/button';
import { Disclosure } from '@kerfjs/ui/disclosure';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { CircleAlert, FileX, FolderX, Lock, ServerOff, Unlink } from 'lucide';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import {
  explainProjectUnavailable,
  type ProjectUnavailableCause,
  projectUnavailableTechnicalDetails,
} from '../project-unavailable-cause';

export interface ProjectRestoreFailure {
  root: string;
  name: string;
  error: string;
  recoveryPid?: number;
  busy?: boolean;
  /** Opens Technical Details initially; the demo uses it to show the open state (HS2-1ZKMS9). */
  detailsOpen?: boolean;
}

export function rememberedProjectName(root: string): string {
  const normalized = root.replace(/[\\/]+$/, '');
  return normalized.split(/[\\/]/).at(-1) || root || 'Remembered project';
}

export function projectRestoreTabId(root: string): string {
  return `project-restore-error:${root}`;
}

const CAUSE_ICONS = {
  'folder-missing': { icon: FolderX, name: 'folder-x' },
  'not-a-folder': { icon: FileX, name: 'file-x' },
  'permission-denied': { icon: Lock, name: 'lock' },
  'server-unreachable': { icon: ServerOff, name: 'server-off' },
  'store-link-broken': { icon: Unlink, name: 'unlink' },
  unknown: { icon: CircleAlert, name: 'circle-alert' },
} satisfies Record<ProjectUnavailableCause, { icon: typeof CircleAlert; name: string }>;

/**
 * The workspace of a remembered project that could not be reopened. It explains the cause in plain
 * language and offers Retry and Close Tab, primary by cause; the raw error stays behind Technical
 * Details for diagnosis (HS2-ZKAVZZ), in the EmptyState's start-aligned secondary region so the open
 * disclosure keeps its chevron beside its title (HS2-1ZKMS9, Kerf KF-GZDZXH). The shell around it keeps
 * its regions (HS2-H0QGK7).
 */
export function ProjectRestoreError({
  root,
  name,
  error,
  recoveryPid,
  busy = false,
  detailsOpen = false,
}: ProjectRestoreFailure) {
  const failure = { root, name, error, ...(recoveryPid ? { recoveryPid } : {}) },
    explanation = explainProjectUnavailable(failure),
    icon = CAUSE_ICONS[explanation.cause];
  return (
    <List
      fill
      hAlign="center"
      vAlign="middle"
      className="project-restore-error"
      rootAttributes={{ 'data-project-unavailable-cause': explanation.cause }}
    >
      <EmptyState
        title={explanation.title}
        detail={explanation.detail}
        busy={busy}
        icon={<LucideIcon icon={icon.icon} name={icon.name} />}
        action={
          <Row hAlign="center" gap="xs" wrap>
            <Button
              emphasis={explanation.primaryAction === 'retry' ? 'primary' : 'secondary'}
              action={NAVIGATION_AND_TABS_ACTIONS.retryProjectRestore.value}
              disabled={busy}
              rootAttributes={{ 'data-project-root': root }}
            >
              {busy ? 'Retrying…' : 'Retry'}
            </Button>
            <Button
              emphasis={explanation.primaryAction === 'close' ? 'primary' : 'secondary'}
              action={TERMINALS_ACTIONS.closeProjectTab.value}
              rootAttributes={{ 'data-project-id': projectRestoreTabId(root) }}
            >
              Close Tab
            </Button>
          </Row>
        }
        secondaryContent={
          <Disclosure
            title="Technical Details"
            size="compact"
            open={detailsOpen}
            rootAttributes={{ 'data-project-unavailable-details': '' }}
          >
            <List gap="xs">
              {projectUnavailableTechnicalDetails(failure).map((line) => (
                <Text size="compact" tone="quiet" font="monospace" wrap="anywhere">
                  {line}
                </Text>
              ))}
            </List>
          </Disclosure>
        }
      />
    </List>
  );
}
