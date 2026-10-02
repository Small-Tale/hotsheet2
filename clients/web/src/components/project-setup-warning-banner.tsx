import './project-setup-warning-banner.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { TriangleAlert } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

/**
 * A project opened, but its setup refresh was skipped (HS2-0TXM8S): for example a stale
 * development CLI refused to rewrite newer AI-tool guidance. The project stays usable; the banner
 * explains what was skipped and how to fix it, and can be dismissed.
 */
export function ProjectSetupWarningBanner({ detail }: { detail: string }) {
  return (
    <div class="project-setup-warning-banner" data-component="project-setup-warning-banner">
      <StateBanner
        title="Project setup was skipped"
        detail={detail}
        tone="warning"
        urgency="status"
        copyLayout="stacked"
        actionPlacement="below"
        icon={<LucideIcon icon={TriangleAlert} name="triangle-alert" />}
        action={
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissProjectSetupWarning.attrs}>
            Dismiss
          </button>
        }
      />
    </div>
  );
}
