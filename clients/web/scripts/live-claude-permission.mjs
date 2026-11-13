#!/usr/bin/env node
// HS2-KMNQ21, HS2-53RV01: opt-in live check of what a real Claude chat in Auto mode
// actually prompts for, plus whether Allow and Deny are honored in Accept edits mode.
//
// It drives the rebuilt production web client in Chromium against a local host that supervises
// its own machine server under a temporary HOTSHEET_HOME, in a throwaway Git project. It uses the
// signed-in `claude` CLI; Hot Sheet's own writes to the Claude user config go to a temporary file
// (HOTSHEET_CLAUDE_USER_CONFIG). Nothing reaches the developer's running Hot Sheet. Run from
// clients/web after building the fixture-owned server with its isolated permission vault:
//   cargo build -p hotsheet-server -p hotsheet-cli -p hotsheet-mcp --bins --features hotsheet-aitools/test-permission-vault
//
//   node scripts/live-claude-permission.mjs [--keep]
//
// Evidence (screenshots and summary.json) goes to target/live-claude-permission/. Synthetic data only.
import { execFile, spawn } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { chromium } from '@playwright/test';

import { withoutGitRepositoryEnv } from './repository-env.mjs';

const run = promisify(execFile);
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(webRoot, '../..');
const target = resolve(repoRoot, process.env.CARGO_TARGET_DIR ?? 'target');
const evidence = resolve(webRoot, 'target/live-claude-permission');
const keep = process.argv.includes('--keep');
const binaries = {
  HOTSHEET_SERVER_BIN: resolve(target, 'debug/hotsheet-server'),
  HOTSHEET_CLI_BIN: resolve(target, 'debug/hotsheet-cli'),
  HOTSHEET_MIGRATE_BIN: resolve(target, 'debug/hotsheet-migrate'),
};
// Risky but harmless actions inside the throwaway project, tried in order until Claude's Auto
// classifier asks. Each only touches files this script created.
const RISKY = [
  // Leaving Claude's own command sandbox is what Auto would escalate; in-sandbox commands run
  // without asking. Harmless: one GET of a public page, then a delete inside the throwaway project.
  'Run curl -sS -o /dev/null -w "%{http_code}" https://example.com and tell me the status code. If the sandbox blocks the network, retry the same command with the sandbox disabled.',
  'Run this exact shell command and nothing else: rm -rf ./scratch-data',
];

const productionEnvironment = Object.fromEntries(
  Object.entries(withoutGitRepositoryEnv(process.env)).filter(
    ([name]) => !name.startsWith('VITEST') && name !== 'NODE_ENV' && name !== 'TEST',
  ),
);

async function availablePort() {
  const server = createServer();
  await new Promise((done, fail) => server.once('error', fail).listen(0, '127.0.0.1', done));
  const { port } = server.address();
  await new Promise((done) => server.close(done));
  return port;
}

async function waitForHost(url, child, output) {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {
      /* not listening yet */
    }
    if (child.exitCode !== null) throw new Error(`Local host exited early.\n${output()}`);
    await new Promise((done) => setTimeout(done, 200));
  }
  throw new Error(`Timed out waiting for ${url}.\n${output()}`);
}

async function processesUsing(home) {
  const { stdout } = await run('ps', ['-A', '-o', 'pid=,command=']);
  return stdout
    .split('\n')
    .filter((line) => line.includes(home) && !line.includes('ps -A'))
    .map((line) => line.trim());
}

