import { expect, type Locator, type Page, test } from '@playwright/test';

/** Set one glance-gauge demo setting through its live control, as a person's choice would. */
async function choose(page: Page, name: string, value: string): Promise<void> {
  await page
    .locator(`[data-settings="glance-gauge"] [name="${name}"]`)
    .evaluate((node: HTMLElement & { value: string }, next) => {
      node.value = next;
      node.dispatchEvent(new Event('change', { bubbles: true }));
    }, value);
}

/** The needle's rendered rotation in degrees, from its computed transform matrix. */
function needleRotation(gauge: Locator): Promise<number> {
  return gauge.locator('.glance-gauge__needle').evaluate((node) => {
    const matrix = new DOMMatrixReadOnly(getComputedStyle(node).transform);
    return Math.round((Math.atan2(matrix.b, matrix.a) * 180) / Math.PI);
  });
}

test('glance gauge names its reading, sweeps the needle, and keeps bare gauges text-free (HS2-CVE3SK)', async ({
  page,
}) => {
  await page.goto('/ux-demo?component=glance-gauge');
  const interactive = page.getByRole('region', { name: 'Interactive' }).locator('svg.glance-gauge');

  // The hover title is the accessible name: label, reading with unit, and the 30-day peak.
  const typical = page.getByRole('img', { name: 'Token rate: 1.6k tokens/min (30-day peak 6.2k)' });
  await expect(interactive).toHaveAccessibleName('Token rate: 1.6k tokens/min (30-day peak 6.2k)');
  await expect(interactive.locator('title')).toHaveText('Token rate: 1.6k tokens/min (30-day peak 6.2k)');
  await expect(interactive.locator('.glance-gauge__value')).toHaveText('1.6k');
  await expect(interactive.locator('.glance-gauge__unit')).toHaveText('tokens/min');
  expect(await typical.count()).toBeGreaterThan(0);
  await expect.poll(() => needleRotation(interactive)).toBe(-58);

  // Bare gauges at chip height carry no text on the face; the title says everything.
  const bareTokens = page
    .getByRole('region', { name: 'Bare at Chip Height' })
    .locator('svg[data-size="chip"][data-tone="brand"]');
  const bareTickets = page
    .getByRole('region', { name: 'Bare at Chip Height' })
    .locator('svg[data-size="chip"][data-tone="success"]');
  await expect(bareTokens).toHaveAccessibleName('Token rate: 1.6k tokens/min (30-day peak 6.2k)');
  await expect(bareTickets).toHaveAccessibleName('Tickets updated: 14 tickets/h (30-day peak 22)');
  await expect(bareTokens.locator('text')).toHaveCount(0);
  const chip = await bareTokens.boundingBox();
  expect(chip?.width).toBe(40);
  expect(chip?.height).toBeCloseTo(31.2, 0);

  // A new reading updates the name and sweeps the same needle node (it is morphed, not replaced).
  const needle = interactive.locator('.glance-gauge__needle');
  await needle.evaluate((node) => {
    Object.assign(node, { hs2Cve3skIdentity: 'kept' });
  });
  await choose(page, 'reading', '100');
  await expect(interactive).toHaveAccessibleName('Token rate: 6.2k tokens/min (30-day peak 6.2k)');
  expect(await needle.evaluate((node) => (node as Element & { hs2Cve3skIdentity?: string }).hs2Cve3skIdentity)).toBe(
    'kept',
  );
  await expect(needle).toHaveCSS('transition-duration', '0.6s');
  await expect.poll(() => needleRotation(interactive)).toBe(120);

  // Above its peak the reading pins to the end; meaning switches to green tickets updated/h.
  await choose(page, 'reading', '140');
  await expect(interactive).toHaveAttribute('data-angle', '120');
  await choose(page, 'tone', 'success');
  await expect(interactive).toHaveAccessibleName('Tickets updated: 31 tickets/h (30-day peak 22)');
  await expect(interactive.locator('.glance-gauge__fill')).toHaveCSS(
    'stroke',
    await page.evaluate(() => {
      const probe = document.createElement('span');
      probe.style.color = 'var(--kui-color-success-fill-loud)';
      document.body.append(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    }),
  );

  // Reset restores the defaults in the live controls and the gauge, and a further edit applies.
  const closedSettings = page.locator('[data-action="toggle-settings"][aria-expanded="false"]');
  if ((await closedSettings.count()) > 0) await closedSettings.first().click();
  await page.locator('[data-settings="glance-gauge"] [data-action="reset-settings"]').click();
  await expect(interactive).toHaveAccessibleName('Token rate: 1.6k tokens/min (30-day peak 6.2k)');
  await expect(page.locator('[data-settings="glance-gauge"] [name="reading"]')).toHaveJSProperty('value', '26');
  await choose(page, 'variant', 'bare');
  await expect(interactive).toHaveAttribute('data-variant', 'bare');
  await expect(interactive.locator('text')).toHaveCount(0);
});

test('glance gauge jumps straight to a new reading under reduced motion (HS2-CVE3SK)', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/ux-demo?component=glance-gauge');
  const interactive = page.getByRole('region', { name: 'Interactive' }).locator('svg.glance-gauge');
  await expect(interactive.locator('.glance-gauge__needle')).toHaveCSS('transition-duration', '0s');
  await choose(page, 'reading', '64');
  await expect(interactive).toHaveAccessibleName('Token rate: 4k tokens/min (30-day peak 6.2k)');
  expect(await needleRotation(interactive)).toBe(34);
});
