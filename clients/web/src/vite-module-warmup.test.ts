import { describe, expect, it } from 'vitest';

import { appRequests } from '../tests/app-requests';
import { type WarmableEnvironment, warmClientModuleGraph } from './vite-module-warmup';

/** A fake Vite client environment whose graph is known only after each module is transformed. */
function environment(graph: Record<string, string[]>, failing: string[] = []) {
  const transformed: string[] = [];
  const known = new Set<string>();
  const fake: WarmableEnvironment = {
    transformRequest(url) {
      transformed.push(url);
      if (failing.includes(url)) return Promise.reject(new Error(`cannot transform ${url}`));
      known.add(url);
      return Promise.resolve({ code: '' });
    },
    moduleGraph: {
      getModuleByUrl(url) {
        return Promise.resolve(
          known.has(url) ? { importedModules: (graph[url] ?? []).map((imported) => ({ url: imported })) } : undefined,
        );
      },
    },
  };
  return { fake, transformed };
}

describe('warmClientModuleGraph (HS2-15EYK1)', () => {
  it('transforms every module reachable from the entry exactly once, including shared and cyclic imports', async () => {
    const { fake, transformed } = environment({
      '/src/main.tsx': ['/src/app.tsx', '/src/style.css'],
      '/src/app.tsx': ['/src/shared.ts', '/node_modules/.vite/deps/kerfjs.js?v=1'],
      '/src/style.css': ['/src/shared.ts'],
      '/src/shared.ts': ['/src/app.tsx'],
    });
    expect(await warmClientModuleGraph(fake, '/src/main.tsx')).toBe(5);
    expect([...transformed].sort()).toEqual([
      '/node_modules/.vite/deps/kerfjs.js?v=1',
      '/src/app.tsx',
      '/src/main.tsx',
      '/src/shared.ts',
      '/src/style.css',
    ]);
  });

  it('skips a module that fails to transform and keeps warming its siblings', async () => {
    const { fake, transformed } = environment(
      { '/src/main.tsx': ['/src/broken.ts', '/src/fine.ts'], '/src/broken.ts': ['/src/never.ts'] },
      ['/src/broken.ts'],
    );
    expect(await warmClientModuleGraph(fake, '/src/main.tsx')).toBe(3);
    expect(transformed).not.toContain('/src/never.ts');
    expect(transformed).toContain('/src/fine.ts');
  });

  it('visits only the entry when it imports nothing', async () => {
    const { fake, transformed } = environment({});
    expect(await warmClientModuleGraph(fake, '/src/main.tsx')).toBe(1);
    expect(transformed).toEqual(['/src/main.tsx']);
  });
});

describe('appRequests (HS2-15EYK1)', () => {
  it('leaves Vite module requests to the network and still routes app and API requests', () => {
    for (const url of [
      'http://127.0.0.1:4175/src/main.tsx',
      'http://hotsheet-remote.test/src/app/style.css',
      'http://127.0.0.1:4175/node_modules/.vite/deps/kerfjs.js?v=abc',
      'http://127.0.0.1:4175/@vite/env',
      'http://127.0.0.1:4175/@id/virtual:module',
      'http://127.0.0.1:4175/@fs/Users/example/file.ts',
    ])
      expect(appRequests.test(url), url).toBe(false);
    for (const url of [
      'http://127.0.0.1:4175/',
      'http://hotsheet-remote.test/',
      'http://127.0.0.1:4175/__hotsheet/projects/open',
      'http://127.0.0.1:4175/__hotsheet/project-api/terminal-feedback/tickets?text=src',
      'http://127.0.0.1:4175/api/a/permissions',
      'http://127.0.0.1:4175/project/src/tickets',
      'http://127.0.0.1:4175/favicon.svg',
    ])
      expect(appRequests.test(url), url).toBe(true);
  });
});
