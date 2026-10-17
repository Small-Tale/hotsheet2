import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';
import { wireCornerStatus } from '@kerfjs/ui/wire-corner-status';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { BulkUpdateProgress } from '../bulk-update-progress';
import { AppLoadingIndicator } from '../components/app-loading-indicator';
import type { DemoModule } from './demo-module';

export const appLoadingDemoProgress = signal<BulkUpdateProgress | undefined>(undefined);

export function AppLoadingIndicatorDemo() {
  return (
    <CatalogExample
      label="App Loading Indicator States"
      note="Choose a loading state; the app indicator appears in the viewport corner."
    >
      <Row gap="xs" wrap>
        <Button type="button" rootAttributes={{ 'data-app-loading-demo': 'project' }}>
          Project Load
        </Button>
        <Button type="button" rootAttributes={{ 'data-app-loading-demo': 'start' }}>
          Update Start
        </Button>
        <Button type="button" rootAttributes={{ 'data-app-loading-demo': 'progress' }}>
          Update Progress
        </Button>
        <Button type="button" rootAttributes={{ 'data-app-loading-demo': 'complete' }}>
          Update Complete
        </Button>
      </Row>
      <AppLoadingIndicator progress={appLoadingDemoProgress.value} />
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireAppLoadingIndicatorDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(wireCornerStatus(root));
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
