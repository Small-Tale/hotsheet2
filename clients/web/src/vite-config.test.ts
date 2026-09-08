import { describe,expect,it } from 'vitest';

import { viteDependencyIsolation } from '../vite.config';

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
});
