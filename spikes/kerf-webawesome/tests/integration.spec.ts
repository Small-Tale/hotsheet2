import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await page.waitForFunction(() => customElements.get('wa-input') && customElements.get('wa-dialog'));
});

test('uses host input/change events and does not emit wa-prefixed aliases', async ({ page }) => {
  const input = page.locator('wa-input');
  await input.locator('input').fill('edited');
  await input.locator('input').press('Tab');
  await expect(page.locator('[data-value]')).toHaveText('edited');
  const events = await page.evaluate(() => (globalThis as typeof globalThis & { spike: { events: string[] } }).spike.events);
  expect(events).toContain('input');
  expect(events).toContain('change');
  expect(events).not.toContain('wa-input');
  expect(events).not.toContain('wa-change');
});

test('kerf morph preserves the upgraded element, live value, and focus', async ({ page }) => {
  const host = page.locator('wa-input');
  await host.locator('input').fill('survives');
  const identityBefore = await page.evaluate(() => {
    const el = document.querySelector('wa-input')!;
    (globalThis as typeof globalThis & { originalInput?: Element }).originalInput = el;
    return el.localName;
  });
  await page.locator('[data-action="rerender"]').click();
  expect(identityBefore).toBe('wa-input');
  expect(await page.evaluate(() => document.querySelector('wa-input') === (globalThis as typeof globalThis & { originalInput?: Element }).originalInput)).toBe(true);
  await expect(host).toHaveJSProperty('value', 'survives');
  await host.locator('input').focus();
  await page.locator('[data-action="rerender"]').evaluate((button: HTMLElement) => button.click());
  await expect(host.locator('input')).toBeFocused();
});

test('dialog custom events delegate and keyboard dismissal remains accessible', async ({ page }) => {
  await page.locator('[data-action="open"]').click();
  await expect(page.locator('wa-dialog')).toHaveJSProperty('open', true);
  await page.keyboard.press('Escape');
  await expect(page.locator('wa-dialog')).toHaveJSProperty('open', false);
  const events = await page.evaluate(() => (globalThis as typeof globalThis & { spike: { events: string[] } }).spike.events);
  expect(events).toEqual(expect.arrayContaining(['wa-show', 'wa-after-show', 'wa-hide', 'wa-after-hide']));
});

test('theme token changes and production build makes no external requests', async ({ page }) => {
  const requests: string[] = [];
  page.on('request', request => {
    if (!request.url().startsWith('http://127.0.0.1:4199')) requests.push(request.url());
  });
  await page.reload();
  await page.waitForFunction(() => customElements.get('wa-button'));
  const button = page.locator('wa-button[variant="brand"]');
  const before = await button.evaluate(el => getComputedStyle(el).getPropertyValue('--wa-color-brand-fill-loud').trim());
  await page.locator('[data-action="theme"]').click();
  const after = await button.evaluate(el => getComputedStyle(el).getPropertyValue('--wa-color-brand-fill-loud').trim());
  expect(after).not.toBe(before);
  expect(requests).toEqual([]);
});
