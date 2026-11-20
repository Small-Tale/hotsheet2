import type { Page } from '@playwright/test';

import { allFeaturesOn } from '../src/feature-areas';

/**
 * HS2-3GJZGG: turn every feature area on for a spec that exercises an experimental or optional feature
 * through mocked routes. Register it after the spec's own catch-all route: Playwright tries the most
 * recently registered route first. A real-server spec gets the same through `startRealTicketServer`.
 */
export async function routeAllFeaturesOn(page: Page): Promise<void> {
  await page.route(
    (url) => url.pathname.endsWith('/feature-settings'),
    (route) => route.fulfill({ json: allFeaturesOn() }),
  );
}
