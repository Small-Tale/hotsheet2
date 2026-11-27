import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import config from '../playwright.config';
import { projectTab } from '../tests/open-mocked-project';

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

describe('Playwright native keychain isolation (HS2-XTQHPK)', () => {
  it('denies the OS keychain to the runner, its workers, and the dev bridge they spawn', () => {
    // The config sets it on process.env before the web server and workers start, so both inherit it.
    expect(process.env.HOTSHEET_NATIVE_SECRETS).toBe('deny');
  });
});

describe('mocked project open waits for the ticket list (HS2-GDBK53)', () => {
  it('opens mocked projects through openMockedProject, except where a test holds the list read', () => {
    const tests = new URL('../tests/', import.meta.url),
      raw =
        /(?<!\/\/ HS2-GDBK53:[^\n]*\n[ \t]*)await (\w+)\.getByRole\('button', \{ name: 'Open Project' \}\)\.click\(\);\s*await \1\.getByRole\('button', \{ name: 'Local Project' \}\)\.click\(\);\s*await \1\.getByRole\('button', \{ name: 'Open Project', exact: true \}\)\.last\(\)\.click\(\);/;
    const offenders = readdirSync(tests)
      .filter((name) => name.endsWith('.spec.ts'))
      .filter((name) => raw.test(readFileSync(new URL(name, tests), 'utf8')));
    expect(offenders).toEqual([]);
  });
});

describe('project tabs are found by name, not by their full label (HS2-YGSYJT)', () => {
  it('locates page-level project tabs through projectTab, never by exact accessible name', () => {
    // A project tab's accessible name carries its work counts ("drafts 1 Up Next ticket"), so an
    // exact-name match stops finding it once its project gains work. Scoped tab lists (the inspector,
    // the terminal drawer, a pending tab's own name) may still match exactly.
    const tests = new URL('../tests/', import.meta.url),
      exact = /\bpage\.getByRole\(\s*'tab',\s*\{\s*name: [^,}]+,\s*exact: true\s*\}\)/u;
    const offenders = readdirSync(tests)
      .filter((name) => name.endsWith('.ts'))
      .filter((name) => exact.test(readFileSync(new URL(name, tests), 'utf8')));
    expect(offenders).toEqual([]);
  });

  it('matches a project name before its counts and nothing longer', () => {
    let used: { name?: unknown } = {};
    const scope = {
      getByRole: (_role: string, options: { name?: unknown }) => {
        used = options;
        return {};
      },
    } as unknown as Parameters<typeof projectTab>[0];
    const pattern = (project: string) => {
      projectTab(scope, project);
      return used.name as RegExp;
    };
    expect(pattern('drafts').test('drafts')).toBe(true);
    expect(pattern('drafts').test('drafts 1 Up Next ticket')).toBe(true);
    expect(pattern('drafts').test('drafts-old')).toBe(false);
    expect(pattern('other').test('other-project')).toBe(false);
    expect(pattern('a.b').test('axb')).toBe(false);
  });
});

/**
 * A loaded host can hold an actionable control's click for seconds while the app stays idle
 * (HS2-8DPRAB), so an action capped below the test budget fails for the host, not the app. An action
 * timeout under the budget must sit under a `Sub-budget action cap (HS2-4DCFMG): <reason>` comment
 * naming the retry, probe or latency requirement it bounds.
 */
const ACTION_CAP =
  /\.(click|dblclick|tap|check|uncheck|setChecked|hover|focus|fill|press|dragTo|selectOption)\(\s*(?:[^(){}]*,\s*)?\{[^{}]*?\btimeout\s*:\s*([0-9_]+)[^{}]*\}/gu;
const CAP_REASON = /\/\/ Sub-budget action cap \(HS2-4DCFMG\): \S/u;

function uncommentedActionCaps(source: string, budget: number): number[] {
  const lines = source.split('\n'),
    offenders: number[] = [];
  for (const match of source.matchAll(ACTION_CAP)) {
    if (Number(match[2].replaceAll('_', '')) >= budget) continue;
    const line = source.slice(0, match.index).split('\n').length;
    if (!lines.slice(Math.max(0, line - 6), line).some((text) => CAP_REASON.test(text))) offenders.push(line);
  }
  return offenders;
}

describe('Playwright action caps stay within the test budget (HS2-4DCFMG)', () => {
  it('hs2_4dcfmg_no_uncommented_sub_budget_action_caps in tests/', () => {
    const tests = new URL('../tests/', import.meta.url),
      budget = config.timeout!;
    expect(budget).toBe(60_000);
    const offenders = readdirSync(tests)
      .filter((name) => name.endsWith('.ts'))
      .flatMap((name) =>
        uncommentedActionCaps(readFileSync(new URL(name, tests), 'utf8'), budget).map((line) => `${name}:${line}`),
      );
    expect(offenders).toEqual([]);
  });

  it('hs2_4dcfmg_detector flags a bare cap and accepts a named reason or the budget', () => {
    expect(uncommentedActionCaps('await a.click({ timeout: 10_000 });', 60_000)).toEqual([1]);
    expect(uncommentedActionCaps('await a\n  .click({ trial: true, timeout: 2_000 });', 60_000)).toEqual([2]);
    expect(uncommentedActionCaps("await a.fill('x', { timeout: 3_000 });", 60_000)).toEqual([1]);
    expect(uncommentedActionCaps('await a.click({ timeout: 60_000 });', 60_000)).toEqual([]);
    expect(uncommentedActionCaps('await a.click();', 60_000)).toEqual([]);
    expect(
      uncommentedActionCaps(
        '// Sub-budget action cap (HS2-4DCFMG): bounds one toPass attempt.\nawait a.click({ timeout: 3_000 });',
        60_000,
      ),
    ).toEqual([]);
    // A marker without a reason does not count.
    expect(
      uncommentedActionCaps('// Sub-budget action cap (HS2-4DCFMG):\nawait a.click({ timeout: 3_000 });', 60_000),
    ).toEqual([2]);
  });
});
