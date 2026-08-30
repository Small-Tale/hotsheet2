import { defineConfig } from '@playwright/test';

export default defineConfig({testDir:'tests',webServer:{command:'npm run dev',url:'http://127.0.0.1:4175/ux-demo',reuseExistingServer:true},use:{baseURL:'http://127.0.0.1:4175'}});
