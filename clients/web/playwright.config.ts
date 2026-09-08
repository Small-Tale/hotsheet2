import { mkdtempSync,rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from '@playwright/test';

const port=Number(process.env.HOTSHEET_WEB_TEST_PORT??4176);
const viteCacheDir=mkdtempSync(join(tmpdir(),'hotsheet-playwright-vite-'));
process.once('exit',()=>{rmSync(viteCacheDir,{recursive:true,force:true})});

export default defineConfig({testDir:'tests',webServer:{command:`npm run dev:hot -- --port ${port}`,url:`http://127.0.0.1:${port}/ux-demo`,reuseExistingServer:false,env:{HOTSHEET_VITE_CACHE_DIR:viteCacheDir,HOTSHEET_WEB_STABLE_DEV:'1'}},use:{baseURL:`http://127.0.0.1:${port}`}});
