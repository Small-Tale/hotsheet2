import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ServerCog } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ShellBanner } from './shell-banner';

/**
 * A pinned host opened the project through a machine server it did not select (HS2-46BKH7): another
 * host or build replaced the shared server, and a safe restart was refused, unsupported, or
 * ineffective. The project stays usable; the banner says what the host will do and can be dismissed.
 */
export function ServerBuildNoticeBanner({ detail }: { detail: string }) {
  return (
    <ShellBanner
      component="server-build-notice-banner"
      title="Server build differs from this host"
      detail={detail}
      tone="info"
      actionPlacement="below"
      icon={<LucideIcon icon={ServerCog} name="server-cog" />}
      action={
        <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissServerBuildNotice.attrs}>
          Dismiss
        </button>
      }
    />
  );
}
