import {installDevReloadDiagnostics} from './dev-reload-diagnostics';

// Keep `import.meta.hot` out of main.tsx: Vite rewrites any module containing it to import
// `/@vite/client`, even after the HTML client tag is removed. Stable-dev never imports this
// entry module; dev:hot and Playwright retain the diagnostics and ordinary HMR behavior.
const hot=import.meta.hot;
if(hot){
  installDevReloadDiagnostics({
    hot:{on:(event,callback)=>{hot.on(event as 'vite:beforeFullReload',callback)}},
    storage:sessionStorage,
    persistentStorage:localStorage,
  });
}
