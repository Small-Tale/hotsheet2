import { existsSync, readdirSync, readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import config from '../playwright.config';

describe('Playwright output isolation (HS2-ZJJ4K6)', () => {
  it('rotates browser artifacts only under the generated directory excluded from UI doctor traversal', () => {
    expect(config.outputDir).toBe('target/playwright-test-results');
  });

  it('keeps visual capture paths portable and prepares their nested folders (HS2-SRGAH3)', () => {
    const tests = new URL('../tests/', import.meta.url);
    for (const name of readdirSync(tests).filter((name) => name.endsWith('.spec.ts')))
      expect(readFileSync(new URL(name, tests), 'utf8'), name).not.toContain('/private/tmp/');
    expect(existsSync(new URL('../target/visual-captures/claude/', import.meta.url))).toBe(true);
    expect(existsSync(new URL('../target/visual-captures/claude-501/', import.meta.url))).toBe(true);
  });
});

describe('Playwright web server ownership (HS2-BJ00D1)', () => {
  it('waits on its own server identity and never reuses another run’s server', () => {
    const server = Array.isArray(config.webServer) ? config.webServer[0] : config.webServer;
    expect(server?.reuseExistingServer).toBe(false);
    const id = server?.env?.HOTSHEET_WEB_TEST_SERVER_ID;
    expect(id).toMatch(/^[0-9a-f-]{36}$/u);
    expect(server?.url).toBe(`${config.use?.baseURL}/__hotsheet-test-server/${id}`);
  });
});

describe('Playwright web server lifetime (HS2-F6F8MA)', () => {
  it('binds the detached Vite group to the runner through the owned launcher’s stdin', () => {
    const server = Array.isArray(config.webServer) ? config.webServer[0] : config.webServer;
    // A runner ended by SIGTERM or SIGKILL skips Playwright's teardown; stdin EOF must stop Vite.
    expect(server?.command).toMatch(
      /^node scripts\/owned-server\.mjs --exit-on-stdin-eof -- node node_modules\/vite\/bin\/vite\.js /u,
    );
  });
});
