import { execFile, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  appendFile,
  chmod,
  copyFile,
  cp,
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  realpath,
  rename,
  rm,
  stat,
  writeFile,
} from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { chromium } from '@playwright/test';
import { afterAll, beforeAll, expect, it } from 'vitest';

// HS2-587N4D: `npm run prod` serves the built client through the same local bridge without Vite.
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  repoRoot = resolve(webRoot, '../..'),
  run = promisify(execFile);

async function availablePort() {
  const server = createServer();
  await new Promise((resolveListen, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolveListen));
  const address = server.address();
  await new Promise((resolveClose) => server.close(resolveClose));
  if (!address || typeof address === 'string') throw new Error('Could not allocate a test port.');
  return address.port;
}

async function waitForHost(url, child, output) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {
      /* not listening yet */
    }
    if (child.exitCode !== null) throw new Error(`Local host exited early.\n${output()}`);
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  throw new Error(`Timed out waiting for ${url}.\n${output()}`);
}

/** Live processes whose command line mentions `home` (the test's isolated HOTSHEET_HOME). */
async function processesUsing(home) {
  const { stdout } = await run('ps', ['-Ao', 'pid=,command=']);
  return stdout
    .split('\n')
    .filter((line) => line.includes(home))
    .map((line) => line.trim());
}

/**
 * Stop the machine server the host supervised under `home` (HS2-WQ65KT). The CLI must see the same
 * HOTSHEET_HOME to find that server's discovery record, and match the build the host launched.
 */
async function stopMachineServer(home) {
  // Forced: the pinned production host owns this server, and an ordinary stop refuses it (HS2-9AWEX5).
  await run(
    workingTreeBinaries.HOTSHEET_CLI_BIN,
    ['serve', '-C', resolve(home, 'server-bootstrap.hs2'), '--stop', '--force'],
    { env: { ...process.env, HOTSHEET_HOME: home } },
  ).catch(() => undefined);
  const deadline = Date.now() + 10_000;
  while ((await processesUsing(home)).length > 0 && Date.now() < deadline)
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
}

// Build the production bundles in an isolated directory without the test runner's Vite mode.
const productionEnvironment = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.startsWith('VITEST') && name !== 'NODE_ENV' && name !== 'TEST'),
);

// Exercise the working tree's server, not a possibly stale release build the host would prefer.
const workingTreeBinaries = {
  HOTSHEET_SERVER_BIN: resolve(repoRoot, 'target/debug/hotsheet-server'),
  HOTSHEET_CLI_BIN: resolve(repoRoot, 'target/debug/hotsheet-cli'),
  HOTSHEET_MIGRATE_BIN: resolve(repoRoot, 'target/debug/hotsheet-migrate'),
};

/** The bootstrap machine server's registration under `home` (pid, launcher, build), or undefined. */
async function machineServer(home) {
  try {
    const store = await realpath(resolve(home, 'server-bootstrap.hs2')),
      id = createHash('sha256').update(store).digest('hex').slice(0, 16);
    return JSON.parse(await readFile(resolve(home, 'instances', `${id}.json`), 'utf8'));
  } catch {
    return undefined;
  }
}

async function copyBinaries(directory) {
  await mkdir(directory, { recursive: true });
  return Object.fromEntries(
    await Promise.all(
      Object.entries(workingTreeBinaries).map(async ([variable, source]) => {
        const destination = resolve(directory, variable.toLowerCase());
        // A server started from an earlier copy may still be executing `destination`; Linux
        // refuses to overwrite a running executable (ETXTBSY) but allows replacing it by rename.
        const staged = `${destination}.${process.pid}.${Date.now()}.tmp`;
        await copyFile(source, staged);
        await chmod(staged, (await stat(source)).mode & 0o777);
        await rename(staged, destination);
        return [variable, destination];
      }),
    ),
  );
}

