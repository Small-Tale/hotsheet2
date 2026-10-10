import './codex-hooks-notice-banner.css';

import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { ShieldCheck } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

/**
 * Successful setup changed Codex hook hashes; review is per checkout (HS2-4AR09Z). The rounded
 * banner sits on the header surface, not the sunken pane content tint (HS2-0YPWH8).
 */
export function CodexHooksNoticeBanner({ path }: { path: string }) {
  return (
    <div class="codex-hooks-notice-banner" data-component="codex-hooks-notice-banner">
      <List controlInsets="trbl" rootAttributes={{ 'data-codex-hooks-notice': 'true' }}>
        <StateBanner
          title="Review updated Codex hooks"
          detail={`Hot Sheet updated ${path}. Run /hooks in Codex for this checkout to review and trust the updated hooks.`}
          tone="warning"
          urgency="status"
          copyLayout="stacked"
          actionPlacement="below"
          icon={<LucideIcon icon={ShieldCheck} name="shield-check" />}
          action={
            <wa-button size="small" appearance="outlined" {...PROJECT_LIFECYCLE_ACTIONS.dismissCodexHooksNotice.attrs}>
              Dismiss
            </wa-button>
          }
        />
      </List>
    </div>
  );
}
