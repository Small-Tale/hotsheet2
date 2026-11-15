import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { resolve } from 'node:path';

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
    // `exec` keeps the launcher the group leader under any POSIX `sh -c`, including dash.
    expect(server?.command).toMatch(
      /^(exec )?node scripts\/owned-server\.mjs --exit-on-stdin-eof -- node node_modules\/vite\/bin\/vite\.js /u,
    );
    expect(server?.command.startsWith('exec ')).toBe(process.platform !== 'win32');
  });
});

describe('Playwright bridge home isolation (HS2-9NB1KA)', () => {
  it('gives the dev bridge its own Hot Sheet homes, never the developer real ones', () => {
    const server = Array.isArray(config.webServer) ? config.webServer[0] : config.webServer;
    const home = server?.env?.HOTSHEET_HOME,
      legacy = server?.env?.HOTSHEET_LEGACY_HOME;
    for (const directory of [home, legacy]) {
      expect(directory).toBeTruthy();
      expect(resolve(directory!).startsWith(resolve(tmpdir()))).toBe(true);
      expect(existsSync(directory!)).toBe(true);
    }
    expect(home).not.toBe(legacy);
    expect(home).not.toBe(resolve(homedir(), '.hotsheet2'));
    expect(legacy).not.toBe(resolve(homedir(), '.hotsheet'));
  });
});
