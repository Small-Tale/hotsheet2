import { describe, expect, it } from 'vitest';

import {
  stableDevHmr,
  stableDevInjectClientScript,
  stripStableDevClientTag,
  viteDependencyIsolation,
} from '../vite.config';

describe('Vite dependency isolation', () => {
  it('disables runtime discovery, pre-bundles the scanned browser dependencies, and uses the stable session cache', () => {
    expect(
      viteDependencyIsolation({ HOTSHEET_WEB_STABLE_DEV: '1', HOTSHEET_VITE_CACHE_DIR: '/tmp/stable-cache' }, () => [
        'kerfjs',
        'lucide',
      ]),
    ).toEqual({
      cacheDir: '/tmp/stable-cache',
      optimizeDeps: { noDiscovery: true, include: ['kerfjs', 'lucide'] },
    });
  });

  it('scans the real browser graph for stable dev by default (HS2-N9RD7X)', () => {
    const include = viteDependencyIsolation({ HOTSHEET_WEB_STABLE_DEV: '1' }).optimizeDeps?.include ?? [];
    expect(include).toContain('kerfjs');
    expect(include).toContain('@xterm/xterm');
  });

  it('isolates a hot test server cache without disabling hot dependency discovery', () => {
    expect(viteDependencyIsolation({ HOTSHEET_VITE_CACHE_DIR: '/tmp/test-cache' })).toEqual({
      cacheDir: '/tmp/test-cache',
    });
    expect(viteDependencyIsolation({})).toEqual({});
  });

  it('disables HMR only in the frozen stable-dev snapshot (HS2-8JV12R)', () => {
    expect(stableDevHmr({ HOTSHEET_WEB_STABLE_DEV: '1' })).toBe(false);
    expect(stableDevHmr({})).toBeUndefined();
  });

  it('skips the @hono/vite-dev-server client injection only in stable-dev so nothing auto-reloads (HS2-8JV12R)', () => {
    // false → no `<script>import("/@vite/client")</script>`, so no websocket and no connection-loss reload.
    expect(stableDevInjectClientScript({ HOTSHEET_WEB_STABLE_DEV: '1' })).toBe(false);
    // undefined → the dev-server injects the client as usual (npm run dev:hot / Playwright keep HMR).
    expect(stableDevInjectClientScript({})).toBeUndefined();
  });

  it('removes Vite core client tags only from stable-dev index HTML (HS2-8JV12R)', () => {
    const html =
      '<head><script type="module" src="/@vite/client"></script><script type="module" src="/src/main.tsx"></script></head>';
    expect(stripStableDevClientTag(html, { HOTSHEET_WEB_STABLE_DEV: '1' })).toBe(
      '<head><script type="module" src="/src/main.tsx"></script></head>',
    );
    expect(stripStableDevClientTag(html, {})).toBe(html);
    expect(
      stripStableDevClientTag('<script src="/base/@vite/client" type="module"></script><main>ok</main>', {
        HOTSHEET_WEB_STABLE_DEV: '1',
      }),
    ).toBe('<main>ok</main>');
  });
});
