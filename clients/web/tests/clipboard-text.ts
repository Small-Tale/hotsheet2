import type { Page } from '@playwright/test';

/**
 * The page's clipboard text with line endings normalized to LF (HS2-YBYCAB).
 *
 * The app writes LF text through `navigator.clipboard.writeText`. Chromium on Windows stores it on the
 * OS clipboard with CRLF line endings and reads it back that way, as every Windows paste target expects.
 * Comparing the read-back with the app's LF text would then fail on line endings alone, so compare it
 * normalized. The exact content and line order are still asserted.
 */
export async function readClipboardText(page: Page): Promise<string> {
  return (await page.evaluate(() => navigator.clipboard.readText())).replace(/\r\n/g, '\n');
}
