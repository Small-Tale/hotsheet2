// HS2-M6TZB5: the browser slot pool across separate processes, through the real target/debug
// hotsheet-cli and its local resource-claim backend in an isolated machine home. Each holder is a
// disposable Node process that stands in for a browser; no browser starts. Synthetic data only.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterAll, describe, expect, it } from 'vitest';

const cli = resolve(fileURLToPath(new URL('../../../target/debug/hotsheet-cli', import.meta.url)));
const module = new URL('./browser-slots.mjs', import.meta.url).href;
const home = mkdtempSync(join(tmpdir(), 'hotsheet-browser-slots-'));
const children = [];

afterAll(() => {
  for (const child of children) if (child.exitCode === null) child.kill('SIGKILL');
  rmSync(home, { recursive: true, force: true });
});

/** A holder process: takes a WebKit slot, says which, and releases it when told to on stdin. */
function holder(leaseMs = 120_000) {
  const program = `
import { acquireBrowserSlot } from ${JSON.stringify(module)};
const slot = await acquireBrowserSlot({ engine: 'webkit', leaseMs: ${leaseMs}, purpose: 'e2e' });
console.log('acquired ' + slot.resource + ' ' + Date.now());
process.stdin.setEncoding('utf8');
process.stdin.once('data', async () => { await slot.release(); console.log('released ' + Date.now()); process.exit(0); });
`;
  const child = spawn(process.execPath, ['--input-type=module', '-e', program], {
    env: {
      ...process.env,
      CI: '',
      HOTSHEET_HOME: home,
      HOTSHEET_CLI_BIN: cli,
      HOTSHEET_WEBKIT_SLOTS: '1',
      HOTSHEET_BROWSER_SLOTS: '',
      HOTSHEET_BROWSER_SLOTS_HELD: '',
    },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  children.push(child);
  let output = '';
  const lines = [];
  const waiters = [];
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', (chunk) => {
    output += chunk;
    for (const line of output.split('\n').slice(0, -1)) lines.push(line);
    output = output.slice(output.lastIndexOf('\n') + 1);
    for (const waiter of waiters.splice(0)) waiter();
  });
  let stderr = '';
  child.stderr.on('data', (chunk) => (stderr += chunk));
  const line = (prefix) =>
    new Promise((done, fail) => {
      const check = () => {
        const found = lines.find((entry) => entry.startsWith(prefix));
        if (found) return done(Number(found.split(' ').at(-1)));
        if (child.exitCode !== null) return fail(new Error(`holder exited without ${prefix}: ${stderr}`));
        waiters.push(check);
      };
      child.once('exit', () => waiters.length && check());
      check();
    });
  return { child, line, has: (prefix) => lines.some((entry) => entry.startsWith(prefix)) };
}

describe.skipIf(!existsSync(cli) || process.platform === 'win32')('browser slots across processes (HS2-M6TZB5)', () => {
  it('lets one holder at a time take a one-slot pool and hands it over on release', async () => {
    const first = holder();
    const firstAt = await first.line('acquired webkit-1');
    const second = holder();
    // The second process waits on the claim service while the first holds the only slot.
    await new Promise((done) => setTimeout(done, 1_500));
    expect(second.has('acquired')).toBe(false);
    first.child.stdin.write('release\n');
    const releasedAt = await first.line('released');
    const secondAt = await second.line('acquired webkit-1');
    expect(secondAt).toBeGreaterThanOrEqual(firstAt);
    expect(secondAt).toBeGreaterThanOrEqual(releasedAt - 50);
    second.child.stdin.write('release\n');
    await second.line('released');
  }, 60_000);

  it('frees the slot of a crashed holder when its lease lapses', async () => {
    const crashed = holder(3_000);
    await crashed.line('acquired webkit-1');
    crashed.child.kill('SIGKILL');
    const killedAt = Date.now();
    const next = holder();
    const nextAt = await next.line('acquired webkit-1');
    // Nothing released the slot: the waiter got it only once the 3 s lease ran out.
    expect(nextAt - killedAt).toBeGreaterThan(500);
    expect(nextAt - killedAt).toBeLessThan(30_000);
    next.child.stdin.write('release\n');
    await next.line('released');
  }, 60_000);
});
