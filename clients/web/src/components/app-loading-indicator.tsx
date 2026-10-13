import { CornerStatus } from '@kerfjs/ui/corner-status';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { StateBanner } from '@kerfjs/ui/state-banner';

import type { BulkUpdateProgress } from '../bulk-update-progress';

/** The id of the app's corner loading status, which `wireCornerStatus` opens in the top layer. */
export const APP_LOADING_INDICATOR_ID = 'app-loading-indicator';

/**
 * The app's layout-neutral loading feedback for project loads and provider writes: a Kerf CornerStatus
 * (KF-PZCPHQ, HS2-5C4Y5Q) pinned to the bottom-end corner in the top layer, never focused and never
 * dismissible, whose StateBanner is the polite live region. The owner renders it only while work runs.
 */
export function AppLoadingIndicator({ progress }: { progress?: BulkUpdateProgress }) {
  const updating = progress !== undefined;
  return (
    <CornerStatus id={APP_LOADING_INDICATOR_ID} placement="bottom-end">
      <StateBanner
        title={updating ? `Updating tickets… ${progress.completed} of ${progress.total}` : 'Loading…'}
        tone="neutral"
        urgency="status"
        icon={<LoadingSpinner label={updating ? 'Updating tickets' : 'Loading project'} size={16.8} />}
      />
    </CornerStatus>
  );
}
