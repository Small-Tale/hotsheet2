import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { CircleFadingArrowUp } from 'lucide';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ShellBanner } from './shell-banner';

/**
 * The host now serves a newer web client than the bundle this page loaded (HS2-KPJWBK). It is a
 * persistent, dismissible info strip, never an automatic reload: Reload first flushes unsaved text
 * edits through the same path as leaving the page (HS2-RE1PS6).
 */
export function ClientUpdateNoticeBanner() {
  return (
    <ShellBanner
      component="client-update-notice-banner"
      title="A newer version of Hot Sheet is available"
      detail="Reload to use it. Unsaved edits are saved before the page reloads."
      tone="info"
      actionPlacement="below"
      icon={<LucideIcon icon={CircleFadingArrowUp} name="circle-fading-arrow-up" />}
      action={
        <Row gap="xs">
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.dismissClientUpdateNotice.attrs}>
            Dismiss
          </button>
          <button type="button" {...PROJECT_LIFECYCLE_ACTIONS.reloadForClientUpdate.attrs}>
            Reload
          </button>
        </Row>
      }
    />
  );
}
