import { getPriority, setPriority } from 'node:os';

import { defineConfig } from '@playwright/test';

// Run local Playwright at a lower CPU priority so several parallel suites can't starve the
// machine; its workers, browsers, and web server inherit it. PLAYWRIGHT_NICE overrides the
// default of 10 (0 opts out), and CI keeps its priority unless overridden.
{
  const requested = Number.parseInt(process.env.PLAYWRIGHT_NICE ?? '', 10);
  if ((process.env.CI ?? '') === '' || Number.isFinite(requested)) {
    const target = Math.min(19, Number.isFinite(requested) ? requested : 10);
    try {
      if (target > getPriority()) setPriority(target);
    } catch {
      // Unsupported platform or permission: keep the inherited priority.
    }
  }
}

export default defineConfig({
  testDir: './tests',
  workers: 1,
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
    { name: 'webkit', use: { browserName: 'webkit' } },
  ],
  use: { baseURL: 'http://127.0.0.1:4199' },
  webServer: {
    command: 'npm run build && npx vite preview --host 127.0.0.1 --port 4199',
    port: 4199,
    reuseExistingServer: false,
  },
});
