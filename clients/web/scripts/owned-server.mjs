// HS2-F6F8MA: Playwright runs its `webServer` in a detached process group and stops it only from a
// live runner (its own teardown or a SIGINT handler). A runner ended by SIGTERM or SIGKILL (an
// interrupted local gate, a lease-loss stop, a crash) never reached that teardown, so the Vite group
// kept serving the fixture port. This launcher ties the server's lifetime to the runner instead:
// the runner holds this process's stdin pipe, and the kernel closes it however the runner ends.
//
// Ownership is the ChildProcess handle this launcher created, plus its own process group while it is
// alive in it. It never looks processes up by name or port, and Node disables `kill` on a handle once
// the child has been reaped, so a reused pid cannot receive a signal meant for the server.
import { spawn, spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const STOP_SIGNALS = ['SIGINT', 'SIGTERM', 'SIGHUP'];
const SIGNAL_EXIT = { SIGINT: 130, SIGTERM: 143, SIGHUP: 129, SIGKILL: 137 };

/** `[--exit-on-stdin-eof] [--grace-ms N] -- <program> [args...]` */
export function parseOwnedServerArguments(argv) {
  const separator = argv.indexOf('--');
  if (separator < 0 || separator === argv.length - 1) throw new Error('Usage: owned-server [options] -- <command>');
  const options = { exitOnStdinEof: false, graceMs: 5000 };
  const flags = argv.slice(0, separator);
  for (let index = 0; index < flags.length; index += 1) {
    const flag = flags[index];
    if (flag === '--exit-on-stdin-eof') options.exitOnStdinEof = true;
    else if (flag === '--grace-ms') {
      const value = Number(flags[(index += 1)]);
      if (!Number.isInteger(value) || value < 0) throw new Error('--grace-ms needs a non-negative integer');
      options.graceMs = value;
    } else throw new Error(`Unknown owned-server option: ${flag}`);
  }
  const [program, ...args] = argv.slice(separator + 1);
  return { ...options, program, args };
}

/** Whether this live process leads its own process group (Playwright's detached webServer group). */
export function leadsOwnProcessGroup(pid = process.pid, run = spawnSync) {
  const result = run('ps', ['-o', 'pgid=', '-p', String(pid)], { encoding: 'utf8' });
  return result.status === 0 && Number(result.stdout.trim()) === pid;
}

/**
 * Run one owned server child until it exits, stopping it when this process is signalled or, with
 * `exitOnStdinEof`, when the parent's stdin pipe closes. Resolves to the exit code to report.
 */
export function runOwnedServer({
  program,
  args,
  exitOnStdinEof,
  graceMs,
  processHost = process,
  stdin = process.stdin,
  spawnChild = spawn,
  timers = globalThis,
  leadsGroup = leadsOwnProcessGroup,
}) {
  // Decided before the server starts, while this process is certainly alive in its own group.
  const sweepGroup = leadsGroup();
  // The server stays in this launcher's process group (no `detached`), so Playwright's own group
  // SIGKILL still covers both. Its stdin is not inherited: only this launcher watches the parent pipe.
  const child = spawnChild(program, args, { stdio: ['ignore', 'inherit', 'inherit'] });
  let stopReason;
  let forceKill;
  const stop = (reason) => {
    if (stopReason !== undefined) return;
    stopReason = reason;
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.kill('SIGTERM');
    // The timer lives in this launcher, which stays alive until the child closes.
    forceKill = timers.setTimeout(() => {
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
    }, graceMs);
  };
  const handlers = STOP_SIGNALS.map((signal) => [signal, () => stop(signal)]);
  for (const [signal, handler] of handlers) processHost.on(signal, handler);
  const onEof = () => stop('stdin-eof');
  if (exitOnStdinEof) {
    stdin.on('end', onEof);
    stdin.on('close', onEof);
    stdin.on('error', onEof);
    // Discard anything written; reading is what lets the stream observe end-of-file.
    stdin.resume();
  }
  return new Promise((resolveExit) => {
    let finished = false;
    const finish = (code) => {
      if (finished) return;
      finished = true;
      if (forceKill !== undefined) timers.clearTimeout(forceKill);
      for (const [signal, handler] of handlers) processHost.off(signal, handler);
      if (exitOnStdinEof) {
        stdin.off('end', onEof);
        stdin.off('close', onEof);
        stdin.off('error', onEof);
        stdin.pause();
        stdin.destroy?.();
      }
      resolveExit(code);
    };
    child.once('error', () => finish(1));
    child.once('close', (code, signal) => {
      // A same-group descendant that outlived the server's own shutdown is still ours: this live
      // leader's group id cannot be reused. One SIGKILL ends it, and this launcher, which has no
      // further work. A descendant that left the group (setsid/setpgid) is out of reach here; it
      // must be bound to its parent the way Vite's esbuild service watches its stdin.
      if (stopReason !== undefined && sweepGroup) processHost.kill(0, 'SIGKILL');
      if (stopReason !== undefined && stopReason in SIGNAL_EXIT) finish(SIGNAL_EXIT[stopReason]);
      else finish(code ?? SIGNAL_EXIT[signal] ?? 1);
    });
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await runOwnedServer(parseOwnedServerArguments(process.argv.slice(2)));
}
