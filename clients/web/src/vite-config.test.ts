import { describe, expect, it } from 'vitest';

import {
  bundleClientBuildId,
  clientBuildIdPlugin,
  developmentClientBuildId,
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

describe('client build id stamping (HS2-KPJWBK)', () => {
  type Handler = (html: string, context: { bundle?: Record<string, { type: string; fileName: string }> }) => unknown;
  const handler = (id: string) => {
    const hook = clientBuildIdPlugin(id).transformIndexHtml as { order: string; handler: Handler };
    expect(hook.order).toBe('post');
    return hook.handler;
  };

  it('derives the production id from the hashed bundle names, independent of order', () => {
    const id = bundleClientBuildId(['assets/index-AAA.js', 'assets/index-BBB.css']);
    expect(id).toMatch(/^bundle-[0-9a-f]{16}$/);
    expect(bundleClientBuildId(['assets/index-BBB.css', 'assets/index-AAA.js'])).toBe(id);
    expect(bundleClientBuildId(['assets/index-CCC.js', 'assets/index-BBB.css'])).not.toBe(id);
  });

  it('stamps the bundle id in a build and the process id under the dev server', () => {
    const bundle = {
      'assets/index-AAA.js': { type: 'chunk', fileName: 'assets/index-AAA.js' },
      'assets/index-BBB.css': { type: 'asset', fileName: 'assets/index-BBB.css' },
      'assets/index-AAA.js.map': { type: 'asset', fileName: 'assets/index-AAA.js.map' },
      'favicon.svg': { type: 'asset', fileName: 'favicon.svg' },
    };
    expect(handler('dev-x')('<html></html>', { bundle })).toEqual([
      {
        tag: 'meta',
        attrs: {
          name: 'hotsheet-client-build',
          content: bundleClientBuildId(['assets/index-AAA.js', 'assets/index-BBB.css']),
        },
        injectTo: 'head',
      },
    ]);
    expect(handler('dev-x')('<html></html>', {})).toEqual([
      { tag: 'meta', attrs: { name: 'hotsheet-client-build', content: 'dev-x' }, injectTo: 'head' },
    ]);
  });

  it('gives each development server process its own id', () => {
    expect(developmentClientBuildId(36, 'ab12cd34')).toBe('dev-10-ab12cd34');
    expect(developmentClientBuildId()).not.toBe(developmentClientBuildId());
  });
});
