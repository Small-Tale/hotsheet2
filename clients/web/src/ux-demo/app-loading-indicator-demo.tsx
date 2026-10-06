import { signal } from 'kerfjs';

import type { BulkUpdateProgress } from '../bulk-update-progress';
import { AppLoadingIndicator } from '../components/app-loading-indicator';

export const appLoadingDemoProgress = signal<BulkUpdateProgress | undefined>(undefined);

export function AppLoadingIndicatorDemo() {
  return (
    <section aria-label="App loading indicator states">
      <p>Choose a loading state; the app indicator appears in the viewport corner.</p>
      <wa-button type="button" data-app-loading-demo="project">
        Project load
      </wa-button>
      <wa-button type="button" data-app-loading-demo="start">
        Update start
      </wa-button>
      <wa-button type="button" data-app-loading-demo="progress">
        Update progress
      </wa-button>
      <wa-button type="button" data-app-loading-demo="complete">
        Update complete
      </wa-button>
      <AppLoadingIndicator progress={appLoadingDemoProgress.value} />
    </section>
  );
}