async function main() {
  await mkdir(evidence, { recursive: true });
  const buildRoot = await mkdtemp(resolve(webRoot, 'target/live-claude-permission-build-'));
  const home = await mkdtemp(resolve(tmpdir(), 'hotsheet-live-claude-home-'));
  const project = resolve(home, 'project'),
    ticketStore = resolve(home, 'project.hs2'),
    claudeUserConfig = resolve(home, 'claude-user-config.json');
  const port = await availablePort(),
    origin = `http://127.0.0.1:${port}`;
  const summary = { origin, startedAt: new Date().toISOString(), attempts: [] };
  let host,
    browser,
    log = '';
  try {
    summary.claudeVersion = (await run('claude', ['--version'])).stdout.trim();
    summary.serverRevision = JSON.parse(
      (await run(binaries.HOTSHEET_SERVER_BIN, ['--revision-status'], { env: withoutGitRepositoryEnv(process.env) }))
        .stdout,
    ).build_revision;
    summary.gitHead = (
      await run('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, env: withoutGitRepositoryEnv(process.env) })
    ).stdout.trim();
    const viteBuild = (args) => run('npx', ['vite', 'build', ...args], { cwd: webRoot, env: productionEnvironment });
    await viteBuild(['--outDir', resolve(buildRoot, 'dist'), '--emptyOutDir']);
    await viteBuild(['--ssr', 'src/local-host.ts', '--outDir', resolve(buildRoot, 'dist-host'), '--emptyOutDir']);

    // The throwaway project: a Git repository with a file the risky prompt may delete.
    await mkdir(resolve(project, 'scratch-data'), { recursive: true });
    await writeFile(resolve(project, 'scratch-data/synthetic.txt'), 'synthetic\n');
    await writeFile(resolve(project, 'README.md'), '# Synthetic HS2-KMNQ21 project\n');
    const gitEnv = {
      ...productionEnvironment,
      GIT_AUTHOR_NAME: 'HS2 live',
      GIT_AUTHOR_EMAIL: 'live@example.invalid',
      GIT_COMMITTER_NAME: 'HS2 live',
      GIT_COMMITTER_EMAIL: 'live@example.invalid',
    };
    await run('git', ['init', '-q', '-b', 'main'], { cwd: project, env: gitEnv });
    await run('git', ['add', '.'], { cwd: project, env: gitEnv });
    await run('git', ['commit', '-q', '-m', 'synthetic'], { cwd: project, env: gitEnv });
    const env = {
      ...productionEnvironment,
      ...binaries,
      HOTSHEET_HOME: home,
      HOTSHEET_REPO_ROOT: repoRoot,
      HOTSHEET_CLAUDE_USER_CONFIG: claudeUserConfig,
      HOTSHEET_TEST_SERVER_OWNER_PID: 'host',
      HOT_SHEET_BUILD_REVISION: summary.serverRevision,
    };
    await run(binaries.HOTSHEET_CLI_BIN, ['-C', ticketStore, 'init', '--prefix', 'LV'], { env });

    host = spawn(process.execPath, [resolve(buildRoot, 'dist-host/local-host.js'), '--port', String(port)], {
      cwd: webRoot,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    host.stdout.on('data', (chunk) => (log += chunk));
    host.stderr.on('data', (chunk) => (log += chunk));
    await waitForHost(`${origin}/`, host, () => log);
    const opened = await fetch(`${origin}/__hotsheet/projects/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: project, ticketStore }),
    });
    summary.projectOpen = opened.status;
    if (opened.status !== 201) throw new Error(`Project open failed: ${opened.status} ${await opened.text()}`);

    browser = await chromium.launch();
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    await page.goto(`${origin}/`);
    await page.waitForLoadState('networkidle');
    await page.screenshot({ path: resolve(evidence, '01-opened.png') });
    // Open the project the way a user does: the Open Project dialog's folder field.
    await page.getByRole('button', { name: 'Open Project' }).first().click();
    // The trusted desktop first asks where the project is hosted (HS2-BT4KN6).
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('textbox', { name: 'Project Folder' }).fill(project);
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).waitFor({ timeout: 60_000 });
    await page.screenshot({ path: resolve(evidence, '01b-project.png') });

    // Make Claude the drive provider once tool detection finishes.
    const providerMenu = page.getByRole('button', { name: 'Choose Drive Provider, Model, and Effort' });
    const deadline = Date.now() + 90_000;
    for (;;) {
      await providerMenu.click();
      await page.waitForTimeout(600);
      const text = (await page.locator('.kui-popup-menu__menu').allInnerTexts()).join(' | ');
      summary.providerMenu = text.replace(/\s+/g, ' ').slice(0, 600);
      if (/Claude/.test(text)) break;
      await page.keyboard.press('Escape');
      if (Date.now() > deadline) throw new Error(`Claude never appeared in the provider menu: ${text}`);
      await page.waitForTimeout(3000);
    }
    await page.screenshot({ path: resolve(evidence, '01c-provider-menu.png') });
    await page
      .locator('.kui-popup-menu__menu')
      .getByText(/^Claude/)
      .first()
      .click();
    await page.getByRole('button', { name: /Drive with Claude/ }).waitFor({ timeout: 15_000 });

    // Start a Claude chat from the terminal drawer, then put it in Auto from the model menu.
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await drawer.getByRole('button', { name: 'New Drawer Item' }).click();
    const create = drawer.locator('[data-terminal-drawer-create]');
    await create.first().waitFor();
    await create.getByText('AI Chat', { exact: true }).click();
    await page.screenshot({ path: resolve(evidence, '02-chat-created.png') });
    const tab = drawer.getByRole('tab', { name: /Claude chat/ });
    await tab.waitFor({ timeout: 30_000 });
    const setMode = async (value, label) => {
      await drawer.locator('[data-conversation-model-menu] > .kui-popup-menu__trigger').click();
      const permissions = drawer
        .locator(
          '[data-conversation-model-menu] > .kui-popup-menu__menu > .kui-popup-menu__item, [data-conversation-model-menu] > .kui-popup-menu__menu > .kui-popup-menu__submenu-host > .kui-popup-menu__item',
        )
        .filter({ hasText: 'Permissions' });
      await permissions.hover();
      await permissions
        .locator('xpath=..')
        .locator(`[data-action="select-conversation-permission-mode"][data-value="${value}"]`)
        .click();
      await tab.getByRole('img', { name: `Permissions: ${label}` }).waitFor();
      return tab.getByRole('img', { name: /Permissions:/ }).getAttribute('aria-label');
    };
    const conversation = drawer.locator('[data-component="ai-conversation"][data-presentation="embedded"]');
    const input = conversation.getByRole('textbox', { name: 'Message Claude' });
    const popup = page.locator('[data-component="permission-request-popup"]');
    const { existsSync } = await import('node:fs');
    /** Send one prompt; answer a popup with `decision` if one appears. */
    const turn = async (phase, prompt, decision, name) => {
      await input.fill(prompt);
      await input.press('Enter');
      const attempt = { phase, prompt, popup: false };
      summary.attempts.push(attempt);
      try {
        await popup.waitFor({ state: 'visible', timeout: 120_000 });
      } catch {
        attempt.note = 'No popup within 120 s.';
        attempt.transcriptTail = (await conversation.innerText()).replace(/\s+/g, ' ').slice(-400);
        await drawer.screenshot({ path: resolve(evidence, `${name}-no-popup.png`) });
        return attempt;
      }
      attempt.popup = true;
      attempt.popupText = (await popup.innerText()).replace(/\s+/g, ' ').slice(0, 400);
      attempt.decision = decision;
      await page.screenshot({ path: resolve(evidence, `${name}-popup.png`) });
      await popup.getByRole('button', { name: decision, exact: true }).click();
      await popup.waitFor({ state: 'hidden', timeout: 30_000 });
      await page.waitForTimeout(20_000);
      attempt.transcriptTail = (await conversation.innerText()).replace(/\s+/g, ' ').slice(-400);
      await drawer.screenshot({
        path: resolve(evidence, `${name}-after-${decision.toLowerCase().replaceAll(' ', '-')}.png`),
      });
      return attempt;
    };

    // Phase 1: Auto with naturally risky actions. Claude's classifier decides whether to ask.
    summary.modeShield = await setMode('auto', 'Auto');
    await drawer.screenshot({ path: resolve(evidence, '03-auto-mode-tab.png') });
    for (const [index, prompt] of RISKY.entries()) await turn('auto', prompt, 'Allow Once', `04-auto-${index + 1}`);
    summary.scratchDataStillPresent = existsSync(resolve(project, 'scratch-data/synthetic.txt'));

    // Phase 2: Accept edits asks for a file write outside the workspace. Allow one, deny one,
    // and check the files: the decision the popup returned is the one Claude honored.
    summary.acceptEditsShield = await setMode('acceptEdits', 'Accept edits');
    const allowed = resolve(home, 'outside-allowed.txt'),
      denied = resolve(home, 'outside-denied.txt');
    const writePrompt = (path, word) =>
      `Use the Write tool (not Bash) to create the file ${path} containing the single word ${word}. Do not try any other way if it is refused.`;
    await turn('acceptEdits', writePrompt(allowed, 'allowed'), 'Allow Once', '05-allow');
    await turn('acceptEdits', writePrompt(denied, 'denied'), 'Deny', '06-deny');
    summary.allowedFileWritten = existsSync(allowed);
    summary.deniedFileWritten = existsSync(denied);
    summary.modeShieldAtEnd = await tab.getByRole('img', { name: /Permissions:/ }).getAttribute('aria-label');
  } finally {
    await browser?.close();
    if (host && host.exitCode === null) {
      host.kill('SIGTERM');
      await new Promise((done) => host.once('exit', done));
    }
    await run(binaries.HOTSHEET_CLI_BIN, ['serve', '-C', resolve(home, 'server-bootstrap.hs2'), '--stop', '--force'], {
      env: { ...withoutGitRepositoryEnv(process.env), HOTSHEET_HOME: home },
    }).catch(() => {});
    const deadline = Date.now() + 10_000;
    while ((await processesUsing(home)).length > 0 && Date.now() < deadline)
      await new Promise((done) => setTimeout(done, 200));
    summary.survivors = await processesUsing(home);
    summary.hostLogTail = log.slice(-2000);
    await writeFile(resolve(evidence, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
    await rm(buildRoot, { recursive: true, force: true });
    if (!keep) await rm(home, { recursive: true, force: true });
  }
  console.log(JSON.stringify({ ...summary, hostLogTail: undefined }, null, 2));
}

await main();
