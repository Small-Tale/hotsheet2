import { Button } from '@kerfjs/ui/button';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CircleAlert } from 'lucide';

import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';

export interface ProjectRestoreFailure {
  root: string;
  name: string;
  error: string;
  recoveryPid?: number;
  busy?: boolean;
}

export function rememberedProjectName(root: string): string {
  const normalized = root.replace(/[\\/]+$/, '');
  return normalized.split(/[\\/]/).at(-1) || root || 'Remembered project';
}

export function projectRestoreTabId(root: string): string {
  return `project-restore-error:${root}`;
}

export function ProjectRestoreError({ root, name, error, recoveryPid, busy = false }: ProjectRestoreFailure) {
  const recovery = recoveryPid ? ` A registered Hot Sheet server process (${recoveryPid}) is not responding.` : '';
  return (
    <EmptyState
      className="project-restore-error"
      title={`${name} could not be reopened.`}
      detail={`${error}${recovery} The project remains remembered. Retry after the local server is available; if the problem continues, verify the project folder and relaunch the Hot Sheet server and client. Project: ${root}`}
      icon={<LucideIcon icon={CircleAlert} name="circle-alert" />}
      action={
        <Button
          emphasis="primary"
          action={NAVIGATION_AND_TABS_ACTIONS.retryProjectRestore.value}
          disabled={busy}
          rootAttributes={{ 'data-project-root': root }}
        >
          {busy ? 'Retrying…' : 'Retry project'}
        </Button>
      }
    />
  );
}
