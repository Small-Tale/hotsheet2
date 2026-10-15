import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Hammer } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ShellBanner } from './shell-banner';

/**
 * Newer local source than the running server's build (HS2-B84P1S): an informational, dismissible
 * notice rather than the alert-toned "different build" banner, because the running client/server
 * pair is consistent and only the next rebuild and restart is affected.
 */
export function SourceStaleNoticeBanner() {
  return (
    <ShellBanner
      component="source-stale-notice-banner"
      title="Newer local source is available"
      detail="Rebuild and restart the server to run it. The running server and this client still match."
      tone="info"
      actionPlacement="below"
      icon={<LucideIcon icon={Hammer} name="hammer" />}
      action={
        <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissSourceStaleNotice.attrs}>
          Dismiss
        </button>
      }
    />
  );
}
