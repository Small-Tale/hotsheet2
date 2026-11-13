import { CornerStatus } from '@kerfjs/ui/corner-status';
import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { StateBanner } from '@kerfjs/ui/state-banner';

import type { BulkUpdateProgress } from '../bulk-update-progress';

/** The id of the app's corner loading status, which `wireCornerStatus` opens in the top layer. */
export const APP_LOADING_INDICATOR_ID = 'app-loading-indicator';

/**
 * The app's layout-neutral loading feedback for project loads, provider writes, and a background AI
 * request such as Add to Group › Automatic (HS2-4C32A0): a Kerf CornerStatus
 * (KF-PZCPHQ, HS2-5C4Y5Q) pinned to the bottom-end corner in the top layer, never focused and never
 * dismissible, whose StateBanner is the polite live region. The owner renders it only while work runs.
 */
export function AppLoadingIndicator({ progress, task }: { progress?: BulkUpdateProgress; task?: string }) {
  const updating = progress !== undefined;
  // A ticket update in progress outranks a background AI request, such as Automatic grouping (HS2-4C32A0).
  return (
    <CornerStatus id={APP_LOADING_INDICATOR_ID} placement="bottom-end">
      <StateBanner
        title={updating ? `Updating tickets… ${progress.completed} of ${progress.total}` : (task ?? 'Loading…')}
        tone="neutral"
        urgency="status"
        icon={
          <LoadingSpinner
            label={updating ? 'Updating tickets' : task !== undefined ? 'Waiting for the AI' : 'Loading project'}
            size={16.8}
          />
        }
      />
    </CornerStatus>
  );
}