async function openThrough(origin, root, ticketStore) {
  const response = await fetch(`${origin}/__hotsheet/projects/open`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ root, ticketStore }),
  });
  return { status: response.status, body: await response.json() };
}

let buildRoot, ownershipBuildRoot;
beforeAll(async () => {
  await mkdir(resolve(webRoot, 'target'), { recursive: true });
  buildRoot = await mkdtemp(resolve(webRoot, 'target/local-host-e2e-'));
  await run('npx', ['vite', 'build', '--outDir', resolve(buildRoot, 'dist'), '--emptyOutDir'], {
    cwd: webRoot,
    env: productionEnvironment,
  });
  await run(
    'npx',
    ['vite', 'build', '--ssr', 'src/local-host.ts', '--outDir', resolve(buildRoot, 'dist-host'), '--emptyOutDir'],
    {
      cwd: webRoot,
      env: productionEnvironment,
    },
  );
  // The first test deletes its build outputs to prove the snapshot; the ownership test needs its own.
  ownershipBuildRoot = await mkdtemp(resolve(webRoot, 'target/local-host-e2e-ownership-'));
  await cp(buildRoot, ownershipBuildRoot, { recursive: true });
}, 180_000);
afterAll(async () => {
  if (buildRoot) await rm(buildRoot, { recursive: true, force: true });
  if (ownershipBuildRoot) await rm(ownershipBuildRoot, { recursive: true, force: true });
});

