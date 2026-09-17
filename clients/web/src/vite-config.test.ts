import { describe,expect,it } from 'vitest';

import { stableDevHmr,stableDevInjectClientScript,viteDependencyIsolation } from '../vite.config';

describe('Vite dependency isolation',()=>{
  it('disables runtime discovery and uses the stable session cache',()=>{
    expect(viteDependencyIsolation({HOTSHEET_WEB_STABLE_DEV:'1',HOTSHEET_VITE_CACHE_DIR:'/tmp/stable-cache'})).toEqual({
      cacheDir:'/tmp/stable-cache',
      optimizeDeps:{noDiscovery:true,include:[]},
    });
  });

  it('isolates a hot test server cache without disabling hot dependency discovery',()=>{
    expect(viteDependencyIsolation({HOTSHEET_VITE_CACHE_DIR:'/tmp/test-cache'})).toEqual({cacheDir:'/tmp/test-cache'});
    expect(viteDependencyIsolation({})).toEqual({});
  });

  it('disables HMR only in the frozen stable-dev snapshot (HS2-8JV12R)',()=>{
    expect(stableDevHmr({HOTSHEET_WEB_STABLE_DEV:'1'})).toBe(false);
    expect(stableDevHmr({})).toBeUndefined();
  });

  it('skips the @hono/vite-dev-server client injection only in stable-dev so nothing auto-reloads (HS2-8JV12R)',()=>{
    // false → no `<script>import("/@vite/client")</script>`, so no websocket and no connection-loss reload.
    expect(stableDevInjectClientScript({HOTSHEET_WEB_STABLE_DEV:'1'})).toBe(false);
    // undefined → the dev-server injects the client as usual (npm run dev:hot / Playwright keep HMR).
    expect(stableDevInjectClientScript({})).toBeUndefined();
  });
});
