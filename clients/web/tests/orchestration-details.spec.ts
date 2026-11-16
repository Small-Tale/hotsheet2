import { expect, test } from '@playwright/test';

import {
  openOrchestration,
  orchestrateSelf,
  planTwoSteps,
  realTicketServer,
  visualPass,
} from './orchestration-real-fixtures';

/**
 * The orchestration details signed off on HS2-2XJSJK, through the real Orchestration view and its card
 * inspector against a real server. Each test names the build ticket it covers.
 */

test('HS2-15G5T4 the card inspector’s Plan section reads the readiness plan', async ({ page }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await orchestrateSelf(server);
    const plan = await planTwoSteps(server);
    await plan.dispatch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openOrchestration(page, server);
    const view = page.locator('[data-orchestration-view="true"]');
    const inspector = page.locator('[data-orchestration-inspector]');
    const section = inspector.locator('[data-orchestration-section="Plan"]');

    // The running step: in progress, and it unblocks the step that waits on it.
    const working = view.locator(`[data-ticket-slug="${plan.first.slug}"]`);
    await working.click();
    await expect(section).toContainText('In progress');
    await expect(section).toContainText(plan.second.slug);
    // Plan sits between Where It Runs and Run.
    const order = await inspector
      .locator('[data-orchestration-section]')
      .evaluateAll((nodes) => nodes.map((node) => node.getAttribute('data-orchestration-section')));
    expect(order.slice(0, 3)).toEqual(['Where It Runs', 'Plan', 'Run']);

    // The waiting step: blocked, and nothing waits on it.
    const waiting = view.locator(`[data-ticket-slug="${plan.second.slug}"]`);
    await waiting.click();
    await expect(section).toContainText('Blocked');
    await expect(section).toContainText('Nothing waits on it');
    await expect(inspector.locator('[data-orchestration-section="Prerequisites"]')).toContainText(plan.first.slug);

    await working.click();
    await visualPass(page, 'hs2-15g5t4', 'plan-running', {
      check: () => expect(section).toContainText('In progress'),
      atPhone: () => view.locator(`[data-ticket-slug="${plan.first.slug}"]`).click(),
    });
  } finally {
    await server.stop(page);
  }
});