// Keep the large process fixture readable while changing only its scheduling (HS2-XSP0GJ).
// prettier-ignore
it.concurrent('serves the production client and the local bridge without Vite', async () => {
  const home = await mkdtemp(resolve(tmpdir(), 'hotsheet-local-host-home-')),
    port = await availablePort(),
    origin = `http://127.0.0.1:${port}`;
  let log = '',
    child,
    other,
    otherBuildRoot,
    legacySnapshot,
    browser;
  // Everything that can fail runs inside the try, so the host and its machine server are always
  // stopped: a browser that failed to launch used to leave both running (HS2-WQ65KT).
  try {
    // HS2-6VQ5RV: a private checkout whose setup templates the test edits after the host starts.
    const checkout = resolve(home, 'checkout');
    await cp(resolve(repoRoot, 'plugins'), resolve(checkout, 'plugins'), { recursive: true });
    const candidateBin = resolve(home, 'candidate-bin');
    await mkdir(candidateBin);
    const candidates = Object.fromEntries(
      await Promise.all(
        Object.entries(workingTreeBinaries).map(async ([variable, source]) => {
          const destination = resolve(candidateBin, variable.toLowerCase());
          await copyFile(source, destination);
          await chmod(destination, (await stat(source)).mode & 0o777);
          return [variable, destination];
        }),
      ),
    );
    const revision = JSON.parse(
      (await run(workingTreeBinaries.HOTSHEET_SERVER_BIN, ['--revision-status'])).stdout,
    ).build_revision;
    child = spawn(process.execPath, [resolve(buildRoot, 'dist-host/local-host.js'), '--port', String(port)], {
      cwd: webRoot,
      env: {
        ...productionEnvironment,
        ...candidates,
        HOT_SHEET_BUILD_REVISION: revision,
        HOTSHEET_HOME: home,
        HOTSHEET_REPO_ROOT: checkout,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    child.stdout.on('data', (chunk) => (log += chunk));
    child.stderr.on('data', (chunk) => (log += chunk));
    browser = await chromium.launch();
    await waitForHost(`${origin}/`, child, () => log);
    // The HTTP listener may be ready before the child stdout chunk reaches us.
    await expect.poll(() => log).toContain(`Hot Sheet production client on ${origin}/`);
    // HS2-D2JQ9A: the host says which server build it launches (release when one is built).
    await expect.poll(() => log).toMatch(/Using (release|debug) Hot Sheet binaries/);
    // A second pinned host later in this test needs the same build after the working outputs are gone.
    otherBuildRoot = await mkdtemp(resolve(webRoot, 'target/local-host-e2e-other-'));
    await cp(buildRoot, otherBuildRoot, { recursive: true });
    // Working builds can replace or delete their outputs while this host stays on its launch snapshot.
    await rm(resolve(buildRoot, 'dist'), { recursive: true });
    for (const candidate of Object.values(candidates)) await rm(candidate);
    const page = await browser.newPage(),
      requests = [],
      pageErrors = [];
    page.on('request', (request) => requests.push(new URL(request.url()).pathname));
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.goto(`${origin}/`);
    await page.getByRole('button', { name: 'Open Project' }).first().waitFor();
    await page.waitForLoadState('networkidle');
    expect(pageErrors).toEqual([]);
    expect(
      requests.filter((path) => path.startsWith('/@') || path.includes('/node_modules/') || path.startsWith('/src/')),
    ).toEqual([]);
    expect(requests.filter((path) => path.startsWith('/assets/')).length).toBeLessThanOrEqual(6);
    // Development-only surfaces are absent; client routes resolve to the document.
    expect((await fetch(`${origin}/ux-demo`)).status).toBe(404);
    expect(await (await fetch(`${origin}/settings/anything`)).text()).toContain(
      '<main id="app" class="kui-app-root" data-viewport="locked">',
    );
    // The bridge is live: it supervises a real machine server under the isolated home.
    const checkouts = await fetch(`${origin}/__hotsheet/checkouts`);
    expect(checkouts.status).toBe(200);
    expect(Array.isArray(await checkouts.json())).toBe(true);

    // HS2-ARJ9J1: a project's store is hosted while a tab has it open, unhosted when the last
    // tab closes it, and hosted again by the next request through that project.
    const project = resolve(home, 'app'),
      ticketStore = resolve(home, 'app.hs2');
    await mkdir(project);
    await run(workingTreeBinaries.HOTSHEET_CLI_BIN, ['-C', ticketStore, 'init', '--prefix', 'LH'], {
      env: { ...process.env, HOTSHEET_HOME: home },
    });
    // Simulate the first upgrade restart: an existing Codex session retains the previous
    // host's hook command after that host has already removed its disposable snapshot.
    legacySnapshot = await mkdtemp(resolve(tmpdir(), 'hotsheet-local-host-'));
    const previousHookCli = resolve(legacySnapshot, 'bin/hotsheet-cli');
    await mkdir(resolve(project, '.codex'));
    await writeFile(
      resolve(project, '.codex/hooks.json'),
      JSON.stringify({ hooks: { PermissionRequest: [{ hooks: [{ command: `${previousHookCli} permission-hook --agent codex` }] }] } }),
    );
    await rm(legacySnapshot, { recursive: true });
    const opened = await fetch(`${origin}/__hotsheet/projects/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: project, ticketStore }),
    });
    expect(opened.status).toBe(201);
    const openedBody = await opened.json();
    expect(openedBody.setupWarning).toBeUndefined();
    const { id, apiPath } = openedBody,
      api = `${origin}${apiPath}`;
    // /projects/open schedules a second setup refresh in the server. Check the hook only
    // after that writer has finished, so this proves the final command survives both paths.
    await expect
      .poll(
        async () => {
          const response = await fetch(`${api}/lifecycle/quiescence`);
          expect(response.status).toBe(200);
          const { report } = await response.json();
          return report.blockers.some((blocker) => blocker.kind === 'setup_refreshes');
        },
        { timeout: 10_000 },
      )
      .toBe(false);
    // The host has already removed its working CLI build. A Codex process that keeps this
    // hook command through the next host restart still needs an executable outside the snapshot.
    const stableHookCli = resolve(home, 'bin/hotsheet-cli'),
      codexHooks = JSON.parse(await readFile(resolve(project, '.codex/hooks.json'), 'utf8'));
    const hookCommand = codexHooks.hooks.PermissionRequest[0].hooks[0].command,
      expectedHookCommand = `${stableHookCli} permission-hook --agent codex`;
    if (hookCommand !== expectedHookCommand) {
      const snapshotCli = hookCommand.split(' ')[0],
        marker = await readFile(resolve(dirname(snapshotCli), '.stable-hook-cli'), 'utf8').catch(() => '<missing>'),
        server = await machineServer(home),
        serverEnvironment =
          process.platform === 'linux' && server?.pid
            ? await readFile(`/proc/${server.pid}/environ`, 'utf8').catch(() => '')
            : '',
        setupVariables = serverEnvironment
          .split('\0')
          .filter((entry) => /^(HOTSHEET_HOME|HOTSHEET_HOOK_CLI_BIN|HOTSHEET_LOCAL_HOST_SNAPSHOT)=/.test(entry));
      throw new Error(
        `Hook CLI mismatch: expected=${expectedHookCommand}; actual=${hookCommand}; marker=${marker}; serverSetupEnv=${JSON.stringify(setupVariables)}; hostLog=${log.slice(-500)}`,
      );
    }
    expect((await run(stableHookCli, ['--help'])).stdout).toContain('permission-hook');
    expect((await run(previousHookCli, ['--help'])).stdout).toContain('permission-hook');
    const hostsProject = async () => (await (await fetch(`${api}/stores`)).json()).some((store) => store.prefix === 'LH');
    expect((await fetch(`${api}/ws/poll?timeout_ms=0&since=0&client=e2e-tab`)).status).toBe(200);
    expect(await hostsProject()).toBe(true);
    expect((await fetch(`${api}/close?client=e2e-tab`, { method: 'POST' })).status).toBe(204);
    let unhosted = false;
    for (let attempt = 0; attempt < 50 && !unhosted; attempt += 1) {
      unhosted = !(await hostsProject());
      if (!unhosted) await new Promise((resolveWait) => setTimeout(resolveWait, 100));
    }
    expect(unhosted).toBe(true);
    expect((await fetch(`${api}/checkouts/${encodeURIComponent(id)}/tickets`)).status).toBe(200);
    expect(await hostsProject()).toBe(true);

    // HS2-6VQ5RV: setup templates change in the checkout after the host started (a source edit
    // without a relaunch). Refreshes and other devices must not see a setup banner.
    const pluginDirectory = (await readdir(resolve(checkout, 'plugins'), { withFileTypes: true })).find((entry) =>
      entry.isDirectory(),
    );
    const pluginFile = (await readdir(resolve(checkout, 'plugins', pluginDirectory.name))).find(Boolean);
    await appendFile(resolve(checkout, 'plugins', pluginDirectory.name, pluginFile), '\nedited after host start\n');
    const reopened = await fetch(`${origin}/__hotsheet/projects/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: project, ticketStore }),
    });
    expect(reopened.status).toBe(201);
    expect((await reopened.json()).setupWarning).toBeUndefined();
    const setupBanner = '[data-component="project-setup-warning-banner"]';

    // A second browser client can render the project through the still-running production host.
    // Capture the connected state at the viewport of the reported failure and on a phone.
    await page.setViewportSize({ width: 1969, height: 1240 });
    await page.goto(origin);
    await page.getByRole('button', { name: 'Open Project' }).first().click();
    await page.getByRole('textbox', { name: 'Project Folder' }).fill(project);
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('heading', { name: 'Queue', level: 1 }).waitFor({ state: 'visible' });
    expect(await page.locator(setupBanner).count()).toBe(0);
    await page.reload();
    await page.getByRole('heading', { name: 'Queue', level: 1 }).waitFor({ state: 'visible' });
    expect(await page.locator(setupBanner).count()).toBe(0);
    await mkdir(resolve(webRoot, 'target/visual-captures'), { recursive: true });
    await page.screenshot({
      path: resolve(webRoot, 'target/visual-captures/hs2-efewsc-connected-wide.png'),
      fullPage: true,
    });
    const phone = await browser.newPage({ viewport: { width: 390, height: 844 } });
    await phone.goto(origin);
    await phone.getByRole('button', { name: 'Open Project' }).first().click();
    await phone.getByRole('textbox', { name: 'Project Folder' }).fill(project);
    await phone.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await phone.getByRole('combobox', { name: 'Ticket view' }).waitFor({ state: 'visible' });
    expect(await phone.locator(setupBanner).count()).toBe(0);
    await phone.screenshot({
      path: resolve(webRoot, 'target/visual-captures/hs2-efewsc-connected-narrow.png'),
      fullPage: true,
    });
    await phone.close();

    // A fresh browser has no remembered tabs; the server registry must supply the URL's project.
    const created = await run(
      workingTreeBinaries.HOTSHEET_CLI_BIN,
      ['-C', ticketStore, 'new', 'Linked ticket', '--category', 'task'],
      {
        env: { ...process.env, HOTSHEET_HOME: home },
      },
    );
    const slug = created.stdout.match(/LH-[A-Z0-9]+/)?.[0];
    expect(slug).toBeTruthy();
    const fresh = await browser.newContext();
    const linked = await fresh.newPage();
    await linked.goto(`${origin}/?project=${encodeURIComponent(project)}`);
    await linked.getByRole('heading', { name: 'Queue', level: 1 }).waitFor({ state: 'visible' });
    expect(await linked.locator(setupBanner).count()).toBe(0);
    await linked.screenshot({ path: resolve(webRoot, 'target/visual-captures/hs2-r4209f-project-link.png') });
    const narrowLink = await browser.newPage({ viewport: { width: 390, height: 844 } });
    await narrowLink.goto(`${origin}/?project=${encodeURIComponent(project)}`);
    await narrowLink.getByRole('combobox', { name: 'Ticket view' }).waitFor({ state: 'visible' });
    await narrowLink.screenshot({
      path: resolve(webRoot, 'target/visual-captures/hs2-r4209f-project-link-narrow.png'),
    });
    await narrowLink.close();
    await linked.goto(`${origin}/?project=${encodeURIComponent(project)}&ticket=${slug}`);
    await linked.locator('[data-component="ticket-reader"][data-reader-active="true"]').waitFor({ state: 'visible' });
    await linked.screenshot({ path: resolve(webRoot, 'target/visual-captures/hs2-r4209f-ticket-link.png') });
    await fresh.close();

    // HS2-46BKH7: a second pinned host on the same machine selected a build the shared machine
    // server does not run (another host replaced it). While that server is busy, the host must still
    // open the project through it and explain the deferred restart, instead of failing every open.
    await mkdir(resolve(project, '.hotsheet2'), { recursive: true });
    await writeFile(
      resolve(project, '.hotsheet2/settings.local.json'),
      JSON.stringify({ commands: [{ id: 'hold', title: 'Hold the server busy', program: 'sleep', args: ['20'] }] }),
    );
    const hold = await fetch(`${api}/checkouts/${encodeURIComponent(id)}/commands/hold/run`, { method: 'POST' });
    expect(hold.status, await hold.clone().text()).toBe(202);
    const otherPort = await availablePort(),
      otherOrigin = `http://127.0.0.1:${otherPort}`;
    other = spawn(process.execPath, [resolve(otherBuildRoot, 'dist-host/local-host.js'), '--port', String(otherPort)], {
      cwd: webRoot,
      env: {
        ...productionEnvironment,
        ...workingTreeBinaries,
        HOT_SHEET_BUILD_REVISION: `source-sha256:${'0'.repeat(64)}`,
        HOTSHEET_HOME: home,
        HOTSHEET_REPO_ROOT: checkout,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    other.stdout.on('data', (chunk) => (log += chunk));
    other.stderr.on('data', (chunk) => (log += chunk));
    await waitForHost(`${otherOrigin}/`, other, () => log);
    const mismatched = await fetch(`${otherOrigin}/__hotsheet/projects/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: project, ticketStore }),
    });
    expect(mismatched.status).toBe(201);
    const mismatchedBody = await mismatched.json();
    expect(mismatchedBody.serverBuildNotice).toMatch(/busy with active work/);
    expect(mismatchedBody.serverBuildNotice).not.toMatch(/server:rebuild/);
    expect(mismatchedBody.compatibility.kind).toBe('compatible');
    // A refresh and another device both open through the running server and see the notice.
    const noticeBanner = '[data-component="server-build-notice-banner"]';
    const mismatchedWide = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    await mismatchedWide.goto(`${otherOrigin}/?project=${encodeURIComponent(project)}`);
    await mismatchedWide.getByRole('heading', { name: 'Queue', level: 1 }).waitFor({ state: 'visible' });
    await mismatchedWide.locator(noticeBanner).waitFor({ state: 'visible' });
    await mismatchedWide.screenshot({
      path: resolve(webRoot, 'target/visual-captures/hs2-46bkh7-server-build-notice-wide.png'),
    });
    await mismatchedWide.reload();
    await mismatchedWide.getByRole('heading', { name: 'Queue', level: 1 }).waitFor({ state: 'visible' });
    await mismatchedWide.locator(noticeBanner).waitFor({ state: 'visible' });
    await mismatchedWide.close();
    const mismatchedPhone = await browser.newPage({ viewport: { width: 390, height: 844 } });
    await mismatchedPhone.goto(`${otherOrigin}/?project=${encodeURIComponent(project)}`);
    await mismatchedPhone.getByRole('combobox', { name: 'Ticket view' }).waitFor({ state: 'visible' });
    await mismatchedPhone.locator(noticeBanner).waitFor({ state: 'visible' });
    expect(await mismatchedPhone.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
    await mismatchedPhone.screenshot({
      path: resolve(webRoot, 'target/visual-captures/hs2-46bkh7-server-build-notice-narrow.png'),
      fullPage: true,
    });
    await mismatchedPhone.close();
  } finally {
    await browser?.close();
    for (const host of [child, other]) {
      if (!host) continue;
      host.kill('SIGTERM');
      await new Promise((resolveExit) =>
        host.exitCode === null && host.signalCode === null ? host.once('exit', resolveExit) : resolveExit(),
      );
    }
    await stopMachineServer(home);
    const survivors = await processesUsing(home);
    await rm(home, { recursive: true, force: true });
    if (legacySnapshot) await rm(legacySnapshot, { recursive: true, force: true });
    if (otherBuildRoot) await rm(otherBuildRoot, { recursive: true, force: true });
    // Nothing the test started may outlive it: not the host, not its supervised machine server.
    expect(survivors).toEqual([]);
  }
}, 120_000);

// HS2-9AWEX5: the pinned production host and a dev host (`npm run dev`) share one machine server per
// HOTSHEET_HOME. A dev host must never take that server over, and `server:rebuild`'s stop must not
// pull it out from under the production host. After every step a fresh browser on the pinned host
// opens the project without an error or a server-build notice.
// Keep the large process fixture readable while changing only its scheduling (HS2-XSP0GJ).
// prettier-ignore
it.concurrent('keeps the pinned host on its own machine server across dev hosts and rebuild stops', async () => {
  const home = await mkdtemp(resolve(tmpdir(), 'hotsheet-local-host-owner-')),
    runtimeTemp = await mkdtemp(resolve(tmpdir(), 'hotsheet-local-host-owner-runtime-')),
    pinnedPort = await availablePort(),
    devPort = await availablePort(),
    pinnedOrigin = `http://127.0.0.1:${pinnedPort}`,
    devOrigin = `http://127.0.0.1:${devPort}`,
    env = { ...process.env, HOTSHEET_HOME: home };
  let log = '',
    pinned,
    dev,
    browser;
  const capture = (child) => {
    child.stdout.on('data', (chunk) => (log += chunk));
    child.stderr.on('data', (chunk) => (log += chunk));
  };
  try {
    const project = resolve(home, 'app'),
      ticketStore = resolve(home, 'app.hs2');
    await mkdir(project);
    await run(workingTreeBinaries.HOTSHEET_CLI_BIN, ['-C', ticketStore, 'init', '--prefix', 'OW'], { env });
    const pinnedBinaries = await copyBinaries(resolve(home, 'pinned-bin')),
      devBinaries = await copyBinaries(resolve(home, 'dev-bin')),
      revision = JSON.parse(
        (await run(workingTreeBinaries.HOTSHEET_SERVER_BIN, ['--revision-status'])).stdout,
      ).build_revision;
    browser = await chromium.launch();
    const freshOpenOnPinnedHost = async (step, capturePrefix) => {
      const context = await browser.newContext(),
        page = await context.newPage(),
        errors = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.goto(`${pinnedOrigin}/?project=${encodeURIComponent(project)}`);
      await page.getByRole('heading', { name: 'Queue', level: 1 }).waitFor({ state: 'visible' });
      expect(await page.locator('[data-component="server-build-notice-banner"]').count(), step).toBe(0);
      expect(errors, step).toEqual([]);
      if (capturePrefix) {
        await mkdir(resolve(webRoot, 'target/visual-captures'), { recursive: true });
        await page.screenshot({ path: resolve(webRoot, `target/visual-captures/${capturePrefix}-wide.png`) });
        const phone = await context.newPage();
        await phone.setViewportSize({ width: 390, height: 844 });
        await phone.goto(`${pinnedOrigin}/?project=${encodeURIComponent(project)}`);
        await phone.getByRole('combobox', { name: 'Ticket view' }).waitFor({ state: 'visible' });
        await phone.getByText('No tickets yet').waitFor({ state: 'visible' });
        expect(await phone.locator('[data-component="server-build-notice-banner"]').count(), step).toBe(0);
        expect(await phone.evaluate(() => document.documentElement.scrollWidth), step).toBeLessThanOrEqual(390);
        await phone.screenshot({ path: resolve(webRoot, `target/visual-captures/${capturePrefix}-narrow.png`) });
      }
      await context.close();
    };

    // (b) A dev host starts first and launches the machine server from its own build.
    const devEnvironment = {
      ...process.env,
      ...devBinaries,
      HOTSHEET_HOME: home,
      HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp,
    };
    delete devEnvironment.HOT_SHEET_BUILD_REVISION;
    delete devEnvironment.HOTSHEET_LOCAL_HOST_SNAPSHOT;
    dev = spawn(
      process.execPath,
      [resolve(webRoot, 'scripts/stable-dev.mjs'), '--port', String(devPort), '--strictPort'],
      { env: devEnvironment, stdio: ['ignore', 'pipe', 'pipe'] },
    );
    capture(dev);
    const deadline = Date.now() + 90_000;
    while (Date.now() < deadline) {
      try {
        if ((await fetch(`${devOrigin}/`)).ok) break;
      } catch {
        /* Vite is still starting */
      }
      if (dev.exitCode !== null) throw new Error(`Dev host exited early.\n${log}`);
      await new Promise((resolveWait) => setTimeout(resolveWait, 200));
    }
    const devFirst = await openThrough(devOrigin, project, ticketStore);
    expect(devFirst.status, JSON.stringify(devFirst.body)).toBe(201);
    const devLaunched = await machineServer(home);
    expect(devLaunched?.launcher).toBe('stable-dev');

    // The pinned host then takes ownership: the idle dev-launched server restarts onto its snapshot.
    pinned = spawn(
      process.execPath,
      [resolve(ownershipBuildRoot, 'dist-host/local-host.js'), '--port', String(pinnedPort)],
      {
        cwd: webRoot,
        env: { ...productionEnvironment, ...pinnedBinaries, HOT_SHEET_BUILD_REVISION: revision, HOTSHEET_HOME: home },
        stdio: ['ignore', 'pipe', 'pipe'],
      },
    );
    capture(pinned);
    await waitForHost(`${pinnedOrigin}/`, pinned, () => log);
    const adopted = await openThrough(pinnedOrigin, project, ticketStore);
    expect(adopted.status, JSON.stringify(adopted.body)).toBe(201);
    expect(adopted.body.serverBuildNotice).toBeUndefined();
    const owned = await machineServer(home);
    expect(owned?.launcher).toBe('local-host-snapshot');
    expect(owned?.pid).not.toBe(devLaunched.pid);
    await freshOpenOnPinnedHost('after the pinned host adopted the dev-launched server');

    // The dev host reopens through the pinned server and never restarts or replaces it.
    const devAgain = await openThrough(devOrigin, project, ticketStore);
    expect(devAgain.status, JSON.stringify(devAgain.body)).toBe(201);
    expect((await machineServer(home))?.pid).toBe(owned.pid);
    await freshOpenOnPinnedHost('after the dev host reopened');

    // (a) `npm run server:rebuild` ends with this stop. It is refused, so no host can relaunch the
    // server from another build; the dev host keeps attaching to the pinned server.
    const refused = await run(
      workingTreeBinaries.HOTSHEET_CLI_BIN,
      ['serve', '-C', resolve(home, 'server-bootstrap.hs2'), '--stop'],
      { env },
    ).then(
      () => undefined,
      (error) => error,
    );
    expect(refused, 'the ordinary stop must fail').toBeDefined();
    expect(String(refused.stderr)).toMatch(/refusing to stop[\s\S]*launcher=local-host-snapshot/);
    expect(String(refused.stderr)).toContain('--force');
    expect((await machineServer(home))?.pid).toBe(owned.pid);
    expect((await openThrough(devOrigin, project, ticketStore)).status).toBe(201);
    expect((await machineServer(home))?.pid).toBe(owned.pid);
    await freshOpenOnPinnedHost('after the refused server:rebuild stop');

    // (c) Rebuilding the dev host's binaries (here: replacing them in place) does not touch the
    // running pinned server; both hosts keep opening through it.
    await copyBinaries(resolve(home, 'dev-bin'));
    expect((await openThrough(devOrigin, project, ticketStore)).status).toBe(201);
    const pinnedAgain = await openThrough(pinnedOrigin, project, ticketStore);
    expect(pinnedAgain.status).toBe(201);
    expect(pinnedAgain.body.serverBuildNotice).toBeUndefined();
    expect((await machineServer(home))?.pid).toBe(owned.pid);
    await freshOpenOnPinnedHost('after a dev rebuild', 'hs2-9awex5-pinned-after-dev-rebuild');
  } finally {
    await browser?.close();
    for (const host of [dev, pinned]) {
      if (!host || host.exitCode !== null || host.signalCode !== null) continue;
      const exited = new Promise((resolveExit) => host.once('exit', resolveExit));
      host.kill('SIGTERM');
      const force = setTimeout(() => host.kill('SIGKILL'), 10_000);
      await exited;
      clearTimeout(force);
    }
    await stopMachineServer(home);
    // The dev-launched server started the detached terminal broker from the dev binary under this
    // home. Brokers deliberately outlive server stops and restarts (they idle-exit later), so stop
    // this test's broker explicitly rather than leaving it to its idle timeout.
    for (const line of await processesUsing(home))
      if (line.includes('--terminal-broker-process')) process.kill(Number(line.split(/\s+/)[0]), 'SIGTERM');
    const deadline = Date.now() + 10_000;
    while ((await processesUsing(home)).length > 0 && Date.now() < deadline)
      await new Promise((resolveWait) => setTimeout(resolveWait, 100));
    const survivors = await processesUsing(home);
    await rm(home, { recursive: true, force: true });
    await rm(runtimeTemp, { recursive: true, force: true });
    expect(survivors).toEqual([]);
  }
}, 240_000);
