import { spawnSync } from 'node:child_process';

import { windowsProcessTable } from './windows-process-table.mjs';

/** Observe the owned server before it can exit (HS2-0BQ8J9/HS2-XP3Y4D). */
export function trackServerProcess(child, { drained } = {}) {
  const exited = new Promise((resolveExit) => child.once('exit', (code) => resolveExit(code)));
  const closed = new Promise((resolveClose) => child.once('close', (code) => resolveClose(code)));
  return { child, exited, closed, drained };
}

/**
 * Query only children of the server PID this fixture launched, never by process name or port. The
 * shared process-table helper answers without a PowerShell cold start per call (HS2-S5BQRD).
 */
export async function windowsDescendants(pid, table = windowsProcessTable()) {
  if (process.platform !== 'win32') return [];
  if (!Number.isSafeInteger(pid) || pid <= 0) throw new Error('Missing owned Windows server PID.');
  let rows;
  try {
    rows = await table.snapshot();
  } catch (error) {
    throw new Error(`Could not inspect owned Windows descendants (${error.message}).`, { cause: error });
  }
  return [...rows.values()].filter((row) => row.ppid === pid).map((row) => ({ ProcessId: row.pid, Name: row.name }));
}

async function waitForWindowsDescendants(pid) {
  const deadline = Date.now() + 10_000;
  for (;;) {
    const remaining = await windowsDescendants(pid);
    if (remaining.length === 0) return;
    if (Date.now() >= deadline) {
      throw new Error(`Owned Windows server ${pid} retained ${remaining.length} descendants after shutdown.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

/** Stop only this test's server, then wait for inherited pipes or an explicit descendant drain. */
export async function stopServerProcess({ child, closed, drained }) {
  // HS2-0BQ8J9: esbuild inherits Vite's stderr. Parent exit does not mean its native service
  // has closed that pipe or finished writing. Windows may close inherited pipes before a
  // descendant exits, so callers with an explicit lifetime signal pass `drained` too.
  // The taken-port child may already have exited.
  if (child.exitCode !== null || child.signalCode !== null) {
    await closed;
    if (drained) await drained;
    else await waitForWindowsDescendants(child.pid);
    return;
  }
  if (process.platform === 'win32' && !drained) {
    // Taskkill follows this still-live owned parent and its esbuild service before Node can
    // reap the parent PID. A post-close query below also covers descendants on an exit race.
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true });
  } else {
    child.kill('SIGTERM');
  }
  const force = setTimeout(() => child.kill('SIGKILL'), 10_000);
  try {
    await closed;
    if (drained) await drained;
    else await waitForWindowsDescendants(child.pid);
  } finally {
    clearTimeout(force);
  }
}
