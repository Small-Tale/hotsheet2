#!/usr/bin/env node

import { chromium } from '@playwright/test';
import { execFile, spawn } from 'node:child_process';
import { mkdir, mkdtemp, realpath, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { createServer } from 'node:net';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const scriptDir = dirname(fileURLToPath(import.meta.url));
const webRoot = resolve(scriptDir, '..');
const repoRoot = resolve(webRoot, '../..');
const cli = resolve(repoRoot, 'target/debug/hotsheet-cli');
const server = resolve(repoRoot, 'target/debug/hotsheet-server');
const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

export function parseScaleCounts(value) {
  const counts = String(value).split(',').map(item => Number(item.trim()));
  if (!counts.length || counts.some(item => !Number.isSafeInteger(item) || item < 1)) {
    throw new Error(`Invalid --counts value: ${value}`);
  }
  return [...new Set(counts)].sort((left, right) => left - right);
}

function optionValue(argv, name, fallback) {
  const inline = argv.find(argument => argument.startsWith(`${name}=`));
  if (inline) return inline.slice(name.length + 1);
  const index = argv.indexOf(name);
  return index >= 0 ? argv[index + 1] : fallback;
}

export function parseArguments(argv) {
  if (argv.includes('--help')) return { help: true };
  return {
    counts: parseScaleCounts(optionValue(argv, '--counts', '10000,100000,1000000')),
    keep: argv.includes('--keep'),
    skipWeb: argv.includes('--skip-web'),
    assertCliBudgets: argv.includes('--assert-cli-budgets'),
    assertCliMutationBudgets: argv.includes('--assert-cli-mutation-budgets'),
    assertReindexBudgets: argv.includes('--assert-reindex-budgets'),
    assertWeb100k: argv.includes('--assert-web-100k'),
    output: optionValue(argv, '--output', join(tmpdir(), `hotsheet-scale-${Date.now()}.json`)),
    timeoutMs: Number(optionValue(argv, '--timeout-ms', '300000')),
  };
}

const cliReadBudgetsMs = new Map([
  [10_000, 2_000],
  [100_000, 5_000],
]);

export function assertCliReadBudgets(count, scenarios) {
  const budget = cliReadBudgetsMs.get(count);
  if (!budget) return;
  for (const name of ['list_first_100', 'full_text_query', 'show_ticket']) {
    const result = scenarios[name];
    if (!result || result.error || result.timed_out || typeof result.wall_ms !== 'number' || result.wall_ms > budget) {
      throw new Error(`CLI ${name} at ${count} tickets exceeded ${budget}ms: ${JSON.stringify(result)}`);
    }
  }
}

const cliMutationBudgetsMs = new Map([
  [10_000, 5_000],
  [100_000, 30_000],
]);

const reindexBudgetsMs = new Map([
  [100_000, 60_000],
  [1_000_000, 600_000],
]);

export function assertReindexBudgets(count, scenarios) {
  const budget = reindexBudgetsMs.get(count);
  if (!budget) return;
  const result = scenarios.reindex;
  if (!result || result.error || result.timed_out || typeof result.wall_ms !== 'number' || result.wall_ms > budget) {
    throw new Error(`CLI reindex at ${count} tickets exceeded ${budget}ms: ${JSON.stringify(result)}`);
  }
}

export function assertCliMutationBudgets(count, scenarios) {
  const budget = cliMutationBudgetsMs.get(count);
  if (!budget) return;
  for (const name of ['create_ticket', 'modify_ticket']) {
    const result = scenarios[name];
    if (!result || result.error || result.timed_out || typeof result.wall_ms !== 'number' || result.wall_ms > budget) {
      throw new Error(`CLI ${name} at ${count} tickets exceeded ${budget}ms: ${JSON.stringify(result)}`);
    }
  }
}

export function assertWeb100kAcceptance(count, server, web) {
  if (count !== 100_000) return;
  for (const name of ['list_compact', 'list_compact_next']) {
    const page = server?.scenarios?.[name];
    if (!page || page.error || page.item_count !== 200 || !page.has_next_cursor || page.response_bytes > 1_000_000 || page.wall_ms > 60_000) {
      throw new Error(`100K server ${name} exceeded the bounded-page acceptance threshold: ${JSON.stringify(page)}`);
    }
  }
  const scenarios = web?.scenarios;
  if (!scenarios || !scenarios.initial_load || scenarios.initial_load.wall_ms > 120_000 || scenarios.browser_heap_mb == null || scenarios.browser_heap_mb > 192) {
    throw new Error(`100K web initial load/heap exceeded acceptance thresholds: ${JSON.stringify(scenarios)}`);
  }
  for (const name of ['switch_backlog', 'switch_archive', 'switch_queue']) {
    if (!scenarios[name] || scenarios[name].wall_ms > 2_000) {
      throw new Error(`100K web ${name} exceeded 2000ms: ${JSON.stringify(scenarios[name])}`);
    }
  }
}

function base32(value, width) {
  let remaining = BigInt(value);
  let encoded = '';
  do {
    encoded = alphabet[Number(remaining & 31n)] + encoded;
    remaining >>= 5n;
  } while (remaining > 0n);
  return encoded.padStart(width, '0').slice(-width);
}

export function syntheticTicket(index, epochMs = 1_788_739_200_000) {
  const id = base32((BigInt(epochMs) << 80n) | BigInt(index), 26);
  const slug = `ST-${base32(index, 8)}`;
  const state = ['backlog', 'archive', 'started', 'not_started'][index % 4];
  const timestamp = new Date(epochMs + (index % 86_400_000)).toISOString();
  const upNext = state === 'not_started' && index % 20 === 3 ? 'up_next: true\n' : '';
  const body = `---\nid: ${id}\nslug: ${slug}\ntitle: Scale ticket ${index}\ncategory: task\npriority: default\nstatus: ${state}\n${upNext}created_at: ${timestamp}\nupdated_at: ${timestamp}\nschema: hotsheet/v2-bounded-notes\n---\n\n<!-- hotsheet:body:begin -->\nSynthetic scale fixture ${index}; search token needle-${index}.\n<!-- hotsheet:body:end -->\n\n<!-- hotsheet:notes:begin -->\n## Notes\n\n<!-- hotsheet:notes:end -->\n`;
  return { body, id, slug };
}

async function requireBinary(path, hint) {
  try { await stat(path); } catch { throw new Error(`${hint} is missing at ${path}. Run cargo build --workspace first.`); }
}

async function processRssKb(pid) {
  try {
    const { stdout } = await execFileAsync('ps', ['-o', 'rss=', '-p', String(pid)]);
    return Number(stdout.trim()) || 0;
  } catch { return 0; }
}

function boundedAppend(current, chunk, limit = 64_000) {
  const next = current + chunk;
  return next.length > limit ? next.slice(-limit) : next;
}

async function runMeasured(command, args, options = {}) {
  const started = performance.now();
  const child = spawn(command, args, {
    cwd: options.cwd ?? repoRoot,
    env: options.env ?? process.env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stdout = '', stderr = '', peakRssKb = 0, sampling = false;
  child.stdout.on('data', chunk => { stdout = boundedAppend(stdout, chunk.toString()); });
  child.stderr.on('data', chunk => { stderr = boundedAppend(stderr, chunk.toString()); });
  const sample = async () => {
    if (sampling) return;
    sampling = true;
    peakRssKb = Math.max(peakRssKb, await processRssKb(child.pid));
    sampling = false;
  };
  void sample();
  const interval = setInterval(() => { void sample(); }, 100);
  let timedOut = false;
  const timeout = setTimeout(() => {
    timedOut = true;
    child.kill('SIGTERM');
    setTimeout(() => { if (child.exitCode === null) child.kill('SIGKILL'); }, 2_000).unref();
  }, options.timeoutMs ?? 300_000);
  const code = await new Promise((resolveExit, reject) => {
    child.once('error', reject);
    child.once('exit', resolveExit);
  });
  clearInterval(interval);
  clearTimeout(timeout);
  await sample();
  const result = { wall_ms: Math.round(performance.now() - started), peak_rss_kb: peakRssKb, exit_code: code, timed_out: timedOut };
  if (code !== 0 && !options.allowFailure) throw new Error(`${command} ${args.join(' ')} failed (${code}): ${stderr || stdout}`);
  return { ...result, stdout, stderr };
}

async function generateTickets(store, start, end) {
  const started = performance.now();
  let bytes = 0;
  const directories = new Set();
  for (let index = start; index <= end; index += 1) {
    const ticket = syntheticTicket(index);
    const shard = ticket.id.slice(-2);
    const directory = join(store, 'tickets', shard);
    if (!directories.has(directory)) {
      await mkdir(directory, { recursive: true });
      directories.add(directory);
    }
    await writeFile(join(directory, `${ticket.id}.md`), ticket.body);
    bytes += Buffer.byteLength(ticket.body);
    if (index % 10_000 === 0 || index === end) process.stdout.write(`\rGenerated ${index.toLocaleString()} tickets`);
  }
  process.stdout.write('\n');
  return { wall_ms: Math.round(performance.now() - started), bytes };
}

async function checkoutFor(env, store, projectRoot) {
  const result = await runMeasured(cli, ['-C', store, 'checkout', 'list'], { env });
  const checkouts = JSON.parse(result.stdout);
  const checkout = checkouts.find(item => item.root === projectRoot);
  if (!checkout) throw new Error(`Bootstrap did not register checkout ${projectRoot}`);
  return checkout;
}

function summarizeProcess(result) {
  return { wall_ms: result.wall_ms, peak_rss_mb: Number((result.peak_rss_kb / 1024).toFixed(1)), ...(result.exit_code !== 0 ? { exit_code: result.exit_code } : {}), ...(result.timed_out ? { timed_out: true } : {}) };
}

function processFailure(error) { return { error: error instanceof Error ? error.message : String(error) }; }
function processSucceeded(result) { return !result.error && !result.timed_out && (result.exit_code === undefined || result.exit_code === 0); }

export async function commitFixtureTier({ store, count, env, timeoutMs, run = runMeasured }) {
  let stage;
  try { stage = summarizeProcess(await run('git', ['-C', store, 'add', 'tickets'], { env, timeoutMs, allowFailure: true })); }
  catch (error) { stage = processFailure(error); }
  if (!processSucceeded(stage)) return { stage, commit: { skipped: 'stage failed' }, mutations_safe: false };
  let commit;
  try { commit = summarizeProcess(await run('git', ['-C', store, '-c', 'user.name=Hot Sheet Scale', '-c', 'user.email=scale@hotsheet.local', 'commit', '-m', `Scale fixture ${count}`], { env, timeoutMs, allowFailure: true })); }
  catch (error) { commit = processFailure(error); }
  return { stage, commit, mutations_safe: processSucceeded(commit) };
}

async function benchmarkCli(env, store, count, timeoutMs, allowMutations = true) {
  const scenarios = {};
  const processOptions = { env, timeoutMs };
  const record = async (name, operation) => {
    try { return scenarios[name] = summarizeProcess(await operation()); }
    catch (error) { return scenarios[name] = { error: error instanceof Error ? error.message : String(error) }; }
  };
  await record('reindex', () => runMeasured(cli, ['-C', store, 'reindex'], processOptions));
  await record('list_first_100', () => runMeasured(cli, ['-C', store, 'ls', '--open', '--limit', '100'], processOptions));
  await record('full_text_query', () => runMeasured(cli, ['-C', store, 'ls', '--text', `needle-${count - 1}`, '--limit', '20'], processOptions));
  await record('show_ticket', () => runMeasured(cli, ['-C', store, 'show', syntheticTicket(count).slug], processOptions));
  if (!allowMutations) {
    scenarios.create_ticket = { skipped: 'fixture commit did not complete' };
    scenarios.modify_ticket = { skipped: 'fixture commit did not complete' };
    return scenarios;
  }
  let created;
  try { created = await runMeasured(cli, ['-C', store, 'new', '--title', `CLI scale mutation ${count}`, '--category', 'task'], processOptions); }
  catch (error) {
    scenarios.create_ticket = { error: error instanceof Error ? error.message : String(error) };
    scenarios.modify_ticket = { skipped: 'create_ticket failed' };
    return scenarios;
  }
  const createdSlug = created.stdout.match(/\bST-[0-9A-Z]+\b/)?.[0];
  scenarios.create_ticket = summarizeProcess(created);
  if (createdSlug) await record('modify_ticket', () => runMeasured(cli, ['-C', store, 'edit', createdSlug, '--priority', 'high'], processOptions));
  else scenarios.modify_ticket = { error: `Could not parse created ticket slug from: ${created.stdout.trim()}` };
  return scenarios;
}

async function fetchMeasured(url, secret, path, init = {}) {
  const started = performance.now();
  const { capture, timeoutMs, ...request } = init;
  const response = await fetch(`${url}${path}`, {
    ...request,
    headers: { 'content-type': 'application/json', 'x-hotsheet-secret': secret, ...request.headers },
    signal: AbortSignal.timeout(timeoutMs ?? 300_000),
  });
  let bytes = 0, text = '';
  if (capture) {
    text = await response.text();
    bytes = Buffer.byteLength(text);
  } else if (response.body) {
    for await (const chunk of response.body) bytes += chunk.byteLength;
  }
  if (!response.ok) throw new Error(`${request.method ?? 'GET'} ${path} returned ${response.status}: ${text}`);
  return { wall_ms: Math.round(performance.now() - started), response_bytes: bytes, text };
}

async function startMeasuredServer(env, store, indexPath, timeoutMs) {
  const secret = `scale-${Date.now()}`;
  const started = performance.now();
  const child = spawn(server, ['-C', store, '--bind', '127.0.0.1:0', '--secret', secret, '--index', indexPath], { cwd: repoRoot, env, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', stderr = '', peakRssKb = 0, sampling = false;
  const sample = async () => {
    if (sampling) return;
    sampling = true;
    peakRssKb = Math.max(peakRssKb, await processRssKb(child.pid));
    sampling = false;
  };
  void sample();
  const interval = setInterval(() => { void sample(); }, 100);
  child.stderr.on('data', chunk => { stderr = boundedAppend(stderr, chunk.toString()); });
  let url;
  try {
    url = await new Promise((resolveUrl, reject) => {
      const timer = setTimeout(() => reject(new Error(`Server startup timed out: ${stderr}`)), timeoutMs);
      child.stdout.on('data', chunk => {
        stdout = boundedAppend(stdout, chunk.toString());
        const match = stdout.match(/listening on (http:\/\/[^ ]+)/);
        if (match) { clearTimeout(timer); resolveUrl(match[1]); }
      });
      child.once('error', reject);
      child.once('exit', code => reject(new Error(`Server exited during startup (${code}): ${stderr || stdout}`)));
    });
  } catch (error) {
    clearInterval(interval);
    if (child.exitCode === null) child.kill('SIGTERM');
    throw error;
  }
  return {
    child, secret, url,
    startup_ms: Math.round(performance.now() - started),
    peakRssMb: () => Number((peakRssKb / 1024).toFixed(1)),
    async stop() {
      child.kill('SIGTERM');
      await Promise.race([new Promise(resolveExit => child.once('exit', resolveExit)), new Promise(resolveWait => setTimeout(resolveWait, 5_000))]);
      clearInterval(interval);
      await sample();
    },
  };
}

async function benchmarkServer(env, store, checkout, count, root, timeoutMs, allowMutations = true) {
  const instance = await startMeasuredServer(env, store, join(root, `index-${count}.sqlite`), timeoutMs);
  try {
    const encodedCheckout = encodeURIComponent(checkout.id);
    const scenarios = {};
    const record = async (name, operation) => {
      try { return scenarios[name] = await operation(); }
      catch (error) { return scenarios[name] = { error: error instanceof Error ? error.message : String(error) }; }
    };
    let firstPage;
    await record('list_compact', async () => {
      const measured = await fetchMeasured(instance.url, instance.secret, `/checkouts/${encodedCheckout}/tickets?compact=true&page_size=200`, { capture: true, timeoutMs });
      firstPage = JSON.parse(measured.text);
      return { wall_ms: measured.wall_ms, response_bytes: measured.response_bytes, item_count: firstPage.items.length, has_next_cursor: Boolean(firstPage.next_cursor), total_count: firstPage.counts.total };
    });
    if (firstPage?.next_cursor) await record('list_compact_next', async () => {
      const measured = await fetchMeasured(instance.url, instance.secret, `/checkouts/${encodedCheckout}/tickets?compact=true&page_size=200&cursor=${encodeURIComponent(firstPage.next_cursor)}`, { capture: true, timeoutMs });
      const page = JSON.parse(measured.text);
      return { wall_ms: measured.wall_ms, response_bytes: measured.response_bytes, item_count: page.items.length, has_next_cursor: Boolean(page.next_cursor), total_count: page.counts.total };
    });
    await record('view_ticket', () => fetchMeasured(instance.url, instance.secret, `/checkouts/${encodedCheckout}/tickets/${syntheticTicket(count).slug}`, { timeoutMs }));
    if (!allowMutations) {
      scenarios.create_ticket = { skipped: 'fixture commit did not complete' };
      scenarios.modify_ticket = { skipped: 'fixture commit did not complete' };
      return { startup_ms: instance.startup_ms, peak_rss_mb: instance.peakRssMb(), scenarios };
    }
    let created;
    try { created = await fetchMeasured(instance.url, instance.secret, `/checkouts/${encodedCheckout}/tickets`, { method: 'POST', body: JSON.stringify({ title: `Server scale mutation ${count}`, category: 'task', status: 'not_started' }), capture: true, timeoutMs }); }
    catch (error) {
      scenarios.create_ticket = { error: error instanceof Error ? error.message : String(error) };
      scenarios.modify_ticket = { skipped: 'create_ticket failed' };
      return { startup_ms: instance.startup_ms, peak_rss_mb: instance.peakRssMb(), scenarios };
    }
    const ticket = JSON.parse(created.text);
    scenarios.create_ticket = { wall_ms: created.wall_ms, response_bytes: created.response_bytes };
    await record('modify_ticket', () => fetchMeasured(instance.url, instance.secret, `/checkouts/${encodedCheckout}/tickets/${encodeURIComponent(ticket.qualified_id ?? ticket.slug)}`, { method: 'PATCH', body: JSON.stringify({ priority: 'low' }), timeoutMs }));
    return { startup_ms: instance.startup_ms, peak_rss_mb: instance.peakRssMb(), scenarios };
  } finally { await instance.stop(); }
}

async function unusedPort() {
  const listener = createServer();
  await new Promise((resolveListen, reject) => listener.listen(0, '127.0.0.1', resolveListen).once('error', reject));
  const address = listener.address();
  await new Promise(resolveClose => listener.close(resolveClose));
  return address.port;
}

async function startVite(env, timeoutMs) {
  const port = await unusedPort();
  const child = spawn('npm', ['run', 'dev:hot', '--', '--port', String(port)], { cwd: webRoot, env, detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  child.stdout.on('data', chunk => { output = boundedAppend(output, chunk.toString()); });
  child.stderr.on('data', chunk => { output = boundedAppend(output, chunk.toString()); });
  const baseUrl = `http://127.0.0.1:${port}`;
  const started = performance.now();
  while (performance.now() - started < timeoutMs) {
    if (child.exitCode !== null) throw new Error(`Vite exited during startup: ${output}`);
    try { if ((await fetch(`${baseUrl}/ux-demo`)).ok) return { baseUrl, child }; } catch { /* retry */ }
    await new Promise(resolveWait => setTimeout(resolveWait, 100));
  }
  child.kill('SIGTERM');
  throw new Error(`Vite startup timed out: ${output}`);
}

async function stopStoreServer(env, store) {
  await runMeasured(server, ['-C', store, '--stop'], { env, timeoutMs: 15_000, allowFailure: true });
  await new Promise(resolveWait => setTimeout(resolveWait, 250));
}

function stopVite(instance) {
  if (!instance || instance.child.exitCode !== null) return;
  try {
    if (process.platform === 'win32') instance.child.kill('SIGTERM');
    else process.kill(-instance.child.pid, 'SIGTERM');
  } catch { instance.child.kill('SIGTERM'); }
}

async function removeWorkspace(path) {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    try { await rm(path, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 }); return; }
    catch (error) {
      if (attempt === 9) throw error;
      await new Promise(resolveWait => setTimeout(resolveWait, 250));
    }
  }
}

async function measuredUiAction(action, ready) {
  const started = performance.now();
  await action();
  await ready();
  return { wall_ms: Math.round(performance.now() - started) };
}

async function benchmarkWeb(browser, baseUrl, projectRoot, count, timeoutMs, allowMutations = true) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  page.setDefaultTimeout(timeoutMs);
  await page.addInitScript(root => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', root);
  }, projectRoot);
  const scenarios = {};
  try {
    const loadStarted = performance.now();
    await page.goto(`${baseUrl}/?dev-review=false`, { waitUntil: 'domcontentloaded', timeout: timeoutMs });
    await page.getByRole('heading', { name: 'Queue' }).waitFor();
    await page.locator('[data-component="ticket-list-row"]').first().waitFor();
    scenarios.initial_load = { wall_ms: Math.round(performance.now() - loadStarted) };
    for (const view of ['Backlog', 'Archive', 'Queue']) {
      scenarios[`switch_${view.toLowerCase()}`] = await measuredUiAction(
        () => page.getByRole('button', { name: new RegExp(view) }).click(),
        () => page.getByRole('heading', { name: view }).waitFor(),
      );
    }
    const first = page.locator('[data-component="ticket-list-row"]').first();
    const slug = await first.getAttribute('data-ticket-slug');
    scenarios.view_ticket = await measuredUiAction(() => first.click(), () => page.locator('[data-component="ticket-inspector"]').getByText(slug, { exact: true }).waitFor());
    if (!allowMutations) {
      scenarios.create_ticket = { skipped: 'fixture commit did not complete' };
      scenarios.modify_ticket = { skipped: 'fixture commit did not complete' };
      scenarios.browser_heap_mb = await page.evaluate(() => {
        const memory = performance.memory;
        return memory ? Number((memory.usedJSHeapSize / 1024 / 1024).toFixed(1)) : null;
      });
      return { scenarios };
    }
    const title = `Web scale mutation ${count}-${Date.now()}`;
    scenarios.create_ticket = await measuredUiAction(async () => {
      await page.getByRole('button', { name: 'New ticket…' }).click();
      await page.getByRole('textbox', { name: 'Ticket title' }).fill(title);
      await page.getByRole('button', { name: 'Create ticket' }).click();
    }, () => page.locator('[data-component="ticket-list-row"]', { hasText: title }).waitFor());
    const created = page.locator('[data-component="ticket-list-row"]', { hasText: title });
    const upNext = created.getByRole('button', { name: /Up Next/ });
    const response = page.waitForResponse(item => item.request().method() === 'PATCH' && item.url().includes('/tickets/'));
    scenarios.modify_ticket = await measuredUiAction(() => upNext.click(), () => response);
    scenarios.browser_heap_mb = await page.evaluate(() => {
      const memory = performance.memory;
      return memory ? Number((memory.usedJSHeapSize / 1024 / 1024).toFixed(1)) : null;
    });
    return { scenarios };
  } finally { await context.close(); }
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  if (options.help) {
    console.log('Usage: npm run stress:scale -- [--counts 10000,100000,1000000] [--skip-web] [--assert-cli-budgets] [--assert-cli-mutation-budgets] [--assert-reindex-budgets] [--assert-web-100k] [--keep] [--timeout-ms 300000] [--output /path/report.json]');
    return;
  }
  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs < 1_000) throw new Error('--timeout-ms must be at least 1000');
  await Promise.all([requireBinary(cli, 'hotsheet-cli'), requireBinary(server, 'hotsheet-server')]);
  const root = await realpath(await mkdtemp(join(tmpdir(), 'hotsheet-scale-')));
  const projectRoot = join(root, 'project');
  const store = join(root, 'project.hs2');
  const home = join(root, 'home');
  await Promise.all([mkdir(projectRoot), mkdir(home)]);
  const env = { ...process.env, HOTSHEET_HOME: home };
  const report = {
    generated_at: new Date().toISOString(),
    host: { platform: process.platform, arch: process.arch, node: process.version },
    configuration: { counts: options.counts, timeout_ms: options.timeoutMs, web: !options.skipWeb },
    dataset: { root, store, kept: options.keep },
    runs: [],
  };
  let vite, browser, generated = 0, generatedBytes = 0;
  const cliBudgetErrors = [];
  try {
    console.log(`Disposable scale workspace: ${root}`);
    report.bootstrap = summarizeProcess(await runMeasured(cli, ['bootstrap', '--project', projectRoot, '--store', store, '--prefix', 'ST'], { env, timeoutMs: options.timeoutMs }));
    const checkout = await checkoutFor(env, store, projectRoot);
    if (!options.skipWeb) {
      vite = await startVite(env, options.timeoutMs);
      browser = await chromium.launch({ headless: true });
    }
    for (const count of options.counts) {
      console.log(`\nScale ${count.toLocaleString()}`);
      const generation = await generateTickets(store, generated + 1, count);
      generated = count;
      generatedBytes += generation.bytes;
      const run = { count, generation: { ...generation, total_bytes: generatedBytes } };
      run.fixture_commit = await commitFixtureTier({ store, count, env, timeoutMs: options.timeoutMs });
      const allowMutations = run.fixture_commit.mutations_safe;
      for (const [name, operation] of [
        ['cli', () => benchmarkCli(env, store, count, options.timeoutMs, allowMutations)],
        ['server', () => benchmarkServer(env, store, checkout, count, root, options.timeoutMs, allowMutations)],
        ...(!options.skipWeb ? [['web', async () => {
          try { return await benchmarkWeb(browser, vite.baseUrl, projectRoot, count, options.timeoutMs, allowMutations); }
          finally { await stopStoreServer(env, store); }
        }]] : []),
      ]) {
        try { run[name] = await operation(); }
        catch (error) { run[name] = { error: error instanceof Error ? error.message : String(error) }; }
      }
      if (options.assertCliBudgets) {
        try {
          assertCliReadBudgets(count, run.cli);
          run.cli_budget = { passed: true };
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          run.cli_budget = { passed: false, error: message };
          cliBudgetErrors.push(message);
        }
      }
      if (options.assertCliMutationBudgets) {
        try {
          assertCliMutationBudgets(count, run.cli);
          run.cli_mutation_budget = { passed: true };
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          run.cli_mutation_budget = { passed: false, error: message };
          cliBudgetErrors.push(message);
        }
      }
      if (options.assertReindexBudgets) {
        try {
          assertReindexBudgets(count, run.cli);
          if (reindexBudgetsMs.has(count)) run.reindex_budget = { passed: true };
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          run.reindex_budget = { passed: false, error: message };
          cliBudgetErrors.push(message);
        }
      }
      if (options.assertWeb100k) {
        try {
          assertWeb100kAcceptance(count, run.server, run.web);
          if (count === 100_000) run.web_100k_acceptance = { passed: true };
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          run.web_100k_acceptance = { passed: false, error: message };
          cliBudgetErrors.push(message);
        }
      }
      report.runs.push(run);
      await mkdir(dirname(options.output), { recursive: true });
      await writeFile(options.output, `${JSON.stringify(report, null, 2)}\n`);
      console.log(JSON.stringify(run, null, 2));
    }
  } finally {
    if (browser) await browser.close();
    stopVite(vite);
    await stopStoreServer(env, store);
    await mkdir(dirname(options.output), { recursive: true });
    await writeFile(options.output, `${JSON.stringify(report, null, 2)}\n`);
    if (!options.keep) await removeWorkspace(root);
  }
  console.log(`\nReport: ${options.output}`);
  if (cliBudgetErrors.length) throw new Error(cliBudgetErrors.join('\n'));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error); process.exitCode = 1; });
}
