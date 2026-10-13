import { CatalogExample } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { BulkUpdateProgress } from '../bulk-update-progress';
import { AppLoadingIndicator } from '../components/app-loading-indicator';
import type { DemoModule } from './demo-module';

export const appLoadingDemoProgress = signal<BulkUpdateProgress | undefined>(undefined);

export function AppLoadingIndicatorDemo() {
  return (
    <CatalogExample
      label="App loading indicator states"
      note="Choose a loading state; the app indicator appears in the viewport corner."
    >
      <Row gap="xs" wrap>
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
      </Row>
      <AppLoadingIndicator progress={appLoadingDemoProgress.value} />
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireAppLoadingIndicatorDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'click', '[data-app-loading-demo]', (_event, target) => {
      const state = (target as HTMLElement).dataset.appLoadingDemo;
      appLoadingDemoProgress.value =
        state === 'start'
          ? { completed: 0, total: 2 }
          : state === 'progress'
            ? { completed: 1, total: 2 }
            : state === 'complete'
              ? { completed: 2, total: 2 }
              : undefined;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'app-loading-indicator': {
      catalogExamples: true,
      Demo: AppLoadingIndicatorDemo,
      reset: () => {
        appLoadingDemoProgress.value = undefined;
      },
    },
  },
  wire: wireAppLoadingIndicatorDemo,
};
