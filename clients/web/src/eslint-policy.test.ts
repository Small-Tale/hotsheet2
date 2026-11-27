import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

import { ESLint } from 'eslint';
import ts from 'typescript';
import { beforeAll, describe, expect, it } from 'vitest';

import packageJson from '../package.json';

describe('web ESLint policy', () => {
  it('HS2-7KXGMC schedules the cold policy route before parallel unit work without dropping fast-suite coverage', () => {
    const config = readFileSync(new URL('../vitest.config.ts', import.meta.url), 'utf8');
    expect(config).toContain(
      "name: 'eslint-policy', include: ['src/eslint-policy.test.ts'], sequence: { groupOrder: 0 }",
    );
    expect(config).toContain("...BINARY_SUITES, 'src/eslint-policy.test.ts'");
    // HS2-3FQNGT adds one isolated jsdom pilot after the policy route.
    expect(config.match(/sequence: \{ groupOrder: 1 \}/g)).toHaveLength(3);
    expect(config).toContain("'src/**/*.dom.test.ts'");
    expect(config).toContain("name: 'dom-pilot'");
    expect(config).toContain("environment: 'jsdom'");
    expect(packageJson.scripts['test:unit:fast']).toContain('--project eslint-policy --project unit');
    expect(config).not.toMatch(/\b(?:hookTimeout|testTimeout|maxWorkers)\b/);
  });

  it('HS2-V94A8M never waits for networkidle in browser tests while the change long poll stays open', () => {
    // A live page always has a change poll in flight (HS2-AZ11V9), so `networkidle` never arrives.
    const tests = new URL('../tests/', import.meta.url);
    const offenders = readdirSync(tests)
      .filter((name) => name.endsWith('.ts'))
      .filter((name) =>
        /networkidle/.test(readFileSync(new URL(name, tests), 'utf8').replace(/^\s*(?:\/\/|\*).*$/gm, '')),
      );
    expect(offenders).toEqual([]);
  });

  it('HS2-6E87E4 proxies routed browser requests only through the shared idempotent-retry helper', () => {
    // A raw `route.fetch` on a reused keep-alive socket fails with ECONNRESET when the server closes
    // it idle (HS2-NWFZ7J); `proxyFetch` retries only GET and HEAD. HS2-QY232J moved the helper to
    // the types-only `tests/proxy-fetch.ts` and extended the scan to `src/` files that drive Playwright.
    const tests = join(import.meta.dirname, '../tests');
    const offenders: string[] = [];
    const scan = (directory: string, playwrightOnly: boolean) => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) scan(path, playwrightOnly);
        else if (/\.[cm]?[jt]sx?$/.test(entry.name) && path !== import.meta.filename) {
          const source = readFileSync(path, 'utf8').replace(/^\s*(?:\/\/|\*).*$/gm, '');
          if (playwrightOnly && !/from '(?:@playwright\/test|playwright(?:-core)?)'/.test(source)) continue;
          const calls = source.match(/\.fetch\(/g)?.length ?? 0;
          const allowed = path === join(tests, 'proxy-fetch.ts') ? 1 : 0;
          // `bridge.fetch(request)` is an APIRequestContext, not a routed request (HS2-ZMVQR4).
          const contexts = source.match(/\bbridge\.fetch\(/g)?.length ?? 0;
          if (calls - contexts !== allowed) offenders.push(relative(join(tests, '..'), path).split(sep).join('/'));
        }
      }
    };
    scan(tests, false);
    scan(import.meta.dirname, true);
    expect(offenders).toEqual([]);
    const helper = readFileSync(join(tests, 'proxy-fetch.ts'), 'utf8');
    expect(helper).toMatch(/const idempotent = method === 'GET' \|\| method === 'HEAD';/);
    // Types only, so `src/` tests can import it without pulling in the real-server fixture.
    expect(helper.match(/^import .*$/gm)).toEqual(["import type { APIResponse, Route } from '@playwright/test';"]);
    expect(readFileSync(join(tests, 'real-ticket-server.ts'), 'utf8')).toContain(
      "export { proxyFetch } from './proxy-fetch.ts';",
    );
  });

  it('HS2-QY232J removes proxy routes before stopping the real server they target', () => {
    // A page still routing to a stopped server proxies its next poll to a closed port, and the
    // route handler's rejection fails the test. `stop(page)` unroutes first.
    const tests = join(import.meta.dirname, '../tests');
    const fixture = readFileSync(join(tests, 'real-ticket-server.ts'), 'utf8');
    expect(fixture).toMatch(
      /const stop = async \(\.\.\.proxies: RouteOwner\[\]\) => \{\n\s+for \(const proxy of proxies\) await proxy\.unrouteAll\(\{ behavior: 'ignoreErrors' \}\);/,
    );
    const offenders: string[] = [];
    for (const name of readdirSync(tests).filter((entry) => entry.endsWith('.ts'))) {
      const source = readFileSync(join(tests, name), 'utf8');
      if (!source.includes('proxyFetch(')) continue;
      const lines = source.split('\n');
      lines.forEach((line, index) => {
        if (!/^\s*await \w+\.stop\(\);$/.test(line)) return;
        // A bare stop is fine once the routes are gone or the page's context or browser is closed.
        const before = lines.slice(Math.max(0, index - 4), index).join('\n');
        if (!/unrouteAll\(|\.close\(\)/.test(before)) offenders.push(`${name}:${String(index + 1)}`);
      });
    }
    expect(offenders).toEqual([]);
  });

  it('HS2-N8FTCK opens routeWebSocket upstreams only through the error-listening, stop-aware helper', () => {
    // `unrouteAll` leaves WebSocket routes installed, so after `stop()` a reconnecting page still
    // reaches the handler. A raw upstream socket opened there dials the stopped server, and without
    // an `error` listener its ECONNREFUSED is unhandled (HS2-6TK3SB). `openUpstreamSocket` listens
    // for `error` and refuses a stopped server; a raw `new …WebSocket(` inside a handler is rejected.
    const tests = join(import.meta.dirname, '../tests');
    const offenders: string[] = [];
    const scan = (directory: string) => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) {
          scan(path);
          continue;
        }
        if (!/\.[cm]?[jt]sx?$/.test(entry.name)) continue;
        const text = readFileSync(path, 'utf8');
        if (!text.includes('routeWebSocket(')) continue;
        const file = ts.createSourceFile(path, text, ts.ScriptTarget.Latest, true);
        const name = relative(tests, path).split(sep).join('/');
        const visit = (node: ts.Node, handler: boolean) => {
          if (
            handler &&
            ts.isNewExpression(node) &&
            /WebSocket$/.test(node.expression.getText(file)) &&
            !/WebSocketServer$/.test(node.expression.getText(file))
          )
            offenders.push(`${name}:${String(file.getLineAndCharacterOfPosition(node.getStart(file)).line + 1)}`);
          const routed =
            ts.isCallExpression(node) &&
            ts.isPropertyAccessExpression(node.expression) &&
            node.expression.name.text === 'routeWebSocket';
          ts.forEachChild(node, (child) => {
            visit(child, handler || routed);
          });
        };
        visit(file, false);
      }
    };
    scan(tests);
    expect(offenders).toEqual([]);
    const fixture = readFileSync(join(tests, 'real-ticket-server.ts'), 'utf8');
    const helper = fixture.slice(fixture.indexOf('export function openUpstreamSocket('));
    expect(helper).toMatch(/if \(stoppedHosts\.has\(host\)\) \{\n\s+closeRoute\(\);\n\s+return undefined;/);
    expect(helper).toContain("upstream.on('error', closeRoute);");
    expect(fixture).toMatch(/if \(host\) \{\n\s+stoppedHosts\.add\(host\);/);
  });

  it('HS2-QY232J waits for the terminal drawer to finish opening before using New Drawer Item', () => {
    // New Drawer Item slides with the ~220 ms drawer-open transition; under load a click during it
    // timed out on an unstable element (HS2-S4FJ85). Wait for the shell's transition flag first.
    const tests = join(import.meta.dirname, '../tests');
    const offenders: string[] = [];
    for (const name of readdirSync(tests).filter((entry) => entry.endsWith('.ts'))) {
      const lines = readFileSync(join(tests, name), 'utf8').split('\n');
      lines.forEach((line, index) => {
        if (!line.includes("name: 'Show Terminal Drawer' }).click()")) return;
        for (const next of lines.slice(index + 1, index + 6)) {
          if (next.includes("'data-terminal-drawer-transitioning', 'false'")) return;
          if (next.includes("name: 'New Drawer Item' })")) {
            offenders.push(`${name}:${String(index + 1)}`);
            return;
          }
        }
      });
    }
    expect(offenders).toEqual([]);
  });

  it('HS2-7YN9VV routes app requests with appRequests instead of a catch-all glob', () => {
    // A `'**/*'` or `'**'` route sends each of the ~850 unbundled Vite module requests of a cold page
    // load through the test worker (HS2-15EYK1). Route `appRequests` from `tests/app-requests.ts`
    // instead. A handler that must see every request, for example to count or delay all of them,
    // goes in this allowlist as `'<file>': '<reason>'`.
    const allowed: Record<string, string> = {};
    const tests = join(import.meta.dirname, '../tests');
    const found: Record<string, number> = {};
    const scan = (directory: string) => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) scan(path);
        else if (/\.[cm]?[jt]sx?$/.test(entry.name)) {
          const source = readFileSync(path, 'utf8').replace(/^\s*(?:\/\/|\*).*$/gm, '');
          const count = source.match(/\.route\(\s*(['"`])\*\*(?:\/\*)?\1/g)?.length ?? 0;
          if (count > 0) found[relative(tests, path).split(sep).join('/')] = count;
        }
      }
    };
    scan(tests);
    expect(Object.keys(found).filter((file) => !(file in allowed))).toEqual([]);
    for (const [file, reason] of Object.entries(allowed)) {
      expect(found[file], `${file} no longer needs its catch-all allowlist entry`).toBeGreaterThan(0);
      expect(reason.trim()).not.toBe('');
    }
    const helper = readFileSync(join(tests, 'app-requests.ts'), 'utf8');
    expect(helper).toContain('export const appRequests = /^(?![a-z]+:');
  });

  it('HS2-3TWWWD bounds the type-aware ESLint heap so it does not depend on machine memory', () => {
    // Typed linting of the client peaks near 3.3 GB and needs about 2.5 GB of heap. Node's default
    // heap limit is derived from RAM, so the 7 GB macOS runner aborted at about 2 GB. Pin an explicit,
    // cross-platform bound on the one ESLint invocation.
    // HS2-9N8ZMD moved this invocation into the observed lint entrypoint. Its focused
    // tests check the ordered arguments and first-failure behavior; keep the heap
    // bound pinned here as a policy on the shipped helper.
    expect(packageJson.scripts['lint:web']).toBe('node scripts/lint-web-observations.mjs');
    const helper = readFileSync(new URL('../scripts/lint-web-observations.mjs', import.meta.url), 'utf8');
    expect(helper).toMatch(/id: 'web-lint:eslint',[\s\S]*?'--max-old-space-size=4096'/);
    expect(helper).toMatch(/id: 'web-lint:eslint',[\s\S]*?'node_modules\/eslint\/bin\/eslint\.js'/);
    expect(Object.values(packageJson.scripts).filter((script) => /\beslint(?:\.js)? \./.test(script))).toEqual([]);
  });

  it('HS2-TY5JHN bounds the type-aware Kerf UI doctor heap the same way', () => {
    // After HS2-3TWWWD the macOS runner's ESLint passed and the next type-aware step, the Kerf UI
    // doctor, aborted at the same RAM-derived ~2 GB default heap. Pin the same explicit bound.
    expect(packageJson.scripts['ui:doctor']).toBe(
      'node --max-old-space-size=4096 node_modules/@kerfjs/ui/doctor/cli.mjs --full',
    );
    expect(Object.values(packageJson.scripts).filter((script) => /\bkerf-ui-doctor\b/.test(script))).toEqual([]);
  });

  it('HS2-N9X6D5 keeps browser fixture timers for measured windows and cadence only', () => {
    const read = (name: string) => readFileSync(new URL(`../scripts/${name}`, import.meta.url), 'utf8');
    const stable = read('stable-dev.e2e.test.mjs');
    const scale = read('scale-stress.mjs');
    const profile = read('change-stream-worker-profile.mjs');
    expect(stable.match(/\.waitForTimeout\(/g)).toHaveLength(2);
    expect(stable).toContain('waitForTimeout(reconnectObservationMs)');
    expect(stable).toContain('waitForTimeout(lateReloadObservationMs)');
    expect(stable).toContain("response.url().includes('/migration-jobs')");
    // A 202 or a generic app control is not migration readiness: both pages wait for the rendered job banner.
    expect(stable.match(/await renderedImportJob\((?:wide|narrow)\)/g)).toHaveLength(2);
    expect(stable).toContain('[data-component="hs1-job-banner"]');
    expect(scale.match(/\.waitForTimeout\(/g)).toHaveLength(1);
    expect(scale).toContain('waitForTimeout(redundantRequestObservationMs)');
    expect(profile.match(/\.waitForTimeout\(/g)).toHaveLength(1);
    expect(profile).toContain('waitForTimeout(clickCadenceMs)');
    expect(profile).toContain('lastVisibleChurnSlug');
  });

  // HS2-X3JM3J: each proof runs the real ESLint route; a typed lint of a src/ path builds the
  // TypeScript program inside its case (2-13 s on a loaded gate host). The budget covers that work;
  // no lint result is cached.
  describe('HS2-RY22P6 shares configuration setup across actual Node lint proofs', { timeout: 30_000 }, () => {
    const webRoot = join(import.meta.dirname, '..');
    const eslint = new ESLint({ cwd: webRoot });
    const scripts = readdirSync(join(webRoot, 'scripts'))
      .filter((name) => /\.(?:mjs|mts)$/.test(name))
      .map((name) => `scripts/${name}`);
    beforeAll(async () => {
      // Config/plugin loading is shared setup, not the first file's lint work (HS2-RY22P6).
      // Keep the default hook budget and every per-input proof; no lint result is cached here.
      await eslint.calculateConfigForFile(join(webRoot, 'eslint.config.mjs'));
    });
    // HS2-2SPPTQ: preserve every real lint input without aggregating their work into one timed case.
    it.each([...scripts, 'eslint.config.mjs'])('HS2-2SPPTQ preserves HS2-HK6KMT lint coverage for %s', async (name) => {
      const filePath = join(webRoot, name);
      expect(await eslint.isPathIgnored(filePath)).toBe(false);
      const realResults = await eslint.lintFiles([filePath]);
      expect(realResults.map((result) => result.filePath)).toEqual([filePath]);
      expect(realResults.every((result) => result.errorCount === 0 && result.warningCount === 0)).toBe(true);
    });

    it('HS2-2SPPTQ preserves HS2-HK6KMT rejection through the actual Node lint route', async () => {
      const [invalid] = await eslint.lintText('const value = missingReference;\n', {
        filePath: join(webRoot, 'scripts/hs2-hk6kmt-invalid.mjs'),
      });
      expect(invalid.messages.map((message) => message.ruleId)).toContain('no-undef');
    });

    it('HS2-K5E2KF rejects imports of the trailing-task implementation', async () => {
      const [invalid] = await eslint.lintText(
        "import { createTrailingTaskInternal } from './trailing-task-internal';\n",
        {
          filePath: join(webRoot, 'src/trailing-task.test.ts'),
        },
      );
      expect(invalid.messages.map((message) => message.ruleId)).toContain('no-restricted-imports');
      const publicSource = readFileSync(join(webRoot, 'src/trailing-task.ts'), 'utf8');
      expect(publicSource.match(/^export (?:interface|function).*$/gm)).toEqual([
        'export interface TrailingTask<T> {',
        'export function createTrailingTask<T>(delayMs: number, run: (value: T) => void): TrailingTask<T> {',
      ]);
    });

    it('HS2-ECT6FE rejects every form of the web APIs the desktop webview cannot show (HS2-JVSFES, HS2-WMZ6Z5)', async () => {
      const forms = [
        "alert('a');",
        "confirm('a');",
        "prompt('a');",
        "window.alert('a');",
        "window.confirm('a');",
        "window.prompt('a');",
        "globalThis.confirm('a');",
        "self.prompt('a');",
        "open('https://example.test');",
        "window.open('https://example.test');",
        "globalThis.open('https://example.test');",
      ];
      const [result] = await eslint.lintText(`${forms.join('\n')}\n`, {
        filePath: join(webRoot, 'src/child-process.ts'),
      });
      const banned = new Set(['no-alert', 'no-restricted-globals', 'no-restricted-properties']);
      const flaggedLines = new Set(
        result.messages.filter((message) => banned.has(message.ruleId ?? '')).map((message) => message.line),
      );
      expect([...flaggedLines].sort((left, right) => left - right)).toEqual(forms.map((_, index) => index + 1));
    });

    it('HS2-ECT6FE HS2-6S1346 HS2-0A7FWS allows no native dialog and opens windows only through openExternal', () => {
      let dialogs = 0;
      const windowOpeners: string[] = [];
      const scan = (directory: string) => {
        for (const entry of readdirSync(directory, { withFileTypes: true })) {
          const path = join(directory, entry.name);
          if (entry.isDirectory()) scan(path);
          else if (/\.tsx?$/.test(entry.name) && !entry.name.endsWith('.test.ts')) {
            const source = readFileSync(path, 'utf8');
            // Any suppressed alert, confirm, or prompt counts, whatever ticket its reason names.
            dialogs += source.match(/eslint-disable[^\n]*\bno-alert\b/g)?.length ?? 0;
            // Any suppressed window.open (the only no-restricted-properties use for it).
            for (const match of source.match(/eslint-disable[^\n]*\bno-restricted-properties\b[^\n]*/g) ?? [])
              if (/open|window|HS2-0A7FWS/i.test(match))
                windowOpeners.push(relative(webRoot, path).split(sep).join('/'));
          }
        }
      };
      scan(join(webRoot, 'src'));
      // Confirmations use the shared in-app ConfirmationDialog (HS2-6S1346). New windows go through
      // src/open-external.ts, which the desktop shell routes to the system browser (HS2-0A7FWS).
      expect(dialogs).toBe(0);
      expect(windowOpeners).toEqual(['src/open-external.ts']);
    });
  });

  it('HS2-6ABCVC keeps the four Kerf packages on one exact version', () => {
    const versions = [
      packageJson.dependencies.kerfjs,
      packageJson.dependencies['@kerfjs/ui'],
      packageJson.devDependencies['create-kerf-component'],
      packageJson.devDependencies['eslint-plugin-kerfjs'],
    ];
    expect(new Set(versions).size).toBe(1);
    for (const version of versions) {
      expect(version).toMatch(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
    }
  });

  it('HS2-WMZ6Z5 runs all replacement-parent flows in the required WebKit route', () => {
    const packageSource = readFileSync(new URL('../package.json', import.meta.url), 'utf8');
    const script = packageSource.match(/"test:ci:webkit": "([^"]+)"/)?.[1];
    expect(script).toContain('tests/providers.spec.ts');
    expect(script).toContain('tests/native-prompt-replacements.spec.ts');
    expect(script).toContain('--browser=webkit');
    expect(script).toContain('--grep @ci-webkit');
    const flows = readFileSync(new URL('../tests/native-prompt-replacements.spec.ts', import.meta.url), 'utf8');
    expect(flows.match(/@ci-smoke @ci-webkit/g)).toHaveLength(3);
  });

  it('HS2-9HPSW9 runs the note click re-flow regression in the required WebKit route', () => {
    // WebKit has no scroll anchoring, so it is the stricter engine for the HS2-628K80 re-flow.
    const packageSource = readFileSync(new URL('../package.json', import.meta.url), 'utf8');
    const script = packageSource.match(/"test:ci:webkit": "([^"]+)"/)?.[1];
    expect(script).toContain('tests/note-click-reflow.spec.ts');
    const spec = readFileSync(new URL('../tests/note-click-reflow.spec.ts', import.meta.url), 'utf8');
    expect(spec.match(/\(\$\{surface\}\) @ci-webkit`/g)).toHaveLength(1);
  });

  it('HS2-WM3SBV ignores generated web target files globally', () => {
    const config = readFileSync(new URL('../eslint.config.mjs', import.meta.url), 'utf8');
    const globalIgnores = config.match(/ignores: \[([\s\S]*?)\]/)?.[1];
    expect(globalIgnores).toContain("'target/**'");
    expect(globalIgnores?.match(/'target\/\*\*'/g)).toHaveLength(1);
  });

  it('HS2-ZGVG8T renders disclosures with Kerf Disclosure, never a hand-built native details element', () => {
    // Kerf's component selection: "Do not hand-build a details element with its own marker CSS"; use
    // Disclosure (@kerfjs/ui/disclosure). Tests, which may assert on Kerf's rendered <details>, are exempt.
    const offenders: string[] = [];
    const scan = (directory: string) => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) scan(path);
        else if (/\.[jt]sx?$/.test(entry.name) && !/\.(?:dom\.)?test\.[jt]sx?$/.test(entry.name)) {
          const source = readFileSync(path, 'utf8').replace(/^\s*(?:\/\/|\*).*$/gm, '');
          if (/<details[\s>]|<summary[\s>]|createElement\(\s*['"](?:details|summary)['"]/.test(source))
            offenders.push(
              relative(import.meta.dirname, path)
                .split(sep)
                .join('/'),
            );
        } else if (entry.name.endsWith('.css')) {
          const css = readFileSync(path, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
          if (/(?:^|[\s,>+~(])(?:details|summary)(?=[\s,.:[>{)]|$)/m.test(css))
            offenders.push(
              relative(import.meta.dirname, path)
                .split(sep)
                .join('/'),
            );
        }
      }
    };
    scan(import.meta.dirname);
    expect(offenders).toEqual([]);
  });

  it('HS2-GXJTSE insets every shell banner like ShellBanner: 8px around and between', () => {
    // The shell's banner slot (hot-sheet-shell.tsx `banner={…}`) stacks banners 8px from its sides,
    // top, and bottom and 8px apart: each banner takes only the Kerf List's top control inset, and the
    // slot's bottom padding ends the stack. Side insets on some banners and top+bottom insets on
    // another left the stack with two side insets and a doubled gap (HS2-GXJTSE).
    const shell = readFileSync(join(import.meta.dirname, 'features/hot-sheet-shell.tsx'), 'utf8'),
      slot = /\n\s+banner=\{([\s\S]*?)\n\s+pageHeader=/.exec(shell)?.[1] ?? '',
      names = [
        ...new Set(
          [...slot.matchAll(/<([A-Z]\w*Banner\w*)\b|\{([A-Z]\w*Banner)\(/g)].map((m) => m.slice(1).find(Boolean) ?? ''),
        ),
      ];
    expect(names.length).toBeGreaterThan(10);
    const components = join(import.meta.dirname, 'components'),
      sources = readdirSync(components)
        .filter((name) => name.endsWith('.tsx'))
        .map((name) => readFileSync(join(components, name), 'utf8')),
      offenders: string[] = [];
    for (const name of names) {
      const source = sources.find((text) => new RegExp(`export function ${name}\\b`).test(text));
      if (!source) {
        offenders.push(`${name}: not found`);
        continue;
      }
      const body = source.slice(source.search(new RegExp(`export function ${name}\\b`)));
      const end = body.indexOf('\nexport function ', 1);
      const own = end > 0 ? body.slice(0, end) : body;
      // Composes ShellBanner, delegates to another banner component, or insets only the top.
      const insets = [...own.matchAll(/controlInsets="(\w+)"/g)].map((m) => m[1]);
      const ok =
        /<ShellBanner\b/.test(own) ||
        (insets.length > 0 && insets.every((value) => value === 't')) ||
        (!/<StateBanner\b/.test(own) && /<[A-Z]\w*Banner\b/.test(own));
      if (!ok) offenders.push(name);
    }
    expect(offenders).toEqual([]);
  });

  it('HS2-GFZYCZ never runs an infinite animation on a shape inside an SVG', () => {
    // WebKit composites a transform animation on a CSS box (an outer <svg>, a span) but repaints an
    // animated SVG shape every frame. An infinite one kept an idle Safari web app at 20% CPU or more per
    // indicator for as long as it showed (HS2-GFZYCZ). Animate the <svg> box itself instead.
    const shapes = new Set(['path', 'circle', 'rect', 'line', 'polyline', 'polygon', 'ellipse', 'g', 'use', 'text']);
    const files = (directory: string, extension: string): string[] =>
      readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) return files(path, extension);
        return entry.name.endsWith(extension) && !entry.name.includes('.test.') ? [path] : [];
      });
    const markup = files(import.meta.dirname, '.tsx')
      .map((path) => readFileSync(path, 'utf8'))
      .join('\n');
    const offenders: string[] = [];
    let infinite = 0;
    for (const path of files(import.meta.dirname, '.css')) {
      const css = readFileSync(path, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
      for (const [, selectors, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
        if (!/animation(?:-iteration-count)?\s*:[^;]*\binfinite\b/.test(body)) continue;
        infinite += 1;
        for (const selector of selectors.split(',')) {
          const subject =
            selector
              .trim()
              .split(/[\s>+~]+/)
              .pop() ?? '';
          const tag = /^[a-z][\w-]*/.exec(subject)?.[0];
          const classes = [...subject.matchAll(/\.([\w-]+)/g)].map((match) => match[1]);
          const tags = classes.flatMap((name) =>
            [...markup.matchAll(new RegExp(`<([a-z][\\w-]*)\\b[^>]*\\bclass="[^"]*\\b${name}\\b`, 'g'))].map(
              (match) => match[1],
            ),
          );
          for (const candidate of [tag, ...tags])
            if (candidate && shapes.has(candidate))
              offenders.push(`${relative(import.meta.dirname, path)}: ${selector.trim()} animates <${candidate}>`);
        }
      }
    }
    // The project tab's activity ring and the server-busy bars are the app's infinite animations today.
    expect(infinite).toBeGreaterThanOrEqual(2);
    expect(offenders).toEqual([]);
  });

  it('HS2-N2HB7G reads the machine homes only through the shared blank-as-unset helper', () => {
    // `??` resolved `HOTSHEET_HOME=""` against the working directory and `||` kept whitespace-only
    // values; `machine-home.ts` applies the Rust `machine_home` rule (HS2-1D4DQF) for every bridge.
    const read = /\.\s*HOTSHEET_(?:LEGACY_)?HOME\b|\[\s*['"`]HOTSHEET_(?:LEGACY_)?HOME['"`]\s*\]/;
    const offenders: string[] = [];
    const check = (path: string) => {
      const source = readFileSync(path, 'utf8').replace(/^\s*(?:\/\/|\*).*$/gm, '');
      if (read.test(source))
        offenders.push(
          relative(join(import.meta.dirname, '..'), path)
            .split(sep)
            .join('/'),
        );
    };
    const scan = (directory: string) => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) scan(path);
        else if (/\.[cm]?[jt]sx?$/.test(entry.name) && !/\.test\.[jt]sx?$/.test(entry.name)) check(path);
      }
    };
    scan(import.meta.dirname);
    for (const name of ['vite.config.ts', 'vitest.config.ts', 'playwright.config.ts'])
      check(join(import.meta.dirname, '..', name));
    expect(offenders).toEqual(['src/machine-home.ts']);
  });

  it('HS2-GVSJ5Y reads every other path-valued HOTSHEET_* variable through nonBlank', () => {
    // `||`, `??`, and truthiness kept whitespace-only overrides as paths, and `??` resolved an empty
    // HOTSHEET_REPO_ROOT against the working directory. Every read from an environment record, by
    // name or through `[variable]`, must be the direct argument of `nonBlank` (HS2-N2HB7G's rule).
    const names = [
      'HOTSHEET_REPO_ROOT',
      'HOTSHEET_CLI_BIN',
      'HOTSHEET_MIGRATE_BIN',
      'HOTSHEET_SERVER_BIN',
      'HOTSHEET_HOOK_CLI_BIN',
      'HOTSHEET_MCP_BIN',
      'HOTSHEET_MIGRATOR',
      'HOTSHEET_MIGRATION_JOBS',
      'HOTSHEET_CLIENT_READY_FILE',
      'HOTSHEET_DEV_LATENCY_LOG',
      'HOTSHEET_VITE_CACHE_DIR',
      'HOTSHEET_DESKTOP_STORAGE_DIRECTORY',
      // HS2-MVF6X3: the plain-Node launchers in scripts/ use the non-blank.mjs twin.
      'HOTSHEET_WEB_STABLE_TEMP_ROOT',
      'HOTSHEET_WEB_STABLE_SOURCE_ROOT',
      'HOTSHEET_DESKTOP_NODE',
    ].join('|');
    const read = new RegExp(
      String.raw`\b(?:process\.env|environment|env)(?:\.\s*(?:${names})\b|\[\s*['"\x60](?:${names})['"\x60]\s*\]|\[\s*variable\s*\])`,
      'g',
    );
    const offenders: string[] = [];
    const check = (path: string) => {
      const source = readFileSync(path, 'utf8').replace(/^\s*(?:\/\/|\*).*$/gm, '');
      const file = relative(join(import.meta.dirname, '..'), path)
        .split(sep)
        .join('/');
      for (const match of source.matchAll(read)) {
        const before = source.slice(0, match.index),
          after = source.slice(match.index + match[0].length);
        if (/^\s*=(?!=)/.test(after)) continue;
        if (!/\bnonBlank\(\s*$/.test(before)) offenders.push(`${file}: ${match[0]}`);
      }
    };
    const scan = (directory: string) => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const path = join(directory, entry.name);
        if (entry.isDirectory()) scan(path);
        else if (/\.[cm]?[jt]sx?$/.test(entry.name) && !/\.test\.[jt]sx?$/.test(entry.name)) check(path);
      }
    };
    scan(import.meta.dirname);
    for (const name of ['vite.config.ts', 'vitest.config.ts', 'playwright.config.ts'])
      check(join(import.meta.dirname, '..', name));
    // HS2-MVF6X3: the scripts/ launchers too; their test files set these variables rather than read them.
    const scripts = join(import.meta.dirname, '..', 'scripts');
    for (const entry of readdirSync(scripts))
      if (/\.mjs$/.test(entry) && !/\.test\.mjs$/.test(entry)) check(join(scripts, entry));
    expect(offenders).toEqual([]);
  });

  it('HS2-1K6122 gives an action outcome through the toast, never an inline status line', () => {
    // The maintainer's rule (docs/ui-design-philosophy.md): a pressed action's success or failure is
    // announced through the app's toast. A component keeps a live region only for loading, a read
    // failure, or the persistent record of a failed change. This bug recurred several times, so each
    // inline message region is listed here with why it may stay; the list only shrinks.
    const allowed: Record<string, number> = {
      // Read failures, progress and persistent failure records; their action failures move to the
      // toast in HS2-M4P3QM.
      'ai-tool-settings.tsx': 1,
      'command-settings-editor.tsx': 1,
      'host-disk-settings.tsx': 1,
      'guidance-settings.tsx': 1,
      'settings-workspace.tsx': 1,
      'standby-settings.tsx': 1,
      'terminal-memory-settings.tsx': 1,
      'ticket-attachments.tsx': 1,
      'ticket-code-review.tsx': 2,
      'trash-settings.tsx': 1,
      // The server's busy bars name the request in flight (state, not an outcome).
      'server-busy-bars.tsx': 1,
    };
    const components = new URL('./components/', import.meta.url);
    const found: Record<string, number> = {};
    for (const name of readdirSync(components).filter((file) => file.endsWith('.tsx'))) {
      const source = readFileSync(new URL(name, components), 'utf8');
      // A status line that renders a success as its text: `>{status}` or `>{message || status}`.
      expect(source, name).not.toMatch(/>\s*\{\s*(?:[\w.]+\s*\|\|\s*)?(?:[\w.]+\.)?status\s*\}/);
      const regions = [...source.matchAll(/role="status"[^>]*>\s*\{\s*([\w.]*(?:message|Message)[\w.]*)\s*\}/g)];
      if (regions.length) found[name] = regions.length;
    }
    expect(found).toEqual(allowed);
  });

  it('HS2-8Z9VHH words every orchestration, session and remote refusal code the server can return', async () => {
    // Each error enum's `code()` in the server's orchestration model is an answer a person can see.
    const { ORCHESTRATION_REFUSALS } = await import('./orchestration-refusals');
    const model = new URL('../../../crates/hotsheet-ticketing/src/orchestration/', import.meta.url);
    const enums: Record<string, string> = {
      OrchestrationError: 'mod.rs',
      TargetOpError: 'mod.rs',
      DenialReason: 'actions.rs',
      AdmissionReason: 'capacity.rs',
      TargetFailure: 'contract.rs',
      SiteError: 'contract/sites.rs',
      GateError: 'gates.rs',
      PolicyDenial: 'policy.rs',
      ReadinessError: 'readiness.rs',
      RunError: 'runs.rs',
      SessionError: 'sessions.rs',
      ValidationError: 'validation.rs',
      // HS2-A4RVYA
      TemplateError: 'templates.rs',
      // HS2-KXKA7X
      TriggerError: 'triggers.rs',
    };
    const codes = new Set<string>();
    for (const [name, file] of Object.entries(enums)) {
      const source = readFileSync(new URL(file, model), 'utf8');
      const start = source.indexOf(`pub fn code(`, source.indexOf(`impl ${name} {`));
      const body = source.slice(start, source.indexOf('\n    }\n', start));
      const found = [...body.matchAll(/"([a-z0-9_]+)"/g)].map((match) => match[1]);
      expect(found.length, name).toBeGreaterThan(0);
      for (const code of found) codes.add(code);
    }
    // The host's own setup failure (`TOOL_SETUP_FAILED`) is reported as a reason too.
    const cli = readFileSync(
      new URL('../../../crates/hotsheet-cli/src/orchestration/sessions.rs', import.meta.url),
      'utf8',
    );
    codes.add(/TOOL_SETUP_FAILED: &str = "([a-z_]+)"/.exec(cli)![1]);
    expect([...codes].filter((code) => !(code in ORCHESTRATION_REFUSALS))).toEqual([]);
  });

  it('HS2-2Y535C words every reason the scheduler gives for a step it does not place', async () => {
    // `Blocker::code()` and the explain answer's own codes reach Why It Is Not Running as `reason`.
    const { schedulerReasonText } = await import('./orchestration-scheduler');
    const blocker = readFileSync(
      new URL('../../../crates/hotsheet-ticketing/src/orchestration/scheduler.rs', import.meta.url),
      'utf8',
    );
    const start = blocker.indexOf('pub fn code(', blocker.indexOf('impl Blocker {'));
    const codes = [...blocker.slice(start, blocker.indexOf('\n    }\n', start)).matchAll(/"([a-z0-9_]+)"/g)].map(
      (match) => match[1],
    );
    expect(codes.length).toBeGreaterThan(0);
    const explain = readFileSync(
      new URL('../../../crates/hotsheet-cli/src/orchestration/scheduler.rs', import.meta.url),
      'utf8',
    );
    const own = /answer\.reason = if view\.enabled \{\s*"([a-z_]+)"\s*\} else \{\s*"([a-z_]+)"/.exec(explain);
    expect(own).not.toBeNull();
    codes.push(own![1], own![2]);
    // An unworded code falls back to the code with spaces, which is what this case forbids.
    expect(codes.filter((code) => schedulerReasonText(code) === code.replaceAll('_', ' '))).toEqual([]);
  });

  it('HS2-GK6HC8 words every reason a role session is not started', async () => {
    // `RoleBlocker::code()` reaches the overview's Coordination Sessions, a card's Integration rows and the log.
    const { roleReasonPhrase } = await import('./orchestration-scheduler');
    const source = readFileSync(
      new URL('../../../crates/hotsheet-ticketing/src/orchestration/scheduler.rs', import.meta.url),
      'utf8',
    );
    const start = source.indexOf('pub fn code(', source.indexOf('impl RoleBlocker {'));
    const codes = [...source.slice(start, source.indexOf('\n    }\n', start)).matchAll(/"([a-z0-9_]+)"/g)].map(
      (match) => match[1],
    );
    expect(codes).toContain('dispatcher_not_started');
    const unworded = codes.filter((reason) => {
      const phrase = roleReasonPhrase({ standing: 'not_started', reason });
      return phrase === undefined || phrase === reason.replaceAll('_', ' ');
    });
    expect(unworded).toEqual([]);
  });

  it('hs2_gm6a8w_permission_events_use_typed_builders in every spec (HS2-CSPDW8, HS2-Y59J3K, HS2-F3S1ZZ)', () => {
    // Permission and terminal nudges, ticket change and activity events, and the pending list mirror
    // the server's ChangeEvent and PermissionInfo shapes only through the shared typed builders; a
    // hand-written literal drifts (empty store, no project, a ticket slug for the tool, a `turn` the
    // nudge never carries, a placeholder ticket id or store, an activity event without its payload).
    const tests = new URL('../tests/', import.meta.url);
    const builders = readFileSync(new URL('server-event-fixtures.ts', tests), 'utf8');
    const nudgeLiteral = /[{,]\s*kind: '(?:permission_asked|permission_resolved|terminal_question|terminal_halted)'/;
    const builderLiterals = builders.split('\n').filter((line) => nudgeLiteral.test(line));
    expect(builderLiterals).toHaveLength(3);
    for (const line of builderLiterals)
      expect(line).toMatch(/^\s*(?:\)\s*=>\s*)?\(\{ store, kind: 'permission_\w+', id, slug/);
    expect(builders).toMatch(/const terminalEvent = \([^)]*\) =>\s*\(\{ store: '', kind, id, slug: ''/);
    expect(builders).toContain('): ServerPermissionInfo & Request => ({');
    expect(builders).toMatch(
      /const ticketChangeEvent = \([^)]*\) =>\s*\(\{ store: ticket\.connection_id, kind, id: ticket\.native_id, slug: ticket\.slug \}\)/,
    );
    expect(builders).toMatch(
      /const activityEvent = \(store: string, activity: ServerActivity\) =>\s*\(\{ store, kind: 'activity', id: activity\.id, slug: '', activity \}\)/,
    );
    expect(builders.match(/\bsatisfies ChangeEvent\b/g)).toHaveLength(6);
    const ticketChangeLiteral =
      /[{,]\s*kind: '(?:created|updated|deleted|changed|closed|moved|claimed|released|renewed|assigned|note_added|attachment_\w+)'/;
    const offenders: string[] = [];
    for (const name of readdirSync(tests).filter((entry) => entry.endsWith('.ts'))) {
      if (name === 'server-event-fixtures.ts') continue;
      const source = readFileSync(new URL(name, tests), 'utf8');
      source.split('\n').forEach((line, index) => {
        if (nudgeLiteral.test(line)) offenders.push(`${name}:${index + 1} hand-written nudge literal`);
        if (ticketChangeLiteral.test(line)) offenders.push(`${name}:${index + 1} hand-written ticket change literal`);
      });
      // A change event's `kind: 'activity'` is followed by its `id` and `slug`; a ticket note's
      // `kind: 'activity'` (followed by `created_at`) is not a change event.
      for (const match of source.matchAll(/[{,]\s*kind: 'activity'(?: as const)?,\s*id\b[^\n]*\n?\s*slug:/g))
        offenders.push(`${name}:${source.slice(0, match.index).split('\n').length} hand-written activity event`);
      // Builders replace both forms of a hand-written nudge: an `emit('<kind>')` shorthand, and a
      // spread that adds fields (such as a `turn` envelope) the server's nudge never carries.
      if (/emit\(\d*,?\s*'(?:terminal_|permission_)/.test(source)) offenders.push(`${name} emits a nudge by kind`);
      if (/\.\.\.(?:permission\w+Event|terminalEvent)\(/.test(source))
        offenders.push(`${name} extends a nudge builder`);
      if (/emit\(\d*,?\s*'activity'/.test(source)) offenders.push(`${name} emits an activity event by kind`);
      if (/const (?:permission\w+Event|pendingPermission|terminalEvent|ticketChangeEvent|activityEvent) =/.test(source))
        offenders.push(`${name} redefines a shared builder`);
    }
    expect(offenders).toEqual([]);
  });

  it('keeps the bootstrap and application runtime on the shared typed-rule baseline', () => {
    const config = readFileSync(new URL('../eslint.config.mjs', import.meta.url), 'utf8');
    expect(config).not.toContain("files: ['src/main.tsx']");
    expect(config).not.toContain("files: ['src/app/runtime.tsx']");
  });

  it('limits defensive-condition exceptions to explained next-line boundaries', () => {
    const source = readFileSync(new URL('./app/runtime.tsx', import.meta.url), 'utf8');
    const exceptions =
      source.match(
        /eslint-disable-next-line @typescript-eslint\/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type\./g,
      ) ?? [];
    expect(exceptions.length).toBeGreaterThan(0);
    expect(exceptions.length).toBeLessThanOrEqual(50);
    expect(source).not.toContain('eslint-disable @typescript-eslint/no-unnecessary-condition');
    expect(source).not.toContain('use-unknown-in-catch-callback-variable');
  });
});
