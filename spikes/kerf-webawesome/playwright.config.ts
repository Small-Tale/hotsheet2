import { defineConfig } from '@playwright/test';

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
