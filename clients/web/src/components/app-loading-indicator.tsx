import './app-loading-indicator.css';

import { LoadingSpinner } from '@kerfjs/ui/loading-spinner';
import { StateBanner } from '@kerfjs/ui/state-banner';

import type { BulkUpdateProgress } from '../bulk-update-progress';

/** The app's fixed, layout-neutral loading feedback for project loads and provider writes. */
export function AppLoadingIndicator({ progress }: { progress?: BulkUpdateProgress }) {
  const updating = progress !== undefined;
  return (
    <div
      class="app-loading app-loading-indicator"
      data-component="app-loading-indicator"
      data-loading-kind={updating ? 'tickets' : 'project'}
    >
      <StateBanner
        title={updating ? `Updating tickets… ${progress.completed} of ${progress.total}` : 'Loading…'}
        tone="neutral"
        urgency="status"
        icon={<LoadingSpinner label={updating ? 'Updating tickets' : 'Loading project'} size={16.8} />}
      />
    </div>
  );
}
