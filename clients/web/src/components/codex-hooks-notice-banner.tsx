import './project-setup-warning-banner.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { ShieldCheck } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

/**
 * Successful setup changed Codex hook hashes; review is per checkout (HS2-4AR09Z). Shares the
 * full-width header-strip host of the setup warning so it sits on the header surface (HS2-0YPWH8).
 */
export function CodexHooksNoticeBanner({ path }: { path: string }) {
  return (
    <div class="project-setup-warning-banner" data-codex-hooks-notice="true">
      <StateBanner
        title="Review updated Codex hooks"
        detail={`Hot Sheet updated ${path}. Run /hooks in Codex for this checkout to review and trust the updated hooks.`}
        tone="warning"
        urgency="status"
        copyLayout="stacked"
        actionPlacement="below"
        icon={<LucideIcon icon={ShieldCheck} name="shield-check" />}
        action={
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissCodexHooksNotice.attrs}>
            Dismiss
          </button>
        }
      />
    </div>
  );
}
