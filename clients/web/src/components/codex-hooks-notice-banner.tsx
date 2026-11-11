import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ShieldCheck } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ShellBanner } from './shell-banner';

/**
 * Successful setup changed Codex hook hashes; review is per checkout (HS2-4AR09Z). It is a shell banner,
 * so it shares the inset treatment and header surface of every other application banner (HS2-0YPWH8,
 * HS2-YCB15Z). Dismiss is a native button so StateBanner's action styling matches (HS2-7S60S4).
 */
export function CodexHooksNoticeBanner({ path }: { path: string }) {
  return (
    <ShellBanner
      component="codex-hooks-notice-banner"
      title="Review Updated Codex Hooks"
      detail={`Hot Sheet updated ${path}. Run /hooks in Codex for this checkout to review and trust the updated hooks.`}
      tone="warning"
      actionPlacement="below"
      icon={<LucideIcon icon={ShieldCheck} name="shield-check" />}
      action={
        <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissCodexHooksNotice.attrs}>
          Dismiss
        </button>
      }
    />
  );
}
