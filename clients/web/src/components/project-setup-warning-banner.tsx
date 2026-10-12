import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { TriangleAlert } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ShellBanner } from './shell-banner';

/**
 * A project opened, but its setup refresh did not finish (HS2-0TXM8S): for example a stale
 * development CLI refused to rewrite newer AI-tool guidance. The project stays usable; the banner
 * explains the failure and how to fix it, and can be dismissed.
 */
export function ProjectSetupWarningBanner({ detail }: { detail: string }) {
  return (
    <ShellBanner
      component="project-setup-warning-banner"
      title="Project setup needs attention"
      detail={detail}
      tone="warning"
      actionPlacement="below"
      icon={<LucideIcon icon={TriangleAlert} name="triangle-alert" />}
      action={
        <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissProjectSetupWarning.attrs}>
          Dismiss
        </button>
      }
    />
  );
}
