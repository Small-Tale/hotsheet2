import { defineConfig } from '@playwright/test';

const port=Number(process.env.HOTSHEET_WEB_TEST_PORT??4176);
export default defineConfig({testDir:'tests',webServer:{command:`npm run dev:hot -- --port ${port}`,url:`http://127.0.0.1:${port}/ux-demo`,reuseExistingServer:false},use:{baseURL:`http://127.0.0.1:${port}`}});
