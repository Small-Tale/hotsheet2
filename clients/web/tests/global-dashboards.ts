import { expect, type Page } from '@playwright/test';

/** The shell modes the desktop project strip's Global Dashboards switch to. */
export type GlobalDashboard = 'terminals' | 'systems' | 'stats';

const LABELS: Record<GlobalDashboard, string> = {
  terminals: 'Workspace Grid',
  systems: 'Systems',
  stats: 'Cross-Project Stats',
};

/**
 * HS2-3VGYFG: open a Global Dashboard from the desktop project strip whether its buttons show or have
 * folded into the one Global Dashboards menu, as they do while the project tabs overflow (HS2-B0RJBY).
 * The fold follows Kerf's fit report a frame after layout, so a choice that raced it is made again
 * until the strip shows the dashboard.
 */
export async function openGlobalDashboard(page: Page, dashboard: GlobalDashboard): Promise<void> {
  const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)').first(),
    folded = strip.locator('[data-dashboard-menu]'),
    button = strip.getByRole('button', { name: LABELS[dashboard], exact: true });
  await expect(async () => {
    // Sub-budget action cap (HS2-4DCFMG): each 2 s click bounds one attempt of this toPass retry,
    // which re-reads the fold; a click delivered late lands before the next attempt checks the mode.
    if ((await folded.count()) > 0) {
      await folded.getByRole('button', { name: 'Global Dashboards' }).click({ timeout: 2_000 });
      await page.locator(`[data-action="set-shell-mode"][data-shell-mode="${dashboard}"]`).click({ timeout: 2_000 });
    } else await button.click({ timeout: 2_000 });
    await expect(strip).toHaveAttribute('data-mode', dashboard, { timeout: 2_000 });
  }).toPass({ timeout: 30_000 });
}
